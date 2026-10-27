//! Restore dormant read-only graphs from daemon facts, never from worker database edits.
use crate::{database, dispatcher, workflow, AppState};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
pub fn safe_step(step: &str) -> bool {
    matches!(
        step,
        "project_wait"
            | "await_sprint_gate"
            | "backlog"
            | "declined_wait"
            | "triage"
            | "research"
            | "plan"
            | "plan_review"
            | "plan_gate"
            | "supervisor"
            | "discussion"
            | "sprint_plan"
            | "publish_plans"
            | "start_items"
            | "scan_devops"
            | "scan_qa"
            | "scan_pm"
            | "route_finding"
    )
}
pub fn restore(home: &Path, item: &str, observed: &str) -> anyhow::Result<bool> {
    if !matches!(
        observed,
        "cancelled" | "completed" | "missing" | "orphaned_interrupt"
    ) || crate::execution::Execution::load(home)?.status().stopped
    {
        return Ok(false);
    }
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let Some(mut state) = workflow::state(&tx, item)? else {
        return Ok(false);
    };
    let mut step = state["step"].as_str().unwrap_or("").to_owned();
    // Old projections could route positive plan-review observations to a task
    // owner before a gate. Repair only with the exact completed read receipt,
    // no later attempts, and no approval or uncertain/write effect to replay.
    if state["approved"] != true
        && matches!(
            step.as_str(),
            "backend" | "frontend" | "devops" | "qa" | "docs"
        )
    {
        let receipt:Option<(u64,String,String)> = tx.query_row(
            "SELECT a.generation,d.request_json,a.outcome_json FROM attempts a JOIN dispatches d ON d.attempt_id=a.id WHERE a.item_id=?1 AND a.state='completed' AND NOT EXISTS(SELECT 1 FROM attempts later WHERE later.item_id=a.item_id AND later.generation>a.generation) ORDER BY a.generation DESC LIMIT 1",
            [item], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((generation, request, result)) = receipt {
            let request: Value = serde_json::from_str(&request)?;
            let result: Value = serde_json::from_str(&result)?;
            let unapproved: bool = tx.query_row(
                "SELECT approved_plan_revision_id IS NULL FROM work_items WHERE id=?1",
                [item],
                |r| r.get(0),
            )?;
            if unapproved
                && generation == state["generation"].as_u64().unwrap_or(0)
                && request["capability"] == "read"
                && request["inputs"]["step"] == "plan_review"
                && result["outcome"] == "done"
                && !result["findings"]
                    .as_array()
                    .is_some_and(|fs| fs.iter().any(|f| f["severity"] == "high"))
            {
                step = "plan_gate".into();
                state["step"] = json!(step);
            }
        }
    }
    if !safe_step(&step)
        || state["frozen"] == true
        || state["checkpoint_uncertain"] == true
        || state["closed_on_github"] == true
    {
        return Ok(false);
    }
    let blocked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')) OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND (kind='worker.recover' OR (kind NOT LIKE 'worker.%' AND kind<>'github.status'))) OR EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?1 AND state='running')",[item],|r|r.get(0))?;
    let gates: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending'))",
        [item],
        |r| r.get(0),
    )?;
    if blocked || (gates && step != "await_sprint_gate") {
        return Ok(false);
    }
    let project = dispatcher::item(&tx, item)?.0;
    state["sequence"] = json!(state["sequence"].as_u64().unwrap_or(0) + 1);
    let generation: u64 = tx.query_row(
        "SELECT coalesce((SELECT generation FROM item_generations WHERE item_id=?1),0)",
        [item],
        |r| r.get(0),
    )?;
    state["generation"] = json!(generation.max(state["generation"].as_u64().unwrap_or(0)));
    workflow::save(&tx, item, &state)?;
    tx.execute("DELETE FROM graph_waiters WHERE item_id=?1", [item])?;
    tx.execute("DELETE FROM loop_waiters WHERE item_id=?1", [item])?;
    if state["project"] == true {
        tx.execute("UPDATE project_events SET delivered_at=NULL WHERE project_id=?1 AND delivered_at IS NOT NULL AND NOT EXISTS(SELECT 1 FROM inbox WHERE source='project.ack' AND external_event_id=project_events.id)",[&project])?;
    }
    tx.execute("UPDATE outbox SET delivered_at=?2,result_json='{\"restored_from_daemon_state\":true}' WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind IN('worker.start','worker.resume','worker.cancel')",params![item,plantool_core::now()])?;
    let start = plantool_contract::Start {
        thread_id: item.into(),
        graph: if state["project"] == true {
            plantool_contract::Graph::Project
        } else {
            plantool_contract::Graph::WorkItem
        },
        input: json!({"workflow":state}),
        idempotency_key: format!("restore:{item}:{}", state["sequence"]),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, item, "worker.recover", &serde_json::to_value(start)?)?;
    database::audit(
        &tx,
        "daemon",
        "workflow.restored",
        item,
        &json!({"step":step,"worker_state":observed,"preapproval_projection_repaired":step=="plan_gate"}),
    )?;
    let thread = crate::workspace::item_thread(&tx, item)?;
    crate::workspace::append(&tx,&thread,"daemon","This planning step stopped during a restart. We are restoring it and keeping its history. Approvals are unchanged.",&format!("restored:{item}:{}",state["sequence"]))?;
    tx.commit()?;
    Ok(true)
}

