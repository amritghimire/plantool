//! Owner-authorized replay of failed read-only steps. Never resets approval gates or coding work.
use crate::{database, dispatcher, workflow};
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;

pub fn snapshot(home: &Path, item: &str) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let state = workflow::state(&db, item)?.context("Unknown workflow")?;
    let attempt:Option<(String,String,String)>=db.query_row("SELECT a.id,a.state,d.request_json FROM attempts a JOIN dispatches d ON d.attempt_id=a.id WHERE a.item_id=?1 ORDER BY a.generation DESC LIMIT 1",[item],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let active:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed'))",[item],|r|r.get(0))?;
    let gates: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending'))",
        [item],
        |r| r.get(0),
    )?;
    let external:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status')",[item],|r|r.get(0))?;
    let (id, status, request) = attempt.unwrap_or_default();
    let request: Value = serde_json::from_str(&request).unwrap_or(Value::Null);
    let step = request["inputs"]["step"].as_str().unwrap_or("");
    let read = matches!(
        step,
        "triage"
            | "sprint_plan"
            | "research"
            | "plan"
            | "plan_review"
            | "scan_devops"
            | "scan_qa"
            | "scan_pm"
            | "route_finding"
            | "discussion"
            | "supervisor"
    ) && request["capability"] == "read";
    let eligible = state["checkpoint_uncertain"] != true
        && state["frozen"] == true
        && state["step"] == "escalated"
        && ["failed", "completed"].contains(&status.as_str())
        && read
        && !active
        && !gates
        && !external;
    Ok(
        json!({"state":state,"attempt_id":id,"step":step,"eligible":eligible,"review":status=="completed","active":active,"gates":gates,"external":external}),
    )
}
pub fn retry(home: &Path, item: &str, expected: &str) -> anyhow::Result<Value> {
    queue_retry(home, item, expected, false)
}
fn queue_retry(home: &Path, item: &str, expected: &str, automatic: bool) -> anyhow::Result<Value> {
    let facts = snapshot(home, item)?;
    ensure!(facts["eligible"]==true,"Only failed read-only steps without unresolved effects or gates can be retried; coding/recovery-needed attempts require manual reconciliation");
    ensure!(
        facts["attempt_id"] == expected,
        "Recovery attempt changed; refresh"
    );
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    ensure!(
        workflow::state(&tx, item)? == Some(facts["state"].clone()),
        "Workflow changed; refresh"
    );
    let latest: String = tx.query_row(
        "SELECT id FROM attempts WHERE item_id=?1 ORDER BY generation DESC LIMIT 1",
        [item],
        |r| r.get(0),
    )?;
    ensure!(latest == expected, "Attempt changed; refresh");
    // Recheck mutable blockers under the write lock, rather than trusting the UI snapshot.
    let blocked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')) OR EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending')) OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status')",[item],|r|r.get(0))?;
    ensure!(!blocked, "Recovery blockers changed; refresh");
    let mut state = facts["state"].clone();
    state["step"] = facts["step"].clone();
    if automatic {
        let raw: String = tx.query_row(
            "SELECT outcome_json FROM attempts WHERE id=?1",
            [expected],
            |r| r.get(0),
        )?;
        let result: plantool_contract::RoleResult = serde_json::from_str(&raw)?;
        if result.role == "supervisor" && result.outcome == plantool_contract::Outcome::Done {
            let Some(plantool_contract::RoleDetails::Strategy { action, owner, .. }) =
                &result.details
            else {
                anyhow::bail!("No actionable completed recommendation")
            };
            if action == "replan" {
                state["step"] = json!("plan");
                state["approved"] = json!(false);
            } else {
                ensure!(action == "retry", "Owner decision required");
                state["step"] = json!(workflow::retry_step(
                    owner.as_deref().context("Missing retry owner")?,
                    &state
                )
                .context("Unknown retry owner")?);
            }
        } else {
            ensure!(
                transient_read_failure(&result),
                "This decision requires review"
            );
            let cycles = state["automatic_retries"].as_u64().unwrap_or(0) + 1;
            state["automatic_retries"] = json!(cycles);
            state["retry_not_before"] = json!(
                database::unix_now() + 300_i64.saturating_mul(1_i64 << cycles.min(4)).min(3600)
            );
        }
    }
    state["frozen"] = json!(false);
    state["route_retry"] = json!(false);
    if facts["step"] == "supervisor" {
        state["supervisor_rounds"] = json!(0);
    }
    state["sequence"] = json!(state["sequence"].as_u64().unwrap_or(0) + 1);
    state["generation"] = json!(tx.query_row(
        "SELECT generation FROM item_generations WHERE item_id=?1",
        [item],
        |r| r.get::<_, u64>(0)
    )?);
    workflow::save(&tx, item, &state)?;
    tx.execute(
        "UPDATE workflow_items SET last_error=NULL WHERE item_id=?1",
        [item],
    )?;
    tx.execute("UPDATE outbox SET delivered_at=?2,result_json='{\"superseded_by_owner_recovery\":true}' WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind IN('worker.start','worker.resume','worker.cancel','worker.recover')",params![item,plantool_core::now()])?;
    let key = format!("recover:{}", uuid::Uuid::new_v4());
    let start = plantool_contract::Start {
        thread_id: item.into(),
        graph: if state["project"] == true {
            plantool_contract::Graph::Project
        } else {
            plantool_contract::Graph::WorkItem
        },
        input: json!({"workflow":state}),
        idempotency_key: key,
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, item, "worker.recover", &serde_json::to_value(start)?)?;
    database::audit(
        &tx,
        if automatic { "daemon" } else { "human" },
        "workflow.retry",
        item,
        &json!({"attempt":expected,"step":state["step"],"automatic":automatic,"previous_state":facts["state"]}),
    )?;
    crate::workspace::operational(&tx,item,"Recovery queued",&format!("retry:{}:{}",expected,state["sequence"]),"Retry queued. We will start this step again and keep its history. Any changes still need the usual approval.")?;
    tx.commit()?;
    Ok(json!({"queued":true,"step":state["step"]}))
}

