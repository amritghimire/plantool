//! App publication and head-bound approvals. Every write has a durable intent.
use super::{
    api::{self, Api},
    git_ops::{AppGit, GitOps},
    setup::Repository,
};
use crate::{database, dispatcher, workflow};
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
pub(super) async fn pages(
    api: &dyn Api,
    path: &str,
    field: Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let mut all = vec![];
    for page in 1..=1000 {
        let r = api
            .request(
                "GET",
                &format!(
                    "{path}{}per_page=100&page={page}",
                    if path.contains('?') { "&" } else { "?" }
                ),
                None,
                None,
            )
            .await?;
        ensure!(
            r.status == 200,
            "GitHub lifecycle read failed ({})",
            r.status
        );
        let values = if let Some(field) = field {
            r.body[field].as_array()
        } else {
            r.body.as_array()
        }
        .context("Malformed GitHub collection")?;
        all.extend(values.clone());
        if !r.next {
            return Ok(all);
        }
    }
    anyhow::bail!("Lifecycle pagination limit reached")
}
pub(super) async fn write(
    home: &Path,
    api: &dyn Api,
    item: &str,
    key: &str,
    method: &str,
    path: &str,
    body: Value,
    observed: Option<Value>,
    repeatable: bool,
) -> anyhow::Result<Value> {
    let payload = json!({"method":method,"path":path,"body":body});
    let id = plantool_core::sha256_hex(&format!("{item}:{key}"));
    let previous = {
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        ensure!(
            workflow::state(&tx, item)?.is_some_and(|s| s["frozen"] != true),
            "Item is frozen"
        );
        tx.execute("INSERT OR IGNORE INTO workflow_actions(id,item_id,kind,payload_json) VALUES(?1,?2,?3,?4)",params![id,item,key,payload.to_string()])?;
        let (saved, state, result): (String, String, Option<String>) = tx.query_row(
            "SELECT payload_json,state,result_json FROM workflow_actions WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        ensure!(
            saved == payload.to_string(),
            "External action key reused for changed input"
        );
        tx.commit()?;
        (state, result)
    };
    if previous.0 == "completed" {
        return Ok(serde_json::from_str(
            previous.1.as_deref().unwrap_or("null"),
        )?);
    }
    if let Some(value) = observed {
        database::open(home)?.execute(
            "UPDATE workflow_actions SET state='completed',result_json=?2 WHERE id=?1",
            params![id, value.to_string()],
        )?;
        return Ok(value);
    }
    ensure!(
        previous.0 != "running" || repeatable,
        "External action outcome uncertain; owner recovery required ({key})"
    );
    database::open(home)?.execute(
        "UPDATE workflow_actions SET state='running' WHERE id=?1",
        [&id],
    )?;
    let reply = api.request(method, path, Some(body), None).await?;
    if matches!(reply.status, 401 | 403 | 404 | 422 | 429) {
        database::open(home)?.execute(
            "UPDATE workflow_actions SET state='pending' WHERE id=?1",
            [&id],
        )?;
        anyhow::bail!("GitHub action refused ({})", reply.status)
    }
    ensure!(
        (200..300).contains(&reply.status),
        "GitHub action outcome uncertain ({})",
        reply.status
    );
    database::open(home)?.execute(
        "UPDATE workflow_actions SET state='completed',result_json=?2 WHERE id=?1",
        params![id, reply.body.to_string()],
    )?;
    Ok(reply.body)
}
pub fn guide(home: &Path, item: &str, repo: &Repository) -> anyhow::Result<String> {
    let db = database::open(home)?;
    let (body,sha,number,issue,comment):(String,String,u64,u64,Option<u64>)=db.query_row("SELECT w.docs_body,r.content_sha256,r.number,i.github_issue_number,(SELECT github_comment_id FROM gates WHERE item_id=i.id AND kind='plan' AND state='approved' ORDER BY created_at DESC LIMIT 1) FROM workflow_items w JOIN work_items i ON i.id=w.item_id JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE w.item_id=?1",[item],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let mut q=db.prepare("SELECT a.role,a.id,a.model_route,s.provider,s.model,d.same_family_warning FROM attempts a JOIN runs_summary s ON s.attempt_id=a.id LEFT JOIN dispatches d ON d.attempt_id=a.id WHERE a.item_id=?1 ORDER BY a.generation,a.id")?;
    let rows = q.query_map([item], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<bool>>(5)?.unwrap_or(false),
        ))
    })?;
    let mut models = String::new();
    for row in rows {
        let (role, id, route, provider, model, warning) = row?;
        models.push_str(&format!(
            "- {role} · {id} · {provider}/{} ({route}){}\n",
            model.unwrap_or_default(),
            if warning {
                " · ⚠ same-family warning"
            } else {
                ""
            }
        ));
    }
    let mut findings = String::new();
    let mut q = db.prepare(
        "SELECT issue_number,title FROM workflow_proposals WHERE item_id=?1 ORDER BY id",
    )?;
    for row in q.query_map([item], |r| {
        Ok((r.get::<_, Option<u64>>(0)?, r.get::<_, String>(1)?))
    })? {
        let (number, title) = row?;
        findings.push_str(&format!(
            "- {title}: {}\n",
            number
                .map(|n| format!("https://github.com/{}/issues/{n}", repo.full_name))
                .unwrap_or_else(|| "pending publication".into())
        ));
    }
    Ok(format!("Plan v{number} · sha256:{sha} · [approved plan](https://github.com/{}/issues/{issue}#issuecomment-{})\n\n{body}\n\n## Models used (daemon verified)\n\n{models}\n## Filed findings (daemon verified)\n\n{findings}\nCloses #{issue}\n",repo.full_name,comment.unwrap_or(0)))
}
pub fn policy_facts(home: &Path, item: &str, head: &str) -> anyhow::Result<bool> {
    let db = database::open(home)?;
    let s = workflow::state(&db, item)?.context("No workflow")?;
    if s["frozen"] == true || s["step"] != "pr" {
        return Ok(false);
    }
    if !crate::oversight::valid_gates(&db, item, &s, head)? {
        return Ok(false);
    }
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM workflow_items w JOIN work_items i ON i.id=w.item_id JOIN revisions r ON r.id=i.approved_plan_revision_id WHERE w.item_id=?1 AND w.head_sha=?2 AND w.qa_head=?2 AND w.reviewed_head=?2 AND r.content_sha256=json_extract(w.state_json,'$.plan_sha') AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0 AND EXISTS(SELECT 1 FROM gates g WHERE g.item_id=i.id AND g.kind='plan' AND g.state='approved' AND g.target_digest=r.content_sha256 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id)) AND NOT EXISTS(SELECT 1 FROM gates g WHERE g.item_id=i.id AND g.kind='milestone' AND g.state NOT IN('approved','superseded')) AND NOT EXISTS(SELECT 1 FROM plan_tasks t LEFT JOIN task_progress p ON p.item_id=i.id AND p.task_key=t.task_key WHERE t.revision_id=r.id AND coalesce(p.status,'pending')<>'done'))",params![item,head],|r|r.get(0))?)
}
pub fn latest_approval(
    db: &rusqlite::Connection,
    reviews: &[Value],
    head: &str,
    opened: &str,
) -> anyhow::Result<Option<Value>> {
    let mut latest = std::collections::HashMap::new();
    for r in reviews {
        let Some(id) = r["user"]["id"].as_u64() else {
            continue;
        };
        if r["user"]["type"] != "User" {
            continue;
        }
        let allowed: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM allowlist WHERE github_user_id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !allowed {
            continue;
        }
        let previous = latest
            .get(&id)
            .map(|p: &Value| p["id"].as_u64().unwrap_or(0))
            .unwrap_or(0);
        if r["id"].as_u64().unwrap_or(0) > previous {
            latest.insert(id, r.clone());
        }
    }
    let opened = chrono::DateTime::parse_from_rfc3339(opened)?;
    Ok(latest
        .into_values()
        .filter(|r| {
            r["state"] == "APPROVED"
                && r["commit_id"] == head
                && r["submitted_at"]
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_some_and(|t| t >= opened)
        })
        .max_by_key(|r| r["id"].as_u64().unwrap_or(0)))
}
fn resume(tx: &rusqlite::Transaction<'_>, item: &str, event: &str) -> anyhow::Result<()> {
    let raw:String=tx.query_row("SELECT payload_json FROM workflow_actions WHERE item_id=?1 AND kind='github.pr' ORDER BY rowid DESC LIMIT 1",[item],|r|r.get(0))?;
    let p: Value = serde_json::from_str(&raw)?;
    let state = workflow::state(tx, item)?.context("No workflow")?;
    let request = plantool_contract::Resume {
        thread_id: p["thread_id"].as_str().context("Missing thread")?.into(),
        interrupt_id: p["interrupt_id"]
            .as_str()
            .context("Missing interrupt")?
            .into(),
        value: json!({"event":event}),
        idempotency_key: format!("pr:{item}:{event}:{}", state["sequence"]),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(tx, item, "worker.resume", &serde_json::to_value(request)?)?;
    Ok(())
}
fn merged(home: &Path, item: &str, pr: &Value) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let already: bool = tx.query_row(
        "SELECT merge_state='merged' FROM workflow_items WHERE item_id=?1",
        [item],
        |r| r.get(0),
    )?;
    if already {
        return Ok(());
    }
    tx.execute(
        "UPDATE workflow_items SET merge_state='merged' WHERE item_id=?1",
        [item],
    )?;
    tx.execute(
        "UPDATE work_items SET state='merged',merged_at=?2,closed_at=?2,outcome='done' WHERE id=?1",
        params![item, database::unix_now()],
    )?;
    let mut s = workflow::state(&tx, item)?.unwrap();
    s["step"] = json!("merged");
    workflow::save(&tx, item, &s)?;
    let project = dispatcher::item(&tx, item)?.0;
    if workflow::state(&tx, &format!("{project}:project"))?.is_some() {
        crate::scheduler::event(
            &tx,
            &project,
            "item_closed",
            &json!({"item":item}),
            &format!("closed:{item}"),
        )?;
    }
    resume(&tx, item, "merged")?;
    database::audit(
        &tx,
        "github",
        "workflow.merged",
        item,
        &json!({"number":pr["number"],"head":pr["head"]["sha"],"merged_by":pr["merged_by"]}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn merge_method(repo: &Value) -> anyhow::Result<&'static str> {
    for (name, key) in [
        ("squash", "allow_squash_merge"),
        ("merge", "allow_merge_commit"),
        ("rebase", "allow_rebase_merge"),
    ] {
        if repo[key] == true {
            return Ok(name);
        }
    }
    anyhow::bail!("Repository has no enabled merge method")
}
pub(super) fn rebase(
    home: &Path,
    item: &str,
    root: &Path,
    new_base: &str,
    notify: bool,
) -> anyhow::Result<bool> {
    let db = database::open(home)?;
    let (base, head): (String, String) = db.query_row(
        "SELECT base_sha,head_sha FROM workflow_items WHERE item_id=?1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if base == new_base {
        return Ok(false);
    }
    std::fs::create_dir_all(home.join("worktrees"))?;
    let temp = tempfile::tempdir_in(home.join("worktrees"))?;
    let dir = temp.path().join("rebase");
    crate::git::git(
        root,
        &[
            "clone",
            "--no-hardlinks",
            "--no-checkout",
            &root.to_string_lossy(),
            &dir.to_string_lossy(),
        ],
    )?;
    crate::git::git(&dir, &["checkout", "--detach", &head])?;
    crate::git::git(&dir, &["config", "user.name", "PlanTool"])?;
    crate::git::git(&dir, &["config", "user.email", "plantool@localhost"])?;
    let success = crate::git::git(
        &dir,
        &["-c", "core.hooksPath=/dev/null", "rebase", new_base],
    )
    .is_ok();
    let mut s = workflow::state(&db, item)?.unwrap();
    let event;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if success {
        let next = crate::git::git(&dir, &["rev-parse", "HEAD"])?;
        crate::git::git(
            root,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                &dir.to_string_lossy(),
                &next,
            ],
        )?;
        tx.execute("UPDATE workflow_items SET base_sha=?2,head_sha=?3,qa_head=NULL,reviewed_head=NULL WHERE item_id=?1",params![item,new_base,next])?;
        s["step"] = json!("qa");
        if s["phases"].as_array().is_some_and(|p| !p.is_empty()) {
            crate::oversight::initialize(&mut s);
            s["last_milestone_sha"] = Value::Null;
            tx.execute("UPDATE gates SET state='superseded',closed_at=?2 WHERE item_id=?1 AND kind='milestone'",params![item,plantool_core::now()])?;
        }
        s["rebase_base"] = Value::Null;
        event = "rebased";
    } else {
        let conflicts = crate::git::git(&dir, &["diff", "--name-only", "--diff-filter=U"])?;
        let owner = if conflicts.lines().all(|p| p.starts_with("web/")) {
            "frontend"
        } else if conflicts
            .lines()
            .all(|p| p.starts_with(".github/") || p.contains("infra/"))
        {
            "devops"
        } else {
            "backend"
        };
        s["step"] = json!(owner);
        s["rebase_base"] = json!(new_base);
        s["conflicts"] = json!(conflicts);
        if let Some(phases) = s["phases"].as_array().filter(|p| !p.is_empty()) {
            let index = phases
                .iter()
                .position(|p| {
                    p["areas"]
                        .as_array()
                        .is_some_and(|a| a.contains(&json!(owner)))
                })
                .unwrap_or(s["phase_index"].as_u64().unwrap_or(0) as usize);
            let areas = phases[index]["areas"].clone();
            s["phase_index"] = json!(index);
            s["areas"] = areas;
            s["rebase_restart"] = json!(true);
        }
        event = "rebase_conflict";
    }
    tx.execute("UPDATE gates SET state='superseded',closed_at=?2 WHERE item_id=?1 AND kind='pr' AND state IN('open','approved')",params![item,plantool_core::now()])?;
    workflow::save(&tx, item, &s)?;
    if notify {
        resume(&tx, item, event)?;
    }
    database::audit(
        &tx,
        "daemon",
        "workflow.rebase",
        item,
        &json!({"base":new_base,"success":success}),
    )?;
    tx.commit()?;
    Ok(true)
}
pub(super) async fn push(
    home: &Path,
    api: &dyn Api,
    git: &dyn GitOps,
    repo: &Repository,
    item: &str,
    root: &Path,
    branch: &str,
    head: &str,
    old: Option<&str>,
) -> anyhow::Result<()> {
    let key = format!("push:{item}:{head}");
    let id = plantool_core::sha256_hex(&key);
    let db = database::open(home)?;
    db.execute("INSERT OR IGNORE INTO workflow_actions(id,item_id,kind,payload_json) VALUES(?1,?2,'push',?3)",params![id,item,json!({"branch":branch,"head":head,"old":old}).to_string()])?;
    let status: String = db.query_row(
        "SELECT state FROM workflow_actions WHERE id=?1",
        [&id],
        |r| r.get(0),
    )?;
    if status == "completed" {
        return Ok(());
    }
    let reference = api
        .request(
            "GET",
            &format!("{}/git/ref/heads/{branch}", repo.path()?),
            None,
            None,
        )
        .await?;
    if reference.status == 200 && reference.body["object"]["sha"] == head {
        db.execute(
            "UPDATE workflow_actions SET state='completed' WHERE id=?1",
            [id],
        )?;
        return Ok(());
    }
    ensure!(
        status != "running",
        "Push outcome uncertain; owner recovery required"
    );
    ensure!(
        reference.status == 404 || reference.status == 200,
        "Cannot verify branch before push"
    );
    db.execute(
        "UPDATE workflow_actions SET state='running' WHERE id=?1",
        [&id],
    )?;
    git.push(api, root, &repo.full_name, branch, head, old)
        .await?;
    db.execute(
        "UPDATE workflow_actions SET state='completed' WHERE id=?1",
        [id],
    )?;
    Ok(())
}
pub async fn process_item(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    git: &dyn GitOps,
    item: &str,
) -> anyhow::Result<()> {
    let db = database::open(home)?;
    let s = workflow::state(&db, item)?.context("Missing item graph")?;
    if s["frozen"] == true || s["step"] != "pr" {
        return Ok(());
    }
    let (root,issue,slug,title,head,base,number):(String,u64,String,String,String,String,Option<u64>)=db.query_row("SELECT p.local_checkout,i.github_issue_number,i.slug,i.title,w.head_sha,w.base_sha,w.pr_number FROM workflow_items w JOIN work_items i ON i.id=w.item_id JOIN projects p ON p.id=i.project_id WHERE w.item_id=?1",[item],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
    let root = Path::new(&root);
    let path = repo.path()?;
    if let Some(n) = number {
        let observed = api::get(api, &format!("{path}/pulls/{n}")).await?;
        if observed["merged"] == true {
            return merged(home, item, &observed);
        }
        if observed["state"] == "closed" {
            anyhow::bail!("Owner closed the PR; side effects paused")
        }
    }
    let current = git
        .fetch(api, root, &repo.full_name, &repo.default_branch)
        .await?;
    if base != current {
        rebase(home, item, root, &current, true)?;
        return Ok(());
    }
    ensure!(
        policy_facts(home, item, &head)?,
        "Current plan, task completion, QA and security review evidence are required"
    );
    let commits = crate::git::git(
        root,
        &["log", "--format=%B%x00", &format!("{base}..{head}")],
    )?;
    ensure!(!commits.is_empty(), "No implementation commits to publish");
    for message in commits.split('\0').filter(|m| !m.trim().is_empty()) {
        ensure!(
            message.contains(&format!("PlanTool-Item: {item}"))
                && message.contains(&format!(
                    "PlanTool-Plan: sha256:{}",
                    s["plan_sha"].as_str().unwrap()
                ))
                && message.contains("PlanTool-Attempt:"),
            "Missing PlanTool commit trailers"
        );
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
    if let Some(old) = old {
        let saved: Option<String> = db.query_row(
            "SELECT pr_head FROM workflow_items WHERE item_id=?1",
            [item],
            |r| r.get(0),
        )?;
        ensure!(
            saved.as_deref() == Some(old) || old == head,
            "Remote branch changed outside the workflow"
        );
    }
    push(home, api, git, repo, item, root, &branch, &head, old).await?;
    let body = guide(home, item, repo)?;
    crate::models::proxy::check_payload(
        root,
        &crate::sandbox::NeverSent::new(&crate::execution::Execution::load(home)?.rules(root))?,
        &json!({"body":body}),
    )?;
    let pr = if let Some(pr) = found {
        pr
    } else {
        write(home,api,item,"pr-create","POST",&format!("{path}/pulls"),json!({"title":title,"head":branch,"base":repo.default_branch,"body":body,"draft":false}),None,false).await?
    };
    ensure!(
        pr["user"]["id"].as_u64() == Some(repo.app_bot_id),
        "PR must be App-authored"
    );
    let n = pr["number"].as_u64().context("Missing PR number")?;
    let desired = json!({"title":title,"body":body});
    write(
        home,
        api,
        item,
        &format!("pr-body:{head}:{}", plantool_core::sha256_hex(&body)),
        "PATCH",
        &format!("{path}/pulls/{n}"),
        desired,
        if pr["body"] == body {
            Some(pr.clone())
        } else {
            None
        },
        true,
    )
    .await?;
    db.execute(
        "UPDATE workflow_items SET pr_number=?2,pr_head=?3,merge_state=CASE WHEN merge_state='refused' THEN 'refused' ELSE 'waiting' END WHERE item_id=?1",
        params![item, n, head],
    )?;
    db.execute("UPDATE outbox SET delivered_at=?2,result_json=?3 WHERE aggregate_id=?1 AND kind='github.pr' AND delivered_at IS NULL",params![item,plantool_core::now(),json!({"number":n}).to_string()])?;
    if pr["draft"] == true {
        let node = pr["node_id"]
            .as_str()
            .context("Draft PR node identity missing")?;
        let body = json!({"query":"mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){pullRequest{isDraft}}}","variables":{"id":node}});
        let fresh = api::get(api, &format!("{path}/pulls/{n}")).await?;
        ensure!(
            fresh["head"]["sha"] == head,
            "Draft head changed before ready transition"
        );
        let observed = if fresh["draft"] == false {
            Some(
                json!({"data":{"markPullRequestReadyForReview":{"pullRequest":{"isDraft":false}}}}),
            )
        } else {
            None
        };
        let ready = write(
            home,
            api,
            item,
            &format!("pr-ready:{head}"),
            "POST",
            "/graphql",
            body,
            observed,
            false,
        )
        .await?;
        ensure!(
            ready["data"]["markPullRequestReadyForReview"]["pullRequest"]["isDraft"] == false,
            "Draft ready transition requires reconciliation"
        );
    }
    let gate = {
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let digest = plantool_core::sha256_hex(&format!("{}:{head}", s["plan_sha"]));
        let existing:Option<String>=tx.query_row("SELECT id FROM gates WHERE item_id=?1 AND kind='pr' AND target_sha=?2 AND target_digest=?3 AND state IN('open','approved')",params![item,head,digest],|r|r.get(0)).optional()?;
        let id = if let Some(id) = existing {
            id
        } else {
            tx.execute("UPDATE gates SET state='superseded',closed_at=?2 WHERE item_id=?1 AND kind='pr' AND state IN('open','approved')",params![item,plantool_core::now()])?;
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO gates(id,item_id,kind,target_digest,target_sha,state,created_at) VALUES(?1,?2,'pr',?3,?4,'open',?5)",params![id,item,digest,head,plantool_core::now()])?;
            id
        };
        tx.commit()?;
        id
    };
    let fresh = api::get(api, &format!("{path}/pulls/{n}")).await?;
    if fresh["merged"] == true {
        return merged(home, item, &fresh);
    }
    if fresh["head"]["sha"] != head {
        db.execute("UPDATE gates SET state='superseded' WHERE id=?1", [gate])?;
        anyhow::bail!("PR head changed; approval reset")
    }
    let reviews = pages(api, &format!("{path}/pulls/{n}/reviews"), None).await?;
    let opened: String =
        db.query_row("SELECT created_at FROM gates WHERE id=?1", [&gate], |r| {
            r.get(0)
        })?;
    let approval = latest_approval(&db, &reviews, &head, &opened)?;
    if approval.is_none() {
        let requested = reviews
            .iter()
            .filter(|r| {
                r["state"] == "CHANGES_REQUESTED"
                    && r["commit_id"] == head
                    && super::gates::valid_identity(&db, r)
            })
            .max_by_key(|r| r["id"].as_u64().unwrap_or(0));
        if let Some(review) = requested {
            let mut db = database::open(home)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let fresh=tx.execute("INSERT OR IGNORE INTO decisions(id,gate_id,source,actor_github_id,source_event_id,decision,created_at) VALUES(?1,?2,'github',?3,?4,'changes_requested',?5)",params![uuid::Uuid::new_v4().to_string(),gate,review["user"]["id"].as_u64(),format!("review:{}",review["id"]),plantool_core::now()])?;
            if fresh > 0 {
                let mut state = workflow::state(&tx, item)?.unwrap();
                state["step"] = json!(if state["areas"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!("backend")))
                {
                    "backend"
                } else {
                    "frontend"
                });
                state["owner_feedback"] = review["body"].clone();
                workflow::save(&tx, item, &state)?;
                tx.execute(
                    "UPDATE gates SET state='changes_requested',closed_at=?2 WHERE id=?1",
                    params![gate, plantool_core::now()],
                )?;
                resume(&tx, item, "changes_requested")?;
            }
            tx.commit()?;
            return Ok(());
        }
    }
    if let Some(review) = &approval {
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute("INSERT OR IGNORE INTO decisions(id,gate_id,source,actor_github_id,source_event_id,decision,created_at) VALUES(?1,?2,'github',?3,?4,'approved',?5)",params![uuid::Uuid::new_v4().to_string(),gate,review["user"]["id"].as_u64(),format!("review:{}",review["id"]),plantool_core::now()])?;
        tx.execute(
            "UPDATE gates SET state='approved',closed_at=?2 WHERE id=?1",
            params![gate, plantool_core::now()],
        )?;
        tx.commit()?;
    }
    let checks = pages(
        api,
        &format!("{path}/commits/{head}/check-runs"),
        Some("check_runs"),
    )
    .await?;
    let external = format!("plantool:{item}:{head}");
    let check = checks
        .iter()
        .find(|c| {
            c["name"] == "plantool/policy"
                && c["external_id"] == external
                && c["app"]["id"].as_u64() == Some(repo.app_id)
        })
        .cloned();
    let created=write(home,api,item,&format!("policy-create:{head}"),"POST",&format!("{path}/check-runs"),json!({"name":"plantool/policy","head_sha":head,"external_id":external,"status":"in_progress"}),check,false).await?;
    let check_id = created["id"].as_u64().context("Missing policy check ID")?;
    let passed = approval.is_some() && policy_facts(home, item, &head)?;
    write(home,api,item,&format!("policy-update:{head}:{passed}"),"PATCH",&format!("{path}/check-runs/{check_id}"),json!({"status":"completed","conclusion":if passed{"success"}else{"action_required"},"output":{"title":"PlanTool policy","summary":if passed{"Plan, milestones, QA, security review and current-head owner approval verified"}else{"Waiting for current-head owner approval"}}}),None,true).await?;
    if !passed {
        return Ok(());
    }
    let green = checks
        .iter()
        .filter(|c| {
            !(c["name"] == "plantool/policy" && c["app"]["id"].as_u64() == Some(repo.app_id))
        })
        .all(|c| {
            c["status"] == "completed"
                && matches!(
                    c["conclusion"].as_str(),
                    Some("success" | "neutral" | "skipped")
                )
        });
    let statuses = api::get(api, &format!("{path}/commits/{head}/status")).await?;
    let statuses_green = statuses["state"] == "success"
        || statuses["total_count"].as_u64() == Some(0)
        || statuses["statuses"]
            .as_array()
            .is_some_and(|s| s.is_empty());
    if !green || !statuses_green {
        return Ok(());
    }
    let repository = api::get(api, &path).await?;
    let method = merge_method(&repository)?;
    let refusal: bool = db.query_row(
        "SELECT merge_state='refused' FROM workflow_items WHERE item_id=?1",
        [item],
        |r| r.get(0),
    )?;
    if refusal {
        return Ok(());
    }
    let final_head = api::get(api, &format!("{path}/pulls/{n}")).await?;
    let final_reviews = pages(api, &format!("{path}/pulls/{n}/reviews"), None).await?;
    if final_head["head"]["sha"] != head
        || latest_approval(&database::open(home)?, &final_reviews, &head, &opened)?.is_none()
        || !policy_facts(home, item, &head)?
    {
        db.execute(
            "UPDATE gates SET state='open',closed_at=NULL WHERE id=?1",
            [&gate],
        )?;
        return Ok(());
    }
    let result = write(
        home,
        api,
        item,
        &format!("merge:{head}"),
        "PUT",
        &format!("{path}/pulls/{n}/merge"),
        json!({"sha":head,"merge_method":method}),
        None,
        false,
    )
    .await;
    match result {
        Ok(value) => {
            ensure!(value["merged"] == true, "GitHub did not merge");
            let final_pr = api::get(api, &format!("{path}/pulls/{n}")).await?;
            ensure!(
                final_pr["merged"] == true,
                "Merge outcome needs reconciliation"
            );
            merged(home, item, &final_pr)
        }
        Err(error) => {
            db.execute(
                "UPDATE workflow_items SET merge_state='refused',last_error=?2 WHERE item_id=?1",
                params![item, error.to_string()],
            )?;
            Err(error)
        }
    }
}
pub async fn proposals(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    let rows = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT f.id,f.item_id,f.title,f.body FROM workflow_proposals f JOIN work_items i ON i.id=f.item_id WHERE i.project_id=?1 AND f.issue_number IS NULL AND NOT EXISTS(SELECT 1 FROM workflow_items w WHERE w.item_id=i.project_id||':finding:'||f.id)")?;
        let rows = q.query_map([&repo.project], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (id, item, title, body) in rows {
        if workflow::state(&database::open(home)?, &item)?.is_some_and(|s| s["frozen"] == true) {
            continue;
        }
        let marker = format!("<!-- plantool-proposal:{id} -->");
        let path = repo.path()?;
        let found = pages(api, &format!("{path}/issues?state=all"), None)
            .await?
            .into_iter()
            .find(|i| {
                i["user"]["id"].as_u64() == Some(repo.app_bot_id)
                    && i["body"].as_str().is_some_and(|b| b.contains(&marker))
            });
        let (_, root) = dispatcher::item(&database::open(home)?, &item)?;
        crate::models::proxy::check_payload(
            &root,
            &crate::sandbox::NeverSent::new(
                &crate::execution::Execution::load(home)?.rules(&root),
            )?,
            &json!({"title":title,"body":body}),
        )?;
        let issue=write(home,api,&item,&format!("proposal:{id}"),"POST",&format!("{path}/issues"),json!({"title":title,"body":format!("Out-of-scope finding from {item}.\n\n{body}\n\n{marker}"),"labels":["plantool:proposed"]}),found,false).await?;
        database::open(home)?.execute(
            "UPDATE workflow_proposals SET issue_number=?2 WHERE id=?1",
            params![
                id,
                issue["number"]
                    .as_u64()
                    .context("Missing proposal number")?
            ],
        )?;
    }
    Ok(())
}
pub async fn process(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    git: &dyn GitOps,
) -> anyhow::Result<()> {
    proposals(home, api, repo).await?;
    let items = {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT w.item_id FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE i.project_id=?1 AND w.merge_state<>'merged'")?;
        let rows = q.query_map([&repo.project], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for item in items {
        if let Err(e) = process_item(home, api, repo, git, &item).await {
            database::open(home)?.execute(
                "UPDATE workflow_items SET last_error=?2 WHERE item_id=?1",
                params![item, e.to_string()],
            )?;
            tracing::warn!(item, "PR lifecycle paused: {e}");
        }
    }
    Ok(())
}
pub async fn start_new(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    issues: &[Value],
) -> anyhow::Result<()> {
    for issue in issues {
        if issue["state"] != "open" || !issue["pull_request"].is_null() {
            continue;
        }
        let db = database::open(home)?;
        if !super::loop_sync::admitted(home, repo, issue)? {
            continue;
        }
        let Some(number) = issue["number"].as_u64() else {
            continue;
        };
        let id = format!("{}:{number}", repo.project);
        if workflow::state(&db, &id)?.is_some() {
            continue;
        }
        let (_, root): (String, String) = db.query_row(
            "SELECT id,local_checkout FROM projects WHERE id=?1",
            [&repo.project],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let base = AppGit
            .fetch(api, Path::new(&root), &repo.full_name, &repo.default_branch)
            .await?;
        workflow::start(
            home,
            &repo.project,
            number,
            issue["title"].as_str().unwrap_or("Untitled issue"),
            issue["body"].as_str().unwrap_or(""),
            &base,
        )?;
    }
    Ok(())
}
