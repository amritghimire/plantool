//! Durable project events. The scheduler wakes graphs; it never runs model loops.
use crate::{database, dispatcher, workflow, workspace};
use anyhow::{ensure, Context};
use chrono::Datelike;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::Path;
pub fn due(
    now: chrono::DateTime<chrono::Utc>,
    day: u32,
    time: &str,
    zone: &str,
) -> anyhow::Result<Option<String>> {
    ensure!(day < 7, "sprint_day must be Monday=0 through Sunday=6");
    let zone: chrono_tz::Tz = zone
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown IANA timezone"))?;
    let time = chrono::NaiveTime::parse_from_str(time, "%H:%M")?;
    let local = now.with_timezone(&zone);
    let monday =
        local.date_naive() - chrono::Duration::days(local.weekday().num_days_from_monday() as i64);
    let date = monday + chrono::Duration::days(day as i64);
    // Compare wall-clock scheduling: DST gaps are due after the jump, folds once per ISO week.
    if local.naive_local() < date.and_time(time) {
        return Ok(None);
    }
    Ok(Some(format!(
        "{:04}-W{:02}",
        date.iso_week().year(),
        date.iso_week().week()
    )))
}
pub fn schedule(
    db: &Connection,
    project: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<(String, Option<String>)> {
    let (day,time,zone):(u32,String,String)=db.query_row("SELECT coalesce(sprint_day,0),sprint_time,coalesce(timezone,'UTC') FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",[project],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    Ok((
        plantool_core::sha256_hex(&format!("{day}:{time}:{zone}")),
        due(now, day, &time, &zone)?,
    ))
}
pub fn ensure_project(home: &Path, project: &str) -> anyhow::Result<String> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    workspace::ensure_channels(&tx, project)?;
    let item = format!("{project}:project");
    if workflow::state(&tx, &item)?.is_some() {
        return Ok(item);
    }
    let root: String = tx.query_row(
        "SELECT local_checkout FROM projects WHERE id=?1 AND mode='eos'",
        [project],
        |r| r.get(0),
    )?;
    let base = crate::git::git(Path::new(&root), &["rev-parse", "HEAD"])?;
    let s = json!({"item":item,"project":true,"step":"project_wait","sequence":0,"generation":0,"plan_sha":"","approved":false,"frozen":false,"discuss_enabled":true,"retries":{},"areas":[],"owners":{}});
    tx.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES(?1,?2,'eos','Project scheduler','project_wait',?3,?2,'project','{}')",params![item,project,plantool_core::now()])?;
    tx.execute(
        "INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES(?1,?2,?3,?3)",
        params![item, s.to_string(), base],
    )?;
    let start = plantool_contract::Start {
        thread_id: item.clone(),
        graph: plantool_contract::Graph::Project,
        input: json!({"workflow":s}),
        idempotency_key: format!("project-start:{project}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, &item, "worker.start", &serde_json::to_value(start)?)?;
    tx.commit()?;
    Ok(item)
}
pub fn event(
    tx: &Transaction<'_>,
    project: &str,
    kind: &str,
    payload: &Value,
    key: &str,
) -> anyhow::Result<String> {
    ensure!(
        [
            "sprint_planning_due",
            "issue_new",
            "plan_ready",
            "item_closed",
            "finding_filed",
            "sprint_decision"
        ]
        .contains(&kind),
        "Event is outside Phase 4"
    );
    let id = plantool_core::sha256_hex(&format!("project-event:{project}:{key}"));
    tx.execute(
        "INSERT OR IGNORE INTO project_events VALUES(?1,?2,?3,?4,?5,NULL)",
        params![id, project, kind, payload.to_string(), plantool_core::now()],
    )?;
    Ok(id)
}
fn deliver(tx: &Transaction<'_>, item: &str) -> anyhow::Result<()> {
    let Some((thread, interrupt, kind)) = tx
        .query_row(
            "SELECT thread_id,interrupt_id,kind FROM graph_waiters WHERE item_id=?1",
            [item],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(());
    };
    let project = dispatcher::item(tx, item)?.0;
    let Some((id,event,payload))=tx.query_row("SELECT id,kind,payload_json FROM project_events WHERE project_id=?1 AND delivered_at IS NULL ORDER BY created_at,rowid LIMIT 1",[project],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional()? else{return Ok(())};
    ensure!(
        kind == "project" || kind == "sprint",
        "Invalid project waiter"
    );
    let resume = plantool_contract::Resume {
        thread_id: thread,
        interrupt_id: interrupt,
        value: json!({"id":id,"kind":event,"payload":serde_json::from_str::<Value>(&payload)?}),
        idempotency_key: format!("project-event:{id}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(tx, item, "worker.resume", &serde_json::to_value(resume)?)?;
    tx.execute(
        "UPDATE project_events SET delivered_at=?2 WHERE id=?1",
        params![id, plantool_core::now()],
    )?;
    tx.execute("DELETE FROM graph_waiters WHERE item_id=?1", [item])?;
    Ok(())
}
pub fn wait(home: &Path, request: &plantool_contract::ProjectWait) -> anyhow::Result<Value> {
    let result = database::inbox_once(home, "project.wait", &request.idempotency_key, |tx| {
        let s = workflow::state(tx, &request.item)?.context("Unknown graph")?;
        ensure!(
            s["project"] == true && s["frozen"] != true,
            "Active project graph required"
        );
        ensure!(
            request.thread_id == request.item && !request.interrupt_id.is_empty(),
            "Invalid project wait identity"
        );
        ensure!(
            (request.kind == "project" && s["step"] == "project_wait")
                || (request.kind == "sprint" && s["step"] == "await_sprint_gate"),
            "Wrong project wait step"
        );
        tx.execute(
            "INSERT OR IGNORE INTO graph_waiters VALUES(?1,?2,?3,?4)",
            params![
                request.item,
                request.thread_id,
                request.interrupt_id,
                request.kind
            ],
        )?;
        let existing: (String, String) = tx.query_row(
            "SELECT thread_id,interrupt_id FROM graph_waiters WHERE item_id=?1",
            [&request.item],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            existing == (request.thread_id.clone(), request.interrupt_id.clone()),
            "Another project interrupt is active"
        );
        deliver(tx, &request.item)?;
        Ok(json!({"waiting":true,"request":serde_json::to_value(request)?}))
    })?;
    ensure!(
        result["request"] == serde_json::to_value(request)?,
        "Project wait key aliases another request"
    );
    Ok(result)
}

pub fn expire(tx: &Transaction<'_>, project: &str, except: &str) -> anyhow::Result<()> {
    let mut q=tx.prepare("SELECT item_id FROM sprint_items WHERE sprint_id IN(SELECT id FROM sprints WHERE project_id=?1 AND id<>?2 AND state<>'closed') AND state='pending'")?;
    let items = q
        .query_map(params![project, except], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for item in items {
        tx.execute(
            "UPDATE work_items SET sprint_id=NULL,state='backlog' WHERE id=?1",
            [&item],
        )?;
    }
    tx.execute("UPDATE sprint_items SET state='backlog' WHERE sprint_id IN(SELECT id FROM sprints WHERE project_id=?1 AND id<>?2 AND state<>'closed') AND state='pending'",params![project,except])?;
    tx.execute(
        "UPDATE sprints SET state='closed' WHERE project_id=?1 AND id<>?2",
        params![project, except],
    )?;
    Ok(())
}
pub fn candidates(home: &Path, db: &Connection, project: &str) -> anyhow::Result<Value> {
    let mut q=db.prepare("SELECT i.id,i.github_issue_number,i.title,r.number,r.content_sha256 FROM work_items i JOIN revisions r ON r.item_id=i.id AND r.doc_kind='plan' AND r.number=(SELECT max(number) FROM revisions WHERE item_id=i.id AND doc_kind='plan') WHERE i.project_id=?1 AND i.closed_at IS NULL AND (i.approved_plan_revision_id IS NULL OR coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=1) AND EXISTS(SELECT 1 FROM gates g WHERE g.item_id=i.id AND g.kind='plan' AND g.state='open' AND g.target_digest=r.content_sha256) ORDER BY i.created_at,i.id")?;
    let rows=q.query_map([project],|r|Ok(json!({"item":r.get::<_,String>(0)?,"issue":r.get::<_,u64>(1)?,"title":r.get::<_,String>(2)?,"version":r.get::<_,u64>(3)?,"hash":r.get::<_,String>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
    let mut rows = rows;
    for row in &mut rows {
        row["plan"] = json!(String::from_utf8(crate::artifacts::read(
            home,
            row["hash"].as_str().unwrap()
        )?)?);
    }
    Ok(json!(rows))
}
pub fn ack(home: &Path, request: &plantool_contract::EventAck) -> anyhow::Result<Value> {
    database::inbox_once(home, "project.ack", &request.event_id, |tx| {
        let item = format!("{}:project", request.project);
        let mut s = workflow::state(tx, &item)?.context("Project graph missing")?;
        let (kind,payload):(String,String)=tx.query_row("SELECT kind,payload_json FROM project_events WHERE id=?1 AND project_id=?2 AND delivered_at IS NOT NULL",params![request.event_id,request.project],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(
            ["project_wait", "await_sprint_gate"].contains(&s["step"].as_str().unwrap_or("")),
            "Project is not at an event wait"
        );
        let next = request.next_step.as_deref().unwrap_or("project_wait");
        if kind == "sprint_planning_due" {
            ensure!(
                ["discussion", "sprint_plan"].contains(&next),
                "Sprint due must enter planning"
            );
            let p: Value = serde_json::from_str(&payload)?;
            let week = p["week"].as_str().context("Week missing")?;
            let (signature, current) = schedule(tx, &request.project, chrono::Utc::now())?;
            if p["schedule"] != signature || current.as_deref() != Some(week) {
                s["step"] = json!("project_wait");
                s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
                workflow::save(tx, &item, &s)?;
                return Ok(
                    json!({"accepted":false,"stale":true,"request":serde_json::to_value(request)?}),
                );
            }
            let sprint = format!("{}:{week}", request.project);
            expire(tx, &request.project, &sprint)?;
            tx.execute("INSERT OR IGNORE INTO sprints(id,project_id,week,state) VALUES(?1,?2,?3,'planning')",params![sprint,request.project,week])?;
            tx.execute("UPDATE sprints SET state='planning' WHERE id=?1", [&sprint])?;
            s["sprint_id"] = json!(sprint);
            s["week"] = json!(week);
            s["candidates"] = candidates(home, tx, &request.project)?;
            if next == "discussion" {
                workspace::begin(
                    tx,
                    &item,
                    &mut s,
                    "sprint_plan",
                    "Discuss the prepared sprint candidates and risks; PM sizes freely.",
                )?;
            } else {
                s["step"] = json!(next)
            }
        } else if kind == "sprint_decision" {
            ensure!(
                next == "start_items",
                "Sprint decisions release approved items"
            );
            s["step"] = json!(next);
        } else if kind == "plan_ready" && next == "sprint_plan" {
            ensure!(s["sprint_id"].is_string(), "No active scheduled sprint");
            ensure!(
                schedule(tx, &request.project, chrono::Utc::now())?
                    .1
                    .is_some(),
                "Sprint is not due under current settings"
            );
            tx.execute(
                "UPDATE sprints SET state='planning' WHERE id=?1 AND state<>'closed'",
                [s["sprint_id"].as_str()],
            )?;
            s["candidates"] = candidates(home, tx, &request.project)?;
            s["step"] = json!("sprint_plan");
        } else {
            ensure!(
                next == "project_wait",
                "Context events cannot authorize implementation"
            );
            s["step"] = json!(next);
        }
        s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
        workflow::save(tx, &item, &s)?;
        database::audit(
            tx,
            "worker",
            "project.event_ack",
            &request.event_id,
            &json!({"project":request.project,"next":next}),
        )?;
        Ok(json!({"accepted":true,"request":serde_json::to_value(request)?}))
    })
}
pub fn validate(s: &Value, result: &plantool_contract::RoleResult) -> anyhow::Result<()> {
    ensure!(
        result.role == "pm"
            && result.tasks_done.is_empty()
            && result.artifacts.is_empty()
            && result.checks.is_empty(),
        "Sprint planning is read-only PM work"
    );
    let Some(plantool_contract::RoleDetails::Sprint { items, .. }) = &result.details else {
        anyhow::bail!("Typed sprint choices required")
    };
    let mut seen = std::collections::HashSet::new();
    for choice in items {
        ensure!(
            seen.insert(&choice.item)
                && !choice.estimate.trim().is_empty()
                && choice.estimate.len() <= 1000,
            "Invalid sprint choice/estimate"
        );
        ensure!(
            s["candidates"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v["item"] == choice.item)),
            "Sprint item is not a prepared candidate"
        );
    }
    Ok(())
}
pub fn record(result: &plantool_contract::RoleResult, s: &mut Value) -> anyhow::Result<()> {
    if result.outcome != plantool_contract::Outcome::Done {
        s["step"] = json!("escalated");
        s["frozen"] = json!(true);
        return Ok(());
    }
    let Some(plantool_contract::RoleDetails::Sprint { items, notes }) = &result.details else {
        anyhow::bail!("Sprint result missing")
    };
    s["choices"] = serde_json::to_value(items)?;
    s["sprint_notes"] = json!(notes);
    s["step"] = json!("publish_plans");
    Ok(())
}
pub fn publish(home: &Path, request: &plantool_contract::WorkflowPr) -> anyhow::Result<Value> {
    database::inbox_once(home, "project.publish", &request.idempotency_key, |tx| {
        let (project, _) = dispatcher::item(tx, &request.item)?;
        let mut s = workflow::state(tx, &request.item)?.context("Project missing")?;
        ensure!(
            s["project"] == true && s["step"] == "publish_plans" && s["frozen"] != true,
            "Project not publishing plans"
        );
        let sprint = s["sprint_id"].as_str().context("Sprint missing")?;
        let available = candidates(home, tx, &project)?;
        let running:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sprint_items WHERE sprint_id=?1 AND state IN('approved','started'))",[sprint],|r|r.get(0))?;
        let previous: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sprint_snapshots WHERE sprint_id=?1)",
            [sprint],
            |r| r.get(0),
        )?;
        tx.execute("UPDATE work_items SET sprint_id=NULL,state='backlog' WHERE id IN(SELECT item_id FROM sprint_items WHERE sprint_id=?1 AND state='pending')",[sprint])?;
        tx.execute(
            "UPDATE sprint_items SET state='backlog' WHERE sprint_id=?1 AND state='pending'",
            [sprint],
        )?;
        if s["choices"].as_array().is_some_and(|v| v.is_empty()) && !running && !previous {
            tx.execute("UPDATE sprints SET state='empty' WHERE id=?1", [sprint])?;
            s["step"] = json!("project_wait");
            s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
            workflow::save(tx, &request.item, &s)?;
            return Ok(json!({"empty":true}));
        }
        let mut snapshot = vec![];
        for choice in s["choices"].as_array().context("Choices missing")? {
            let candidate = available
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["item"] == choice["item"])
                .context("Sprint plan changed before snapshot")?;
            let mut entry = candidate.clone();
            entry.as_object_mut().unwrap().remove("plan");
            entry["state"] = json!("pending");
            entry["estimate"] = choice["estimate"].clone();
            snapshot.push(entry);
            tx.execute("INSERT INTO sprint_items(sprint_id,item_id,plan_sha,estimate) VALUES(?1,?2,?3,?4) ON CONFLICT(sprint_id,item_id) DO UPDATE SET plan_sha=excluded.plan_sha,estimate=excluded.estimate,state='pending'",params![sprint,choice["item"].as_str(),candidate["hash"].as_str(),choice["estimate"].as_str()])?;
            tx.execute(
                "UPDATE work_items SET sprint_id=?2,state='plan_gate' WHERE id=?1",
                params![choice["item"].as_str(), sprint],
            )?;
        }
        let mut q=tx.prepare("SELECT si.item_id,i.github_issue_number,i.title,r.number,si.plan_sha,si.estimate,i.state FROM sprint_items si JOIN work_items i ON i.id=si.item_id JOIN revisions r ON r.item_id=i.id AND r.doc_kind='plan' AND r.content_sha256=si.plan_sha WHERE si.sprint_id=?1 AND si.state IN('approved','started') ORDER BY si.item_id")?;
        for row in q.query_map([sprint],|r|Ok(json!({"item":r.get::<_,String>(0)?,"issue":r.get::<_,u64>(1)?,"title":r.get::<_,String>(2)?,"version":r.get::<_,u64>(3)?,"hash":r.get::<_,String>(4)?,"estimate":r.get::<_,String>(5)?,"state":r.get::<_,String>(6)?})))? {let row=row?;if !snapshot.iter().any(|p|p["item"]==row["item"]){snapshot.push(row)}}
        let throughput:i64=tx.query_row("SELECT count(*) FROM work_items WHERE project_id=?1 AND state='merged' AND closed_at>=?2",params![project,(chrono::Utc::now()-chrono::Duration::days(28)).timestamp()],|r|r.get(0))?;
        let payload = json!({"items":snapshot,"notes":s["sprint_notes"],"week":s["week"],"recent_throughput":throughput,"throughput_days":28});
        let digest = plantool_core::sha256_hex(&payload.to_string());
        let id = format!("{sprint}:{digest}");
        tx.execute(
            "INSERT OR IGNORE INTO sprint_snapshots VALUES(?1,?2,?3,?4,NULL,NULL,?5)",
            params![
                id,
                sprint,
                digest,
                payload.to_string(),
                plantool_core::now()
            ],
        )?;
        let published: bool = tx.query_row(
            "SELECT comment_id IS NOT NULL FROM sprint_snapshots WHERE id=?1",
            [&id],
            |r| r.get(0),
        )?;
        if !published {
            dispatcher::enqueue(
                tx,
                &request.item,
                "github.sprint",
                &json!({"snapshot":id,"sprint":sprint}),
            )?;
        }
        tx.execute(
            "UPDATE sprints SET state=?2 WHERE id=?1",
            params![sprint, if published { "waiting" } else { "publishing" }],
        )?;
        s["snapshot_id"] = json!(id);
        let empty = s["choices"].as_array().is_some_and(|v| v.is_empty());
        s["step"] = json!(if empty {
            "project_wait"
        } else {
            "await_sprint_gate"
        });
        s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
        workflow::save(tx, &request.item, &s)?;
        Ok(json!({"snapshot":id,"publication":"pending","empty":empty}))
    })
}
pub fn start_items(home: &Path, request: &plantool_contract::WorkflowPr) -> anyhow::Result<Value> {
    database::inbox_once(
        home,
        "project.start_items",
        &request.idempotency_key,
        |tx| {
            let mut s = workflow::state(tx, &request.item)?.context("Project missing")?;
            ensure!(
                s["project"] == true && s["step"] == "start_items",
                "Not releasing a sprint"
            );
            let sprint = s["sprint_id"].as_str().context("Sprint missing")?;
            let rows = {
                let mut q=tx.prepare("SELECT item_id,plan_sha FROM sprint_items WHERE sprint_id=?1 AND state='approved'")?;
                let rows = q
                    .query_map([sprint], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (item, hash) in rows {
                let (gate,thread,interrupt,policy):(String,String,String,u64)=tx.query_row("SELECT g.id,c.thread_id,c.interrupt_id,g.policy_version FROM gates g JOIN github_gate_context c ON c.gate_id=g.id JOIN work_items i ON i.id=g.item_id JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE g.item_id=?1 AND g.kind='plan' AND g.state='approved' AND g.target_digest=?2 AND r.content_sha256=?2 AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id) ORDER BY g.created_at DESC LIMIT 1",params![item,hash],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
                workflow::gate_decision(tx, &item, true)?;
                let resume = plantool_contract::Resume {
                    thread_id: thread,
                    interrupt_id: interrupt,
                    value: json!({"gate":gate,"decision":"approved","plan_sha":hash,"policy_version":policy}),
                    idempotency_key: format!("sprint-start:{sprint}:{item}:{hash}"),
                    contract_version: plantool_contract::CONTRACT_VERSION,
                    graph_version: plantool_contract::GRAPH_VERSION,
                };
                dispatcher::enqueue(tx, &item, "worker.resume", &serde_json::to_value(resume)?)?;
                tx.execute(
                    "UPDATE sprint_items SET state='started' WHERE sprint_id=?1 AND item_id=?2",
                    params![sprint, item],
                )?;
            }
            let pending: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sprint_items WHERE sprint_id=?1 AND state='pending')",
                [s["sprint_id"].as_str()],
                |r| r.get(0),
            )?;
            s["step"] = json!(if pending {
                "await_sprint_gate"
            } else {
                "project_wait"
            });
            s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
            workflow::save(tx, &request.item, &s)?;
            Ok(json!({"released":true}))
        },
    )
}
pub fn tick(home: &Path, now: chrono::DateTime<chrono::Utc>) -> anyhow::Result<()> {
    let projects = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT p.id,coalesce(s.sprint_day,0),coalesce(s.sprint_time,'09:00'),coalesce(s.timezone,'UTC') FROM projects p LEFT JOIN project_settings s ON s.project_id=p.id AND s.version=(SELECT max(version) FROM project_settings WHERE project_id=p.id) WHERE p.mode='eos'")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (project, day, time, zone) in projects {
        let item = ensure_project(home, &project)?;
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(week) = due(now, day, &time, &zone)? {
            let (signature, _) = schedule(&tx, &project, now)?;
            event(
                &tx,
                &project,
                "sprint_planning_due",
                &json!({"week":week,"schedule":signature}),
                &format!("sprint:{week}:{signature}"),
            )?;
        }
        if workflow::state(&tx, &item)?.is_some_and(|s| s["frozen"] != true) {
            deliver(&tx, &item)?;
        }
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn instant(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }
    #[test]
    fn schedule_uses_local_iso_week_and_dst_without_duplicate_wall_clock_runs() {
        assert_eq!(
            due(
                instant("2026-10-19T03:14:00Z"),
                0,
                "09:00",
                "Asia/Kathmandu"
            )
            .unwrap(),
            None
        );
        assert_eq!(
            due(
                instant("2026-10-19T03:15:00Z"),
                0,
                "09:00",
                "Asia/Kathmandu"
            )
            .unwrap()
            .as_deref(),
            Some("2026-W43")
        );
        assert_eq!(
            due(
                instant("2026-11-01T05:30:00Z"),
                6,
                "01:30",
                "America/New_York"
            )
            .unwrap(),
            due(
                instant("2026-11-01T06:30:00Z"),
                6,
                "01:30",
                "America/New_York"
            )
            .unwrap()
        );
        assert!(due(instant("2026-10-19T00:00:00Z"), 7, "09:00", "UTC").is_err());
        assert!(due(instant("2026-10-19T00:00:00Z"), 0, "25:00", "UTC").is_err());
        assert!(due(instant("2026-10-19T00:00:00Z"), 0, "09:00", "unknown").is_err());
        assert_eq!(
            due(instant("2027-01-01T12:00:00Z"), 0, "09:00", "UTC")
                .unwrap()
                .as_deref(),
            Some("2026-W53")
        );
    }
}
