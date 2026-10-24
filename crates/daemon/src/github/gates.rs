//! Plan gates are pinned to immutable revisions and published by the App, never agents.
use super::{
    api::{self, Api},
    commands::{self, Command},
    setup::Repository,
};
use crate::{database, dispatcher};
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::Path;
pub fn request(
    tx: &Transaction<'_>,
    item: &str,
    digest: &str,
    thread: &str,
    interrupt: &str,
) -> anyhow::Result<String> {
    request_revision(tx, item, digest, thread, interrupt, "plan")
}
pub fn request_revision(
    tx: &Transaction<'_>,
    item: &str,
    digest: &str,
    thread: &str,
    interrupt: &str,
    kind: &str,
) -> anyhow::Result<String> {
    ensure!(
        ["plan", "research"].contains(&kind),
        "Unknown revision gate"
    );
    ensure!(
        !thread.is_empty() && !interrupt.is_empty(),
        "Revision gate requires waiting graph identity"
    );
    let (revision,current,project,issue):(String,String,String,i64)=tx.query_row("SELECT r.id,r.content_sha256,i.project_id,i.github_issue_number FROM work_items i JOIN revisions r ON r.item_id=i.id AND r.doc_kind=?2 WHERE i.id=?1 AND i.kind='eos' ORDER BY r.number DESC LIMIT 1",params![item,kind],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    ensure!(
        current == digest.trim_start_matches("sha256:"),
        "Gate must target latest revision"
    );
    ensure!(
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM github_repositories WHERE project_id=?1)",
            [&project],
            |r| r.get::<_, bool>(0)
        )? && issue > 0,
        "GitHub binding required"
    );
    if kind == "plan" {
        tx.execute("UPDATE work_items SET state_json=json_set(state_json,'$.plan_revision_pending',1) WHERE id=?1",[item])?;
    }
    tx.execute("UPDATE gates SET state='superseded',closed_at=?3 WHERE item_id=?1 AND kind=?2 AND state IN('pending','open')",params![item,kind,plantool_core::now()])?;
    let id = uuid::Uuid::new_v4().to_string();
    let policy: i64 = tx.query_row(
        "SELECT coalesce(max(version),0) FROM project_settings WHERE project_id=?1",
        [project],
        |r| r.get(0),
    )?;
    tx.execute("INSERT INTO gates(id,item_id,kind,target_digest,policy_version,state,created_at) VALUES(?1,?2,?3,?4,?5,'pending',?6)",params![id,item,kind,current,policy,plantool_core::now()])?;
    tx.execute("INSERT INTO github_gate_context(gate_id,revision_id,thread_id,interrupt_id,publication_key) VALUES(?1,?2,?3,?4,?1)",params![id,revision,thread,interrupt])?;
    dispatcher::enqueue(
        tx,
        item,
        if kind == "plan" {
            "github.plan"
        } else {
            "github.research"
        },
        &json!({"gate":id}),
    )?;
    database::audit(
        tx,
        "worker",
        &format!("github.{kind}_requested"),
        &id,
        &json!({"revision":revision,"digest":current}),
    )?;
    Ok(id)
}