/// Reconcile an interrupted approved coding attempt only when all effects are
/// observed: no live process/proxy/action, a clean owned worktree, and either the
/// recorded checkpoint or a fast-forward to the synchronized default branch.
/// A completed Supervisor retry is required; no task completion is fabricated.
pub fn reconcile_clean_coding(home: &Path) -> anyhow::Result<()> {
    use plantool_contract::{Dispatch, Outcome, RoleDetails, RoleResult};
    let db = database::open(home)?;
    let mut q=db.prepare("SELECT a.id,a.item_id,a.generation,a.worktree,d.request_json,w.state_json,w.head_sha FROM attempts a JOIN dispatches d ON d.attempt_id=a.id JOIN workflow_items w ON w.item_id=a.item_id WHERE a.state='recovery_needed' AND a.role IN('backend','frontend','devops','qa') AND a.outcome_json IS NULL AND NOT EXISTS(SELECT 1 FROM attempts later WHERE later.item_id=a.item_id AND later.generation>a.generation)")?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, item, generation, path, request, raw, checkpoint) in rows {
        let s: Value = serde_json::from_str(&raw)?;
        if s["approved"] != true
            || s["checkpoint_uncertain"] == true
            || s["closed_on_github"] == true
        {
            continue;
        }
        let request: Dispatch = serde_json::from_str(&request)?;
        let latest:Option<String>=db.query_row("SELECT outcome_json FROM attempts WHERE item_id=?1 AND state='completed' AND role='supervisor' AND generation<?2 ORDER BY generation DESC LIMIT 1",params![item,generation],|r|r.get(0)).optional()?.flatten();
        let Some(latest) = latest else { continue };
        let Ok(recommendation) = serde_json::from_str::<RoleResult>(&latest) else {
            continue;
        };
        let Some(RoleDetails::Strategy {
            action,
            owner: Some(owner),
            ..
        }) = &recommendation.details
        else {
            continue;
        };
        if recommendation.outcome != Outcome::Done
            || action != "retry"
            || workflow::retry_step(owner, &s).as_deref() != request.inputs["step"].as_str()
        {
            continue;
        }
        let blocked:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM supervised_processes WHERE id=?1) OR EXISTS(SELECT 1 FROM proxy_calls WHERE attempt_id=?1 AND state IN('pending','recovery_needed')) OR EXISTS(SELECT 1 FROM attempts WHERE item_id=?2 AND state IN('queued','starting','running','waiting','idle')) OR EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?2 AND state='running') OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?2 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status') OR EXISTS(SELECT 1 FROM gates WHERE item_id=?2 AND state IN('pending','open'))",params![id,item],|r|r.get(0))?;
        let approved:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE i.id=?1 AND r.content_sha256=?2)",params![item,request.plan_sha],|r|r.get(0))?;
        if blocked || !approved {
            continue;
        }
        let expected = home.join("worktrees").join(&id);
        let path = std::path::PathBuf::from(path);
        if path != expected
            || std::fs::symlink_metadata(&path).map_or(true, |m| m.file_type().is_symlink())
        {
            continue;
        }
        if !crate::git::git(
            &path,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?
        .is_empty()
        {
            continue;
        }
        let head = crate::git::git(&path, &["rev-parse", "HEAD"])?;
        let (_, root) = dispatcher::item(&db, &item)?;
        let reference = crate::instructions::default_ref(&root)?;
        let default = crate::git::git(&root, &["rev-parse", &reference])?;
        if head != checkpoint
            && !(head == default
                && crate::git::git(&root, &["merge-base", "--is-ancestor", &checkpoint, &head])
                    .is_ok())
        {
            continue;
        }
        let mut write = database::open(home)?;
        let tx = write.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if workflow::state(&tx, &item)? != Some(s.clone()) {
            continue;
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND state='recovery_needed' AND outcome_json IS NULL) AND NOT EXISTS(SELECT 1 FROM attempts WHERE item_id=?2 AND state IN('queued','starting','running','waiting','idle')) AND NOT EXISTS(SELECT 1 FROM supervised_processes WHERE id=?1)",params![id,item],|r|r.get(0))?;
        if !valid {
            continue;
        }
        let still_safe:bool=tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM proxy_calls WHERE attempt_id=?1 AND state IN('pending','recovery_needed')) AND NOT EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?2 AND state='running') AND NOT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?2 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status') AND NOT EXISTS(SELECT 1 FROM gates WHERE item_id=?2 AND state IN('pending','open')) AND EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE i.id=?2 AND r.content_sha256=?3)",params![id,item,request.plan_sha],|r|r.get(0))?;
        if !still_safe {
            continue;
        }
        let mut next = s.clone();
        next["step"] = request.inputs["step"].clone();
        next["frozen"] = json!(false);
        next["generation"] = json!(generation);
        next["head_sha"] = json!(head);
        next["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
        let result=RoleResult{item:item.clone(),role:request.role.clone(),attempt_id:id.clone(),generation,outcome:Outcome::Failed,summary:"Interrupted attempt reconciled at a clean trusted checkpoint. No implementation or task completion was accepted.".into(),tasks_done:vec![],artifacts:vec![],checks:vec![],findings:vec![],next:None,review_guide:None,details:None};
        tx.execute(
            "UPDATE attempts SET state='failed',ended_at=?2,outcome_json=?3 WHERE id=?1",
            params![id, plantool_core::now(), serde_json::to_string(&result)?],
        )?;
        tx.execute(
            "UPDATE workflow_items SET base_sha=CASE WHEN base_sha=?3 AND ?2=?4 THEN ?2 ELSE base_sha END,head_sha=?2,last_error=NULL WHERE item_id=?1",
            params![item, head, checkpoint,default],
        )?;
        tx.execute("UPDATE work_items SET outcome=NULL WHERE id=?1", [&item])?;
        workflow::save(&tx, &item, &next)?;
        let start = plantool_contract::Start {
            thread_id: item.clone(),
            graph: plantool_contract::Graph::WorkItem,
            input: json!({"workflow":next}),
            idempotency_key: format!("clean-reconcile:{id}"),
            contract_version: plantool_contract::CONTRACT_VERSION,
            graph_version: plantool_contract::GRAPH_VERSION,
        };
        dispatcher::enqueue(&tx, &item, "worker.recover", &serde_json::to_value(start)?)?;
        database::audit(
            &tx,
            "daemon",
            "attempt.clean_reconciled",
            &id,
            &json!({"old_checkpoint":checkpoint,"checkpoint":head,"clean_worktree":path,"approved_plan":request.plan_sha,"supervisor_attempt":recommendation.attempt_id,"previous_state":s}),
        )?;
        crate::workspace::operational(&tx,&item,"Recovery queued",&format!("clean-reconcile:{id}"),"The interrupted attempt left a clean, verified checkpoint. Continuing the already-approved plan; no task completion was assumed.")?;
        tx.commit()?;
    }
    Ok(())
}

