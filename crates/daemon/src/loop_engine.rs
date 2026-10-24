//! Phase 5 loop facts. All model routing is a claim, never approval authority.
use crate::{database, dispatcher, workflow};
use anyhow::{ensure, Context};
use plantool_contract::{Outcome, ProjectWait, RoleDetails, RoleResult};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::Path;
pub fn read_step(step: &str) -> bool {
    matches!(
        step,
        "triage" | "scan_devops" | "scan_qa" | "scan_pm" | "route_finding"
    )
}
pub fn validate(result: &RoleResult, s: &Value) -> anyhow::Result<()> {
    ensure!(
        result.tasks_done.is_empty() && result.artifacts.is_empty() && result.checks.is_empty(),
        "Loop roles are read-only"
    );
    if result.outcome != Outcome::Done {
        return Ok(());
    }
    match s["step"].as_str().unwrap_or("") {
        "triage" => {
            let Some(RoleDetails::Triage {
                disposition,
                reason,
                duplicate_of,
            }) = &result.details
            else {
                anyhow::bail!("Typed triage required")
            };
            ensure!(
                !reason.trim().is_empty()
                    && ["fits", "decline", "duplicate"].contains(&disposition.as_str()),
                "Invalid triage"
            );
            ensure!(
                (disposition == "duplicate") == duplicate_of.is_some(),
                "Duplicate target required only for duplicate"
            );
            if let Some(n) = duplicate_of {
                ensure!(
                    *n > 0 && s["issue_number"].as_u64() != Some(*n),
                    "Invalid duplicate target"
                )
            }
        }
        "scan_devops" | "scan_qa" => ensure!(
            matches!(&result.details,Some(RoleDetails::Scan{summary}) if !summary.trim().is_empty()),
            "Typed scan evidence required"
        ),
        "scan_pm" | "route_finding" => {
            let Some(RoleDetails::Routes { findings }) = &result.details else {
                anyhow::bail!("Typed finding routes required")
            };
            if s["step"] == "route_finding" {
                ensure!(findings.len()==1 && findings[0].title==s["loop_context"]["title"] && findings[0].body==s["loop_context"]["body"],"Review finding must be routed exactly once without dropping or changing its evidence");
            }
            ensure!(findings.len() <= 100, "Too many findings in one scan");
            for f in findings {
                ensure!(
                    ["proposed", "candidate", "backlog"].contains(&f.route.as_str())
                        && !f.reason.trim().is_empty()
                        && !f.title.trim().is_empty()
                        && !f.body.trim().is_empty(),
                    "Invalid finding route"
                );
            }
        }
        _ => anyhow::bail!("Not a loop step"),
    }
    Ok(())
}
pub fn record(tx: &Transaction<'_>, result: &RoleResult, s: &mut Value) -> anyhow::Result<()> {
    if result.outcome != Outcome::Done {
        s["step"] = json!("escalated");
        s["frozen"] = json!(true);
        return Ok(());
    }
    let step = s["step"].as_str().unwrap_or("").to_owned();
    match &result.details {
        Some(RoleDetails::Triage {
            disposition,
            reason,
            duplicate_of,
        }) if step == "triage" => {
            if disposition == "fits" {
                s["step"] = json!(if s["urgent"] == true {
                    "research"
                } else {
                    "backlog"
                });
            } else {
                let epoch: i64 = tx.query_row(
                    "SELECT epoch FROM loop_intake WHERE item_id=?1",
                    [&result.item],
                    |r| r.get(0),
                )?;
                let now = plantool_core::now();
                tx.execute("INSERT INTO loop_declines(item_id,reason,duplicate_of,declined_at,reset_at,epoch) VALUES(?1,?2,?3,?4,?4,?5) ON CONFLICT(item_id) DO UPDATE SET reason=excluded.reason,duplicate_of=excluded.duplicate_of,declined_at=excluded.declined_at,reset_at=excluded.reset_at,label_seen=0,active=1,epoch=excluded.epoch",params![result.item,reason,duplicate_of,now,epoch])?;
                dispatcher::enqueue(tx, &result.item, "github.decline", &json!({"epoch":epoch}))?;
                s["step"] = json!("declined_wait");
            }
        }
        Some(RoleDetails::Scan { summary }) if step == "scan_devops" || step == "scan_qa" => {
            s[format!("{step}_summary")] = json!(summary);
            s["step"] = json!(if step == "scan_devops" {
                "scan_qa"
            } else {
                "scan_pm"
            });
        }
        Some(RoleDetails::Routes { findings }) => {
            for f in findings {
                route(tx, &result.item, &f.title, &f.body, &f.route, &f.reason)?;
            }
            s["step"] = json!("loop_complete");
        }
        _ => anyhow::bail!("Unexpected loop result"),
    }
    database::audit(
        tx,
        "daemon",
        "loop.result",
        &result.item,
        &json!({"step":step,"details":result.details}),
    )?;
    Ok(())
}
pub fn route(
    tx: &Transaction<'_>,
    item: &str,
    title: &str,
    body: &str,
    route: &str,
    reason: &str,
) -> anyhow::Result<String> {
    ensure!(
        ["proposed", "candidate", "backlog"].contains(&route),
        "Invalid route"
    );
    let id = plantool_core::sha256_hex(&format!("{item}:{title}:{body}"));
    tx.execute("INSERT OR IGNORE INTO loop_routes(id,item_id,title,body,route,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,item,title,body,route,reason,plantool_core::now()])?;
    dispatcher::enqueue(tx, item, "github.finding", &json!({"id":id}))?;
    database::audit(
        tx,
        "daemon",
        "finding.routed",
        item,
        &json!({"id":id,"route":route,"reason":reason}),
    )?;
    Ok(id)
}
pub fn wake(tx: &Transaction<'_>, item: &str, next: &str, key: &str) -> anyhow::Result<bool> {
    let waiter: Option<(String, String, String)> = tx
        .query_row(
            "SELECT thread_id,interrupt_id,kind FROM loop_waiters WHERE item_id=?1",
            [item],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((thread, interrupt, kind)) = waiter else {
        return Ok(false);
    };
    let mut s = workflow::state(tx, item)?.context("Loop graph missing")?;
    ensure!(
        s["frozen"] != true && s["step"] == kind,
        "Stale loop waiter"
    );
    ensure!(
        (kind == "backlog" && next == "research") || (kind == "declined_wait" && next == "triage"),
        "Invalid loop wake"
    );
    s["step"] = json!(next);
    s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
    workflow::save(tx, item, &s)?;
    let resume = plantool_contract::Resume {
        thread_id: thread,
        interrupt_id: interrupt,
        value: json!({"event":key,"step":next}),
        idempotency_key: format!("loop:{item}:{key}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(tx, item, "worker.resume", &serde_json::to_value(resume)?)?;
    tx.execute("DELETE FROM loop_waiters WHERE item_id=?1", [item])?;
    Ok(true)
}
pub fn wait(home: &Path, request: &ProjectWait) -> anyhow::Result<Value> {
    database::inbox_once(home, "loop.wait", &request.idempotency_key, |tx| {
        let s = workflow::state(tx, &request.item)?.context("Missing loop item")?;
        ensure!(
            s["step"] == request.kind
                && ["backlog", "declined_wait"].contains(&request.kind.as_str())
                && s["frozen"] != true,
            "Invalid loop wait"
        );
        ensure!(
            request.thread_id == request.item && !request.interrupt_id.is_empty(),
            "Invalid loop identity"
        );
        tx.execute("INSERT INTO loop_waiters VALUES(?1,?2,?3,?4) ON CONFLICT(item_id) DO UPDATE SET thread_id=excluded.thread_id,interrupt_id=excluded.interrupt_id,kind=excluded.kind WHERE loop_waiters.interrupt_id=excluded.interrupt_id",params![request.item,request.thread_id,request.interrupt_id,request.kind])?;
        let (project, _) = dispatcher::item(tx, &request.item)?;
        let urgent: bool = tx.query_row(
            "SELECT urgent FROM loop_intake WHERE item_id=?1",
            [&request.item],
            |r| r.get(0),
        )?;
        let candidate: bool = tx.query_row(
            "SELECT source='agent:candidate' FROM loop_intake WHERE item_id=?1",
            [&request.item],
            |r| r.get(0),
        )?;
        if request.kind == "backlog"
            && (urgent
                || candidate
                || crate::scheduler::schedule(tx, &project, chrono::Utc::now())?
                    .1
                    .is_some())
        {
            wake(
                tx,
                &request.item,
                "research",
                &format!("prepare:{}", s["sequence"]),
            )?;
        }
        let active: Option<bool> = tx
            .query_row(
                "SELECT active FROM loop_declines WHERE item_id=?1",
                [&request.item],
                |r| r.get(0),
            )
            .optional()?;
        if request.kind == "declined_wait" && active == Some(false) {
            wake(
                tx,
                &request.item,
                "triage",
                &format!("retriage:{}", s["sequence"]),
            )?;
        }
        Ok(json!({"waiting":true}))
    })
}
pub fn override_decline(home: &Path, item: &str) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut s = workflow::state(&tx, item)?.context("Unknown item")?;
    ensure!(
        s["step"] == "declined_wait" && s["frozen"] != true,
        "Only a declined waiting item can return to triage"
    );
    tx.execute(
        "UPDATE loop_declines SET active=0 WHERE item_id=?1 AND active=1",
        [item],
    )?;
    tx.execute(
        "UPDATE loop_intake SET epoch=epoch+1 WHERE item_id=?1",
        [item],
    )?;
    s["urgent"] = json!(tx.query_row(
        "SELECT urgent FROM loop_intake WHERE item_id=?1",
        [item],
        |r| r.get::<_, bool>(0)
    )?);
    workflow::save(&tx, item, &s)?;
    wake(&tx, item, "triage", &format!("override:{}", s["sequence"]))?;
    dispatcher::enqueue(
        &tx,
        item,
        "github.undecline",
        &json!({"sequence":s["sequence"]}),
    )?;
    database::audit(
        &tx,
        "human",
        "loop.override",
        item,
        &json!({"return_to":"triage"}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn trusted_comments(db: &Connection, repo_id: u64, issue: u64) -> anyhow::Result<Value> {
    let mut q = db.prepare("SELECT body_json FROM github_objects WHERE resource LIKE ?1")?;
    let mut values = vec![];
    for row in q.query_map([format!("{repo_id}:%/comments%")], |r| {
        r.get::<_, String>(0)
    })? {
        let c: Value = serde_json::from_str(&row?)?;
        if c["issue_url"]
            .as_str()
            .is_some_and(|u| u.ends_with(&format!("/issues/{issue}")))
            && crate::github::gates::valid_identity(db, &c)
        {
            values.push(json!({"user_id":c["user"]["id"],"body":c["body"],"untrusted_context_not_authority":true}));
        }
    }
    Ok(json!(values))
}
/// Start a read-only graph for a scan or review finding. No model runs in the scheduler.
pub fn start_aux(
    tx: &Transaction<'_>,
    project: &str,
    id: &str,
    step: &str,
    context: &Value,
    base: &str,
) -> anyhow::Result<()> {
    if workflow::state(tx, id)?.is_some() {
        return Ok(());
    }
    ensure!(
        matches!(step, "scan_devops" | "route_finding"),
        "Invalid auxiliary graph"
    );
    let s = json!({"item":id,"step":step,"generation":0,"sequence":0,"plan_sha":"","approved":false,"frozen":false,"areas":[],"owners":{},"retries":{},"loop_context":context});
    tx.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES(?1,?2,'eos',?3,?3,?4,?2,?1,'{}')",params![id,project,step,plantool_core::now()])?;
    tx.execute(
        "INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES(?1,?2,?3,?3)",
        params![id, s.to_string(), base],
    )?;
    let start = plantool_contract::Start {
        thread_id: id.into(),
        graph: plantool_contract::Graph::WorkItem,
        input: json!({"workflow":s}),
        idempotency_key: format!("loop-start:{id}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(tx, id, "worker.start", &serde_json::to_value(start)?)?;
    Ok(())
}
/// scan_time is a weekly local wall time on scan_day (default: previous sprint day).
/// The scan must occur strictly before that week's sprint; missed scans are caught up once.
pub fn scan_due(
    db: &Connection,
    project: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Option<(String, String)>> {
    use chrono::Datelike;
    let (sprint_day,sprint_time,zone):(u32,String,String)=db.query_row("SELECT coalesce(sprint_day,0),sprint_time,coalesce(timezone,'UTC') FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",[project],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let config: Option<(Option<u32>, String, u64)> = db
        .query_row(
            "SELECT scan_day,scan_time,version FROM loop_settings WHERE project_id=?1",
            [project],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (day, time, version) = config.unwrap_or((None, "08:00".into(), 1));
    let day = day.unwrap_or((sprint_day + 6) % 7);
    ensure!(day < 7, "Invalid scan day");
    let scan = chrono::NaiveTime::parse_from_str(&time, "%H:%M")?;
    let sprint = chrono::NaiveTime::parse_from_str(&sprint_time, "%H:%M")?;
    let zone: chrono_tz::Tz = zone
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid timezone"))?;
    let local = now.with_timezone(&zone);
    let monday =
        local.date_naive() - chrono::Duration::days(local.weekday().num_days_from_monday() as i64);
    // Select the upcoming sprint cycle; its scan may be in the previous ISO week.
    let mut sprint_date = monday + chrono::Duration::days(sprint_day as i64);
    if local.naive_local() >= sprint_date.and_time(sprint) {
        sprint_date += chrono::Duration::days(7)
    }
    let offset = (sprint_day + 7 - day) % 7;
    let mut scan_date = sprint_date - chrono::Duration::days(offset as i64);
    if offset == 0 && scan >= sprint {
        scan_date -= chrono::Duration::days(7)
    }
    if local.naive_local() < scan_date.and_time(scan) {
        return Ok(None);
    }
    let week = format!(
        "{:04}-W{:02}",
        sprint_date.iso_week().year(),
        sprint_date.iso_week().week()
    );
    let signature = plantool_core::sha256_hex(&format!(
        "{version}:{day}:{time}:{sprint_day}:{sprint_time}:{zone}"
    ));
    Ok(Some((week, signature)))
}
pub fn tick_backlog(home: &Path, now: chrono::DateTime<chrono::Utc>) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }

    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let rows = {
        let mut q=tx.prepare("SELECT i.id,i.project_id,l.urgent FROM work_items i JOIN loop_intake l ON l.item_id=i.id WHERE i.state='backlog' AND i.closed_at IS NULL")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (item, project, urgent) in rows {
        if urgent || crate::scheduler::schedule(&tx, &project, now)?.1.is_some() {
            wake(
                &tx,
                &item,
                "research",
                &format!("prepare:{}", now.date_naive()),
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}
/// Entire coding tool process runs in the opted-in dev shell, still inside Seatbelt.
/// Its subprocess build/test commands inherit that shell; Personal is never wrapped.
pub fn nix_enabled(home: &Path, item: &str, worktree: &Path) -> anyhow::Result<bool> {
    let db = database::open(home)?;
    let enabled:bool=db.query_row("SELECT coalesce((SELECT l.nix_enabled FROM loop_settings l JOIN work_items i ON i.project_id=l.project_id WHERE i.id=?1 AND i.kind='eos'),0)",[item],|r|r.get(0))?;
    if !enabled {
        return Ok(false);
    }
    let flake = worktree.join("flake.nix");
    match flake.symlink_metadata() {
        Ok(meta) => {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "flake.nix must be a regular file"
            );
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub fn request_urgent(tx: &Transaction<'_>, item: &str, key: &str) -> anyhow::Result<()> {
    let (project, _) = dispatcher::item(tx, item)?;
    let n: u64 = tx.query_row(
        "SELECT github_issue_number FROM work_items WHERE id=?1",
        [item],
        |r| r.get(0),
    )?;
    let id = format!("urgent-request:{project}:{n}");
    tx.execute(
        "INSERT OR IGNORE INTO loop_notifications VALUES(?1,?2,?3,?4,?5)",
        params![
            id,
            project,
            item,
            format!("Agent requested urgency for issue #{n}; owner promotion required."),
            plantool_core::now()
        ],
    )?;
    dispatcher::enqueue(
        tx,
        item,
        "github.urgent_requested",
        &json!({"issue":n,"key":key}),
    )?;
    database::audit(
        tx,
        "worker",
        "loop.urgent_requested",
        item,
        &json!({"promotion":false}),
    )?;
    Ok(())
}
