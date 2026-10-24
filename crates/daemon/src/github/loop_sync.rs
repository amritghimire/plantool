//! Verified loop intake and durable GitHub effects. Never execute model-authored commands.
use super::{
    api::Api,
    lifecycle::{pages, write},
    setup::Repository,
};
use crate::{database, dispatcher, loop_engine, workflow};
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
fn labeled(issue: &Value, label: &str) -> bool {
    issue["labels"]
        .as_array()
        .is_some_and(|a| a.iter().any(|l| l["name"] == label))
}
fn stamp(value: &str) -> anyhow::Result<chrono::DateTime<chrono::Utc>> {
    Ok(chrono::DateTime::parse_from_rfc3339(value)?.with_timezone(&chrono::Utc))
}
pub fn admitted(home: &Path, repo: &Repository, issue: &Value) -> anyhow::Result<bool> {
    let n = issue["number"].as_u64().context("Issue number missing")?;
    let db = database::open(home)?;
    let finding:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM loop_routes r JOIN work_items i ON i.id=r.item_id WHERE i.project_id=?1 AND r.issue_number=?2 AND r.route<>'proposed')",params![repo.project,n],|r|r.get(0))?;
    let urgent:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM loop_intake WHERE project_id=?1 AND issue_number=?2 AND urgent=1)",params![repo.project,n],|r|r.get(0))?;
    let accepted = labeled(issue, "plantool:accept")
        && super::sync::authorized_label(home, &repo.project, n as i64, "plantool:accept")?;
    let owner = super::gates::valid_identity(&db, issue);
    if !(owner || accepted || finding || urgent) {
        return Ok(false);
    }
    let item = format!("{}:{n}", repo.project);
    db.execute("INSERT OR IGNORE INTO loop_intake(item_id,project_id,issue_number,source,admitted_at) VALUES(?1,?2,?3,?4,?5)",params![item,repo.project,n,if finding {"agent"}else if accepted {"accepted"}else{"allowlist"},plantool_core::now()])?;
    Ok(true)
}
pub fn promote(
    tx: &rusqlite::Transaction<'_>,
    repo: &Repository,
    n: u64,
    key: &str,
) -> anyhow::Result<()> {
    let item = format!("{}:{n}", repo.project);
    tx.execute("INSERT INTO loop_intake(item_id,project_id,issue_number,source,admitted_at,urgent) VALUES(?1,?2,?3,'urgent',?4,1) ON CONFLICT(item_id) DO UPDATE SET urgent=1",params![item,repo.project,n,plantool_core::now()])?;
    tx.execute(
        "UPDATE work_items SET priority='urgent' WHERE id=?1",
        [&item],
    )?;
    if let Some(mut s) = workflow::state(tx, &item)? {
        s["urgent"] = json!(true);
        workflow::save(tx, &item, &s)?;
        if s["step"] == "backlog" {
            loop_engine::wake(tx, &item, "research", key)?;
        }
        if s["step"] == "declined_wait" {
            tx.execute(
                "UPDATE loop_declines SET active=0 WHERE item_id=?1",
                [&item],
            )?;
            tx.execute(
                "UPDATE loop_intake SET epoch=epoch+1 WHERE item_id=?1",
                [&item],
            )?;
            loop_engine::wake(tx, &item, "triage", key)?;
        }
    }
    database::audit(tx, "github", "loop.urgent", &item, &json!({"source":key}))?;
    Ok(())
}
pub async fn urgent(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    c: &Value,
    n: i64,
    command: &super::commands::Command,
) -> anyhow::Result<bool> {
    if *command != super::commands::Command::Urgent {
        return Ok(false);
    }
    let base = repo.path()?;
    let issue = api
        .request("GET", &format!("{base}/issues/{n}"), None, None)
        .await?;
    if issue.status != 200 || issue.body["state"] != "open" || !issue.body["pull_request"].is_null()
    {
        return Ok(true);
    }
    let id = c["id"].as_u64().context("Comment ID missing")?;
    let fresh = api
        .request("GET", &format!("{base}/issues/comments/{id}"), None, None)
        .await?;
    if fresh.status != 200 || fresh.body != *c {
        return Ok(true);
    }
    let project_item = crate::scheduler::ensure_project(home, &repo.project)?;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let created: String = tx.query_row(
        "SELECT created_at FROM projects WHERE id=?1",
        [&repo.project],
        |r| r.get(0),
    )?;
    let context = super::commands::Context {
        place: super::commands::Place::Issue,
        opened_at: &created,
        snapshot_at: None,
        pending_hash: None,
        gate_commit: None,
        current_commit: None,
        snapshot_items: &[],
    };
    if super::commands::verify(&tx, c, command, &context).is_err() {
        return Ok(true);
    }
    let item = format!("{}:{n}", repo.project);
    // The command may admit a previously unknown issue; its decision targets the project item.
    let gate = format!("urgent:{id}");
    tx.execute("INSERT OR IGNORE INTO gates(id,item_id,kind,target_digest,policy_version,state,created_at,closed_at) SELECT ?1,?2,'production',?3,max(version),'approved',?4,?4 FROM project_settings WHERE project_id=?5",params![gate,project_item,plantool_core::sha256_hex(&c.to_string()),plantool_core::now(),repo.project])?;
    tx.execute("INSERT INTO decisions(id,gate_id,source,actor_github_id,source_event_id,decision,created_at) VALUES(?1,?1,'github',?2,?3,'urgent',?4)",params![gate,c["user"]["id"].as_u64(),id.to_string(),plantool_core::now()])?;
    promote(&tx, repo, n as u64, &format!("comment:{id}"))?;
    database::set(&tx, &format!("urgent-issue:{item}"), &issue.body)?;
    tx.commit()?;
    Ok(true)
}
pub fn labels(home: &Path, repo: &Repository, issue: &Value) -> anyhow::Result<()> {
    let n = issue["number"].as_u64().context("Missing issue number")?;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if labeled(issue, "plantool:urgent")
        && super::sync::authorized_label(home, &repo.project, n as i64, "plantool:urgent")?
    {
        let event:u64=tx.query_row("SELECT event_id FROM github_label_actors WHERE project_id=?1 AND issue_number=?2 AND label='plantool:urgent'",params![repo.project,n],|r|r.get(0))?;
        let key = format!("urgent-label:{}:{event}", repo.project);
        let fresh=tx.execute("INSERT OR IGNORE INTO inbox(source,external_event_id,processed_at,result) VALUES('loop.label',?1,?2,'{}')",params![key,plantool_core::now()])?;
        if fresh > 0 {
            promote(&tx, repo, n, &key)?;
        }
    }
    if labeled(issue, "plantool:urgent-requested") {
        let item = format!("{}:{n}", repo.project);
        let id = format!("urgent-request:{}:{n}", repo.project);
        tx.execute(
            "INSERT OR IGNORE INTO loop_notifications VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                repo.project,
                item,
                format!(
                    "Urgency requested for issue #{n}; only an allowlisted owner can promote it."
                ),
                plantool_core::now()
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}
async fn comment(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    item: &str,
    n: u64,
    key: &str,
    text: &str,
) -> anyhow::Result<Value> {
    let marker = format!(
        "<!-- plantool-loop:{} -->",
        plantool_core::sha256_hex(&format!("{item}:{key}"))
    );
    let path = format!("{}/issues/{n}/comments", repo.path()?);
    let found = pages(api, &path, None).await?.into_iter().find(|c| {
        c["user"]["id"].as_u64() == Some(repo.app_bot_id)
            && c["user"]["type"] == "Bot"
            && c["body"].as_str().is_some_and(|s| s.contains(&marker))
    });
    let (_, root) = dispatcher::item(&database::open(home)?, item)?;
    crate::models::proxy::check_payload(
        &root,
        &crate::sandbox::NeverSent::new(&crate::execution::Execution::load(home)?.rules(&root))?,
        &json!({"body":text}),
    )?;
    write(
        home,
        api,
        item,
        key,
        "POST",
        &path,
        json!({"body":format!("{text}\n\n{marker}")}),
        found,
        false,
    )
    .await
}
pub async fn declines(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }

    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT d.item_id,i.github_issue_number,d.reason,d.duplicate_of,d.reset_at,d.label_seen,d.epoch,d.active FROM loop_declines d JOIN work_items i ON i.id=d.item_id WHERE i.project_id=?1 AND i.closed_at IS NULL")?;
        let rows = q
            .query_map([&repo.project], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<u64>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (item, n, reason, duplicate, mut reset, seen, epoch, active) in rows {
        if workflow::state(&database::open(home)?, &item)?.is_some_and(|s| s["frozen"] == true) {
            continue;
        }
        let path = format!("{}/issues/{n}", repo.path()?);
        let issue = api.request("GET", &path, None, None).await?;
        ensure!(issue.status == 200, "Decline issue refetch failed");
        if !active {
            if labeled(&issue.body, "plantool:declined") {
                write(
                    home,
                    api,
                    &item,
                    &format!("undecline:{epoch}"),
                    "DELETE",
                    &format!("{path}/labels/plantool%3Adeclined"),
                    Value::Null,
                    None,
                    true,
                )
                .await?;
            }
            continue;
        }
        if !seen {
            if let Some(target) = duplicate {
                let r = api
                    .request(
                        "GET",
                        &format!("{}/issues/{target}", repo.path()?),
                        None,
                        None,
                    )
                    .await?;
                ensure!(
                    r.status == 200
                        && r.body["number"].as_u64() == Some(target)
                        && r.body["pull_request"].is_null(),
                    "Duplicate target must be an existing issue"
                );
            }
            let text = if let Some(n) = duplicate {
                format!("Duplicate of #{n}\n\n{reason}")
            } else {
                format!("Declined: {reason}")
            };
            let published = comment(
                home,
                api,
                repo,
                &item,
                n,
                &format!("decline:{epoch}"),
                &text,
            )
            .await?;
            write(
                home,
                api,
                &item,
                &format!("decline-label:{epoch}"),
                "POST",
                &format!("{path}/labels"),
                json!({"labels":["plantool:declined"]}),
                if labeled(&issue.body, "plantool:declined") {
                    Some(json!({"observed":true}))
                } else {
                    None
                },
                true,
            )
            .await?;
            reset = published["created_at"]
                .as_str()
                .unwrap_or(&reset)
                .to_string();
            database::open(home)?.execute(
                "UPDATE loop_declines SET label_seen=1,reset_at=?2 WHERE item_id=?1 AND epoch=?3",
                params![item, reset, epoch],
            )?;
            continue;
        }
        // Once daily, and fetch the full live comments so edits/deletions/cursors cannot hide resets.
        let daily = format!("{item}:{epoch}:{}", now.date_naive());
        let db = database::open(home)?;
        if database::get::<bool>(&db, &format!("decline-day:{daily}"))?.unwrap_or(false) {
            continue;
        }
        if !labeled(&issue.body, "plantool:declined") {
            db.execute(
                "UPDATE loop_declines SET active=0 WHERE item_id=?1",
                [&item],
            )?;
            database::audit(
                &db,
                "github",
                "decline.label_removed",
                &item,
                &json!({"auto_close":false}),
            )?;
            continue;
        }
        let comments = pages(api, &format!("{path}/comments"), None).await?;
        let mut latest = stamp(&reset)?;
        for c in &comments {
            if super::gates::valid_identity(&db, c) {
                if let Some(t) = c["created_at"].as_str().and_then(|s| stamp(s).ok()) {
                    latest = latest.max(t);
                }
            }
        }
        reset = latest.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        db.execute(
            "UPDATE loop_declines SET reset_at=?2 WHERE item_id=?1 AND epoch=?3",
            params![item, reset, epoch],
        )?;
        if now - latest >= chrono::Duration::days(30) && issue.body["state"] == "open" {
            // Recheck the owner override after network reads and before either side effect.
            let active: bool = database::open(home)?.query_row(
                "SELECT active AND epoch=?2 FROM loop_declines WHERE item_id=?1",
                params![item, epoch],
                |r| r.get(0),
            )?;
            if !active {
                continue;
            }
            comment(home,api,repo,&item,n,&format!("decline-close-comment:{epoch}:{reset}"),"Closing after 30 days declined without an allowlisted owner comment. Reopen and use the PlanTool override to return this item to triage.").await?;
            write(
                home,
                api,
                &item,
                &format!("decline-close:{epoch}:{reset}"),
                "PATCH",
                &path,
                json!({"state":"closed","state_reason":"not_planned"}),
                None,
                true,
            )
            .await?;
            let mut db = database::open(home)?;
            let tx = db.transaction()?;
            tx.execute(
                "UPDATE work_items SET state='declined',closed_at=?2 WHERE id=?1",
                params![item, now.timestamp()],
            )?;
            if let Some(mut s) = workflow::state(&tx, &item)? {
                s["step"] = json!("declined_closed");
                workflow::save(&tx, &item, &s)?;
            }
            tx.execute(
                "UPDATE loop_declines SET active=0 WHERE item_id=?1",
                [&item],
            )?;
            dispatcher::enqueue(
                &tx,
                &item,
                "worker.cancel",
                &json!({"thread_id":item,"idempotency_key":format!("decline-close:{item}:{epoch}")}),
            )?;
            database::audit(
                &tx,
                "daemon",
                "decline.closed",
                &item,
                &json!({"reset_at":reset}),
            )?;
            tx.commit()?;
        }
        database::set(
            &database::open(home)?,
            &format!("decline-day:{daily}"),
            &true,
        )?;
    }
    Ok(())
}
/// Bounded static evidence from regular default-branch blobs; never reads worktree secrets.
pub fn scan_files(home: &Path, root: &Path, base: &str) -> anyhow::Result<Value> {
    let policy =
        crate::sandbox::NeverSent::new(&crate::execution::Execution::load(home)?.rules(root))?;
    let tree = crate::git::git(root, &["ls-tree", "-r", "-z", base])?;
    let mut paths = vec![];
    let mut todos = vec![];
    let mut tests = vec![];
    let mut bytes = 0usize;
    let mut truncated = false;
    for entry in tree.split('\0').filter(|s| !s.is_empty()) {
        let Some((meta, path)) = entry.split_once('\t') else {
            continue;
        };
        if !(meta.starts_with("100644 blob ") || meta.starts_with("100755 blob "))
            || policy.check(Path::new(""), Path::new(path)).is_err()
        {
            continue;
        }
        paths.push(path.to_string());
        if crate::workflow::test_path(path) {
            tests.push(path.to_string());
        }
        if paths.len() > 2000 {
            truncated = true;
            break;
        }
        let size = crate::git::git(root, &["cat-file", "-s", &format!("{base}:{path}")])?
            .parse::<usize>()
            .unwrap_or(usize::MAX);
        if size > 128 * 1024 || bytes + size > 2 * 1024 * 1024 {
            truncated = true;
            continue;
        }
        let content = crate::git::git(root, &["show", &format!("{base}:{path}")]);
        let Ok(content) = content else { continue };
        bytes += size;
        for (index, line) in content.lines().enumerate() {
            if (line.contains("TODO") || line.contains("FIXME")) && todos.len() < 200 {
                todos.push(json!({"path":path,"line":index+1,"text":line.chars().take(500).collect::<String>()}));
            }
        }
    }
    let evidence = json!({"default_sha":base,"files":paths,"test_files":tests,"todo_fixme":todos,"truncated":truncated,"note":"Static inventory is not coverage or proof that tests passed."});
    crate::models::proxy::check_payload(root, &policy, &evidence)?;
    Ok(evidence)
}
pub async fn scans(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    now: chrono::DateTime<chrono::Utc>,
    git: &dyn super::git_ops::GitOps,
) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }

    let Some((week, signature)) =
        loop_engine::scan_due(&database::open(home)?, &repo.project, now)?
    else {
        return Ok(());
    };
    let id = format!("{}:scan:{week}:{signature}", repo.project);
    if workflow::state(&database::open(home)?, &id)?.is_some() {
        return Ok(());
    }
    let root: String = database::open(home)?.query_row(
        "SELECT local_checkout FROM projects WHERE id=?1",
        [&repo.project],
        |r| r.get(0),
    )?;
    let root = Path::new(&root);
    let base = git
        .fetch(api, root, &repo.full_name, &repo.default_branch)
        .await?;
    let mut evidence = scan_files(home, root, &base)?;
    let branch = percent_encoding::utf8_percent_encode(
        &repo.default_branch,
        percent_encoding::NON_ALPHANUMERIC,
    );
    let ci = pages(
        api,
        &format!(
            "{}/actions/runs?branch={branch}&status=completed",
            repo.path()?
        ),
        Some("workflow_runs"),
    )
    .await;
    evidence["ci"] = match ci {
        Ok(runs) => json!({"available":true,"runs":runs.into_iter().take(100).collect::<Vec<_>>()}),
        Err(e) => json!({"available":false,"error":e.to_string()}),
    };
    let advisories = pages(
        api,
        &format!("{}/dependabot/alerts?state=open", repo.path()?),
        None,
    )
    .await;
    evidence["dependency_advisories"] = match advisories {
        Ok(rows) => {
            json!({"available":true,"alerts":rows.into_iter().take(100).collect::<Vec<_>>()})
        }
        Err(e) => {
            json!({"available":false,"error":e.to_string(),"note":"Review GitHub App Dependabot-alert read permission and repository advisory availability."})
        }
    };
    crate::models::proxy::check_payload(
        root,
        &crate::sandbox::NeverSent::new(&crate::execution::Execution::load(home)?.rules(root))?,
        &evidence,
    )?;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    loop_engine::start_aux(&tx, &repo.project, &id, "scan_devops", &evidence, &base)?;
    tx.execute(
        "INSERT OR IGNORE INTO loop_scans VALUES(?1,?2,?3,?4,?5)",
        params![
            id,
            repo.project,
            week,
            evidence.to_string(),
            plantool_core::now()
        ],
    )?;
    database::audit(
        &tx,
        "daemon",
        "loop.scan_scheduled",
        &id,
        &json!({"week":week,"default_sha":base}),
    )?;
    tx.commit()?;
    Ok(())
}
pub async fn findings(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }

    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT r.id,r.item_id,r.title,r.body,r.route,r.reason,r.issue_number,r.tracking_published FROM loop_routes r JOIN work_items i ON i.id=r.item_id WHERE i.project_id=?1 AND (r.issue_number IS NULL OR r.tracking_published=0)")?;
        let rows = q
            .query_map([&repo.project], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<u64>>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, item, title, body, route, reason, number, tracked) in rows {
        if workflow::state(&database::open(home)?, &item)?.is_some_and(|s| s["frozen"] == true) {
            continue;
        }
        let marker = format!("<!-- plantool-finding:{id} -->");
        let number = if let Some(n) = number {
            n
        } else {
            let all = pages(api, &format!("{}/issues?state=all", repo.path()?), None).await?;
            let found = all.into_iter().find(|v| {
                v["user"]["id"].as_u64() == Some(repo.app_bot_id)
                    && v["user"]["type"] == "Bot"
                    && v["body"].as_str().is_some_and(|s| s.contains(&marker))
            });
            let (_, root) = dispatcher::item(&database::open(home)?, &item)?;
            crate::models::proxy::check_payload(
                &root,
                &crate::sandbox::NeverSent::new(
                    &crate::execution::Execution::load(home)?.rules(&root),
                )?,
                &json!({"title":title,"body":body,"reason":reason}),
            )?;
            let labels = if route == "proposed" {
                vec!["plantool:proposed"]
            } else {
                vec![]
            };
            let result=write(home,api,&item,&format!("finding:{id}"),"POST",&format!("{}/issues",repo.path()?),json!({"title":title,"body":format!("{body}\n\nRoute: {route}\nReason: {reason}\n\n{marker}"),"labels":labels}),found,false).await?;
            let n = result["number"]
                .as_u64()
                .context("Finding issue number missing")?;
            let mut db = database::open(home)?;
            let tx = db.transaction()?;
            tx.execute(
                "UPDATE loop_routes SET issue_number=?2 WHERE id=?1",
                params![id, n],
            )?;
            // Keep the original review guide's proposal reference linked to the routed issue.
            if let Some(s) = workflow::state(&tx, &item)? {
                if let Some(source) = s["loop_context"]["proposal_id"].as_str() {
                    tx.execute(
                        "UPDATE workflow_proposals SET issue_number=?2 WHERE id=?1",
                        params![source, n],
                    )?;
                }
            }
            if route != "proposed" {
                tx.execute("INSERT OR IGNORE INTO loop_intake(item_id,project_id,issue_number,source,admitted_at) VALUES(?1,?2,?3,?4,?5)",params![format!("{}:{n}",repo.project),repo.project,n,format!("agent:{route}"),plantool_core::now()])?;
            }
            if workflow::state(&tx, &format!("{}:project", repo.project))?.is_some() {
                crate::scheduler::event(
                    &tx,
                    &repo.project,
                    "finding_filed",
                    &json!({"issue":n,"route":route,"reason":reason}),
                    &format!("finding:{id}"),
                )?;
            }
            tx.commit()?;
            n
        };
        if !tracked {
            let tracking:Option<u64>=database::open(home)?.query_row("SELECT tracking_issue_number FROM sprints WHERE project_id=?1 AND tracking_issue_number IS NOT NULL ORDER BY week DESC LIMIT 1",[&repo.project],|r|r.get(0)).optional()?;
            if let Some(tracking) = tracking {
                comment(
                    home,
                    api,
                    repo,
                    &item,
                    tracking,
                    &format!("finding-tracking:{id}"),
                    &format!("Finding #{number}: {title}\n\nRoute: {route}\nReason: {reason}"),
                )
                .await?;
                database::open(home)?.execute(
                    "UPDATE loop_routes SET tracking_published=1 WHERE id=?1",
                    [&id],
                )?;
            }
        }
    }
    Ok(())
}
pub async fn urgent_requests(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }

    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT o.id,o.aggregate_id,o.payload_json FROM outbox o JOIN work_items i ON i.id=o.aggregate_id WHERE o.kind='github.urgent_requested' AND o.delivered_at IS NULL AND i.project_id=?1")?;
        let rows = q
            .query_map([&repo.project], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, item, body) in rows {
        let v: Value = serde_json::from_str(&body)?;
        let n = v["issue"].as_u64().context("Issue missing")?;
        write(
            home,
            api,
            &item,
            &format!("urgent-request:{id}"),
            "POST",
            &format!("{}/issues/{n}/labels", repo.path()?),
            json!({"labels":["plantool:urgent-requested"]}),
            None,
            true,
        )
        .await?;
        database::open(home)?.execute(
            "UPDATE outbox SET delivered_at=?2 WHERE id=?1",
            params![id, plantool_core::now()],
        )?;
    }
    Ok(())
}
