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
        if result.role == "supervisor"
            && (result.outcome == plantool_contract::Outcome::Done
                || (result.outcome == plantool_contract::Outcome::Blocked
                    && matches!(
                        &result.details,
                        Some(plantool_contract::RoleDetails::Strategy { action, .. })
                            if action == "escalate"
                    )
                    && retryable_approved_qa_output(&tx, item, &facts["state"], expected)?))
        {
            let Some(plantool_contract::RoleDetails::Strategy { action, owner, .. }) =
                &result.details
            else {
                anyhow::bail!("No actionable completed recommendation")
            };
            if action == "replan" {
                state["step"] = json!("plan");
                state["approved"] = json!(false);
            } else if action == "escalate"
                && retryable_plan_review_evidence(&tx, item, &facts["state"], expected)?
            {
                // A contract-shape rejection is not a product decision. Retry this
                // read-only review once with the corrected schema; the plan still
                // reaches its normal owner approval gate if the review succeeds.
                state["step"] = json!("plan_review");
                state["approved"] = json!(false);
                state["automatic_retries"] = json!(1);
                state["retry_not_before"] = Value::Null;
                state["supervisor_rounds"] = json!(0);
            } else if action == "escalate"
                && retryable_approved_qa_output(&tx, item, &facts["state"], expected)?
            {
                // An escalation based on a missing typed QA result is not a
                // decision about the approved implementation. Retry the
                // read-only verification once, under the same plan and commit.
                // Do not count the malformed response as QA evidence.
                state["step"] = json!("qa");
                state["qa_output_retries"] = json!(1);
                state["retry_not_before"] = Value::Null;
                state["supervisor_rounds"] = json!(0);
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

/// A completed Supervisor can carry a stale infrastructure narrative. If its
/// latest underlying QA attempt produced no typed result, retry that read-only
/// step once only when the exact implementation plan remains approved and
/// there are no recovery blockers (checked by `snapshot`/`queue_retry`).
fn retryable_approved_qa_output(
    db: &rusqlite::Connection,
    item: &str,
    state: &Value,
    supervisor_attempt: &str,
) -> anyhow::Result<bool> {
    if state["step"] != "escalated"
        || state["approved"] != true
        || state["qa_output_retries"].as_u64().unwrap_or(0) >= 1
        || state["plan_sha"].as_str().is_none()
        || state["checkpoint_uncertain"] == true
        || state["closed_on_github"] == true
    {
        return Ok(false);
    }
    let plan_sha = state["plan_sha"].as_str().unwrap();
    // Honor the per-step retry bound across upgrades too: older builds recorded
    // this exact retry only in the append-only audit log.
    let retried: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM audit WHERE subject=?1 AND action='workflow.retry'
           AND json_extract(metadata_json,'$.automatic')=1
           AND json_extract(metadata_json,'$.step')='qa'
           AND json_extract(metadata_json,'$.previous_state.plan_sha')=?2)",
        params![item, plan_sha],
        |row| row.get(0),
    )?;
    if retried {
        return Ok(false);
    }
    let eligible: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id
           WHERE i.id=?1 AND i.deleted_at IS NULL AND i.closed_at IS NULL
             AND i.state NOT IN('closed','merged') AND r.doc_kind='plan'
             AND r.content_sha256=?2
         )
         AND EXISTS(
           SELECT 1 FROM attempts qa JOIN dispatches qd ON qd.attempt_id=qa.id
           WHERE qa.item_id=?1 AND qa.role='qa' AND qa.state='completed'
             AND qa.generation=(
               SELECT max(previous.generation) FROM attempts previous
               WHERE previous.item_id=?1 AND previous.role='qa'
                 AND previous.generation<(SELECT supervisor.generation FROM attempts supervisor
                   WHERE supervisor.id=?3 AND supervisor.item_id=?1
                     AND supervisor.role='supervisor' AND supervisor.state='completed')
             )
             AND json_extract(qd.request_json,'$.inputs.step')='qa'
             AND json_extract(qd.request_json,'$.capability')='tests'
             AND json_extract(qd.request_json,'$.plan_sha')=?2
             AND json_extract(qa.outcome_json,'$.outcome')='needs_fix'
             AND json_extract(qa.outcome_json,'$.summary') IN(
               'Provider turn completed without a submitted typed result; checkpoint required',
               'Provider exited without a typed result; review its transcript before retrying'
             )
         )
         AND EXISTS(
           SELECT 1 FROM dispatches sd WHERE sd.attempt_id=?3
             AND json_extract(sd.request_json,'$.inputs.step')='supervisor'
             AND json_extract(sd.request_json,'$.capability')='read'
             AND json_extract(sd.request_json,'$.plan_sha')=?2
         )",
        params![item, plan_sha, supervisor_attempt],
        |row| row.get(0),
    )?;
    Ok(eligible)
}