/// Retry only an uncommitted plan-gate request whose completed read-only review
/// is still current. Gate creation and its GitHub publication intent share one
/// transaction, so the absence of both proves that no approval request escaped.
fn restore_uncommitted_plan_gate(home: &Path, item: &str, observed: &str) -> anyhow::Result<bool> {
    if !matches!(observed, "recovery_needed" | "missing") {
        return Ok(false);
    }
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let Some(mut state) = workflow::state(&tx, item)? else {
        return Ok(false);
    };
    if state["step"] != "escalated"
        || state["failed_step"] != "plan_gate"
        || state["frozen"] != true
        || state["checkpoint_uncertain"] != true
        || state["approved"] == true
        || state["closed_on_github"] == true
        || state["uncommitted_gate_retry"] == true
    {
        return Ok(false);
    }
    let plan_sha = state["plan_sha"].as_str().unwrap_or("").to_owned();
    let generation = state["generation"].as_u64().unwrap_or(0);
    let sequence = state["sequence"].as_u64().unwrap_or(0);
    if plan_sha.is_empty() || generation == 0 {
        return Ok(false);
    }
    let github_binding: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM work_items i JOIN github_repositories g ON g.project_id=i.project_id WHERE i.id=?1 AND i.github_issue_number>0)",
        [item],
        |row| row.get(0),
    )?;
    if !github_binding {
        return Ok(false);
    }
    let receipt: Option<(String, String, String)> = tx
        .query_row(
            "SELECT d.request_json,a.outcome_json,r.content_sha256
             FROM attempts a JOIN dispatches d ON d.attempt_id=a.id
             JOIN revisions r ON r.item_id=a.item_id AND r.doc_kind='plan'
             WHERE a.item_id=?1 AND a.generation=?2 AND a.role='cto'
               AND a.state='completed' AND r.content_sha256=?3
               AND r.id=(SELECT id FROM revisions WHERE item_id=a.item_id AND doc_kind='plan' ORDER BY number DESC LIMIT 1)
             ORDER BY a.ended_at DESC LIMIT 1",
            params![item, generation, plan_sha],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((request, outcome, _)) = receipt else {
        return Ok(false);
    };
    let request: Value = serde_json::from_str(&request)?;
    let outcome: Value = serde_json::from_str(&outcome)?;
    if request["capability"] != "read"
        || request["inputs"]["step"] != "plan_review"
        || request["plan_sha"] != plan_sha
        || outcome["outcome"] != "done"
        || outcome["findings"]
            .as_array()
            .is_some_and(|findings| findings.iter().any(|finding| finding["severity"] == "high"))
    {
        return Ok(false);
    }
    let review_key = format!(
        "node:{}",
        plantool_core::sha256_hex(&format!("{item}:{sequence}:plan_gate"))
    );
    let uncertain_or_external: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed'))
          OR EXISTS(SELECT 1 FROM inbox WHERE source='worker.gate' AND external_event_id=?2)
          OR EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND kind='plan' AND target_digest=?3)
          OR EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?1 AND state='running')
          OR EXISTS(SELECT 1 FROM outbox o JOIN github_gate_context c ON c.gate_id=json_extract(o.payload_json,'$.gate')
                    JOIN revisions r ON r.id=c.revision_id WHERE o.kind='github.plan' AND r.item_id=?1 AND r.content_sha256=?3)
          OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status')",
        params![item, review_key, plan_sha],
        |row| row.get(0),
    )?;
    if uncertain_or_external {
        return Ok(false);
    }

    state["step"] = json!("plan_gate");
    state["failed_step"] = Value::Null;
    state["frozen"] = json!(false);
    state["checkpoint_uncertain"] = json!(false);
    state["uncommitted_gate_retry"] = json!(true);
    state["sequence"] = json!(sequence + 1);
    workflow::save(&tx, item, &state)?;
    tx.execute(
        r#"UPDATE outbox SET delivered_at=?2,result_json='{"restored_from_daemon_state":true}' WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind IN('worker.start','worker.resume','worker.cancel')"#,
        params![item, plantool_core::now()],
    )?;
    let start = plantool_contract::Start {
        thread_id: item.into(),
        graph: if state["project"] == true {
            plantool_contract::Graph::Project
        } else {
            plantool_contract::Graph::WorkItem
        },
        input: json!({"workflow":state}),
        idempotency_key: format!("auto-gate-retry:{item}:{plan_sha}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, item, "worker.recover", &serde_json::to_value(start)?)?;
    database::audit(
        &tx,
        "daemon",
        "workflow.uncommitted_gate_retried",
        item,
        &json!({"plan_sha":plan_sha,"approved":false}),
    )?;
    let thread = crate::workspace::item_thread(&tx, item)?;
    crate::workspace::append(
        &tx,
        &thread,
        "daemon",
        "The plan review passed. I’m restoring its review request; approval is still yours.",
        &format!("gate-retry:{item}:{plan_sha}"),
    )?;
    tx.commit()?;
    Ok(true)
}
fn has_current_handoff(
    db: &rusqlite::Connection,
    item: &str,
    interrupt: &str,
) -> anyhow::Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND (kind IN('worker.start','worker.recover','worker.cancel') OR (kind='worker.resume' AND json_extract(payload_json,'$.interrupt_id')=?2))) OR EXISTS(SELECT 1 FROM graph_waiters WHERE item_id=?1 AND interrupt_id=?2) OR EXISTS(SELECT 1 FROM loop_waiters WHERE item_id=?1 AND interrupt_id=?2)",params![item,interrupt],|r|r.get(0))?)
}
pub async fn reconcile(state: &AppState) -> anyhow::Result<()> {
    probe(state, None, false).await
}
pub async fn reconcile_project(state: &AppState, project: &str) -> anyhow::Result<()> {
    probe(state, Some(project), true).await
}
async fn probe(state: &AppState, project: Option<&str>, force: bool) -> anyhow::Result<()> {
    if state.runs.execution.status().stopped || !state.runs.worker.availability().available {
        return Ok(());
    }
    let rows = {
        let db = database::open(&state.config.home)?;
        let mut q=db.prepare("SELECT w.item_id,w.state_json FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE i.kind='eos' AND (?2 IS NULL OR i.project_id=?2) AND i.closed_at IS NULL AND (coalesce(json_extract(w.state_json,'$.frozen'),0)=0 OR (json_extract(w.state_json,'$.step')='escalated' AND json_extract(w.state_json,'$.failed_step')='plan_gate' AND coalesce(json_extract(w.state_json,'$.checkpoint_uncertain'),0)=1 AND coalesce(json_extract(w.state_json,'$.approved'),0)=0 AND coalesce(json_extract(w.state_json,'$.uncommitted_gate_retry'),0)=0)) AND NOT EXISTS(SELECT 1 FROM attempts a WHERE a.item_id=i.id AND a.state IN('queued','starting','running','waiting','idle','recovery_needed')) AND coalesce((SELECT CAST(value_json AS INTEGER) FROM settings WHERE key='graph.probe:'||w.item_id),0)<=?1 ORDER BY coalesce((SELECT CAST(value_json AS INTEGER) FROM settings WHERE key='graph.probe:'||w.item_id),0),w.item_id LIMIT 32")?;
        let rows = q
            .query_map(
                params![
                    if force {
                        i64::MAX
                    } else {
                        database::unix_now() - 15
                    },
                    project
                ],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (item, raw) in rows {
        if state.runs.execution.status().stopped || !state.runs.worker.availability().available {
            break;
        }
        let facts: Value = serde_json::from_str(&raw)?;
        let step = facts["step"].as_str().unwrap_or("");
        let uncertain_gate = facts["checkpoint_uncertain"] == true
            && facts["failed_step"] == "plan_gate"
            && facts["frozen"] == true;
        if !safe_step(step)
            && !uncertain_gate
            && !(facts["approved"] != true
                && matches!(step, "backend" | "frontend" | "devops" | "qa" | "docs"))
        {
            continue;
        }
        database::set(
            &database::open(&state.config.home)?,
            &format!("graph.probe:{item}"),
            &database::unix_now(),
        )?;
        if force {
            database::open(&state.config.home)?.execute("UPDATE outbox SET next_attempt_at=0 WHERE aggregate_id=?1 AND kind LIKE 'worker.%' AND delivered_at IS NULL",[&item])?;
        }
        let path = format!(
            "status/{}",
            percent_encoding::utf8_percent_encode(&item, percent_encoding::NON_ALPHANUMERIC)
        );
        let observed = match state.runs.worker.call(&path, None).await {
            Ok(value) => value,
            Err(error)
                if error
                    .downcast_ref::<crate::worker::CallError>()
                    .is_some_and(|e| e.status == 404) =>
            {
                json!({"state":"missing"})
            }
            Err(_) => continue,
        };
        database::set(
            &database::open(&state.config.home)?,
            &format!("graph.status:{item}"),
            &json!({"state":observed["state"],"interrupt_id":observed["interrupt_id"],"checked_at":database::unix_now()}),
        )?;
        let kind = observed["state"].as_str().unwrap_or("");
        if uncertain_gate {
            restore_uncommitted_plan_gate(&state.config.home, &item, kind)?;
        } else if kind == "interrupted" {
            let db = database::open(&state.config.home)?;
            let live =
                has_current_handoff(&db, &item, observed["interrupt_id"].as_str().unwrap_or(""))?;
            if !live {
                restore(&state.config.home, &item, "orphaned_interrupt")?;
            }
        } else {
            restore(&state.config.home, &item, kind)?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, rusqlite::Connection) {
        let t = tempfile::tempdir().unwrap();
        let db = database::open(t.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,'/checkout','eos','public','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO project_settings(project_id,version,effective_at,set_by) VALUES('p',1,'now','owner')",[]).unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('p:project','p','eos','Scheduler','project_wait','now','p','project','{}')",[]).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('p:project','{\"item\":\"p:project\",\"project\":true,\"step\":\"project_wait\",\"sequence\":0,\"generation\":0,\"frozen\":false}','b','b')",[]).unwrap();
        (t, db)
    }
    #[test]
    fn a_stale_pending_result_does_not_hide_an_orphaned_interrupt() {
        let (t, db) = setup();
        dispatcher::enqueue(
            &db,
            "p:project",
            "worker.resume",
            &json!({"thread_id":"p:project","interrupt_id":"old","idempotency_key":"old-result"}),
        )
        .unwrap();
        assert!(has_current_handoff(&db, "p:project", "old").unwrap());
        assert!(!has_current_handoff(&db, "p:project", "current").unwrap());
        dispatcher::enqueue(
            &db,
            "p:project",
            "worker.recover",
            &json!({"thread_id":"p:project","idempotency_key":"restore"}),
        )
        .unwrap();
        assert!(has_current_handoff(&db, "p:project", "current").unwrap());
        drop(t);
    }
    #[test]
    fn stale_preapproval_coding_projection_requires_completed_read_receipt() {
        let (t, db) = setup();
        db.execute("UPDATE workflow_items SET state_json='{\"step\":\"backend\",\"approved\":false,\"generation\":1,\"sequence\":1,\"frozen\":false}'",[]).unwrap();
        assert!(!restore(t.path(), "p:project", "orphaned_interrupt").unwrap());
        db.execute("INSERT INTO attempts VALUES('review','p:project','cto',1,'route','completed','/checkout','now','now','{\"outcome\":\"done\",\"findings\":[{\"severity\":\"medium\"}]}','review','{}',0)",[]).unwrap();
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('review','{\"capability\":\"read\",\"inputs\":{\"step\":\"plan_review\"}}','digest','p:project','old',0)",[]).unwrap();
        assert!(restore(t.path(), "p:project", "orphaned_interrupt").unwrap());
        let s = workflow::state(&db, "p:project").unwrap().unwrap();
        assert_eq!(s["step"], "plan_gate");
        assert_eq!(s["approved"], false);
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM gates", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert!(!restore(t.path(), "p:project", "orphaned_interrupt").unwrap());
    }
    #[test]
    fn uncertain_uncommitted_plan_gate_retries_once_without_approving() {
        let (t, db) = setup();
        let plan = "a".repeat(64);
        db.execute(
            "UPDATE work_items SET github_issue_number=9 WHERE id='p:project'",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO github_repositories(project_id,app_id,installation_id,repo_id,full_name,default_branch,app_bot_id) VALUES('p',1,1,1,'o/r','main',1)",[]).unwrap();
        db.execute(
            "INSERT INTO artifacts(sha256,size,created_at) VALUES(?1,1,1)",
            [&plan],
        )
        .unwrap();
        db.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) VALUES('rev','p:project','plan',1,?1,'worker','now')",[&plan]).unwrap();
        db.execute("UPDATE workflow_items SET state_json=?1",[json!({"item":"p:project","project":false,"step":"escalated","failed_step":"plan_gate","frozen":true,"checkpoint_uncertain":true,"approved":false,"plan_sha":plan,"generation":1,"sequence":4}).to_string()]).unwrap();
        db.execute("INSERT INTO attempts(id,item_id,role,generation,model_route,state,worktree,started_at,ended_at,outcome_json,idempotency_key,run_json) VALUES('review','p:project','cto',1,'codex','completed','/checkout','now','now',?1,'review','{}')",[json!({"outcome":"done","findings":[]}).to_string()]).unwrap();
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('review',?1,'digest','p:project','interrupt',0)",[json!({"capability":"read","inputs":{"step":"plan_review"},"plan_sha":plan}).to_string()]).unwrap();
        drop(db);

        assert!(restore_uncommitted_plan_gate(t.path(), "p:project", "recovery_needed").unwrap());
        let db = database::open(t.path()).unwrap();
        let state = workflow::state(&db, "p:project").unwrap().unwrap();
        assert_eq!(state["step"], "plan_gate");
        assert_eq!(state["frozen"], false);
        assert_eq!(state["checkpoint_uncertain"], false);
        assert_eq!(state["approved"], false);
        assert_eq!(state["uncommitted_gate_retry"], true);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert!(!restore_uncommitted_plan_gate(t.path(), "p:project", "recovery_needed").unwrap());
    }
    #[test]
    fn cancelled_idle_graph_restores_only_unacknowledged_events_once() {
        let (t, db) = setup();
        db.execute(
            "INSERT INTO graph_waiters VALUES('p:project','p:project','old','project')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO project_events VALUES('unacked','p','sprint_planning_due','{}','now','now'),('acked','p','issue_new','{}','now','now')",[]).unwrap();
        db.execute(
            "INSERT INTO inbox VALUES('project.ack','acked','now','{}')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO item_generations VALUES('p:project',3)", [])
            .unwrap();
        database::set(&db, "graph.status:p:project", &json!({"state":"cancelled"})).unwrap();
        assert_eq!(
            crate::sprint_view::progress(&db, "p").unwrap()["state"],
            "stalled"
        );
        assert!(restore(t.path(), "p:project", "cancelled").unwrap());
        assert!(!restore(t.path(), "p:project", "cancelled").unwrap());
        let state = workflow::state(&db, "p:project").unwrap().unwrap();
        assert_eq!(state["sequence"], 1);
        assert_eq!(state["generation"], 3);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM project_events WHERE delivered_at IS NULL",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM graph_waiters", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            crate::sprint_view::progress(&db, "p").unwrap()["state"],
            "restoring"
        );
    }
    #[test]
    fn restoration_never_revives_closed_frozen_or_coding_work() {
        for state in [
            json!({"step":"backend"}),
            json!({"step":"research","frozen":true}),
            json!({"step":"research","checkpoint_uncertain":true}),
            json!({"step":"research","closed_on_github":true}),
        ] {
            let (t, db) = setup();
            db.execute(
                "UPDATE workflow_items SET state_json=?1",
                [state.to_string()],
            )
            .unwrap();
            assert!(!restore(t.path(), "p:project", "cancelled").unwrap());
        }
        let (t, db) = setup();
        db.execute("INSERT INTO attempts VALUES('a','p:project','pm',1,'route','queued','/checkout','now',NULL,NULL,'attempt','{}',0)",[]).unwrap();
        assert!(!restore(t.path(), "p:project", "cancelled").unwrap());
        assert_eq!(
            crate::sprint_view::progress(&db, "p").unwrap()["state"],
            "queued"
        );
        db.execute("INSERT INTO leases(id,holder_attempt_id,holder_kind,model_route,acquired_at,expires_at) VALUES(1,'a','coding','route','now',9999999999)",[]).unwrap();
        assert_eq!(
            crate::sprint_view::progress(&db, "p").unwrap()["state"],
            "running"
        );
        db.execute("DELETE FROM leases", []).unwrap();
        db.execute("UPDATE attempts SET state='failed'", [])
            .unwrap();
        db.execute("INSERT INTO workflow_actions VALUES('action','p:project','publish','{}','running',NULL)",[]).unwrap();
        assert!(!restore(t.path(), "p:project", "cancelled").unwrap());
    }
    #[test]
    fn healthy_checkpoints_and_stop_all_are_not_overridden() {
        let (t, _) = setup();
        assert!(!restore(t.path(), "p:project", "interrupted").unwrap());
        let exec = crate::execution::Execution::load(t.path()).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(exec.stop_all(&crate::identities::RunTokens::default()))
            .unwrap();
        assert!(!restore(t.path(), "p:project", "cancelled").unwrap());
    }
    #[test]
    fn planning_trigger_wakes_an_existing_waiter_without_waiting_for_github_sync() {
        let (t, db) = setup();
        db.execute(
            "INSERT INTO graph_waiters VALUES('p:project','p:project','current','project')",
            [],
        )
        .unwrap();
        crate::sprint_view::trigger(t.path(), "p", "plan-now").unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.resume' AND delivered_at IS NULL",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM project_events WHERE delivered_at IS NOT NULL",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
}

/// Read-only summary after Resume; frozen/uncertain work retains its review fence.
pub fn resume_report(home: &Path) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let mut q = db.prepare("SELECT w.item_id,i.title,w.state_json FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE i.deleted_at IS NULL AND i.closed_at IS NULL")?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut attention = vec![];
    let mut eligible = 0;
    for (id, title, raw) in rows {
        let s: Value = serde_json::from_str(&raw)?;
        if s["closed_on_github"] == true {
            continue;
        }
        let uncertain: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state='recovery_needed')",
            [&id],
            |r| r.get(0),
        )?;
        if s["frozen"] == true || s["checkpoint_uncertain"] == true || uncertain {
            attention.push(json!({"item":id,"title":title,"reason":if uncertain || s["checkpoint_uncertain"]==true {"Uncertain work needs reconciliation"}else{"Paused decision needs review"}}));
        } else if safe_step(s["step"].as_str().unwrap_or("")) {
            eligible += 1;
        }
    }
    Ok(json!({"eligible":eligible,"attention":attention}))
}
