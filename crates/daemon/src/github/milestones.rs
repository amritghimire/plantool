//! Commit-bound milestone stops and App-authored draft PR publication.
use super::{
    api::{self, Api},
    commands::{self, Command, Context as Verification, Place},
    gates,
    git_ops::GitOps,
    lifecycle::{pages, push, write},
    setup::Repository,
};
use crate::{database, dispatcher, oversight, workflow};
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::Path;
pub fn request(tx: &Transaction<'_>, request: &plantool_contract::Gate) -> anyhow::Result<String> {
    let s = workflow::state(tx, &request.item)?.context("Milestone graph missing")?;
    ensure!(
        s["step"] == "milestone" && s["frozen"] != true && oversight::stop_required(&s)?,
        "Not at a required milestone"
    );
    ensure!(
        oversight::complete(tx, &request.item, &s)?,
        "Phase tasks incomplete"
    );
    ensure!(
        request.target_digest.trim_start_matches("sha256:") == s["plan_sha"].as_str().unwrap_or(""),
        "Milestone plan mismatch"
    );
    let (head, base, qa, review): (String, String, Option<String>, Option<String>) = tx.query_row(
        "SELECT head_sha,base_sha,qa_head,reviewed_head FROM workflow_items WHERE item_id=?1",
        [&request.item],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    ensure!(
        request.target_sha.as_deref() == Some(&head)
            && qa.as_deref() == Some(&head)
            && review.as_deref() == Some(&head),
        "Milestone needs current-head QA and security review"
    );
    let phase = s["phase_index"]
        .as_u64()
        .context("Milestone phase missing")?;
    let (project, _) = dispatcher::item(tx, &request.item)?;
    let policy: u64 = tx.query_row(
        "SELECT max(version) FROM project_settings WHERE project_id=?1",
        [project],
        |r| r.get(0),
    )?;
    let approved:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_items i JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE i.id=?1 AND r.content_sha256=?2 AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0)",params![request.item,s["plan_sha"].as_str()],|r|r.get(0))?;
    ensure!(approved, "Approved plan required for milestone");
    let digest = plantool_core::sha256_hex(&format!("{}:{phase}:{head}:{policy}", s["plan_sha"]));
    let id = format!("milestone:{}:{digest}", request.item);
    tx.execute("UPDATE gates SET state='superseded',closed_at=?3 WHERE item_id=?1 AND kind='milestone' AND id IN(SELECT gate_id FROM milestone_context WHERE phase_index=?2) AND id<>?4",params![request.item,phase,plantool_core::now(),id])?;
    tx.execute("INSERT OR IGNORE INTO gates(id,item_id,kind,target_digest,target_sha,policy_version,state,created_at) VALUES(?1,?2,'milestone',?3,?4,?5,'pending',?6)",params![id,request.item,s["plan_sha"].as_str(),head,policy,plantool_core::now()])?;
    tx.execute("INSERT OR IGNORE INTO milestone_context(gate_id,phase_index,previous_sha,thread_id,interrupt_id) VALUES(?1,?2,?3,?4,?5)",params![id,phase,s["last_milestone_sha"].as_str().unwrap_or(&base),request.thread_id.as_deref().context("Milestone thread missing")?,request.interrupt_id.as_deref().context("Milestone interrupt missing")?])?;
    dispatcher::enqueue(tx, &request.item, "github.milestone", &json!({"gate":id}))?;
    Ok(id)
}
pub async fn publish(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    git: &dyn GitOps,
) -> anyhow::Result<()> {
    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT g.id,g.item_id,g.target_sha,c.previous_sha,c.phase_index FROM gates g JOIN milestone_context c ON c.gate_id=g.id JOIN work_items i ON i.id=g.item_id WHERE i.project_id=?1 AND g.state='pending' ORDER BY g.created_at")?;
        let rows = q
            .query_map([&repo.project], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, usize>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let path = repo.path()?;
    for (gate, item, head, previous, phase) in rows {
        let db = database::open(home)?;
        let s = workflow::state(&db, &item)?.context("Milestone item missing")?;
        if s["frozen"] == true {
            continue;
        }
        ensure!(
            s["step"] == "milestone" && s["phase_index"] == phase,
            "Milestone state changed"
        );
        let (root,issue,slug,title,saved,base):(String,u64,String,String,Option<String>,String)=db.query_row("SELECT p.local_checkout,i.github_issue_number,i.slug,i.title,w.pr_head,w.base_sha FROM work_items i JOIN projects p ON p.id=i.project_id JOIN workflow_items w ON w.item_id=i.id WHERE i.id=?1",[&item],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
        let current = git
            .fetch(api, Path::new(&root), &repo.full_name, &repo.default_branch)
            .await?;
        if base != current {
            // Rebase changes commit identity. Invalidate the stop before rerunning checks.
            super::lifecycle::rebase(home, &item, Path::new(&root), &current, false)?;
            let mut db = database::open(home)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            tx.execute(
                "UPDATE gates SET state='superseded',closed_at=?2 WHERE id=?1",
                params![gate, plantool_core::now()],
            )?;
            resume(&tx, &gate, &json!({"event":"rebase"}))?;
            tx.commit()?;
            continue;
        }
        let branch = format!("plantool/{issue}-{}", crate::git::sanitize_slug(&slug));
        let owner = repo.full_name.split('/').next().unwrap();
        let found = pages(
            api,
            &format!("{path}/pulls?state=all&head={owner}:{branch}"),
            None,
        )
        .await?
        .into_iter()
        .find(|p| p["user"]["id"].as_u64() == Some(repo.app_bot_id) && p["head"]["ref"] == branch);
        let old = found.as_ref().and_then(|p| p["head"]["sha"].as_str());
        ensure!(
            old.is_none() || old == Some(&head) || old == saved.as_deref(),
            "Milestone remote branch changed outside the workflow"
        );
        push(
            home,
            api,
            git,
            repo,
            &item,
            Path::new(&root),
            &branch,
            &head,
            old,
        )
        .await?;
        let pr = if let Some(pr) = found {
            ensure!(
                pr["state"] == "open" && pr["draft"] == true,
                "Milestones require an open draft PR"
            );
            pr
        } else {
            write(home,api,&item,"pr-create","POST",&format!("{path}/pulls"),json!({"title":title,"head":branch,"base":repo.default_branch,"draft":true,"body":format!("Milestone work for #{issue}. Approved plan sha256:{}. Final review guide follows after milestone gates.",s["plan_sha"].as_str().unwrap())}),None,false).await?
        };
        ensure!(
            pr["user"]["id"].as_u64() == Some(repo.app_bot_id),
            "Draft PR must be App-authored"
        );
        let n = pr["number"].as_u64().context("Draft PR number missing")?;
        let preset: plantool_core::presets::Preset = serde_json::from_value(s["preset"].clone())?;
        let phases = s["phases"].as_array().context("Phase list missing")?;
        let total = phases
            .iter()
            .filter(|p| preset.milestone(p["tagged"] == true))
            .count();
        let ordinal = phases
            .iter()
            .take(phase + 1)
            .filter(|p| preset.milestone(p["tagged"] == true))
            .count();
        let marker = format!("<!-- plantool-milestone:{gate} -->");
        let body=format!("Milestone {ordinal}/{total} ready at `{head}`\n\n[Compare changes](https://github.com/{}/compare/{previous}...{head})\n\nPlan sha256:{} · phase {}\n\nContinue: `/plantool continue`\nRequest changes: `/plantool request-changes <reason>`\n\n{marker}",repo.full_name,s["plan_sha"].as_str().unwrap(),phases[phase]["name"].as_str().unwrap());
        let found = pages(api, &format!("{path}/issues/{n}/comments"), None)
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
            &format!("milestone-comment:{gate}"),
            "POST",
            &format!("{path}/issues/{n}/comments"),
            json!({"body":body}),
            found,
            false,
        )
        .await?;
        ensure!(
            comment["user"]["id"].as_u64() == Some(repo.app_bot_id)
                && comment["user"]["type"] == "Bot",
            "Milestone comment must be App-authored"
        );
        let created = comment["created_at"]
            .as_str()
            .context("Milestone time missing")?;
        chrono::DateTime::parse_from_rfc3339(created)?;
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("UPDATE gates SET state='open',github_comment_id=?2,created_at=?3 WHERE id=?1 AND state='pending'",params![gate,comment["id"].as_u64(),created])?;
        tx.execute(
            "UPDATE milestone_context SET pr_number=?2 WHERE gate_id=?1",
            params![gate, n],
        )?;
        tx.execute(
            "UPDATE workflow_items SET pr_number=?2,pr_head=?3 WHERE item_id=?1",
            params![item, n, head],
        )?;
        tx.execute("UPDATE outbox SET delivered_at=?2 WHERE kind='github.milestone' AND json_extract(payload_json,'$.gate')=?1",params![gate,plantool_core::now()])?;
        tx.commit()?;
    }
    Ok(())
}
fn resume(tx: &Transaction<'_>, gate: &str, value: &Value) -> anyhow::Result<()> {
    let (item,thread,interrupt):(String,String,String)=tx.query_row("SELECT g.item_id,c.thread_id,c.interrupt_id FROM gates g JOIN milestone_context c ON c.gate_id=g.id WHERE g.id=?1",[gate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let resume = plantool_contract::Resume {
        thread_id: thread,
        interrupt_id: interrupt,
        value: value.clone(),
        idempotency_key: format!(
            "milestone-resume:{gate}:{}",
            plantool_core::sha256_hex(&value.to_string())
        ),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(tx, &item, "worker.resume", &serde_json::to_value(resume)?)?;
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
    if !matches!(command, Command::Continue | Command::RequestChanges(_)) {
        return Ok(false);
    }
    let db = database::open(home)?;
    let row:Option<(String,String,String,String,u64)>=db.query_row("SELECT g.id,g.item_id,g.target_sha,g.created_at,g.policy_version FROM gates g JOIN milestone_context m ON m.gate_id=g.id JOIN work_items i ON i.id=g.item_id WHERE i.project_id=?1 AND m.pr_number=?2 AND g.state='open' ORDER BY g.created_at DESC LIMIT 1",params![repo.project,number],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let Some((gate, item, head, opened, policy)) = row else {
        return Ok(false);
    };
    let path = repo.path()?;
    let pr = api::get(api, &format!("{path}/pulls/{number}")).await?;
    if pr["state"] != "open"
        || pr["draft"] != true
        || pr["user"]["id"].as_u64() != Some(repo.app_bot_id)
    {
        return Ok(true);
    }
    let context = Verification {
        place: Place::DraftPr,
        opened_at: &opened,
        snapshot_at: None,
        pending_hash: None,
        gate_commit: Some(&head),
        current_commit: pr["head"]["sha"].as_str(),
        snapshot_items: &[],
    };
    if let Err(error) = commands::verify(&db, c, command, &context) {
        gates::near_miss(
            home,
            repo,
            number,
            c["id"].as_u64().unwrap(),
            &error.to_string(),
        )?;
        return Ok(true);
    }
    let id = c["id"].as_u64().context("Milestone comment ID missing")?;
    let fresh = api
        .request("GET", &format!("{path}/issues/comments/{id}"), None, None)
        .await?;
    if fresh.status == 404 {
        return Ok(true);
    }
    ensure!(fresh.status == 200, "Milestone command refetch failed");
    if fresh.body != *c {
        return Ok(true);
    }
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    commands::verify(&tx, c, command, &context)?;
    let mut s = workflow::state(&tx, &item)?.context("Milestone state missing")?;
    let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM gates g JOIN workflow_items w ON w.item_id=g.item_id JOIN work_items i ON i.id=g.item_id WHERE g.id=?1 AND g.state='open' AND w.head_sha=g.target_sha AND g.policy_version=?2 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id))",params![gate,policy],|r|r.get(0))?;
    ensure!(
        valid && s["step"] == "milestone" && s["frozen"] != true,
        "Milestone changed or frozen"
    );
    let approved = matches!(command, Command::Continue);
    let decision = if approved {
        "approved"
    } else {
        "changes_requested"
    };
    tx.execute("INSERT INTO decisions(id,gate_id,source,actor_github_id,source_event_id,decision,created_at) VALUES(?1,?2,'github',?3,?4,?5,?6)",params![uuid::Uuid::new_v4().to_string(),gate,c["user"]["id"].as_u64(),id.to_string(),decision,plantool_core::now()])?;
    tx.execute(
        "UPDATE gates SET state=?2,closed_at=?3 WHERE id=?1",
        params![gate, decision, plantool_core::now()],
    )?;
    if approved {
        s["last_milestone_sha"] = json!(head);
        oversight::advance(&mut s);
    } else {
        s["step"] = json!(oversight::first_step(&s["areas"]));
        if let Command::RequestChanges(reason) = command {
            s["owner_feedback"] = json!(reason)
        }
        tx.execute(
            "UPDATE workflow_items SET qa_head=NULL,reviewed_head=NULL WHERE item_id=?1",
            [&item],
        )?;
    }
    workflow::save(&tx, &item, &s)?;
    resume(&tx, &gate, &json!({"decision":decision,"head":head}))?;
    database::audit(
        &tx,
        "github",
        "milestone.decision",
        &gate,
        &json!({"comment":id,"head":head,"decision":decision}),
    )?;
    tx.commit()?;
    Ok(true)
}