/// Recover one known plan-review contract defect after a Supervisor escalated it.
/// This is safe only for an unapproved, current plan and a failed read-only review;
/// it never accepts the review or changes plan approval.
fn retryable_plan_review_evidence(
    db: &rusqlite::Connection,
    item: &str,
    state: &Value,
    supervisor_attempt: &str,
) -> anyhow::Result<bool> {
    if state["step"] != "escalated"
        || state["failed_step"] != "plan_review"
        || state["approved"] == true
        || state["automatic_retries"].as_u64().unwrap_or(0) >= 1
        || state["plan_sha"].as_str().is_none()
        || state["checkpoint_uncertain"] == true
        || state["closed_on_github"] == true
    {
        return Ok(false);
    }
    let plan_sha = state["plan_sha"].as_str().unwrap();
    let exists: bool = db.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM work_items i JOIN revisions r ON r.item_id=i.id
           WHERE i.id=?1 AND i.approved_plan_revision_id IS NULL
             AND i.deleted_at IS NULL AND i.closed_at IS NULL
             AND i.state NOT IN('closed','merged') AND r.doc_kind='plan'
             AND r.content_sha256=?2
         )
         AND EXISTS(
           SELECT 1 FROM attempts a JOIN dispatches d ON d.attempt_id=a.id
           WHERE a.item_id=?1 AND a.role='cto' AND a.state='failed'
             AND a.generation=(
               SELECT max(previous.generation) FROM attempts previous
               JOIN dispatches pd ON pd.attempt_id=previous.id
               WHERE previous.item_id=?1 AND previous.role='cto'
                 AND previous.generation<(SELECT supervisor.generation FROM attempts supervisor WHERE supervisor.id=?3 AND supervisor.item_id=?1 AND supervisor.role='supervisor' AND supervisor.state='completed')
                 AND json_extract(pd.request_json,'$.inputs.step')='plan_review'
                 AND json_extract(pd.request_json,'$.plan_sha')=?2
             )
             AND json_extract(d.request_json,'$.inputs.step')='plan_review'
             AND json_extract(d.request_json,'$.capability')='read'
             AND json_extract(d.request_json,'$.plan_sha')=?2
             AND json_extract(a.outcome_json,'$.summary') LIKE '%Checks require artifact evidence%'
         )
         AND EXISTS(
           SELECT 1 FROM attempts supervisor JOIN dispatches sd ON sd.attempt_id=supervisor.id
           WHERE supervisor.id=?3 AND supervisor.item_id=?1 AND supervisor.role='supervisor'
             AND supervisor.state='completed'
             AND json_extract(sd.request_json,'$.inputs.step')='supervisor'
             AND json_extract(sd.request_json,'$.capability')='read'
             AND json_extract(sd.request_json,'$.plan_sha')=?2
         )",
        params![item, plan_sha, supervisor_attempt],
        |row| row.get(0),
    )?;
    Ok(exists)
}

/// Reconcile an interrupted approved coding attempt only when all effects are
/// observed: no live process/proxy/action, a clean owned worktree, and either the
/// recorded checkpoint or a fast-forward to the synchronized default branch.
/// A completed Supervisor retry is required; no task completion is fabricated.
pub fn reconcile_clean_coding(home: &Path) -> anyhow::Result<()> {
    use plantool_contract::{Dispatch, Outcome, RoleDetails, RoleResult};
    let db = database::open(home)?;
    let mut q=db.prepare("SELECT a.id,a.item_id,a.generation,a.worktree,d.request_json,w.state_json,w.head_sha FROM attempts a JOIN dispatches d ON d.attempt_id=a.id JOIN workflow_items w ON w.item_id=a.item_id WHERE a.state='recovery_needed' AND a.role IN('backend','frontend','devops','qa') AND (a.outcome_json IS NULL OR (json_extract(a.outcome_json,'$.outcome')='failed' AND json_extract(a.outcome_json,'$.summary')='Recovery needed: the daemon/worker stopped without a verified outcome')) AND NOT EXISTS(SELECT 1 FROM attempts later WHERE later.item_id=a.item_id AND later.generation>a.generation)")?;
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
        if request.role == "qa"
            && request.capability == "tests"
            && request.inputs["step"] == "qa"
            && s["failed_step"] == "qa"
            && reconcile_interrupted_qa(home, &id, &item, generation, &request, &s)?
        {
            continue;
        }
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
        let blocked:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM supervised_processes WHERE id=?1) OR EXISTS(SELECT 1 FROM proxy_calls WHERE attempt_id=?1 AND state='pending') OR EXISTS(SELECT 1 FROM attempts WHERE item_id=?2 AND state IN('queued','starting','running','waiting','idle')) OR EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?2 AND state='running') OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?2 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status') OR EXISTS(SELECT 1 FROM gates WHERE item_id=?2 AND state IN('pending','open'))",params![id,item],|r|r.get(0))?;
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
        let (_, root) = dispatcher::item(&db, &item)?;
        let reference = crate::instructions::default_ref(&root)?;
        let default = crate::git::git(&root, &["rev-parse", &reference])?;
        let rules = crate::execution::Execution::load(home)?.rules(&crate::git::main_root(&root));
        let policy = crate::sandbox::NeverSent::new(&rules)?;
        let Some(head) = observed_checkpoint(&path, &checkpoint, &default, &s, &policy)? else {
            continue;
        };
        let based_on_current_default =
            crate::git::git(&path, &["merge-base", "--is-ancestor", &default, &head]).is_ok();
        if request.role == "qa" && head != checkpoint && head != default {
            continue;
        }
        let mut write = database::open(home)?;
        let tx = write.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if workflow::state(&tx, &item)? != Some(s.clone()) {
            continue;
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND state='recovery_needed' AND (outcome_json IS NULL OR (json_extract(outcome_json,'$.outcome')='failed' AND json_extract(outcome_json,'$.summary')='Recovery needed: the daemon/worker stopped without a verified outcome'))) AND NOT EXISTS(SELECT 1 FROM attempts WHERE item_id=?2 AND state IN('queued','starting','running','waiting','idle')) AND NOT EXISTS(SELECT 1 FROM supervised_processes WHERE id=?1)",params![id,item],|r|r.get(0))?;
        if !valid {
            continue;
        }
        let still_safe:bool=tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM proxy_calls WHERE attempt_id=?1 AND state='pending') AND NOT EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?2 AND state='running') AND NOT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?2 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status') AND NOT EXISTS(SELECT 1 FROM gates WHERE item_id=?2 AND state IN('pending','open')) AND EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE i.id=?2 AND r.content_sha256=?3)",params![id,item,request.plan_sha],|r|r.get(0))?;
        if !still_safe {
            continue;
        }
        let mut next = s.clone();
        next["step"] = request.inputs["step"].clone();
        next["frozen"] = json!(false);
        next["generation"] = json!(generation);
        next["head_sha"] = json!(head);
        next["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
        let result=RoleResult{item:item.clone(),role:request.role.clone(),attempt_id:id.clone(),generation,outcome:Outcome::Failed,summary:"Interrupted attempt reconciled at an observed approved-scope checkpoint. No task or check completion was accepted.".into(),tasks_done:vec![],artifacts:vec![],checks:vec![],findings:vec![],next:None,review_guide:None,details:None};
        // These are pure inference replies, never GitHub writes or tool actions.
        // Preserve their receipts as failed and refuse reuse of their idempotency keys.
        tx.execute(
            "UPDATE proxy_calls SET state='failed' WHERE attempt_id=?1 AND state='recovery_needed'",
            [&id],
        )?;
        tx.execute(
            "UPDATE attempts SET state='failed',ended_at=?2,outcome_json=?3 WHERE id=?1",
            params![id, plantool_core::now(), serde_json::to_string(&result)?],
        )?;
        tx.execute(
            "UPDATE workflow_items SET base_sha=CASE WHEN ?3 THEN ?4 ELSE base_sha END,head_sha=?2,last_error=NULL WHERE item_id=?1",
            params![item, head, based_on_current_default, default],
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
            &json!({"old_checkpoint":checkpoint,"checkpoint":head,"preserved_worktree":path,"approved_plan":request.plan_sha,"supervisor_attempt":recommendation.attempt_id,"previous_state":s}),
        )?;
        crate::workspace::operational(&tx,&item,"Recovery queued",&format!("clean-reconcile:{id}"),"The interrupted attempt left a verified checkpoint. Its worktree and evidence are preserved. Continuing verification under the existing approval; no task completion was assumed.")?;
        tx.commit()?;
    }
    Ok(())
}

