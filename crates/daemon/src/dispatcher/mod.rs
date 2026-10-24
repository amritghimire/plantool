//! Durable dispatch admission. Graph events are claims, never authority.
use crate::{artifacts, database};
use plantool_contract::{Dispatch, DispatchReply, Outcome, RoleResult};
use rusqlite::{params, Transaction};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
pub fn item(db: &rusqlite::Connection, id: &str) -> anyhow::Result<(String, PathBuf)> {
    Ok(db.query_row("SELECT i.project_id,p.local_checkout FROM work_items i JOIN projects p ON p.id=i.project_id WHERE i.id=?1 AND i.kind='eos' AND i.deleted_at IS NULL",[id],|r|Ok((r.get(0)?,PathBuf::from(r.get::<_,String>(1)?))))?)
}
pub fn admit(
    home: &Path,
    request: &Dispatch,
    execution_generation: u64,
) -> anyhow::Result<DispatchReply> {
    let role = request.role.as_str();
    anyhow::ensure!(
        [
            "backend",
            "frontend",
            "reviewer",
            "cto",
            "pm",
            "qa",
            "devops",
            "docs",
            "supervisor"
        ]
        .contains(&role),
        "Unknown role"
    );
    let managed = crate::workflow::state(&database::open(home)?, &request.item)?.is_some();
    let discussion = crate::workflow::state(&database::open(home)?, &request.item)?
        .is_some_and(|s| s["step"] == "discussion");
    let expected = if discussion {
        "read"
    } else if managed {
        crate::workflow::capability(role)
    } else if ["backend", "frontend", "devops", "docs"].contains(&role) {
        "write"
    } else {
        "read"
    };
    anyhow::ensure!(request.capability == expected, "Role capability mismatch");
    let body = serde_json::to_value(request)?;
    let value = database::inbox_once(home, "worker.dispatch", &request.idempotency_key, |tx| {
        let (project, root) = item(tx, &request.item)?;
        let policy: u64 = tx.query_row(
            "SELECT coalesce(max(version),0) FROM project_settings WHERE project_id=?1",
            [project],
            |r| r.get(0),
        )?;
        anyhow::ensure!(policy == request.policy_version, "Stale policy version");
        let approved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE i.id=?1 AND r.content_sha256=?2 AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0)",params![request.item,request.plan_sha],|r|r.get(0))?;
        let planning = crate::workflow::check(tx, request)?;
        anyhow::ensure!(
            approved || planning,
            "Approved immutable plan required for this hash"
        );
        let deps:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_item_deps d JOIN work_items i ON i.id=d.depends_on_id WHERE d.item_id=?1 AND i.state NOT IN('done','merged'))",[&request.item],|r|r.get(0))?;
        anyhow::ensure!(!deps, "Dependencies are incomplete");
        let generation: u64 = tx.query_row(
            "SELECT coalesce((SELECT generation FROM item_generations WHERE item_id=?1),0)",
            [&request.item],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            request.generation == generation + 1,
            "Stale dispatch generation"
        );
        let unresolved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state='recovery_needed')",
            [&request.item],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            !unresolved,
            "This item has an unresolved recovery-needed attempt; owner review is required"
        );
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle'))",[&request.item],|r|r.get(0))?;
        anyhow::ensure!(!active, "Another attempt is active for this item");
        let id = format!("a_{}", uuid::Uuid::new_v4().simple());
        tx.execute("INSERT INTO item_generations VALUES(?1,?2) ON CONFLICT(item_id) DO UPDATE SET generation=excluded.generation",params![request.item,request.generation])?;
        tx.execute("INSERT INTO attempts(id,item_id,role,generation,model_route,state,worktree,started_at,idempotency_key,run_json) VALUES(?1,?2,?3,?4,'pending','queued',?5,?6,?7,'{}')",params![id,request.item,role,request.generation,root.to_string_lossy(),plantool_core::now(),request.idempotency_key])?;
        tx.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES(?1,?2,?3,?4,?5,?6)",params![id,body.to_string(),plantool_core::sha256_hex(&body.to_string()),request.thread_id,request.interrupt_id,execution_generation])?;
        enqueue(tx, &id, "attempt.dispatch", &json!({"attempt_id":id}))?;
        database::audit(tx, "worker", "attempt.authorized", &id, &body)?;
        Ok(serde_json::to_value(DispatchReply {
            attempt_id: id,
            generation: request.generation,
        })?)
    })?;
    let reply: DispatchReply = serde_json::from_value(value)?;
    // A key may replay its original request, but must never alias changed input.
    let digest: String = database::open(home)?.query_row(
        "SELECT digest FROM dispatches WHERE attempt_id=?1",
        [&reply.attempt_id],
        |r| r.get(0),
    )?;
    anyhow::ensure!(
        digest == plantool_core::sha256_hex(&body.to_string()),
        "Dispatch idempotency key reused for different input"
    );
    Ok(reply)
}
pub fn enqueue(
    tx: &Transaction<'_>,
    aggregate: &str,
    kind: &str,
    payload: &Value,
) -> anyhow::Result<String> {
    let id = plantool_core::sha256_hex(&format!("{aggregate}:{kind}:{payload}"));
    tx.execute("INSERT OR IGNORE INTO outbox(id,aggregate_id,kind,payload_json,created_at,idempotency_key) VALUES(?1,?2,?3,?4,?5,?1)",params![id,aggregate,kind,payload.to_string(),plantool_core::now()])?;
    Ok(id)
}
pub fn report(home: &Path, result: &RoleResult) -> anyhow::Result<Value> {
    let mut body = serde_json::to_value(result)?;
    let mut claim = body.clone();
    claim.as_object_mut().unwrap().remove("review_guide");
    let request_digest = plantool_core::sha256_hex(&claim.to_string());
    let db = database::open(home)?;
    let (item_id, role, generation, worktree): (String, String, u64, String) = db.query_row(
        "SELECT item_id,role,generation,worktree FROM attempts WHERE id=?1",
        [&result.attempt_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    anyhow::ensure!(
        item_id == result.item && role == result.role && generation == result.generation,
        "Result scope/generation mismatch"
    );
    if crate::workflow::state(&db, &result.item)?.is_some() && crate::workflow::coding(&result.role)
    {
        let expected: Option<String> = db.query_row(
            "SELECT git_config_sha FROM dispatches WHERE attempt_id=?1",
            [&result.attempt_id],
            |r| r.get(0),
        )?;
        if let Some(expected) = expected {
            anyhow::ensure!(
                plantool_core::sha256_hex(&std::fs::read_to_string(
                    Path::new(&worktree).join(".git/config")
                )?) == expected,
                "Attempt Git configuration was modified"
            );
        }
    }
    let warning: bool = db.query_row(
        "SELECT same_family_warning FROM dispatches WHERE attempt_id=?1",
        [&result.attempt_id],
        |r| r.get(0),
    )?;
    body["review_guide"] = json!({"same_family_warning":warning});
    for artifact in &result.artifacts {
        if artifact.kind == "commit" {
            anyhow::ensure!(
                (40..=64).contains(&artifact.reference.len())
                    && artifact.reference.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid commit reference"
            );
            crate::git::git(
                Path::new(&worktree),
                &[
                    "cat-file",
                    "-e",
                    &format!("{}^{{commit}}", artifact.reference),
                ],
            )?;
            crate::git::git(
                Path::new(&worktree),
                &["merge-base", "--is-ancestor", &artifact.reference, "HEAD"],
            )?;
        } else if let Some(sha) = artifact.reference.strip_prefix("artifact://") {
            artifacts::read(home, sha)?;
        } else {
            anyhow::bail!("Unverified result artifact")
        }
    }
    for check in &result.checks {
        let sha = check
            .evidence
            .strip_prefix("artifact://")
            .ok_or_else(|| anyhow::anyhow!("Checks require artifact evidence"))?;
        artifacts::read(home, sha)?;
        anyhow::ensure!(
            ["pass", "fail", "skipped"].contains(&check.status.as_str()),
            "Unknown check status"
        );
    }
    for finding in &result.findings {
        anyhow::ensure!(
            ["high", "medium", "low"].contains(&finding.severity.as_str())
                && ["in", "out"].contains(&finding.scope.as_str()),
            "Invalid finding classification"
        );
    }
    crate::workflow::validate_result(home, result, Path::new(&worktree))?;
    database::inbox_once(home,"attempt.result",&format!("{}:{}",result.attempt_id,result.generation),|tx|{
        let current:u64=tx.query_row("SELECT generation FROM item_generations WHERE item_id=?1",[&result.item],|r|r.get(0))?;
        anyhow::ensure!(current==result.generation,"Stale result generation");
        let status=if matches!(result.outcome,Outcome::Failed){"failed"}else{"completed"};
        tx.execute("UPDATE attempts SET state=?2,outcome_json=?3,ended_at=?4 WHERE id=?1",params![result.attempt_id,status,body.to_string(),plantool_core::now()])?;
        if !result.tasks_done.is_empty(){
            let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id JOIN dispatches d ON d.attempt_id=?2 WHERE i.id=?1 AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0 AND r.content_sha256=json_extract(d.request_json,'$.plan_sha'))",params![result.item,result.attempt_id],|r|r.get(0))?;
            anyhow::ensure!(valid,"Plan scope changed; task progress needs fresh approval");
        }
        for key in &result.tasks_done {
            if let Some(s)=crate::workflow::state(tx,&result.item)? {anyhow::ensure!(crate::oversight::active_key(&s,key),"Task is outside the current phase");}
            let approved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_items i JOIN plan_tasks t ON t.revision_id=i.approved_plan_revision_id WHERE i.id=?1 AND t.task_key=?2)",params![result.item,key],|r|r.get(0))?;
            anyhow::ensure!(approved,"Result references an unapproved task");
            if crate::workflow::state(tx,&result.item)?.is_some() {
                let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_items i JOIN plan_tasks t ON t.revision_id=i.approved_plan_revision_id WHERE i.id=?1 AND t.task_key=?2 AND t.area=?3)",params![result.item,key,result.role],|r|r.get(0))?;
                anyhow::ensure!(owned,"Role cannot complete another area's task");
            }
            tx.execute("INSERT INTO task_progress VALUES(?1,?2,'done',?3,?4) ON CONFLICT(item_id,task_key) DO UPDATE SET status='done',updated_by_attempt=excluded.updated_by_attempt,updated_at=excluded.updated_at",params![result.item,key,result.attempt_id,plantool_core::now()])?;
        }
        crate::workflow::record(home,tx,result,Path::new(&worktree))?;
        let (thread,interrupt):(String,String)=tx.query_row("SELECT thread_id,interrupt_id FROM dispatches WHERE attempt_id=?1",[&result.attempt_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let resume=plantool_contract::Resume{thread_id:thread,interrupt_id:interrupt,value:body.clone(),idempotency_key:format!("result:{}:{}",result.attempt_id,result.generation),contract_version:plantool_contract::CONTRACT_VERSION,graph_version:plantool_contract::GRAPH_VERSION};
        enqueue(tx,&result.item,"worker.resume",&serde_json::to_value(resume)?)?;
        database::audit(tx,"agent","attempt.result",&result.attempt_id,&body)?;
        Ok(json!({"accepted":true,"digest":request_digest}))
    }).and_then(|receipt|{anyhow::ensure!(receipt["digest"]==request_digest,"Result replay changed payload");Ok(receipt)})
}
/// Read a process start stamp without signalling it or reading its environment.
pub fn process_stamp(pid: u32) -> anyhow::Result<String> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()?;
    anyhow::ensure!(out.status.success(), "Process is gone");
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}
pub fn reconcile(home: &Path) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let rows = {
        let mut q=db.prepare("SELECT a.id,a.item_id,a.role,a.generation,a.outcome_json,d.pid,d.process_started FROM attempts a JOIN dispatches d ON d.attempt_id=a.id WHERE a.state IN('starting','running','waiting','idle')")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<u32>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, item, role, generation, outcome, pid, stamp) in rows {
        let _ = (pid, stamp); // Recovered PIDs are inspected by supervision, never signalled.
        if let Some(result) = outcome.and_then(|s| serde_json::from_str::<RoleResult>(&s).ok()) {
            report(home, &result)?;
        } else {
            let result = RoleResult {
                item: item.clone(),
                role,
                attempt_id: id.clone(),
                generation,
                outcome: Outcome::Failed,
                summary: "Recovery needed: the daemon/worker stopped without a verified outcome"
                    .into(),
                tasks_done: vec![],
                artifacts: vec![],
                checks: vec![],
                findings: vec![],
                next: None,
                review_guide: None,
                details: None,
            };
            report(home, &result)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "UPDATE attempts SET state='recovery_needed' WHERE id=?1",
                [&id],
            )?;
            tx.execute(
                "UPDATE work_items SET outcome='recovery_needed' WHERE id=?1",
                [item],
            )?;
            database::audit(&tx, "daemon", "attempt.recovery_needed", &id, &json!({}))?;
            tx.commit()?;
        }
    }
    db.execute("UPDATE attempts SET state='recovery_needed',ended_at=?1 WHERE id IN(SELECT attempt_id FROM proxy_calls WHERE state='pending') AND state='running'",[plantool_core::now()])?;
    db.execute("UPDATE work_items SET outcome='recovery_needed' WHERE id IN(SELECT item_id FROM attempts WHERE state='recovery_needed')",[])?;
    db.execute(
        "UPDATE proxy_calls SET state='recovery_needed' WHERE state='pending'",
        [],
    )?;
    // Leases are never inherited by a replacement process.
    db.execute("DELETE FROM leases", [])?;
    db.execute("DELETE FROM model_queue", [])?;
    Ok(())
}
pub mod runtime;
/// Deliver outside the transaction, then acknowledge. A crash after delivery
/// reuses the same receiver idempotency key; never invent a replacement key.
pub async fn deliver_pending<F, Fut>(
    home: &Path,
    kinds: &[&str],
    mut deliver: F,
) -> anyhow::Result<usize>
where
    F: FnMut(String, Value) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Value>>,
{
    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT id,kind,payload_json FROM outbox WHERE delivered_at IS NULL AND next_attempt_at<=?2 AND kind IN(SELECT value FROM json_each(?1)) ORDER BY CASE kind WHEN 'worker.cancel' THEN 0 WHEN 'worker.start' THEN 1 ELSE 2 END,created_at,id LIMIT 128")?;
        let rows = q
            .query_map(
                params![serde_json::to_string(kinds)?, database::unix_now()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut count = 0;
    for (id, kind, body) in rows {
        if !kinds.contains(&kind.as_str()) {
            continue;
        }
        database::open(home)?
            .execute("UPDATE outbox SET attempts=attempts+1,next_attempt_at=?2+min(3600,(1 << min(attempts,10))) WHERE id=?1", params![id,database::unix_now()])?;
        let result = deliver(kind, serde_json::from_str(&body)?).await?;
        database::finish_intent(home, &id, &result)?;
        count += 1;
    }
    Ok(count)
}