fn timestamp(value: &str) -> anyhow::Result<chrono::DateTime<chrono::FixedOffset>> {
    Ok(chrono::DateTime::parse_from_rfc3339(value)?)
}
pub fn valid_identity(db: &rusqlite::Connection, c: &Value) -> bool {
    let Some(id) = c["user"]["id"].as_u64() else {
        return false;
    };
    c["user"]["type"] == "User"
        && db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM allowlist WHERE github_user_id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false)
}
pub fn hash_matches(prefix: &str, current: &str) -> bool {
    let hash = current.trim_start_matches("sha256:");
    (12..=64).contains(&hash.len())
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && (12..=64).contains(&prefix.len())
        && prefix.bytes().all(|b| b.is_ascii_hexdigit())
        && hash
            .to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
}
pub async fn comment(home: &Path, api: &dyn Api, repo: &Repository, id: u64) -> anyhow::Result<()> {
    // Never trust the list object: re-fetch the authoritative object, including deletions.
    let base = repo.path()?;
    let response = api
        .request("GET", &format!("{base}/issues/comments/{id}"), None, None)
        .await?;
    if response.status == 404 {
        return Ok(());
    }
    ensure!(
        response.status == 200,
        "Comment refetch failed ({})",
        response.status
    );
    let c = response.body;
    ensure!(c["id"].as_u64() == Some(id), "Comment identity mismatch");
    let Some(number) = c["issue_url"]
        .as_str()
        .and_then(|s| s.strip_prefix(&format!("https://api.github.com{base}/issues/")))
        .and_then(|s| s.parse::<i64>().ok())
    else {
        return Ok(());
    };
    let body = c["body"].as_str().unwrap_or("");
    let db = database::open(home)?;
    if !valid_identity(&db, &c) {
        return Ok(());
    }
    let Some(command) = commands::parse(body) else {
        if body.lines().any(|line| line.starts_with("/plantool ")) {
            near_miss(
                home,
                repo,
                number,
                id,
                "Command does not match the exact supported syntax",
            )?
        }
        return Ok(());
    };
    if super::milestones::handle(home, api, repo, &c, number, &command).await? {
        return Ok(());
    }
    if super::sprints::handle(home, api, repo, &c, number, &command).await? {
        return Ok(());
    }
    // Phase 5 commands remain parsed but have no executor.
    if !matches!(
        command,
        Command::ApprovePlan(_) | Command::ApproveResearch(_) | Command::RequestChanges(_)
    ) {
        return Ok(());
    }
    let gate:Option<(String,String,String,String,String,u64)>=db.query_row("SELECT g.id,g.item_id,g.target_digest,g.created_at,c.revision_id,g.policy_version FROM gates g JOIN github_gate_context c ON c.gate_id=g.id JOIN work_items i ON i.id=g.item_id WHERE i.project_id=?1 AND i.github_issue_number=?2 AND g.kind IN('plan','research') AND g.state='open' ORDER BY g.created_at DESC LIMIT 1",params![repo.project,number],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((gate, item, digest, opened, revision, policy)) = gate else {
        return Ok(());
    };
    let research: bool = db.query_row(
        "SELECT kind='research' FROM gates WHERE id=?1",
        [&gate],
        |r| r.get(0),
    )?;
    if (research && matches!(command, Command::ApprovePlan(_)))
        || (!research && matches!(command, Command::ApproveResearch(_)))
    {
        return Ok(());
    }
    let context = commands::Context {
        place: commands::Place::Issue,
        opened_at: &opened,
        snapshot_at: None,
        pending_hash: Some(&digest),
        gate_commit: None,
        current_commit: None,
        snapshot_items: &[],
    };
    if let Err(e) = commands::verify(&db, &c, &command, &context) {
        near_miss(home, repo, number, id, &e.to_string())?;
        return Ok(());
    }
    // Re-fetch the issue too; comments cannot approve a deleted/closed/PR target.
    let issue = api::get(api, &format!("{base}/issues/{number}")).await?;
    if issue["state"] != "open" || !issue["pull_request"].is_null() {
        return Ok(());
    }
    // Revalidate last, after the issue lookup: an edit/deletion during that
    // request must not turn the earlier representation into an approval.
    let fresh = api
        .request("GET", &format!("{base}/issues/comments/{id}"), None, None)
        .await?;
    if fresh.status == 404 {
        return Ok(());
    }
    ensure!(
        fresh.status == 200,
        "Final comment refetch failed ({})",
        fresh.status
    );
    if fresh.body != c {
        return Ok(());
    }
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !valid_identity(&tx, &c) {
        return Ok(());
    }
    let unused: bool = tx.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM decisions WHERE source='github' AND source_event_id=?1)",
        [id.to_string()],
        |r| r.get(0),
    )?;
    if !unused {
        return Ok(());
    }
    let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM gates g JOIN github_gate_context c ON c.gate_id=g.id JOIN revisions r ON r.id=c.revision_id JOIN work_items i ON i.id=g.item_id WHERE g.id=?1 AND g.state='open' AND c.revision_id=?2 AND r.id=(SELECT id FROM revisions WHERE item_id=g.item_id AND doc_kind=g.kind ORDER BY number DESC LIMIT 1) AND g.target_digest=r.content_sha256 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id))",params![gate,revision],|r|r.get(0))?;
    if !current {
        tx.rollback()?;
        near_miss(
            home,
            repo,
            number,
            id,
            "The current plan revision or policy changed; a new plan publication is required",
        )?;
        return Ok(());
    }
    let decision = if matches!(
        command,
        Command::ApprovePlan(_) | Command::ApproveResearch(_)
    ) {
        "approved"
    } else {
        "changes_requested"
    };
    tx.execute("INSERT INTO decisions(id,gate_id,source,actor_github_id,source_event_id,decision,created_at) VALUES(?1,?2,'github',?3,?4,?5,?6)",params![uuid::Uuid::new_v4().to_string(),gate,c["user"]["id"].as_u64(),id.to_string(),decision,plantool_core::now()])?;
    tx.execute(
        "UPDATE gates SET state=?2,closed_at=?3 WHERE id=?1",
        params![gate, decision, plantool_core::now()],
    )?;
    if decision == "approved" && !research {
        tx.execute("UPDATE work_items SET approved_plan_revision_id=?2,state_json=json_set(state_json,'$.plan_revision_pending',0) WHERE id=?1",params![item,revision])?;
    }
    if research {
        crate::workflow::research_decision(&tx, &item, decision == "approved")?;
    } else {
        crate::workflow::gate_decision(&tx, &item, decision == "approved")?;
    }
    let (thread, interrupt): (String, String) = tx.query_row(
        "SELECT thread_id,interrupt_id FROM github_gate_context WHERE gate_id=?1",
        [&gate],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let resume = plantool_contract::Resume {
        thread_id: thread,
        interrupt_id: interrupt,
        value: json!({"gate":gate,"decision":decision,"plan_sha":digest,"policy_version":policy,"reason":if let Command::RequestChanges(reason)=&command{Some(reason)}else{None}}),
        idempotency_key: format!("github-decision:{id}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, &item, "worker.resume", &serde_json::to_value(resume)?)?;
    database::audit(
        &tx,
        "github",
        "github.decision",
        &gate,
        &json!({"comment_id":id,"actor_id":c["user"]["id"],"decision":decision,"digest":digest}),
    )?;
    tx.commit()?;
    Ok(())
}
pub(super) fn near_miss(
    home: &Path,
    repo: &Repository,
    issue: i64,
    id: u64,
    message: &str,
) -> anyhow::Result<()> {
    database::inbox_once(home, "github.near_miss", &id.to_string(), |tx| {
        dispatcher::enqueue(
            tx,
            &repo.project,
            "github.reply",
            &json!({"project":repo.project,"issue":issue,"body":format!("PlanTool: {message}. No decision was recorded."),"comment":id}),
        )?;
        Ok(json!({"queued":true}))
    })?;
    Ok(())
}
/// Paginate all comments before reconciling an uncertain publication.
async fn find_publication(
    api: &dyn Api,
    base: &str,
    issue: i64,
    bot: u64,
    marker: &str,
) -> anyhow::Result<Option<Value>> {
    for page in 1..=1000 {
        let r = api
            .request(
                "GET",
                &format!("{base}/issues/{issue}/comments?per_page=100&page={page}"),
                None,
                None,
            )
            .await?;
        ensure!(
            r.status == 200,
            "Publication reconciliation failed ({})",
            r.status
        );
        let values = r.body.as_array().context("Expected comments")?;
        for c in values {
            if c["user"]["id"].as_u64() == Some(bot)
                && c["user"]["type"] == "Bot"
                && c["body"].as_str().is_some_and(|b| b.contains(marker))
            {
                return Ok(Some(c.clone()));
            }
        }
        if !r.next {
            return Ok(None);
        }
    }
    anyhow::bail!("Too many comments to reconcile safely")
}
pub async fn publish(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT o.id,o.payload_json,o.attempts,o.kind FROM outbox o WHERE delivered_at IS NULL AND kind IN('github.plan','github.research','github.reply') AND (aggregate_id=?1 OR aggregate_id IN(SELECT id FROM work_items WHERE project_id=?1)) ORDER BY created_at,id")?;
        let r = q.query_map([&repo.project], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        r.collect::<Result<Vec<_>, _>>()?
    };
    for (outbox, payload, attempts, kind) in rows {
        let p: Value = serde_json::from_str(&payload)?;
        let marker = format!("<!-- plantool:{outbox} -->");
        let base = repo.path()?;
        let (issue, body, gate) = if kind == "github.plan" || kind == "github.research" {
            let db = database::open(home)?;
            let g = p["gate"].as_str().context("Missing gate")?;
            let (issue,sha,number,state,revision):(i64,String,i64,String,String)=db.query_row("SELECT i.github_issue_number,r.content_sha256,r.number,g.state,r.id FROM gates g JOIN github_gate_context c ON c.gate_id=g.id JOIN revisions r ON r.id=c.revision_id JOIN work_items i ON i.id=g.item_id WHERE g.id=?1",[g],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            let current:bool=db.query_row("SELECT id=?2 FROM revisions WHERE item_id=(SELECT item_id FROM revisions WHERE id=?2) AND doc_kind=(SELECT kind FROM gates WHERE id=?1) ORDER BY number DESC LIMIT 1",params![g,revision],|r|r.get(0))?;
            if state != "pending" || !current {
                db.execute(
                    "UPDATE outbox SET delivered_at=?2,result_json='{}' WHERE id=?1",
                    params![outbox, plantool_core::now()],
                )?;
                continue;
            }
            let gate_item: String =
                db.query_row("SELECT item_id FROM gates WHERE id=?1", [g], |r| r.get(0))?;
            if crate::workflow::state(&db, &gate_item)?.is_some_and(|s| s["frozen"] == true) {
                continue;
            }
            let plan = String::from_utf8(crate::artifacts::read(home, &sha)?)?;
            let (root,patterns):(String,String)=db.query_row("SELECT p.local_checkout,coalesce((SELECT never_sent_globs_json FROM project_settings WHERE project_id=p.id ORDER BY version DESC LIMIT 1),'[]') FROM projects p WHERE p.id=?1",[&repo.project],|r|Ok((r.get(0)?,r.get(1)?)))?;
            crate::models::proxy::check_payload(
                Path::new(&root),
                &crate::sandbox::NeverSent::new(&serde_json::from_str::<Vec<String>>(&patterns)?)?,
                &json!({"plan":plan}),
            )?;
            let summary: String = db.query_row(
                "SELECT summary FROM github_gate_context WHERE gate_id=?1",
                [g],
                |r| r.get(0),
            )?;
            crate::models::proxy::check_payload(
                Path::new(&root),
                &crate::sandbox::NeverSent::new(&serde_json::from_str::<Vec<String>>(&patterns)?)?,
                &json!({"summary":summary}),
            )?;
            let port = crate::store::read_daemon_info(home)?
                .map(|i| i.port)
                .unwrap_or_else(crate::config::default_port);
            let label = if kind == "github.research" {
                "Research"
            } else {
                "Plan"
            };
            let approval_command = if kind == "github.research" {
                "approve-research"
            } else {
                "approve-plan"
            };
            let body=format!("### {label} v{number} · sha256:{}\n\n<details><summary>Full hash</summary>\n\nsha256:{sha}\n\n</details>\n\n{summary}\n\n[PlanTool thread and immutable plan](http://localhost:{port}/github/plans/{g})\n\n{plan}\n\nTo approve: `/plantool {approval_command} sha256:{}`\n\n{marker}",&sha[..12],&sha[..12]);
            (issue, body, Some(g.to_string()))
        } else {
            let comment = p["comment"].as_u64().context("Missing reply source")?;
            let source = api
                .request(
                    "GET",
                    &format!("{base}/issues/comments/{comment}"),
                    None,
                    None,
                )
                .await?;
            ensure!(
                source.status == 200 || source.status == 404,
                "Reply source refetch failed ({})",
                source.status
            );
            let issue = p["issue"].as_i64().context("Missing issue")?;
            if crate::workflow::state(&database::open(home)?, &format!("{}:{issue}", repo.project))?
                .is_some_and(|s| s["frozen"] == true)
            {
                continue;
            }
            let expected_url = format!("https://api.github.com{base}/issues/{issue}");
            if source.status == 404
                || !valid_identity(&database::open(home)?, &source.body)
                || source.body["issue_url"].as_str() != Some(expected_url.as_str())
            {
                database::open(home)?.execute(
                    "UPDATE outbox SET delivered_at=?2,result_json=?3 WHERE id=?1",
                    params![
                        outbox,
                        plantool_core::now(),
                        json!({"ignored":"reply author is no longer eligible"}).to_string()
                    ],
                )?;
                continue;
            }
            (
                issue,
                format!(
                    "{}\n\n{marker}",
                    p["body"].as_str().context("Missing reply")?
                ),
                None,
            )
        };
        let existing = find_publication(api, &base, issue, repo.app_bot_id, &marker).await?;
        let comment = if let Some(existing) = existing {
            existing
        } else {
            ensure!(
                attempts == 0,
                "Publication outcome uncertain; inspect GitHub before retrying (outbox {outbox})"
            );
            database::open(home)?.execute(
                "UPDATE outbox SET attempts=attempts+1 WHERE id=?1",
                [&outbox],
            )?;
            let r = api
                .request(
                    "POST",
                    &format!("{base}/issues/{issue}/comments"),
                    Some(json!({"body":body})),
                    None,
                )
                .await?;
            if matches!(r.status, 401 | 403 | 404 | 422 | 429) {
                database::open(home)?.execute("UPDATE outbox SET attempts=0,result_json=?2 WHERE id=?1 AND delivered_at IS NULL",params![outbox,json!({"http_status":r.status,"outcome":"not_created"}).to_string()])?;
                anyhow::bail!(
                    "Comment was not created (HTTP {}); retry after the API error clears",
                    r.status
                );
            }
            ensure!(
                r.status == 201,
                "Comment publication outcome requires reconciliation ({})",
                r.status
            );
            r.body
        };
        ensure!(
            comment["user"]["id"].as_u64() == Some(repo.app_bot_id)
                && comment["user"]["type"] == "Bot",
            "Publication must be App-authored"
        );
        let comment_id = comment["id"].as_u64().context("Missing comment ID")?;
        let created = comment["created_at"]
            .as_str()
            .context("Missing publication time")?;
        timestamp(created)?;
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(gate) = &gate {
            tx.execute("UPDATE gates SET state='open',github_comment_id=?2,created_at=?3 WHERE id=?1 AND state='pending'",params![gate,comment_id,created])?;
        }
        tx.execute(
            "UPDATE outbox SET delivered_at=?2,result_json=?3 WHERE id=?1",
            params![
                outbox,
                plantool_core::now(),
                json!({"comment_id":comment_id}).to_string()
            ],
        )?;
        if let Some(gate) = &gate {
            if kind == "github.plan"
                && crate::workflow::state(&tx, &format!("{}:project", repo.project))?.is_some()
            {
                let item: String =
                    tx.query_row("SELECT item_id FROM gates WHERE id=?1", [gate], |r| {
                        r.get(0)
                    })?;
                crate::scheduler::event(
                    &tx,
                    &repo.project,
                    "plan_ready",
                    &json!({"item":item,"planning_due":crate::scheduler::schedule(&tx,&repo.project,chrono::Utc::now())?.1.is_some()}),
                    &format!("plan:{gate}"),
                )?;
            }
        }
        tx.commit()?;
    }
    Ok(())
}