/// Resume an interrupted QA step only when its exact approved implementation
/// commit is already recorded and present in a clean project checkout. The
/// interrupted QA result is marked failed; no test evidence or completion is
/// accepted, and the next attempt gets a fresh idempotency key.
fn reconcile_interrupted_qa(
    home: &Path,
    attempt: &str,
    item: &str,
    generation: u64,
    request: &plantool_contract::Dispatch,
    state: &Value,
) -> anyhow::Result<bool> {
    use plantool_contract::{Outcome, RoleResult};
    if state["approved"] != true
        || state["checkpoint_uncertain"] == true
        || state["closed_on_github"] == true
        || state["frozen"] == true
    {
        return Ok(false);
    }
    let db = database::open(home)?;
    // Use the exact commit from the latest accepted implementation receipt when
    // older workflow projections omit their head. A workflow_items.head_sha can
    // still be the base commit after a lost event, so it is not sufficient proof
    // of which code QA should inspect.
    let (backend_outcome, backend_request): (String, String) = match db
        .query_row(
            "SELECT a.outcome_json,d.request_json FROM attempts a JOIN dispatches d ON d.attempt_id=a.id
             WHERE a.item_id=?1 AND a.generation<?2 AND a.state='completed'
               AND a.role IN('backend','frontend','devops')
             ORDER BY a.generation DESC LIMIT 1",
            params![item, generation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
    {
        Some(receipt) => receipt,
        None => return Ok(false),
    };
    let backend: RoleResult = serde_json::from_str(&backend_outcome)?;
    let backend_request: plantool_contract::Dispatch = serde_json::from_str(&backend_request)?;
    if backend.outcome != Outcome::Done || backend_request.plan_sha != request.plan_sha {
        return Ok(false);
    }
    let Some(backend_checkpoint) = backend
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "commit")
        .map(|artifact| artifact.reference.as_str())
    else {
        return Ok(false);
    };
    let checkpoint = state["head_sha"]
        .as_str()
        .filter(|sha| !sha.is_empty())
        .unwrap_or(backend_checkpoint);
    if checkpoint != backend_checkpoint {
        // A conflicting head needs explicit reconciliation; never guess which
        // completed attempt represents the approved implementation.
        return Ok(false);
    }
    let checkpoint = checkpoint.to_owned();
    let (_, root) = dispatcher::item(&db, item)?;
    let facts: Option<(String, bool, bool, bool, bool)> = db
        .query_row(
            "SELECT a.worktree,
               EXISTS(SELECT 1 FROM attempts WHERE item_id=?2 AND id<>?1 AND state IN('queued','starting','running','waiting','idle')),
               EXISTS(SELECT 1 FROM supervised_processes WHERE id=?1)
                 OR EXISTS(SELECT 1 FROM proxy_calls WHERE attempt_id=?1 AND state='pending')
                 OR EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?2 AND state='running')
                 OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?2 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status'),
               EXISTS(SELECT 1 FROM gates WHERE item_id=?2 AND state IN('open','pending')),
               EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id
                      WHERE i.id=?2 AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0
                        AND r.content_sha256=?3 AND r.content_sha256=json_extract(?4,'$.plan_sha'))
             FROM attempts a WHERE a.id=?1 AND a.item_id=?2 AND a.state='recovery_needed'
               AND a.role='qa' AND a.generation=?5",
        params![attempt, item, request.plan_sha, state.to_string(), generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((recorded_worktree, active, effects, gate, approved)) = facts else {
        return Ok(false);
    };
    if active || effects || gate || !approved {
        return Ok(false);
    }
    let root = root.canonicalize()?;
    if Path::new(&recorded_worktree).canonicalize().ok().as_deref() != Some(root.as_path())
        || !crate::git::git(&root, &["status", "--porcelain", "--untracked-files=all"])?.is_empty()
    {
        return Ok(false);
    }
    crate::git::git(
        &root,
        &["cat-file", "-e", &format!("{checkpoint}^{{commit}}")],
    )?;
    let default_ref = crate::instructions::default_ref(&root)?;
    let default = crate::git::git(&root, &["rev-parse", &default_ref])?;
    let on_current_base = crate::git::git(
        &root,
        &["merge-base", "--is-ancestor", &default, &checkpoint],
    )
    .is_ok();
    let mut approved_base_for_reapply = None;
    let reapply_on_current_base = if on_current_base {
        false
    } else {
        let (approved_base, current_head): (String, String) = db.query_row(
            "SELECT base_sha,head_sha FROM workflow_items WHERE item_id=?1",
            [item],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let already_reapplied: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM audit WHERE subject=?1
               AND action='attempt.qa_stale_base_reapplied'
               AND json_extract(metadata_json,'$.approved_plan')=?2)",
            params![item, request.plan_sha],
            |row| row.get(0),
        )?;
        if already_reapplied
            || approved_base.is_empty()
            || current_head != default
            || crate::git::git(
                &root,
                &["merge-base", "--is-ancestor", &approved_base, &default],
            )
            .is_err()
            || crate::git::git(
                &root,
                &["merge-base", "--is-ancestor", &approved_base, &checkpoint],
            )
            .is_err()
        {
            return Ok(false);
        }
        approved_base_for_reapply = Some(approved_base);
        true
    };
    let rules = crate::execution::Execution::load(home)?.rules(&root);
    let never_sent = crate::sandbox::NeverSent::new(&rules)?;
    let changed = if reapply_on_current_base {
        crate::git::git(
            &root,
            &[
                "diff",
                "--name-only",
                approved_base_for_reapply.as_deref().unwrap(),
                &checkpoint,
            ],
        )?
    } else {
        crate::git::git(&root, &["diff", "--name-only", &default, &checkpoint])?
    };
    if changed.is_empty() {
        return Ok(false);
    }
    for file in changed.lines() {
        let in_scope = state["affected_files"].as_array().is_some_and(|patterns| {
            patterns.iter().filter_map(Value::as_str).any(|pattern| {
                file == pattern
                    || pattern
                        .strip_suffix("/**")
                        .is_some_and(|prefix| file.starts_with(&format!("{prefix}/")))
            })
        });
        if !in_scope || never_sent.check(&root, &root.join(file)).is_err() {
            return Ok(false);
        }
    }
    let mut conn = database::open(home)?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if workflow::state(&tx, item)? != Some(state.clone()) {
        return Ok(false);
    }
    let still_safe: bool = tx.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM attempts WHERE item_id=?2 AND id<>?1 AND state IN('queued','starting','running','waiting','idle'))
           AND NOT EXISTS(SELECT 1 FROM proxy_calls WHERE attempt_id=?1 AND state='pending')
           AND NOT EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?2 AND state='running')
           AND NOT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?2 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status')
           AND NOT EXISTS(SELECT 1 FROM gates WHERE item_id=?2 AND state IN('open','pending'))
           AND EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE i.id=?2 AND r.content_sha256=?3 AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0)",
        params![attempt, item, request.plan_sha],
        |r| r.get(0),
    )?;
    if !still_safe {
        return Ok(false);
    }
    if reapply_on_current_base {
        let still_current: bool = tx.query_row(
            "SELECT base_sha=?2 AND head_sha=?3 FROM workflow_items WHERE item_id=?1",
            params![item, approved_base_for_reapply.as_deref().unwrap(), default],
            |row| row.get(0),
        )?;
        if !still_current {
            return Ok(false);
        }
    }
    let result = RoleResult {
        item: item.into(),
        role: "qa".into(),
        attempt_id: attempt.into(),
        generation,
        outcome: Outcome::Failed,
        summary: if reapply_on_current_base {
            "Interrupted QA result was not accepted. The approved change is based on an older default, so implementation is being reapplied on the current default before QA.".into()
        } else {
            "Interrupted QA attempt reconciled; no test result was accepted.".into()
        },
        tasks_done: vec![],
        artifacts: vec![],
        checks: vec![],
        findings: vec![],
        next: None,
        review_guide: None,
        details: None,
    };
    tx.execute(
        "UPDATE proxy_calls SET state='failed' WHERE attempt_id=?1 AND state='recovery_needed'",
        [attempt],
    )?;
    tx.execute("UPDATE attempts SET state='failed',ended_at=?2,outcome_json=?3 WHERE id=?1 AND state='recovery_needed'", params![attempt, plantool_core::now(), serde_json::to_string(&result)?])?;
    let mut next = state.clone();
    next["step"] = json!(if reapply_on_current_base {
        "backend"
    } else {
        "qa"
    });
    next["head_sha"] = json!(if reapply_on_current_base {
        &default
    } else {
        &checkpoint
    });
    if reapply_on_current_base {
        next["stale_base_reapplies"] = json!(1);
    }
    next["sequence"] = json!(state["sequence"].as_u64().unwrap_or(0) + 1);
    next["generation"] = json!(generation);
    workflow::save(&tx, item, &next)?;
    if reapply_on_current_base {
        tx.execute(
            "UPDATE workflow_items SET base_sha=?2,head_sha=?2,last_error=NULL WHERE item_id=?1",
            params![item, default],
        )?;
    } else {
        tx.execute(
            "UPDATE workflow_items SET head_sha=?2,last_error=NULL WHERE item_id=?1",
            params![item, checkpoint],
        )?;
    }
    tx.execute(
        "UPDATE outbox SET delivered_at=?2,result_json=?3 WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind IN('worker.start','worker.resume','worker.cancel','worker.recover')",
        params![item, plantool_core::now(), json!({"superseded_by_safe_qa_recovery":true}).to_string()],
    )?;
    let start = plantool_contract::Start {
        thread_id: item.into(),
        graph: plantool_contract::Graph::WorkItem,
        input: json!({"workflow":next}),
        idempotency_key: if reapply_on_current_base {
            format!("qa-stale-base-reapply:{attempt}")
        } else {
            format!("qa-reconcile:{attempt}")
        },
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, item, "worker.recover", &serde_json::to_value(start)?)?;
    database::audit(
        &tx,
        "daemon",
        if reapply_on_current_base {
            "attempt.qa_stale_base_reapplied"
        } else {
            "attempt.qa_reconciled"
        },
        attempt,
        &json!({"checkpoint":checkpoint,"current_default":default,"approved_plan":request.plan_sha,"previous_state":state}),
    )?;
    crate::workspace::operational(
        &tx,
        item,
        if reapply_on_current_base {
            "Implementation resumed"
        } else {
            "QA resumed"
        },
        &format!("qa-reconcile:{attempt}"),
        if reapply_on_current_base {
            "The old QA run did not produce a result. The approved change is being reapplied to the current default before checks run."
        } else {
            "The approved implementation is intact. The interrupted QA run was not counted; verification is starting again."
        },
    )?;
    tx.commit()?;
    Ok(true)
}