fn transient_read_failure(result: &plantool_contract::RoleResult) -> bool {
    result.outcome != plantool_contract::Outcome::Done
        && [
            "invalid_role_result",
            "Final answer is not",
            "Plan needs keyed tasks",
            "JSONDecodeError",
            "429",
            "timed out",
            "ReadTimeout",
            "workspace routing discovery",
            "No available, allowed route",
            "daemon/worker stopped",
        ]
        .iter()
        .any(|s| result.summary.contains(s))
}
/// Consume only a validated completed read-only recommendation or a known
/// transient read failure. Existing gates, uncertain effects and Stop All win.
pub fn resume_automatic(home: &Path) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }
    let db = database::open(home)?;
    let mut q=db.prepare("SELECT w.item_id FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE json_extract(w.state_json,'$.frozen')=1 AND i.deleted_at IS NULL AND i.closed_at IS NULL")?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for item in ids {
        let facts = snapshot(home, &item)?;
        if facts["eligible"] != true {
            continue;
        }
        let expected = facts["attempt_id"].as_str().unwrap_or("");
        let raw: Option<String> = db.query_row(
            "SELECT outcome_json FROM attempts WHERE id=?1",
            [expected],
            |r| r.get(0),
        )?;
        let Some(raw) = raw else { continue };
        let Ok(result) = serde_json::from_str::<plantool_contract::RoleResult>(&raw) else {
            continue;
        };
        let recommendation = result.role == "supervisor"
            && result.outcome == plantool_contract::Outcome::Done
            && matches!(&result.details,Some(plantool_contract::RoleDetails::Strategy{action,owner,..}) if action=="replan" || (action=="retry" && owner.as_deref().and_then(|o|workflow::retry_step(o,&facts["state"])).is_some()));
        if !recommendation && !transient_read_failure(&result) {
            continue;
        }
        if let Err(error) = queue_retry(home, &item, expected, true) {
            tracing::debug!(item,error=%error,"Automatic read retry deferred by current safety checks");
        }
    }
    Ok(())
}

