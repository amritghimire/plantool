//! Durable waits are normal work, not worker failures. No approval or write is replayed here.
use crate::{database, models::router};
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;
pub fn defer(home: &Path, attempt: &str, next: i64, reason: &str) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "UPDATE attempts SET state='queued' WHERE id=?1 AND state='starting'",
        [attempt],
    )?;
    if changed == 1 {
        tx.execute("INSERT INTO attempt_waits VALUES(?1,?2,?3,1) ON CONFLICT(attempt_id) DO UPDATE SET next_at=excluded.next_at,reason=excluded.reason,cycles=attempt_waits.cycles+1",params![attempt,next,reason])?;
        database::audit(
            &tx,
            "daemon",
            "attempt.retry_scheduled",
            attempt,
            &json!({"next_at":next,"reason":reason}),
        )?;
    }
    tx.commit()?;
    Ok(())
}
/// Dependency completion gates implementation, not research and proposal preparation.
pub fn await_dependencies(
    home: &Path,
    attempt: &str,
    item: &str,
    step: &str,
) -> anyhow::Result<bool> {
    if !matches!(
        step,
        "backend" | "frontend" | "devops" | "qa" | "reviewer" | "docs"
    ) {
        return Ok(false);
    }
    let db = database::open(home)?;
    let unfinished:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM work_item_deps d JOIN work_items i ON i.id=d.depends_on_id WHERE d.item_id=?1 AND i.state NOT IN('done','merged'))",[item],|r|r.get(0))?;
    if unfinished {
        defer(
            home,
            attempt,
            database::unix_now() + 60,
            "Waiting for a prerequisite work item to finish",
        )?;
    }
    Ok(unfinished)
}
/// A read-only role may try another route; credentials and permissions stay visible.
pub fn read_failure(
    home: &Path,
    attempt: &str,
    project: &str,
    item: &str,
    role: &str,
    selected: &router::Selected,
    error: &str,
    status: u16,
) -> anyhow::Result<bool> {
    let now = database::unix_now();
    let db = database::open(home)?;
    let detail: Value = serde_json::from_str(error).unwrap_or(Value::Null);
    let upstream = detail["detail"]["upstream_status"]
        .as_u64()
        .unwrap_or(status as u64);
    if [401, 403].contains(&upstream) {
        db.execute("UPDATE route_health SET available=0,error='Owner access required: check credentials and provider permissions' WHERE route=?1",[&selected.name])?;
    } else if [400, 422].contains(&upstream) {
        db.execute("UPDATE route_health SET available=0,error='Owner configuration required: the provider rejected this request; check model capabilities in diagnostics' WHERE route=?1",[&selected.name])?;
    } else {
        let malformed = error.contains("invalid_role_result");
        if malformed {
            let key = format!("format_failures:{item}:{role}");
            let failures = database::get::<u64>(&db, &key)?.unwrap_or(0) + 1;
            database::set(&db, &key, &failures)?;
            if failures >= 3 {
                return Ok(false);
            }
        }
        // Keep provider Retry-After if already recorded; avoid extending a short cooldown to an hour.
        let until:Option<i64>=db.query_row("SELECT max(CAST(exhausted_until AS INTEGER)) FROM budgets WHERE route=?1 AND CAST(exhausted_until AS INTEGER)>?2",params![selected.name,now],|r|r.get(0))?;
        if until.is_none() {
            router::exhaust(
                home,
                selected,
                Some(now + if upstream == 404 { 900 } else { 60 }),
                now,
            )?;
        }
    }
    let can_continue = router::select(home, project, item, role, now).is_ok()
        || router::retry_at(home, project, role, now)?.is_some();
    if can_continue {
        db.execute(
            "UPDATE dispatches SET quota_pending=1 WHERE attempt_id=?1",
            [attempt],
        )?;
    }
    Ok(can_continue)
}
pub fn next_action(db: &rusqlite::Connection, item: &str, state: &Value) -> anyhow::Result<Value> {
    use rusqlite::OptionalExtension;
    let wait:Option<(i64,String)>=db.query_row("SELECT w.next_at,w.reason FROM attempt_waits w JOIN attempts a ON a.id=w.attempt_id WHERE a.item_id=?1 AND a.state='queued' ORDER BY a.generation DESC LIMIT 1",[item],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((at, reason)) = wait {
        if reason.starts_with("Waiting for a prerequisite work item") {
            return Ok(
                json!({"kind":"prerequisite","message":"Waiting for a prerequisite issue to finish. We will check it automatically.","retry_at":at}),
            );
        }
        return Ok(
            json!({"kind":"retry","message":"Waiting for an available model. We will retry automatically.","retry_at":at,"detail":reason,"href":"/settings/models","button":"Check models"}),
        );
    }
    let project: String = db.query_row(
        "SELECT project_id FROM work_items WHERE id=?1",
        [item],
        |r| r.get(0),
    )?;
    let step = state["step"].as_str().unwrap_or("");
    if matches!(
        step,
        "plan_gate" | "research_gate" | "milestone" | "await_sprint_gate" | "pr"
    ) {
        let repo: Option<String> = db.query_row(
            "SELECT repo_full_name FROM projects WHERE id=?1",
            [&project],
            |r| r.get(0),
        )?;
        let number: Option<u64> = if step == "pr" {
            db.query_row(
                "SELECT pr_number FROM workflow_items WHERE item_id=?1",
                [item],
                |r| r.get(0),
            )?
        } else if step == "await_sprint_gate" {
            db.query_row(
                "SELECT tracking_issue_number FROM sprints WHERE id=?1",
                [state["sprint_id"].as_str().unwrap_or("")],
                |r| r.get(0),
            )
            .optional()?
            .flatten()
        } else {
            db.query_row(
                "SELECT github_issue_number FROM work_items WHERE id=?1",
                [item],
                |r| r.get(0),
            )?
        };
        if let (Some(repo), Some(number)) = (repo, number) {
            let kind = if step == "pr" { "pull" } else { "issues" };
            return Ok(
                json!({"kind":"approval","message":if step=="pr" {"Review the pull request and its checks on GitHub."}else{"Review the current proposal on GitHub."},"href":format!("https://github.com/{repo}/{kind}/{number}"),"button":if step=="pr" {"Review PR"}else{"Review proposal"}}),
            );
        }
    }
    let active:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle'))",[item],|r|r.get(0))?;
    let pending:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND kind LIKE 'worker.%' AND delivered_at IS NULL)",[item],|r|r.get(0))?;
    if !active
        && !pending
        && crate::graph_recovery::safe_step(step)
        && state["frozen"] != true
        && !matches!(
            step,
            "backlog" | "declined_wait" | "project_wait" | "await_sprint_gate"
        )
    {
        return Ok(
            json!({"kind":"stalled","message":"This step is not running. We are checking its saved progress."}),
        );
    }
    let (kind, message, href, button) = if step == "closed" || step == "merged" {
        ("done", "This item is finished.", String::new(), "")
    } else if step == "owner_question"
        || state["owner_question"].is_object()
        || crate::workspace::blocker_question(db, item, state)?.is_some()
    {
        (
            "question",
            "Reply to the question so planning can continue.",
            format!(
                "/workspace?project={}&item={}",
                percent_encoding::utf8_percent_encode(&project, percent_encoding::NON_ALPHANUMERIC),
                percent_encoding::utf8_percent_encode(item, percent_encoding::NON_ALPHANUMERIC)
            ),
            "Answer question",
        )
    } else if step == "prerequisite_wait" {
        (
            "prerequisite",
            "Waiting for a prerequisite. We will check it automatically.",
            String::new(),
            "",
        )
    } else if state["frozen"] == true {
        (
            "attention",
            "Check what failed and use the recovery action below.",
            String::new(),
            "",
        )
    } else if matches!(
        step,
        "plan_gate" | "research_gate" | "milestone" | "await_sprint_gate" | "pr"
    ) {
        (
            "approval",
            "Review the current proposal on GitHub.",
            String::new(),
            "",
        )
    } else if state["route_retry"] == true {
        (
            "fallback",
            "Trying another available model.",
            String::new(),
            "",
        )
    } else {
        (
            "progress",
            "The team will continue this work automatically.",
            String::new(),
            "",
        )
    };
    Ok(json!({"kind":kind,"message":message,"href":href,"button":button}))
}
/// Wake only verified prerequisites, after the old read-only turn has finished recording.
pub fn wake_prerequisites(home: &Path, project: &str, files_fresh: bool) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let rows = {
        let mut q=tx.prepare("SELECT w.item_id,w.state_json,p.repo_id,p.local_checkout FROM workflow_items w JOIN work_items i ON i.id=w.item_id JOIN projects p ON p.id=i.project_id WHERE i.project_id=?1 AND i.closed_at IS NULL AND json_extract(w.state_json,'$.step')='prerequisite_wait'")?;
        let rows = q
            .query_map([project], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (item, raw, repo, root) in rows {
        let mut state: Value = serde_json::from_str(&raw)?;
        let blocked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')) OR EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending')) OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind<>'github.status')",[&item],|r|r.get(0))?;
        if blocked || state["frozen"] == true {
            continue;
        }
        let value = state["prerequisite"]["value"].as_str().unwrap_or("");
        let ready = if state["prerequisite"]["kind"] == "pr" {
            tx.query_row("SELECT EXISTS(SELECT 1 FROM github_objects WHERE resource=?1 AND object_id=?2 AND json_extract(body_json,'$.merged')=1)",params![format!("{repo}:pr_state"),value],|r|r.get::<_,bool>(0))?
        } else {
            if !files_fresh {
                continue;
            }
            let path = std::path::Path::new(value);
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                || value.contains(['\0', '\n', '\r', ':', '*', '?', '[', ']'])
            {
                continue;
            }
            let root = Path::new(&root);
            let policy = crate::sandbox::NeverSent::new(
                &crate::execution::Execution::load(home)?.rules(root),
            )?;
            if policy.check(Path::new(""), path).is_err() {
                continue;
            }
            let Ok(reference) = crate::instructions::default_ref(root) else {
                continue;
            };
            crate::git::git(root, &["ls-tree", &reference, "--", value])
                .is_ok_and(|tree| tree.starts_with("100644 ") || tree.starts_with("100755 "))
        };
        if !ready {
            continue;
        }
        state["prerequisite_met"] = state["prerequisite"].clone();
        state["prerequisite"] = Value::Null;
        state["step"] = json!("supervisor");
        state["supervisor_rounds"] = json!(0);
        state["sequence"] = json!(state["sequence"].as_u64().unwrap_or(0) + 1);
        crate::workflow::save(&tx, &item, &state)?;
        let start = plantool_contract::Start {
            thread_id: item.clone(),
            graph: plantool_contract::Graph::WorkItem,
            input: json!({"workflow":state}),
            idempotency_key: format!("prerequisite:{item}:{}", state["sequence"]),
            contract_version: plantool_contract::CONTRACT_VERSION,
            graph_version: plantool_contract::GRAPH_VERSION,
        };
        crate::dispatcher::enqueue(&tx, &item, "worker.recover", &serde_json::to_value(start)?)?;
        let thread = crate::workspace::item_thread(&tx, &item)?;
        crate::workspace::append(
            &tx,
            &thread,
            "daemon",
            "The prerequisite is ready. The team will review it and continue planning.",
            &format!("prerequisite-ready:{item}:{}", state["sequence"]),
        )?;
    }
    tx.commit()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, rusqlite::Connection) {
        let home = tempfile::tempdir().unwrap();
        let db = database::open(home.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,'/checkout','eos','public','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO project_settings(project_id,version,allowed_providers_json,effective_at,set_by) VALUES('p',1,'[\"ollama\"]','now','owner')",[]).unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('i','p','eos','Issue','research','now','r','i','{}')",[]).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('i','{\"step\":\"research\",\"frozen\":false}','b','b')",[]).unwrap();
        db.execute("INSERT INTO attempts VALUES('a','i','cto',1,'first','starting','/checkout','now',NULL,NULL,'attempt','{}',0)",[]).unwrap();
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('a','{}','d','i','int',0)",[]).unwrap();
        db.execute("INSERT INTO item_generations VALUES('i',1)", [])
            .unwrap();
        let cfg:router::Chains=serde_yaml::from_str("routes:\n  first: {adapter: opencode, provider: ollama, model: first}\n  second: {adapter: opencode, provider: ollama, model: second}\nbudgets: {}\nchains: {cto: [first, second]}\ndiversity: {}\n").unwrap();
        database::set(&db, "model_chains", &cfg).unwrap();
        db.execute("INSERT INTO route_health(route,available,model) VALUES('first',1,'first'),('second',1,'second')",[]).unwrap();
        (home, db)
    }
    #[test]
    fn transient_failures_fallback_then_wait_and_resume_after_cooldown() {
        let (home, db) = setup();
        let now = database::unix_now();
        let first = router::select(home.path(), "p", "i", "cto", now).unwrap();
        assert!(read_failure(
            home.path(),
            "a",
            "p",
            "i",
            "cto",
            &first,
            r#"{"detail":{"kind":"upstream_http","upstream_status":429}}"#,
            502
        )
        .unwrap());
        let second = router::select(home.path(), "p", "i", "cto", now).unwrap();
        assert_eq!(second.name, "second");
        assert!(read_failure(
            home.path(),
            "a",
            "p",
            "i",
            "cto",
            &second,
            r#"{"detail":{"exception":"ReadTimeout"}}"#,
            502
        )
        .unwrap());
        let next = router::retry_at(home.path(), "p", "cto", now)
            .unwrap()
            .unwrap();
        assert!(next >= now + 60);
        defer(home.path(), "a", next, "Waiting for models").unwrap();
        assert_eq!(
            database::open(home.path())
                .unwrap()
                .query_row(
                    "SELECT next_at FROM attempt_waits WHERE attempt_id='a'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            next
        );
        assert!(router::select(home.path(), "p", "i", "cto", next + 1).is_ok());
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn credentials_and_permissions_need_attention() {
        let (home, db) = setup();
        let route = router::select(home.path(), "p", "i", "cto", database::unix_now()).unwrap();
        assert!(read_failure(
            home.path(),
            "a",
            "p",
            "i",
            "cto",
            &route,
            r#"{"detail":{"upstream_status":403}}"#,
            502
        )
        .unwrap());
        assert_eq!(
            db.query_row(
                "SELECT available FROM route_health WHERE route='first'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            false
        );
        let second = router::select(home.path(), "p", "i", "cto", database::unix_now()).unwrap();
        assert!(!read_failure(
            home.path(),
            "a",
            "p",
            "i",
            "cto",
            &second,
            r#"{"detail":{"upstream_status":401}}"#,
            502
        )
        .unwrap());
        assert!(
            router::retry_at(home.path(), "p", "cto", database::unix_now())
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn merged_pr_wakes_wait_once_without_approving() {
        let (home, db) = setup();
        db.execute("UPDATE attempts SET state='completed'", [])
            .unwrap();
        db.execute("UPDATE workflow_items SET state_json='{\"step\":\"prerequisite_wait\",\"frozen\":false,\"sequence\":1,\"approved\":false,\"prerequisite\":{\"kind\":\"pr\",\"value\":\"3\"}}'",[]).unwrap();
        wake_prerequisites(home.path(), "p", false).unwrap();
        assert_eq!(
            crate::workflow::state(&db, "i").unwrap().unwrap()["step"],
            "prerequisite_wait"
        );
        db.execute(
            "INSERT INTO github_objects VALUES('42:pr_state','3','{\"merged\":true}')",
            [],
        )
        .unwrap();
        wake_prerequisites(home.path(), "p", false).unwrap();
        wake_prerequisites(home.path(), "p", false).unwrap();
        let state = crate::workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "supervisor");
        assert_eq!(state["approved"], false);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn repeated_invalid_output_stops_after_three_attempts() {
        let (home, _db) = setup();
        let route = router::select(home.path(), "p", "i", "cto", database::unix_now()).unwrap();
        for _ in 0..2 {
            assert!(read_failure(
                home.path(),
                "a",
                "p",
                "i",
                "cto",
                &route,
                r#"{"detail":{"kind":"invalid_role_result"}}"#,
                502
            )
            .unwrap());
        }
        assert!(!read_failure(
            home.path(),
            "a",
            "p",
            "i",
            "cto",
            &route,
            r#"{"detail":{"kind":"invalid_role_result"}}"#,
            502
        )
        .unwrap());
    }
    #[test]
    fn restart_continues_safe_read_once_without_approving() {
        let (home, db) = setup();
        db.execute("UPDATE dispatches SET request_json='{\"capability\":\"read\",\"inputs\":{\"step\":\"research\"}}'",[]).unwrap();
        crate::dispatcher::reconcile(home.path()).unwrap();
        crate::dispatcher::reconcile(home.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT state FROM attempts WHERE id='a'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "failed"
        );
        assert_eq!(
            crate::workflow::state(&db, "i").unwrap().unwrap()["route_retry"],
            true
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.resume'",
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
        db.execute(
            "UPDATE item_generations SET generation=2 WHERE item_id='i'",
            [],
        )
        .unwrap();
        crate::workflow::recover(home.path(), "a", "late timeout from the old attempt").unwrap();
        assert_ne!(
            crate::workflow::state(&db, "i").unwrap().unwrap()["frozen"],
            true
        );
    }
    #[test]
    fn file_prerequisite_needs_fresh_default_branch_evidence() {
        let (home, db) = setup();
        let root = home.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.test"],
        ] {
            crate::git::git(&root, &args).unwrap();
        }
        std::fs::write(root.join("ready.txt"), "ready").unwrap();
        crate::git::git(&root, &["add", "ready.txt"]).unwrap();
        crate::git::git(&root, &["commit", "-m", "Fixture"]).unwrap();
        db.execute(
            "UPDATE projects SET local_checkout=?1",
            [root.to_string_lossy().as_ref()],
        )
        .unwrap();
        db.execute("UPDATE attempts SET state='completed'", [])
            .unwrap();
        db.execute("UPDATE workflow_items SET state_json='{\"step\":\"prerequisite_wait\",\"frozen\":false,\"sequence\":1,\"approved\":false,\"prerequisite\":{\"kind\":\"file\",\"value\":\"ready.txt\"}}'",[]).unwrap();
        wake_prerequisites(home.path(), "p", false).unwrap();
        assert_eq!(
            crate::workflow::state(&db, "i").unwrap().unwrap()["step"],
            "prerequisite_wait"
        );
        wake_prerequisites(home.path(), "p", true).unwrap();
        assert_eq!(
            crate::workflow::state(&db, "i").unwrap().unwrap()["step"],
            "supervisor"
        );
    }
    #[test]
    fn dependencies_wait_before_implementation_but_do_not_block_planning() {
        let (home, db) = setup();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('dependency','p','eos','Prerequisite','backlog','now','r','dependency','{}')",[]).unwrap();
        db.execute("INSERT INTO work_item_deps VALUES('i','dependency')", [])
            .unwrap();
        assert!(!await_dependencies(home.path(), "a", "i", "research").unwrap());
        assert!(await_dependencies(home.path(), "a", "i", "backend").unwrap());
        assert_eq!(
            db.query_row("SELECT state FROM attempts WHERE id='a'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "queued"
        );
        db.execute(
            "UPDATE work_items SET state='merged' WHERE id='dependency'",
            [],
        )
        .unwrap();
        assert!(!await_dependencies(home.path(), "a", "i", "backend").unwrap());
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
}