/// Adopt only observed committed state, never uncommitted source edits. Generated
/// evidence remains untouched in the old worktree; it is not accepted as a passed check.
fn observed_checkpoint(
    path: &Path,
    checkpoint: &str,
    default: &str,
    state: &Value,
    policy: &crate::sandbox::NeverSent,
) -> anyhow::Result<Option<String>> {
    if !crate::git::git(path, &["diff", "--no-ext-diff", "--name-only", "HEAD"])?.is_empty() {
        return Ok(None);
    }
    let untracked = crate::git::git(path, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    if untracked.split('\0').filter(|p| !p.is_empty()).any(|file| {
        let p = Path::new(file);
        p.components().count() != 1
            || !(file.ends_with(".log") || file == "result.json")
            || std::fs::symlink_metadata(path.join(p)).map_or(true, |m| {
                !m.is_file() || m.file_type().is_symlink() || m.len() > 16 * 1024 * 1024
            })
    }) {
        return Ok(None);
    }
    let head = crate::git::git(path, &["rev-parse", "HEAD"])?;
    if head == checkpoint {
        return Ok(Some(head));
    }
    if crate::git::git(path, &["merge-base", "--is-ancestor", checkpoint, &head]).is_err() {
        return Ok(None);
    }
    if head == default {
        return Ok(Some(head));
    }
    // Older workflow projections can retain a pre-merge base while an
    // interrupted commit was created on the current default. In that case,
    // validate only the change introduced after the current default; otherwise
    // unrelated merged prerequisite files make a safe checkpoint look out of
    // scope and strand the item. The caller still checks every changed path
    // against the approved plan and never accepts task/check completion here.
    let comparison_base =
        if crate::git::git(path, &["merge-base", "--is-ancestor", default, &head]).is_ok() {
            default
        } else {
            checkpoint
        };
    let files = crate::git::git(
        path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            &comparison_base,
            &head,
        ],
    )?;
    if files.is_empty() {
        return Ok(None);
    }
    for file in files.lines() {
        let allowed = state["affected_files"].as_array().is_some_and(|patterns| {
            patterns.iter().filter_map(Value::as_str).any(|p| {
                file == p
                    || p.strip_suffix("/**")
                        .is_some_and(|prefix| file.starts_with(&format!("{prefix}/")))
            })
        });
        if !allowed || policy.check(path, &path.join(file)).is_err() {
            return Ok(None);
        }
    }
    Ok(Some(head))
}