pub fn announce_existing(home: &Path) -> anyhow::Result<()> {
    let db = database::open(home)?;
    let ids = {
        let mut q = db.prepare(
            "SELECT w.item_id FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE json_extract(w.state_json,'$.frozen')=1 AND i.deleted_at IS NULL",
        )?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for id in ids {
        crate::workspace::operational(&db,&id,"Work remains frozen",&format!("frozen:{}",workflow::state(&db,&id)?.unwrap()["sequence"]),"This item is still paused. Open it to check the failed attempt and the available recovery action. Restarting the worker alone does not resume it.")?;
    }
    let gates = {
        let mut q=db.prepare("SELECT g.id,g.item_id,g.kind,g.state,g.target_digest,coalesce(g.target_sha,'') FROM gates g JOIN work_items i ON i.id=g.item_id WHERE i.kind='eos' AND i.deleted_at IS NULL AND g.state IN('pending','open','approved','rejected','changes_requested','superseded') AND NOT EXISTS(SELECT 1 FROM messages m WHERE m.dedupe_key='ops:'||g.item_id||':gate:'||g.id||':'||g.state)")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (gate, item, kind, status, digest, sha) in gates {
        let next = if status == "pending" {
            "The App is preparing the GitHub gate. Wait for publication before approving."
        } else if status == "open" {
            "Owner action is needed on GitHub. Open the item's Approval gates tab and follow the exact current hash/commit link. A Workspace reply cannot approve it."
        } else {
            "The gate changed on the authoritative workflow. Inspect item activity for the next step; stale hashes and previous approvals must not be reused."
        };
        crate::workspace::operational(
            &db,
            &item,
            &format!("{kind} gate {status}"),
            &format!("gate:{gate}:{status}"),
            &format!("Target hash: {digest}. Commit: {sha}. {next}"),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let db = database::open(temp.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',NULL,'/checkout','eos','private','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('i','p','eos','Issue','escalated','now','p','i','{}')",[]).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('i','{\"item\":\"i\",\"step\":\"escalated\",\"frozen\":true,\"sequence\":1,\"generation\":1}','base','head')",[]).unwrap();
        db.execute("INSERT INTO item_generations VALUES('i',1)", [])
            .unwrap();
        db.execute("INSERT INTO attempts VALUES('a','i','pm',1,'route','failed','/checkout','now','now','{}','attempt','{}',0)",[]).unwrap();
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('a','{\"inputs\":{\"step\":\"triage\"},\"capability\":\"read\"}','d','i','old',0)",[]).unwrap();
        temp
    }
    #[test]
    fn clean_coding_reconciliation_preserves_approval_and_refuses_dirty_work() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        let root = t.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.com"],
        ] {
            crate::git::git(&root, &args).unwrap();
        }
        std::fs::write(root.join("README.md"), "base\n").unwrap();
        crate::git::git(&root, &["add", "."]).unwrap();
        crate::git::git(&root, &["commit", "-qm", "base"]).unwrap();
        let head = crate::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
        let tree = t.path().join("worktrees/a");
        std::fs::create_dir_all(tree.parent().unwrap()).unwrap();
        crate::git::git(
            &root,
            &[
                "clone",
                "--quiet",
                root.to_str().unwrap(),
                tree.to_str().unwrap(),
            ],
        )
        .unwrap();
        let hash = crate::artifacts::put(t.path(), b"# Plan").unwrap();
        crate::artifacts::register(&db, &hash, 6).unwrap();
        db.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) VALUES('r','i','plan',1,?1,'test','now')",[&hash]).unwrap();
        db.execute(
            "UPDATE projects SET local_checkout=?1",
            [root.to_str().unwrap()],
        )
        .unwrap();
        db.execute("UPDATE work_items SET approved_plan_revision_id='r'", [])
            .unwrap();
        db.execute("UPDATE workflow_items SET head_sha=?1,base_sha=?1,state_json=?2",params![head,json!({"item":"i","step":"escalated","frozen":true,"approved":true,"plan_sha":hash,"generation":1,"sequence":1}).to_string()]).unwrap();
        db.execute("UPDATE attempts SET generation=2,role='backend',state='recovery_needed',worktree=?1,outcome_json=NULL",[tree.to_str().unwrap()]).unwrap();
        db.execute("UPDATE item_generations SET generation=2", [])
            .unwrap();
        let prior = plantool_contract::RoleResult {
            item: "i".into(),
            role: "supervisor".into(),
            attempt_id: "prior".into(),
            generation: 1,
            outcome: plantool_contract::Outcome::Done,
            summary: "Retry backend".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: Some(plantool_contract::RoleDetails::Strategy {
                action: "retry".into(),
                owner: Some("backend".into()),
                task_key: None,
                reason: "Retry the approved work".into(),
            }),
        };
        db.execute("INSERT INTO attempts VALUES('prior','i','supervisor',1,'route','completed',?1,'now','now',?2,'prior','{}',0)",params![root.to_str().unwrap(),serde_json::to_string(&prior).unwrap()]).unwrap();
        let request = plantool_contract::Dispatch {
            item: "i".into(),
            role: "backend".into(),
            generation: 2,
            inputs: json!({"step":"backend"}),
            plan_sha: hash.clone(),
            policy_version: 1,
            capability: "write".into(),
            idempotency_key: "a".into(),
            thread_id: "i".into(),
            interrupt_id: "old".into(),
        };
        db.execute(
            "UPDATE dispatches SET request_json=?1",
            [serde_json::to_string(&request).unwrap()],
        )
        .unwrap();
        db.execute("INSERT INTO gates(id,item_id,kind,target_digest,state,created_at) VALUES('approved','i','plan',?1,'approved','now')",[&hash]).unwrap();
        std::fs::write(tree.join("README.md"), "unverified changes\n").unwrap();
        reconcile_clean_coding(t.path()).unwrap();
        assert_eq!(workflow::state(&db, "i").unwrap().unwrap()["frozen"], true);
        std::fs::write(tree.join("README.md"), "base\n").unwrap();
        reconcile_clean_coding(t.path()).unwrap();
        reconcile_clean_coding(t.path()).unwrap();
        let state = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "backend");
        assert_eq!(state["approved"], true);
        assert_eq!(state["frozen"], false);
        assert_eq!(
            db.query_row("SELECT state FROM gates WHERE id='approved'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "approved"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM task_progress WHERE status='done'",
                [],
                |r| r.get::<_, u64>(0)
            )
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
            std::fs::read_to_string(tree.join("README.md")).unwrap(),
            "base\n"
        );
    }
    #[test]
    fn completed_supervisor_retry_resumes_automatically_without_approving() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        let result = plantool_contract::RoleResult {
            item: "i".into(),
            role: "supervisor".into(),
            attempt_id: "a".into(),
            generation: 1,
            outcome: plantool_contract::Outcome::Done,
            summary: "Retry Docs".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: Some(plantool_contract::RoleDetails::Strategy {
                action: "retry".into(),
                owner: Some("Docs".into()),
                task_key: None,
                reason: "Write the documentation proposal".into(),
            }),
        };
        db.execute(
            "UPDATE attempts SET role='supervisor',state='completed',outcome_json=?1",
            [serde_json::to_string(&result).unwrap()],
        )
        .unwrap();
        db.execute("UPDATE dispatches SET request_json='{\"inputs\":{\"step\":\"supervisor\"},\"capability\":\"read\"}'",[]).unwrap();
        resume_automatic(t.path()).unwrap();
        resume_automatic(t.path()).unwrap();
        let state = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "plan");
        assert_eq!(state["frozen"], false);
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
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
            db.query_row("SELECT state FROM attempts WHERE id='a'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "completed"
        );
    }
    #[test]
    fn transient_read_failure_gets_a_timed_retry_but_uncertain_work_does_not() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        let result = plantool_contract::RoleResult {
            item: "i".into(),
            role: "pm".into(),
            attempt_id: "a".into(),
            generation: 1,
            outcome: plantool_contract::Outcome::Failed,
            summary: "Dispatch stopped: invalid_role_result".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        db.execute(
            "UPDATE attempts SET outcome_json=?1,state='recovery_needed'",
            [serde_json::to_string(&result).unwrap()],
        )
        .unwrap();
        resume_automatic(t.path()).unwrap();
        assert_eq!(workflow::state(&db, "i").unwrap().unwrap()["frozen"], true);
        db.execute("UPDATE attempts SET state='failed'", [])
            .unwrap();
        resume_automatic(t.path()).unwrap();
        let state = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "triage");
        assert_eq!(state["frozen"], false);
        assert!(state["retry_not_before"].as_i64().unwrap() > database::unix_now());
    }
    #[test]
    fn old_status_wording_does_not_block_dispatch_recovery() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        crate::workspace::operational(&db, "i", "Work remains frozen", "frozen:1", "Old wording")
            .unwrap();
        // Reproduce an upgrade replaying the same event with revised copy/title.
        db.execute(
            "UPDATE work_items SET title='Renamed issue' WHERE id='i'",
            [],
        )
        .unwrap();
        announce_existing(t.path()).unwrap();
        announce_existing(t.path()).unwrap();
        let bodies: Vec<String> = db
            .prepare("SELECT body FROM messages")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(bodies.len(), 2);
        assert!(bodies.iter().all(|body| body.contains("Old wording")));
        assert_eq!(workflow::state(&db, "i").unwrap().unwrap()["frozen"], true);
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn completed_supervisor_decision_can_be_reviewed_without_approving() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        db.execute(
            "UPDATE attempts SET role='supervisor',state='completed'",
            [],
        )
        .unwrap();
        db.execute("UPDATE dispatches SET request_json='{\"inputs\":{\"step\":\"supervisor\"},\"capability\":\"read\"}'",[]).unwrap();
        db.execute(
            "UPDATE workflow_items SET state_json=json_set(state_json,'$.supervisor_rounds',3)",
            [],
        )
        .unwrap();
        assert_eq!(snapshot(t.path(), "i").unwrap()["eligible"], true);
        retry(t.path(), "i", "a").unwrap();
        let state = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "supervisor");
        assert_eq!(state["supervisor_rounds"], 0);
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn recovery_is_audited_preserves_attempt_and_refuses_replay() {
        let t = fixture();
        assert_eq!(snapshot(t.path(), "i").unwrap()["eligible"], true);
        announce_existing(t.path()).unwrap();
        announce_existing(t.path()).unwrap();
        retry(t.path(), "i", "a").unwrap();
        assert!(retry(t.path(), "i", "a").is_err());
        let db = database::open(t.path()).unwrap();
        let s = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(s["step"], "triage");
        assert_eq!(s["sequence"], 2);
        assert_eq!(s["generation"], 1);
        assert_eq!(
            db.query_row("SELECT state FROM attempts WHERE id='a'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "failed"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4 // Each notification appears in #general and the item thread.
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn uncertain_coding_pending_effect_and_stale_attempt_are_not_recovered() {
        let t = fixture();
        assert!(retry(t.path(), "i", "stale").is_err());
        let db = database::open(t.path()).unwrap();
        for status in ["running", "recovery_needed"] {
            db.execute("UPDATE attempts SET state=?1 WHERE id='a'", [status])
                .unwrap();
            assert!(retry(t.path(), "i", "a").is_err());
        }
        db.execute("UPDATE attempts SET state='failed' WHERE id='a'", [])
            .unwrap();
        db.execute("UPDATE dispatches SET request_json='{\"inputs\":{\"step\":\"backend\"},\"capability\":\"write\"}'",[]).unwrap();
        assert!(retry(t.path(), "i", "a").is_err());
        db.execute("UPDATE dispatches SET request_json='{\"inputs\":{\"step\":\"triage\"},\"capability\":\"read\"}'",[]).unwrap();
        let tx = db.unchecked_transaction().unwrap();
        dispatcher::enqueue(&tx, "i", "github.comment", &json!({"pending":true})).unwrap();
        tx.commit().unwrap();
        assert!(retry(t.path(), "i", "a").is_err());
    }
    #[test]
    fn uncertain_acknowledgement_is_fenced_and_not_owner_retryable() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        db.execute("UPDATE attempts SET state='running'", [])
            .unwrap();
        db.execute("UPDATE workflow_items SET state_json='{\"item\":\"i\",\"step\":\"triage\",\"frozen\":false}'",[]).unwrap();
        freeze_uncertain_start(t.path(), "i").unwrap();
        freeze_uncertain_start(t.path(), "i").unwrap();
        assert_eq!(
            workflow::state(&db, "i").unwrap().unwrap()["failed_step"],
            "triage"
        );
        assert_eq!(
            db.query_row("SELECT state FROM attempts", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "recovery_needed"
        );
        assert!(retry(t.path(), "i", "a").is_err());
    }
    #[test]
    fn gate_notices_are_deduped_context_and_do_not_create_decisions() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        db.execute("INSERT INTO gates(id,item_id,kind,target_digest,state,created_at) VALUES('g','i','plan','hash','open','now')",[]).unwrap();
        announce_existing(t.path()).unwrap();
        announce_existing(t.path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM messages WHERE body LIKE '%plan gate open%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2 // One per destination, even after repeated announcement.
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn coding_quota_fallback_retains_checkpoint_head() {
        let t = fixture();
        let root = t.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        crate::git::git(&root, &["init", "-b", "main"]).unwrap();
        crate::git::git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let base = crate::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
        crate::git::git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "WIP",
            ],
        )
        .unwrap();
        let checkpoint = crate::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
        let mut db = database::open(t.path()).unwrap();
        db.execute(
            "UPDATE projects SET local_checkout=?1",
            [root.to_string_lossy()],
        )
        .unwrap();
        db.execute("UPDATE workflow_items SET base_sha=?1,head_sha=?1", [&base])
            .unwrap();
        db.execute("UPDATE workflow_items SET state_json='{\"item\":\"i\",\"step\":\"backend\",\"frozen\":false,\"sequence\":0,\"generation\":0}'",[]).unwrap();
        db.execute("UPDATE dispatches SET quota_pending=1", [])
            .unwrap();
        let result = plantool_contract::RoleResult {
            item: "i".into(),
            role: "backend".into(),
            attempt_id: "a".into(),
            generation: 1,
            outcome: plantool_contract::Outcome::NeedsFix,
            summary: "Checkpoint on quota".into(),
            tasks_done: vec![],
            artifacts: vec![plantool_contract::Artifact {
                kind: "commit".into(),
                reference: checkpoint.clone(),
            }],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        let tx = db.transaction().unwrap();
        workflow::record(t.path(), &tx, &result, &root).unwrap();
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("SELECT head_sha FROM workflow_items", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            checkpoint
        );
        assert_eq!(
            workflow::state(&db, "i").unwrap().unwrap()["step"],
            "backend"
        );
    }
    #[test]
    fn read_failure_advances_generation_without_escalation_when_fallback_is_pending() {
        let t = fixture();
        let mut db = database::open(t.path()).unwrap();
        db.execute("UPDATE workflow_items SET state_json='{\"item\":\"i\",\"step\":\"triage\",\"frozen\":false,\"sequence\":0,\"generation\":0}'",[]).unwrap();
        db.execute("UPDATE dispatches SET quota_pending=1", [])
            .unwrap();
        let result = plantool_contract::RoleResult {
            item: "i".into(),
            role: "pm".into(),
            attempt_id: "a".into(),
            generation: 1,
            outcome: plantool_contract::Outcome::Failed,
            summary: "Upstream 500".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        let tx = db.transaction().unwrap();
        workflow::record(t.path(), &tx, &result, Path::new("/checkout")).unwrap();
        tx.commit().unwrap();
        let s = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(s["step"], "triage");
        assert_eq!(s["route_retry"], true);
        assert_eq!(s["sequence"], 1);
        assert_eq!(
            db.query_row("SELECT last_error FROM workflow_items", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "Upstream 500"
        );
    }
}

pub fn announce_worker(home: &Path, available: bool) -> anyhow::Result<()> {
    let db = database::open(home)?;
    let ids = {
        let mut q = db.prepare(
            "SELECT w.item_id FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE json_extract(w.state_json,'$.project')=1 AND i.deleted_at IS NULL",
        )?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for item in ids {
        let key = format!("ops.worker:{item}");
        let previous = database::get::<Value>(&db, &key)?;
        if previous
            .as_ref()
            .is_some_and(|p| p["available"] == available)
        {
            continue;
        }
        let sequence = previous
            .as_ref()
            .and_then(|p| p["sequence"].as_u64())
            .unwrap_or(0)
            + 1;
        let tx = db.unchecked_transaction()?;
        crate::workspace::operational(
            &tx,
            &item,
            if available {
                "Worker available"
            } else {
                "Worker unavailable"
            },
            &format!("worker:{sequence}"),
            if available {
                "The worker is ready. Queued work can continue. Paused items still need the recovery action shown on each item."
            } else {
                "Open Settings → Worker to inspect status and repair if needed. Error diagnostics are in worker-state/errors.log. Personal remains available. This does not change any approval."
            },
        )?;
        database::set(
            &tx,
            &key,
            &json!({"available":available,"sequence":sequence}),
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// A checkpoint acknowledgement was lost: retain uncertainty instead of silently stalling.
pub fn freeze_uncertain_start(home: &Path, item: &str) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(mut s) = workflow::state(&tx, item)? {
        if s["closed_on_github"] == true {
            return Ok(());
        }
        if s["step"] != "escalated" {
            s["failed_step"] = s["step"].clone();
        }
        s["step"] = json!("escalated");
        s["frozen"] = json!(true);
        s["checkpoint_uncertain"] = json!(true);
        workflow::save(&tx, item, &s)?;
        tx.execute("UPDATE workflow_items SET last_error='Worker checkpoint acknowledgement is uncertain; manual reconciliation required' WHERE item_id=?1",[item])?;
        tx.execute("UPDATE attempts SET state='recovery_needed' WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle')",[item])?;
        database::audit(
            &tx,
            "daemon",
            "workflow.checkpoint_uncertain",
            item,
            &json!({"frozen":true}),
        )?;
    }
    tx.commit()?;
    Ok(())
}
