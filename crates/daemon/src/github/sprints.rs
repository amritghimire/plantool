//! Snapshot-bound sprint approvals. Every API write is daemon/App-owned and durable.
use super::{
    api::{self, Api},
    commands::{self, Command, Context as Verification, Place},
    gates,
    lifecycle::{pages, write},
    setup::Repository,
};
use crate::{database, dispatcher, scheduler, workflow};
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
fn latest(db: &rusqlite::Connection, sprint: &str) -> anyhow::Result<String> {
    Ok(db.query_row("SELECT id FROM sprint_snapshots WHERE sprint_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT 1",[sprint],|r|r.get(0))?)
}
pub async fn publish(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    let pending = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT x.id,x.sprint_id,x.payload_json,s.tracking_issue_number FROM sprint_snapshots x JOIN sprints s ON s.id=x.sprint_id WHERE s.project_id=?1 AND s.state<>'closed' AND x.comment_id IS NULL ORDER BY x.created_at,x.rowid")?;
        let rows = q
            .query_map([&repo.project], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<u64>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let item = format!("{}:project", repo.project);
    let path = repo.path()?;
    for (id, sprint, payload, issue) in pending {
        if latest(&database::open(home)?, &sprint)? != id {
            continue;
        }
        if workflow::state(&database::open(home)?, &item)?.is_some_and(|s| s["frozen"] == true) {
            continue;
        }
        let p: Value = serde_json::from_str(&payload)?;
        let issue = if let Some(n) = issue {
            n
        } else {
            let marker = format!("<!-- plantool-sprint:{sprint} -->");
            let found = pages(api, &format!("{path}/issues?state=all"), None)
                .await?
                .into_iter()
                .find(|i| {
                    i["pull_request"].is_null()
                        && i["user"]["id"].as_u64() == Some(repo.app_bot_id)
                        && i["body"].as_str().is_some_and(|b| b.contains(&marker))
                });
            let created=write(home,api,&item,&format!("sprint-issue:{sprint}"),"POST",&format!("{path}/issues"),json!({"title":format!("Sprint {}",p["week"].as_str().context("Week missing")?),"body":format!("Immutable snapshot comments below identify plan versions and hashes. Owner commands approve only the latest snapshot.\n\n{marker}"),"labels":["plantool:sprint"]}),found,false).await?;
            let n = created["number"]
                .as_u64()
                .context("Tracking issue missing")?;
            database::open(home)?.execute(
                "UPDATE sprints SET tracking_issue_number=?2 WHERE id=?1",
                params![sprint, n],
            )?;
            n
        };
        let marker = format!("<!-- plantool-snapshot:{id} -->");
        let mut body=format!("## Sprint {} snapshot\n\n| Item | Plan version | Full sha256 | PM estimate |\n| --- | --- | --- | --- |\n",p["week"].as_str().unwrap());
        for entry in p["items"].as_array().context("Snapshot items missing")? {
            body.push_str(&format!(
                "| #{} | v{} | `{}` | {} |\n",
                entry["issue"],
                entry["version"],
                entry["hash"].as_str().unwrap(),
                entry["estimate"]
                    .as_str()
                    .unwrap()
                    .replace('|', "\\|")
                    .replace(['\r', '\n'], " ")
            ));
        }
        body.push_str(&format!("\nRecent throughput: {} merged items in {} days.\n\n{}\n\nApprove all: `/plantool approve-sprint`\nApprove one: `/plantool approve <issue-number>`\n\n{marker}",p["recent_throughput"],p["throughput_days"],p["notes"].as_str().unwrap_or("")));
        let root: String = database::open(home)?.query_row(
            "SELECT local_checkout FROM projects WHERE id=?1",
            [&repo.project],
            |r| r.get(0),
        )?;
        crate::models::proxy::check_payload(
            Path::new(&root),
            &crate::sandbox::NeverSent::new(
                &crate::execution::Execution::load(home)?.rules(Path::new(&root)),
            )?,
            &json!({"body":body}),
        )?;
        let found = pages(api, &format!("{path}/issues/{issue}/comments"), None)
            .await?
            .into_iter()
            .find(|c| {
                c["user"]["id"].as_u64() == Some(repo.app_bot_id)
                    && c["body"].as_str().is_some_and(|b| b.contains(&marker))
            });
        let comment = write(
            home,
            api,
            &item,
            &format!("snapshot:{id}"),
            "POST",
            &format!("{path}/issues/{issue}/comments"),
            json!({"body":body}),
            found,
            false,
        )
        .await?;
        ensure!(
            comment["user"]["id"].as_u64() == Some(repo.app_bot_id)
                && comment["user"]["type"] == "Bot",
            "Snapshot must be App-authored"
        );
        let created = comment["created_at"]
            .as_str()
            .context("Snapshot timestamp missing")?;
        chrono::DateTime::parse_from_rfc3339(created)?;
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE sprint_snapshots SET comment_id=?2,published_at=?3 WHERE id=?1",
            params![id, comment["id"].as_u64(), created],
        )?;
        tx.execute(
            "UPDATE sprints SET state='waiting',snapshot_comment_id=?2 WHERE id=?1",
            params![sprint, comment["id"].as_u64()],
        )?;
        tx.execute("UPDATE outbox SET delivered_at=?2 WHERE kind='github.sprint' AND json_extract(payload_json,'$.snapshot')=?1",params![id,plantool_core::now()])?;
        database::audit(
            &tx,
            "daemon",
            "sprint.snapshot_published",
            &id,
            &json!({"issue":issue,"comment":comment["id"]}),
        )?;
        tx.commit()?;
    }
    Ok(())
}
pub async fn handle(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    c: &Value,
    number: i64,
    command: &Command,
) -> anyhow::Result<bool> {
    if !matches!(command, Command::ApproveSprint | Command::ApproveItem(_)) {
        return Ok(false);
    }
    let db = database::open(home)?;
    let sprint:Option<String>=db.query_row("SELECT id FROM sprints WHERE project_id=?1 AND tracking_issue_number=?2 AND state='waiting'",params![repo.project,number],|r|r.get(0)).optional()?;
    let Some(sprint) = sprint else {
        return Ok(true);
    };
    let snapshot = latest(&db, &sprint)?;
    let path = repo.path()?;
    let issue = api::get(api, &format!("{path}/issues/{number}")).await?;
    if issue["state"] != "open" || !issue["pull_request"].is_null() {
        return Ok(true);
    }
    let id = c["id"].as_u64().context("Missing comment ID")?;
    let fresh = api
        .request("GET", &format!("{path}/issues/comments/{id}"), None, None)
        .await?;
    if fresh.status == 404 {
        return Ok(true);
    }
    ensure!(fresh.status == 200, "Sprint comment refetch failed");
    if fresh.body != *c {
        return Ok(true);
    }
    let (snapshot_comment, published): (u64, String) = db.query_row(
        "SELECT comment_id,published_at FROM sprint_snapshots WHERE id=?1",
        [&snapshot],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let observed = api
        .request(
            "GET",
            &format!("{path}/issues/comments/{snapshot_comment}"),
            None,
            None,
        )
        .await?;
    if observed.status == 404 {
        return Ok(true);
    }
    ensure!(observed.status == 200, "Snapshot refetch failed");
    if observed.body["user"]["id"].as_u64() != Some(repo.app_bot_id)
        || observed.body["created_at"] != published
        || observed.body["updated_at"] != observed.body["created_at"]
        || !observed.body["body"]
            .as_str()
            .is_some_and(|b| b.contains(&format!("<!-- plantool-snapshot:{snapshot} -->")))
    {
        return Ok(true);
    }
    let last = api
        .request("GET", &format!("{path}/issues/comments/{id}"), None, None)
        .await?;
    if last.status == 404 {
        return Ok(true);
    }
    ensure!(last.status == 200, "Final sprint command refetch failed");
    if last.body != *c {
        return Ok(true);
    }
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !gates::valid_identity(&tx, c) || latest(&tx, &sprint)? != snapshot {
        return Ok(true);
    }
    let (published,payload):(String,String)=tx.query_row("SELECT published_at,payload_json FROM sprint_snapshots WHERE id=?1 AND comment_id IS NOT NULL",[&snapshot],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let payload: Value = serde_json::from_str(&payload)?;
    let mut entries = vec![];
    for entry in payload["items"]
        .as_array()
        .context("Snapshot missing items")?
    {
        let item = entry["item"].as_str().unwrap();
        let current:String=tx.query_row("SELECT content_sha256 FROM revisions WHERE item_id=?1 AND doc_kind='plan' ORDER BY number DESC LIMIT 1",[item],|r|r.get(0))?;
        entries.push((
            item.to_owned(),
            entry["hash"].as_str().unwrap().to_owned(),
            current,
            entry["issue"].as_u64().unwrap(),
        ));
    }
    let normalized = if let Command::ApproveItem(target) = command {
        Command::ApproveItem(
            entries
                .iter()
                .find(|e| e.0 == *target || e.3.to_string() == target.trim_start_matches('#'))
                .map(|e| e.0.clone())
                .unwrap_or_else(|| target.clone()),
        )
    } else {
        command.clone()
    };
    let refs = entries
        .iter()
        .map(|e| (e.0.as_str(), e.1.as_str(), e.2.as_str()))
        .collect::<Vec<_>>();
    let context = Verification {
        place: Place::TrackingIssue,
        opened_at: &published,
        snapshot_at: Some(&published),
        pending_hash: None,
        gate_commit: None,
        current_commit: None,
        snapshot_items: &refs,
    };
    if let Err(error) = commands::verify(&tx, c, &normalized, &context) {
        tx.rollback()?;
        gates::near_miss(home, repo, number, id, &error.to_string())?;
        return Ok(true);
    }
    let project_item = format!("{}:project", repo.project);
    ensure!(
        workflow::state(&tx, &project_item)?.is_some_and(|s| s["frozen"] != true),
        "Project is frozen"
    );
    ensure!(
        workflow::state(&tx, &project_item)?.is_some_and(|s| s["step"] == "await_sprint_gate"),
        "Sprint snapshot is being replanned; wait for its replacement"
    );
    let mut approved = 0;
    let parent_gate = format!("sprint-approval:{sprint}:{id}");
    let decision_id = uuid::Uuid::new_v4().to_string();
    for (item, hash, current, _) in entries {
        if let Command::ApproveItem(target) = &normalized {
            if *target != item {
                continue;
            }
        }
        ensure!(hash == current, "Snapshot revision changed");
        let gate:Option<(String,String)>=tx.query_row("SELECT g.id,c.revision_id FROM gates g JOIN github_gate_context c ON c.gate_id=g.id JOIN work_items i ON i.id=g.item_id WHERE g.item_id=?1 AND g.kind='plan' AND g.state='open' AND g.target_digest=?2 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id) ORDER BY g.created_at DESC LIMIT 1",params![item,hash],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((gate, revision)) = gate else {
            let started:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sprint_items WHERE sprint_id=?1 AND item_id=?2 AND plan_sha=?3 AND state IN('approved','started'))",params![sprint,item,hash],|r|r.get(0))?;
            ensure!(started, "Snapshot plan gate or policy changed");
            continue;
        };
        ensure!(
            workflow::state(&tx, &item)?.is_some_and(|s| s["frozen"] != true),
            "Sprint item is frozen"
        );
        if approved == 0 {
            let digest: String = tx.query_row(
                "SELECT digest FROM sprint_snapshots WHERE id=?1",
                [&snapshot],
                |r| r.get(0),
            )?;
            tx.execute("INSERT INTO gates(id,item_id,kind,target_digest,policy_version,state,created_at,closed_at) SELECT ?1,?2,'sprint',?3,max(version),'approved',?4,?4 FROM project_settings WHERE project_id=?5",params![parent_gate,project_item,digest,plantool_core::now(),repo.project])?;
            tx.execute("INSERT INTO decisions(id,gate_id,source,actor_github_id,source_event_id,decision,created_at) VALUES(?1,?2,'github',?3,?4,'approved',?5)",params![decision_id,parent_gate,c["user"]["id"].as_u64(),id.to_string(),plantool_core::now()])?;
        }
        tx.execute(
            "INSERT INTO decision_targets VALUES(?1,?2,?3,?4)",
            params![decision_id, gate, item, hash],
        )?;
        tx.execute(
            "UPDATE gates SET state='approved',closed_at=?2 WHERE id=?1",
            params![gate, plantool_core::now()],
        )?;
        tx.execute("UPDATE work_items SET approved_plan_revision_id=?2,state_json=json_set(state_json,'$.plan_revision_pending',0) WHERE id=?1",params![item,revision])?;
        tx.execute("UPDATE sprint_items SET state='approved' WHERE sprint_id=?1 AND item_id=?2 AND plan_sha=?3",params![sprint,item,hash])?;
        approved += 1;
    }
    if approved > 0 {
        scheduler::event(
            &tx,
            &repo.project,
            "sprint_decision",
            &json!({"sprint":sprint,"snapshot":snapshot}),
            &format!("decision:{id}"),
        )?;
        database::audit(
            &tx,
            "github",
            "sprint.approved",
            &snapshot,
            &json!({"comment":id,"items":approved}),
        )?;
    }
    tx.commit()?;
    Ok(true)
}

/// A changed pending revision gets a fresh immutable snapshot; old comments stay put.
pub fn refresh(home: &Path, repo: &Repository) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let sprints = {
        let mut q = tx.prepare(
            "SELECT id FROM sprints WHERE project_id=?1 AND state IN('waiting','publishing')",
        )?;
        let rows = q
            .query_map([&repo.project], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for sprint in sprints {
        let old = latest(&tx, &sprint)?;
        let raw: String = tx.query_row(
            "SELECT payload_json FROM sprint_snapshots WHERE id=?1",
            [&old],
            |r| r.get(0),
        )?;
        let mut p: Value = serde_json::from_str(&raw)?;
        let mut changed = false;
        for entry in p["items"]
            .as_array_mut()
            .context("Snapshot items missing")?
        {
            let item = entry["item"].as_str().unwrap().to_owned();
            let (version,hash):(u64,String)=tx.query_row("SELECT number,content_sha256 FROM revisions WHERE item_id=?1 AND doc_kind='plan' ORDER BY number DESC LIMIT 1",[&item],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if entry["hash"] != hash {
                entry["hash"] = json!(hash);
                entry["version"] = json!(version);
                changed = true;
                tx.execute("UPDATE sprint_items SET plan_sha=?3,state='pending' WHERE sprint_id=?1 AND item_id=?2",params![sprint,item,hash])?;
            }
        }
        let throughput:i64=tx.query_row("SELECT count(*) FROM work_items WHERE project_id=?1 AND state='merged' AND closed_at>=?2",params![repo.project,(chrono::Utc::now()-chrono::Duration::days(28)).timestamp()],|r|r.get(0))?;
        if p["recent_throughput"] != throughput {
            p["recent_throughput"] = json!(throughput);
            changed = true;
        }
        if changed {
            let digest = plantool_core::sha256_hex(&p.to_string());
            let id = format!("{sprint}:{digest}");
            tx.execute(
                "INSERT OR IGNORE INTO sprint_snapshots VALUES(?1,?2,?3,?4,NULL,NULL,?5)",
                params![id, sprint, digest, p.to_string(), plantool_core::now()],
            )?;
            tx.execute(
                "UPDATE sprints SET state='publishing' WHERE id=?1",
                [&sprint],
            )?;
            dispatcher::enqueue(
                &tx,
                &format!("{}:project", repo.project),
                "github.sprint",
                &json!({"snapshot":id,"sprint":sprint}),
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}