fn transient_read_failure(result: &plantool_contract::RoleResult) -> bool {
    result.outcome != plantool_contract::Outcome::Done
        && [
            "invalid_role_result",
            "Final answer is not",
            "Plan needs keyed tasks",
            "Loop roles are read-only",
            "Checks require artifact evidence",
            "Review finding must be routed exactly once",
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
        let plan_review_evidence_recovery = result.role == "supervisor"
            && result.outcome == plantool_contract::Outcome::Done
            && matches!(&result.details, Some(plantool_contract::RoleDetails::Strategy { action, .. }) if action == "escalate")
            && retryable_plan_review_evidence(&db, &item, &facts["state"], expected)?;
        let approved_qa_output_recovery = result.role == "supervisor"
            && matches!(
                result.outcome,
                plantool_contract::Outcome::Done | plantool_contract::Outcome::Blocked
            )
            && matches!(&result.details, Some(plantool_contract::RoleDetails::Strategy { action, .. }) if action == "escalate")
            && retryable_approved_qa_output(&db, &item, &facts["state"], expected)?;
        if !recommendation
            && !plan_review_evidence_recovery
            && !approved_qa_output_recovery
            && !transient_read_failure(&result)
        {
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
    fn observed_scoped_checkpoint_preserves_logs_and_rejects_uncommitted_or_out_of_scope_work() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.com"],
        ] {
            crate::git::git(root, &args).unwrap();
        }
        std::fs::write(root.join("code.py"), "base").unwrap();
        crate::git::git(root, &["add", "."]).unwrap();
        crate::git::git(root, &["commit", "-qm", "base"]).unwrap();
        let base = crate::git::git(root, &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(root.join("code.py"), "approved change").unwrap();
        crate::git::git(root, &["add", "."]).unwrap();
        crate::git::git(root, &["commit", "-qm", "implementation"]).unwrap();
        let head = crate::git::git(root, &["rev-parse", "HEAD"]).unwrap();
        let state = json!({"affected_files":["code.py"]});
        let policy = crate::sandbox::NeverSent::new(&[]).unwrap();
        std::fs::write(root.join("AC1.log"), "evidence retained, not accepted").unwrap();
        assert_eq!(
            observed_checkpoint(root, &base, &base, &state, &policy).unwrap(),
            Some(head)
        );
        assert!(observed_checkpoint(
            root,
            &base,
            &base,
            &json!({"affected_files":["other.py"]}),
            &policy
        )
        .unwrap()
        .is_none());

        // Simulate a prerequisite merge after the saved workflow base. A
        // clean commit based on the new default is still adoptable when only
        // the approved file changed after that default.
        std::fs::write(root.join("prerequisite.txt"), "merged prerequisite").unwrap();
        crate::git::git(root, &["add", "."]).unwrap();
        crate::git::git(root, &["commit", "-qm", "merged prerequisite"]).unwrap();
        let current_default = crate::git::git(root, &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(root.join("code.py"), "updated after prerequisite").unwrap();
        crate::git::git(root, &["add", "."]).unwrap();
        crate::git::git(
            root,
            &["commit", "-qm", "implementation on current default"],
        )
        .unwrap();
        let refreshed_head = crate::git::git(root, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(
            observed_checkpoint(root, &base, &current_default, &state, &policy).unwrap(),
            Some(refreshed_head.clone())
        );
        std::fs::write(root.join("unexpected.txt"), "outside approved scope").unwrap();
        crate::git::git(root, &["add", "."]).unwrap();
        crate::git::git(root, &["commit", "-qm", "out of scope"]).unwrap();
        let unsafe_head = crate::git::git(root, &["rev-parse", "HEAD"]).unwrap();
        assert!(
            observed_checkpoint(root, &base, &current_default, &state, &policy)
                .unwrap()
                .is_none()
        );
        crate::git::git(root, &["reset", "--hard", &refreshed_head]).unwrap();
        assert_eq!(
            crate::git::git(root, &["rev-parse", "HEAD"]).unwrap(),
            refreshed_head
        );
        assert_ne!(unsafe_head, refreshed_head);
        assert_eq!(
            std::fs::read_to_string(root.join("AC1.log")).unwrap(),
            "evidence retained, not accepted"
        );
        std::fs::write(root.join("code.py"), "uncommitted").unwrap();
        assert!(observed_checkpoint(root, &base, &base, &state, &policy)
            .unwrap()
            .is_none());
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
        std::fs::write(root.join("prerequisite.txt"), "merged prerequisite\n").unwrap();
        crate::git::git(&root, &["add", "."]).unwrap();
        crate::git::git(&root, &["commit", "-qm", "merge prerequisite"]).unwrap();
        let current_default = crate::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
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
        db.execute("UPDATE workflow_items SET head_sha=?1,base_sha=?1,state_json=?2",params![head,json!({"item":"i","step":"escalated","failed_step":"backend","frozen":true,"approved":true,"plan_sha":hash,"affected_files":["README.md"],"generation":1,"sequence":1}).to_string()]).unwrap();
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
        std::fs::write(tree.join("README.md"), "approved checkpoint\n").unwrap();
        crate::git::git(&tree, &["add", "README.md"]).unwrap();
        crate::git::git(&tree, &["commit", "-qm", "approved checkpoint"]).unwrap();
        let checkpoint = crate::git::git(&tree, &["rev-parse", "HEAD"]).unwrap();
        reconcile_clean_coding(t.path()).unwrap();
        reconcile_clean_coding(t.path()).unwrap();
        let state = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "backend");
        assert_eq!(state["approved"], true);
        assert_eq!(state["frozen"], false);
        assert_eq!(state["head_sha"], checkpoint);
        let (base_sha, head_sha): (String, String) = db
            .query_row(
                "SELECT base_sha,head_sha FROM workflow_items WHERE item_id='i'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(base_sha, current_default);
        assert_eq!(head_sha, checkpoint);
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
            "approved checkpoint\n"
        );
    }
    #[test]
    fn interrupted_qa_on_stale_base_reapplies_approved_scope_before_qa() {
        use plantool_contract::{Artifact, Dispatch, Outcome, RoleResult};
        let t = fixture();
        let home = t.path();
        let db = database::open(home).unwrap();
        let root = home.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.com"],
        ] {
            crate::git::git(&root, &args).unwrap();
        }
        std::fs::write(root.join("src.py"), "base\n").unwrap();
        crate::git::git(&root, &["add", "."]).unwrap();
        crate::git::git(&root, &["commit", "-qm", "base"]).unwrap();
        let base = crate::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(root.join("default-only.txt"), "new default content\n").unwrap();
        crate::git::git(&root, &["add", "."]).unwrap();
        crate::git::git(&root, &["commit", "-qm", "advance default branch"]).unwrap();
        let default = crate::git::git(&root, &["rev-parse", "HEAD"]).unwrap();
        let implementation = home.join("worktrees/backend");
        std::fs::create_dir_all(implementation.parent().unwrap()).unwrap();
        crate::git::git(
            &root,
            &[
                "worktree",
                "add",
                "--detach",
                implementation.to_str().unwrap(),
                &base,
            ],
        )
        .unwrap();
        std::fs::write(implementation.join("src.py"), "approved implementation\n").unwrap();
        crate::git::git(&implementation, &["add", "."]).unwrap();
        crate::git::git(
            &implementation,
            &["commit", "-qm", "approved implementation"],
        )
        .unwrap();
        let head = crate::git::git(&implementation, &["rev-parse", "HEAD"]).unwrap();
        crate::git::git(
            &root,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                implementation.to_str().unwrap(),
                &head,
            ],
        )
        .unwrap();

        let plan = crate::artifacts::put(home, b"approved plan").unwrap();
        crate::artifacts::register(&db, &plan, 13).unwrap();
        db.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) VALUES('qa-plan','i','plan',1,?1,'test','now')", [&plan]).unwrap();
        db.execute(
            "UPDATE projects SET local_checkout=?1",
            [root.to_str().unwrap()],
        )
        .unwrap();
        db.execute("UPDATE work_items SET approved_plan_revision_id='qa-plan',state_json='{}' WHERE id='i'", []).unwrap();
        // The projection omitted the implementation checkpoint. The daemon
        // row still identifies the current default, while the accepted backend
        // receipt is based on an older default and remains within plan scope.
        let state = json!({"item":"i","step":"supervisor","failed_step":"qa","approved":true,"frozen":false,"checkpoint_uncertain":false,"closed_on_github":false,"plan_sha":plan,"generation":2,"sequence":8,"affected_files":["src.py"]});
        db.execute(
            "UPDATE workflow_items SET base_sha=?1,head_sha=?2,state_json=?3 WHERE item_id='i'",
            params![base, default, state.to_string()],
        )
        .unwrap();
        db.execute(
            "UPDATE item_generations SET generation=2 WHERE item_id='i'",
            [],
        )
        .unwrap();
        let backend = RoleResult {
            item: "i".into(),
            role: "backend".into(),
            attempt_id: "backend".into(),
            generation: 1,
            outcome: Outcome::Done,
            summary: "Implementation committed".into(),
            tasks_done: vec![],
            artifacts: vec![Artifact {
                kind: "commit".into(),
                reference: head.clone(),
            }],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        db.execute("INSERT INTO attempts VALUES('backend','i','backend',1,'route','completed',?1,'now','now',?2,'backend','{}',0)", params![implementation.to_str().unwrap(),serde_json::to_string(&backend).unwrap()]).unwrap();
        let backend_request = Dispatch {
            item: "i".into(),
            role: "backend".into(),
            generation: 1,
            inputs: json!({"step":"backend"}),
            plan_sha: plan.clone(),
            policy_version: 1,
            capability: "write".into(),
            idempotency_key: "backend-key".into(),
            thread_id: "i".into(),
            interrupt_id: "backend-interrupt".into(),
        };
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('backend',?1,'d','i','backend-interrupt',0)", [serde_json::to_string(&backend_request).unwrap()]).unwrap();
        let interrupted = RoleResult {
            item: "i".into(),
            role: "qa".into(),
            attempt_id: "qa".into(),
            generation: 2,
            outcome: Outcome::Failed,
            summary: "Recovery needed: the daemon/worker stopped without a verified outcome".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        db.execute("INSERT INTO attempts VALUES('qa','i','qa',2,'route','recovery_needed',?1,'now','now',?2,'qa','{}',0)", params![root.to_str().unwrap(),serde_json::to_string(&interrupted).unwrap()]).unwrap();
        let qa_request = Dispatch {
            item: "i".into(),
            role: "qa".into(),
            generation: 2,
            inputs: json!({"step":"qa"}),
            plan_sha: plan.clone(),
            policy_version: 1,
            capability: "tests".into(),
            idempotency_key: "qa-key".into(),
            thread_id: "i".into(),
            interrupt_id: "qa-interrupt".into(),
        };
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('qa',?1,'d','i','qa-interrupt',0)", [serde_json::to_string(&qa_request).unwrap()]).unwrap();

        std::fs::write(
            root.join("owner-notes.txt"),
            "preserve uncommitted owner work\n",
        )
        .unwrap();
        reconcile_clean_coding(home).unwrap();
        let before_resume: String = db
            .query_row("SELECT state FROM attempts WHERE id='qa'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before_resume, "recovery_needed");
        assert_eq!(
            std::fs::read_to_string(root.join("owner-notes.txt")).unwrap(),
            "preserve uncommitted owner work\n"
        );
        std::fs::remove_file(root.join("owner-notes.txt")).unwrap();
        reconcile_clean_coding(home).unwrap();
        let reconciled = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(reconciled["step"], "backend");
        assert_eq!(reconciled["head_sha"], default);
        assert_eq!(reconciled["stale_base_reapplies"], 1);
        assert_eq!(reconciled["approved"], true);
        let status: String = db
            .query_row("SELECT state FROM attempts WHERE id='qa'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "failed");
        let summary: String = db
            .query_row(
                "SELECT json_extract(outcome_json,'$.summary') FROM attempts WHERE id='qa'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(summary.contains("result was not accepted"));
        assert!(summary.contains("older default"));
        let (new_base, new_head): (String, String) = db
            .query_row(
                "SELECT base_sha,head_sha FROM workflow_items WHERE item_id='i'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(new_base, default);
        assert_eq!(new_head, default);
        let recovery: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id='i' AND kind='worker.recover' AND delivered_at IS NULL)", [], |r| r.get(0)).unwrap();
        assert!(recovery);
        let completed_qa: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id='i' AND role='qa' AND state='completed')", [], |r| r.get(0)).unwrap();
        assert!(!completed_qa);
    }

    #[test]
    fn interrupted_qa_without_checkpoint_is_left_for_owner_recovery() {
        use plantool_contract::Dispatch;
        let t = fixture();
        let state = json!({
            "approved": true,
            "frozen": false,
            "checkpoint_uncertain": false,
            "closed_on_github": false,
            "failed_step": "qa"
        });
        let request = Dispatch {
            item: "i".into(),
            role: "qa".into(),
            generation: 1,
            inputs: json!({"step":"qa"}),
            plan_sha: "plan".into(),
            policy_version: 1,
            capability: "tests".into(),
            idempotency_key: "qa-key".into(),
            thread_id: "i".into(),
            interrupt_id: "qa-interrupt".into(),
        };
        assert!(!reconcile_interrupted_qa(t.path(), "missing", "i", 1, &request, &state).unwrap());
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
    fn escalated_plan_review_evidence_error_gets_one_safe_retry_without_approval() {
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        let plan = crate::artifacts::put(t.path(), b"current immutable plan").unwrap();
        crate::artifacts::register(&db, &plan, 22).unwrap();
        db.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) VALUES('plan','i','plan',1,?1,'pm','now')", [&plan]).unwrap();
        let state = json!({"item":"i","step":"escalated","failed_step":"plan_review","frozen":true,"approved":false,"plan_sha":plan,"generation":3,"sequence":2,"automatic_retries":0,"supervisor_rounds":2});
        db.execute(
            "UPDATE workflow_items SET state_json=?1 WHERE item_id='i'",
            [state.to_string()],
        )
        .unwrap();
        db.execute(
            "UPDATE item_generations SET generation=3 WHERE item_id='i'",
            [],
        )
        .unwrap();
        let failed = plantool_contract::RoleResult {
            item: "i".into(),
            role: "cto".into(),
            attempt_id: "review-failed".into(),
            generation: 2,
            outcome: plantool_contract::Outcome::Failed,
            summary: "Dispatch stopped: invalid_role_result: Checks require artifact evidence"
                .into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        db.execute("INSERT INTO attempts VALUES('review-failed','i','cto',2,'route','failed','/checkout','now','now',?1,'review-failed','{}',0)", [serde_json::to_string(&failed).unwrap()]).unwrap();
        let failed_request = plantool_contract::Dispatch {
            item: "i".into(),
            role: "cto".into(),
            generation: 2,
            inputs: json!({"step":"plan_review"}),
            plan_sha: plan.clone(),
            policy_version: 1,
            capability: "read".into(),
            idempotency_key: "failed".into(),
            thread_id: "i".into(),
            interrupt_id: "failed".into(),
        };
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('review-failed',?1,'failed','i','failed',0)", [serde_json::to_string(&failed_request).unwrap()]).unwrap();
        let escalation = plantool_contract::RoleResult {
            item: "i".into(),
            role: "supervisor".into(),
            attempt_id: "a".into(),
            generation: 3,
            outcome: plantool_contract::Outcome::Done,
            summary: "Repeated evidence-format failure".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: Some(plantool_contract::RoleDetails::Strategy {
                action: "escalate".into(),
                owner: None,
                task_key: None,
                reason: "The plan-review output repeatedly failed validation".into(),
            }),
        };
        db.execute("UPDATE attempts SET role='supervisor',generation=3,state='completed',outcome_json=?1 WHERE id='a'", [serde_json::to_string(&escalation).unwrap()]).unwrap();
        let supervisor_request = plantool_contract::Dispatch {
            item: "i".into(),
            role: "supervisor".into(),
            generation: 3,
            inputs: json!({"step":"supervisor"}),
            plan_sha: plan.clone(),
            policy_version: 1,
            capability: "read".into(),
            idempotency_key: "supervisor".into(),
            thread_id: "i".into(),
            interrupt_id: "supervisor".into(),
        };
        db.execute(
            "UPDATE dispatches SET request_json=?1 WHERE attempt_id='a'",
            [serde_json::to_string(&supervisor_request).unwrap()],
        )
        .unwrap();

        assert_eq!(snapshot(t.path(), "i").unwrap()["eligible"], true);
        assert!(retryable_plan_review_evidence(&db, "i", &state, "a").unwrap());
        resume_automatic(t.path()).unwrap();
        let resumed = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(resumed["step"], "plan_review");
        assert_eq!(resumed["frozen"], false);
        assert_eq!(resumed["approved"], false);
        assert_eq!(resumed["automatic_retries"], 1);
        assert_eq!(resumed["supervisor_rounds"], 0);
        assert!(resumed["retry_not_before"].is_null());
        assert!(db
            .query_row(
                "SELECT approved_plan_revision_id IS NULL FROM work_items WHERE id='i'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        assert!(db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE kind='worker.recover' AND delivered_at IS NULL)", [], |r| r.get::<_, bool>(0)).unwrap());
        resume_automatic(t.path()).unwrap();
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
    fn escalated_stale_qa_output_retries_once_under_the_same_approval() {
        use plantool_contract::{Outcome, RoleDetails, RoleResult};
        let t = fixture();
        let db = database::open(t.path()).unwrap();
        let plan = crate::artifacts::put(t.path(), b"approved plan").unwrap();
        crate::artifacts::register(&db, &plan, 13).unwrap();
        db.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) VALUES('plan','i','plan',1,?1,'owner','now')", [&plan]).unwrap();
        db.execute(
            "UPDATE work_items SET approved_plan_revision_id='plan' WHERE id='i'",
            [],
        )
        .unwrap();
        let state = json!({"item":"i","step":"escalated","failed_step":"supervisor","frozen":true,"approved":true,"plan_sha":plan,"generation":3,"sequence":8,"supervisor_rounds":2,"head_sha":"approved-commit","automatic_retries":1});
        db.execute(
            "UPDATE workflow_items SET state_json=?1 WHERE item_id='i'",
            [state.to_string()],
        )
        .unwrap();
        db.execute(
            "UPDATE item_generations SET generation=3 WHERE item_id='i'",
            [],
        )
        .unwrap();
        let qa = RoleResult {
            item: "i".into(),
            role: "qa".into(),
            attempt_id: "qa-failed".into(),
            generation: 2,
            outcome: Outcome::NeedsFix,
            summary:
                "Provider turn completed without a submitted typed result; checkpoint required"
                    .into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: None,
        };
        db.execute("INSERT INTO attempts VALUES('qa-failed','i','qa',2,'ollama-cloud','completed','/checkout','now','now',?1,'qa-key','{}',0)", [serde_json::to_string(&qa).unwrap()]).unwrap();
        let qa_request = plantool_contract::Dispatch {
            item: "i".into(),
            role: "qa".into(),
            generation: 2,
            inputs: json!({"step":"qa"}),
            plan_sha: plan.clone(),
            policy_version: 1,
            capability: "tests".into(),
            idempotency_key: "qa-key".into(),
            thread_id: "i".into(),
            interrupt_id: "qa".into(),
        };
        db.execute("INSERT INTO dispatches(attempt_id,request_json,digest,thread_id,interrupt_id,execution_generation) VALUES('qa-failed',?1,'qa-digest','i','qa',0)", [serde_json::to_string(&qa_request).unwrap()]).unwrap();
        let supervisor = RoleResult {
            item: "i".into(),
            role: "supervisor".into(),
            attempt_id: "a".into(),
            generation: 3,
            outcome: Outcome::Blocked,
            summary: "Old infrastructure escalation".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: Some(RoleDetails::Strategy {
                action: "escalate".into(),
                owner: None,
                task_key: None,
                reason: "Old failure narrative".into(),
            }),
        };
        db.execute("UPDATE attempts SET role='supervisor',generation=3,state='completed',outcome_json=?1 WHERE id='a'", [serde_json::to_string(&supervisor).unwrap()]).unwrap();
        let supervisor_request = plantool_contract::Dispatch {
            item: "i".into(),
            role: "supervisor".into(),
            generation: 3,
            inputs: json!({"step":"supervisor"}),
            plan_sha: plan.clone(),
            policy_version: 1,
            capability: "read".into(),
            idempotency_key: "supervisor-key".into(),
            thread_id: "i".into(),
            interrupt_id: "supervisor".into(),
        };
        db.execute(
            "UPDATE dispatches SET request_json=?1 WHERE attempt_id='a'",
            [serde_json::to_string(&supervisor_request).unwrap()],
        )
        .unwrap();

        assert_eq!(snapshot(t.path(), "i").unwrap()["eligible"], true);
        assert!(retryable_approved_qa_output(&db, "i", &state, "a").unwrap());
        resume_automatic(t.path()).unwrap();
        let resumed = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(resumed["step"], "qa");
        assert_eq!(resumed["frozen"], false);
        assert_eq!(resumed["approved"], true);
        assert_eq!(resumed["plan_sha"], plan);
        assert_eq!(resumed["head_sha"], "approved-commit");
        assert_eq!(resumed["automatic_retries"], 1);
        assert_eq!(resumed["qa_output_retries"], 1);
        assert_eq!(resumed["supervisor_rounds"], 0);
        assert_eq!(
            db.query_row(
                "SELECT approved_plan_revision_id FROM work_items WHERE id='i'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "plan"
        );
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
        resume_automatic(t.path()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        database::audit(
            &db,
            "daemon",
            "workflow.retry",
            "i",
            &json!({"automatic":true,"step":"qa","previous_state":{"plan_sha":plan}}),
        )
        .unwrap();
        assert!(!retryable_approved_qa_output(&db, "i", &state, "a").unwrap());
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
        crate::workspace::ensure_channels(&db, "p").unwrap();
        let thread = crate::workspace::item_thread(&db, "i").unwrap();
        crate::workspace::append(
            &db,
            "p:channel:general:thread",
            "daemon",
            "Old wording",
            "ops:i:frozen:1",
        )
        .unwrap();
        crate::workspace::append(&db, &thread, "daemon", "Old wording", "item-ops:i:frozen:1")
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
            1 // Full technical gate details belong only in #ai-activity.
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
