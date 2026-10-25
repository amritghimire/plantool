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
    let external:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%')",[item],|r|r.get(0))?;
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
            | "docs"
            | "supervisor"
    ) && request["capability"] == "read";
    let eligible = state["frozen"] == true
        && state["step"] == "escalated"
        && status == "failed"
        && read
        && !active
        && !gates
        && !external;
    Ok(
        json!({"state":state,"attempt_id":id,"step":step,"eligible":eligible,"active":active,"gates":gates,"external":external}),
    )
}
pub fn retry(home: &Path, item: &str, expected: &str) -> anyhow::Result<Value> {
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
    let blocked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')) OR EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending')) OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%')",[item],|r|r.get(0))?;
    ensure!(!blocked, "Recovery blockers changed; refresh");
    let mut state = facts["state"].clone();
    state["step"] = facts["step"].clone();
    state["frozen"] = json!(false);
    state["route_retry"] = json!(false);
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
        "human",
        "workflow.retry",
        item,
        &json!({"attempt":expected,"step":facts["step"],"previous_state":facts["state"]}),
    )?;
    crate::workspace::operational(&tx,item,"Recovery queued",&format!("retry:{}:{}",expected,state["sequence"]),"Owner authorized a retry with a fresh passkey. The failed read-only step will start in a new checkpoint epoch; old history remains available. Watch Role attempts for progress. GitHub approval gates remain unchanged.")?;
    tx.commit()?;
    Ok(json!({"queued":true,"step":facts["step"]}))
}

pub fn announce_existing(home: &Path) -> anyhow::Result<()> {
    let db = database::open(home)?;
    let ids = {
        let mut q = db.prepare(
            "SELECT item_id FROM workflow_items WHERE json_extract(state_json,'$.frozen')=1",
        )?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for id in ids {
        crate::workspace::operational(&db,&id,"Work remains frozen",&format!("frozen:{}",workflow::state(&db,&id)?.unwrap()["sequence"]),"Inspect the failed attempt in Engineering OS. Retry an eligible read-only step with Owner recovery; reconcile uncertain coding or external effects manually. Worker availability and execution resume do not unfreeze work.")?;
    }
    Ok(())
}
