//! Authoritative facts/projection of the worker graph, never an approval shortcut.
use crate::{database, dispatcher};
use anyhow::{ensure, Context};
use plantool_contract::{Dispatch, Outcome, RoleDetails, RoleResult};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::Path;
pub fn state(db: &rusqlite::Connection, item: &str) -> anyhow::Result<Option<Value>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT state_json FROM workflow_items WHERE item_id=?1",
            [item],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
pub fn role(step: &str) -> &str {
    match step {
        "research" | "plan_review" => "cto",
        "plan" => "pm",
        other => other,
    }
}
pub fn coding(role: &str) -> bool {
    ["backend", "frontend", "devops", "qa", "reviewer"].contains(&role)
}
pub fn capability(role: &str) -> &str {
    match role {
        "backend" | "frontend" | "devops" => "write",
        "qa" => "tests",
        _ => "read",
    }
}
pub fn context(home: &Path, item: &str) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let state = state(&db, item)?.context("Item graph is not initialized")?;
    let (project, _) = dispatcher::item(&db, item)?;
    let policy: u64 = db.query_row(
        "SELECT max(version) FROM project_settings WHERE project_id=?1",
        [project],
        |r| r.get(0),
    )?;
    Ok(json!({"state":state,"policy_version":policy}))
}
pub fn check(db: &rusqlite::Connection, request: &Dispatch) -> anyhow::Result<bool> {
    let Some(s) = state(db, &request.item)? else {
        return Ok(false);
    };
    ensure!(
        s["frozen"] != true,
        "Item is frozen; owner intervention required"
    );
    let step = s["step"].as_str().context("Missing graph step")?;
    ensure!(
        role(step) == request.role && request.inputs["step"] == step,
        "Graph step/role mismatch"
    );
    ensure!(
        request.capability == capability(&request.role),
        "Role capability mismatch"
    );
    ensure!(
        request.plan_sha == s["plan_sha"].as_str().unwrap_or(""),
        "Graph plan changed"
    );
    Ok(["research", "plan", "plan_review", "supervisor"].contains(&step))
}
pub fn start(
    home: &Path,
    project: &str,
    issue: u64,
    title: &str,
    body: &str,
    base: &str,
) -> anyhow::Result<String> {
    ensure!(issue > 0, "Issue number required");
    let item = format!("{project}:{issue}");
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mode: String = tx.query_row("SELECT mode FROM projects WHERE id=?1", [project], |r| {
        r.get(0)
    })?;
    ensure!(mode == "eos", "Engineering OS project required");
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM workflow_items WHERE item_id=?1)",
        [&item],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(item);
    }
    let state = json!({"item":item,"step":"research","generation":0,"sequence":0,"plan_sha":"","approved":false,"areas":[],"owners":{},"retries":{},"supervisor_rounds":0,"frozen":false,"issue_body":body});
    tx.execute("INSERT INTO work_items(id,project_id,kind,title,github_issue_number,state,created_at,repo_slug,slug,session_json) VALUES(?1,?2,'eos',?3,?4,'research',?5,?2,?6,'{}')",params![item,project,title,issue,plantool_core::now(),format!("issue-{issue}")])?;
    tx.execute(
        "INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES(?1,?2,?3,?3)",
        params![item, state.to_string(), base],
    )?;
    let start = plantool_contract::Start {
        thread_id: item.clone(),
        graph: plantool_contract::Graph::WorkItem,
        input: json!({"workflow":state}),
        idempotency_key: format!("workflow-start:{item}"),
        contract_version: plantool_contract::CONTRACT_VERSION,
        graph_version: plantool_contract::GRAPH_VERSION,
    };
    dispatcher::enqueue(&tx, &item, "worker.start", &serde_json::to_value(start)?)?;
    database::audit(
        &tx,
        "github",
        "workflow.started",
        &item,
        &json!({"issue":issue,"base":base}),
    )?;
    tx.commit()?;
    Ok(item)
}
pub fn after_plan(s: &mut Value, approved: bool) {
    s["approved"] = json!(approved);
    s["step"] = json!(if !approved {
        "plan"
    } else if s["areas"]
        .as_array()
        .is_some_and(|a| a.contains(&json!("backend")))
    {
        "backend"
    } else if s["areas"]
        .as_array()
        .is_some_and(|a| a.contains(&json!("frontend")))
    {
        "frontend"
    } else if s["infra_changed"] == true {
        "devops"
    } else {
        "qa"
    });
}
pub fn gate_decision(tx: &Transaction<'_>, item: &str, approved: bool) -> anyhow::Result<()> {
    if let Some(mut s) = state(tx, item)? {
        ensure!(
            s["step"] == "plan_gate",
            "Graph is not waiting on the plan gate"
        );
        after_plan(&mut s, approved);
        save(tx, item, &s)?;
    }
    Ok(())
}
pub fn save(tx: &Transaction<'_>, item: &str, s: &Value) -> anyhow::Result<()> {
    tx.execute(
        "UPDATE workflow_items SET state_json=?2 WHERE item_id=?1",
        params![item, s.to_string()],
    )?;
    tx.execute(
        "UPDATE work_items SET state=?2 WHERE id=?1",
        params![item, s["step"].as_str().unwrap_or("recovery_needed")],
    )?;
    Ok(())
}
pub fn test_path(file: &str) -> bool {
    file.split('/').any(|p| p == "tests" || p == "test")
        || file.ends_with("_test.rs")
        || file.rsplit('/').next().is_some_and(|name| {
            name.starts_with("test_") && name.ends_with(".py")
                || name.contains(".test.")
                || name.contains(".spec.")
        })
}
pub fn validate_result(home: &Path, result: &RoleResult, worktree: &Path) -> anyhow::Result<()> {
    let db = database::open(home)?;
    let Some(s) = state(&db, &result.item)? else {
        return Ok(());
    };
    ensure!(s["frozen"] != true, "Item is frozen");
    let step = s["step"].as_str().context("Missing step")?;
    ensure!(role(step) == result.role, "Unexpected role result");
    if coding(&result.role) {
        let head = crate::git::git(worktree, &["rev-parse", "HEAD"])?;
        if result.outcome == Outcome::Done {
            ensure!(
                result
                    .artifacts
                    .iter()
                    .any(|a| a.kind == "commit" && a.reference == head),
                "Coding result must cite exact HEAD"
            );
        }
        let base: Option<String> = db.query_row(
            "SELECT base_sha FROM dispatches WHERE attempt_id=?1",
            [&result.attempt_id],
            |r| r.get(0),
        )?;
        if let Some(base) = base {
            let files = crate::git::git(
                worktree,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--name-only",
                    &base,
                    &head,
                ],
            )?;
            for file in files.lines() {
                if result.role == "qa" {
                    ensure!(test_path(file), "QA changed production file {file}");
                }
                let allowed = s["affected_files"].as_array().is_some_and(|patterns| {
                    patterns.iter().filter_map(|p| p.as_str()).any(|p| {
                        file == p
                            || p.strip_suffix("/**")
                                .is_some_and(|prefix| file.starts_with(&format!("{prefix}/")))
                    })
                });
                ensure!(allowed, "File {file} is outside the approved plan");
            }
        }
        for check in &result.checks {
            let sha = check
                .evidence
                .strip_prefix("artifact://")
                .context("Evidence needs artifact reference")?;
            let owned: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM artifact_refs WHERE owner_id=?1 AND sha256=?2)",
                params![result.attempt_id, sha],
                |r| r.get(0),
            )?;
            ensure!(owned, "Evidence is not owned by this attempt");
        }
    }
    if result.outcome != Outcome::Done {
        return Ok(());
    }
    match step {
        "research" => ensure!(
            matches!(&result.details,Some(RoleDetails::Research{markdown}) if !markdown.trim().is_empty()),
            "CTO research document required"
        ),
        "plan" => {
            let Some(RoleDetails::Plan {
                markdown,
                acceptance_criteria,
                affected_files,
            }) = &result.details
            else {
                anyhow::bail!("Typed PM plan required")
            };
            let tasks = plantool_core::tasks::parse(markdown).map_err(anyhow::Error::msg)?;
            ensure!(
                !tasks.is_empty() && !acceptance_criteria.is_empty() && !affected_files.is_empty(),
                "Plan needs keyed tasks, acceptance criteria and affected files"
            );
            for t in tasks {
                ensure!(
                    t.area
                        .as_deref()
                        .is_some_and(
                            |a| ["backend", "frontend", "devops", "qa", "docs"].contains(&a)
                        ),
                    "Every task requires a supported area"
                );
            }
            for file in affected_files {
                ensure!(
                    !file.starts_with('/')
                        && !file.split('/').any(|p| p == "..")
                        && !file.contains(['\n', '\r', '\0']),
                    "Unsafe plan path"
                );
            }
        }
        "qa" => {
            ensure!(
                !result.checks.is_empty()
                    && result
                        .checks
                        .iter()
                        .all(|c| c.status == plantool_contract::CheckStatus::Pass),
                "QA requires passing checks with evidence"
            );
        }
        "reviewer" => {
            let Some(RoleDetails::Security {
                commit,
                passed,
                criteria,
            }) = &result.details
            else {
                anyhow::bail!("Reviewer security pass required")
            };
            let head = crate::git::git(worktree, &["rev-parse", "HEAD"])?;
            ensure!(
                *passed && *commit == head && !criteria.is_empty(),
                "Security review must pass for the current commit"
            );
        }
        "docs" => {
            let Some(RoleDetails::Docs { body }) = &result.details else {
                anyhow::bail!("Docs review guide required")
            };
            for heading in [
                "What changed and why",
                "Where to look first",
                "How to test",
                "AI review rounds",
                "Found during review",
                "Evidence",
            ] {
                ensure!(body.contains(heading), "Review guide missing {heading}");
            }
            ensure!(
                !regex::Regex::new(
                    r"(?i)\b(close[sd]?|fix(e[sd])?|resolve[sd]?)\s+(#[0-9]+\b|https://github.com/[^/\s]+/[^/\s]+/issues/[0-9]+)"
                )?
                .is_match(body),
                "Only the daemon may add closing references"
            );
        }
        "supervisor" => ensure!(
            matches!(&result.details,Some(RoleDetails::Strategy{action,..}) if ["retry","replan","escalate"].contains(&action.as_str())),
            "Supervisor strategy required"
        ),
        _ => {}
    }
    Ok(())
}
pub fn record(
    home: &Path,
    tx: &Transaction<'_>,
    result: &RoleResult,
    worktree: &Path,
) -> anyhow::Result<()> {
    let Some(mut s) = state(tx, &result.item)? else {
        return Ok(());
    };
    let step = s["step"].as_str().unwrap_or("").to_owned();
    s["generation"] = json!(result.generation);
    s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
    if let Some(commit) = result.artifacts.iter().rev().find(|a| a.kind == "commit") {
        tx.execute(
            "UPDATE workflow_items SET head_sha=?2 WHERE item_id=?1",
            params![result.item, commit.reference],
        )?;
    }
    if let Some(RoleDetails::Research { markdown }) = &result.details {
        ensure!(
            step == "research",
            "Only the CTO research step may publish research"
        );
        let sha = crate::artifacts::put(home, markdown.as_bytes())?;
        crate::artifacts::register(tx, &sha, markdown.len())?;
        tx.execute("INSERT OR IGNORE INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) SELECT ?1,?2,'research',coalesce(max(number),0)+1,?3,'cto',?4 FROM revisions WHERE item_id=?2 AND doc_kind='research'",params![format!("{}:research:{sha}",result.item),result.item,sha,plantool_core::now()])?;
    }
    if let Some(RoleDetails::Plan {
        markdown,
        acceptance_criteria,
        affected_files,
    }) = &result.details
    {
        ensure!(step == "plan", "Only PM may create plan revisions");
        let content = format!(
            "{markdown}\n\n## Bound acceptance criteria\n{}\n\n## Bound affected files\n{}\n",
            acceptance_criteria
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
            affected_files
                .iter()
                .map(|s| format!("- `{s}`"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let sha = crate::artifacts::put(home, content.as_bytes())?;
        crate::artifacts::register(tx, &sha, content.len())?;
        let revision = format!("{}:plan:{sha}", result.item);
        tx.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) SELECT ?1,?2,'plan',coalesce(max(number),0)+1,?3,'pm',?4 FROM revisions WHERE item_id=?2 AND doc_kind='plan'",params![revision,result.item,sha,plantool_core::now()])?;
        let tasks = plantool_core::tasks::parse(markdown).map_err(anyhow::Error::msg)?;
        let mut owners = serde_json::Map::new();
        let mut areas = vec![];
        for t in tasks {
            let area = t.area.as_deref().unwrap();
            owners.insert(t.key.clone(), json!(area));
            if !areas.contains(&area.to_string()) {
                areas.push(area.to_string())
            }
            tx.execute(
                "INSERT INTO plan_tasks VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    format!("{revision}:{}", t.key),
                    revision,
                    t.key,
                    t.area,
                    t.title,
                    t.phase,
                    t.review_tag
                ],
            )?;
        }
        s["owners"] = json!(owners);
        s["areas"] = json!(areas);
        s["plan_sha"] = json!(sha);
        s["acceptance_criteria"] = json!(acceptance_criteria);
        s["affected_files"] = json!(affected_files);
        s["approved"] = json!(false);
        tx.execute("UPDATE work_items SET state_json=json_set(state_json,'$.plan_revision_pending',1) WHERE id=?1",[&result.item])?;
    }
    let quota: bool = tx.query_row(
        "SELECT quota_pending FROM dispatches WHERE attempt_id=?1",
        [&result.attempt_id],
        |r| r.get(0),
    )?;
    s["route_retry"] = json!(quota && result.outcome != Outcome::Done);
    if quota && result.outcome != Outcome::Done {
        s["step"] = json!(step);
    } else if step == "supervisor" {
        let rounds = s["supervisor_rounds"].as_u64().unwrap_or(0) + 1;
        s["supervisor_rounds"] = json!(rounds);
        match &result.details {
            Some(RoleDetails::Strategy { action, owner, .. })
                if rounds <= 2
                    && action == "retry"
                    && owner
                        .as_deref()
                        .is_some_and(|o| ["backend", "frontend", "devops", "qa"].contains(&o)) =>
            {
                s["step"] = json!(owner)
            }
            Some(RoleDetails::Strategy { action, .. }) if rounds <= 2 && action == "replan" => {
                s["step"] = json!("plan");
                s["approved"] = json!(false);
            }
            _ => {
                s["step"] = json!("escalated");
                s["frozen"] = json!(true);
            }
        }
    } else if result.outcome != Outcome::Done
        || result.findings.iter().any(|f| {
            f.scope == plantool_contract::FindingScope::In
                || f.severity == plantool_contract::Severity::High
        })
    {
        let key = result
            .findings
            .iter()
            .find_map(|f| f.task_key.as_deref())
            .unwrap_or("item");
        let count = s["retries"][key].as_u64().unwrap_or(0) + 1;
        s["retries"][key] = json!(count);
        let owner = s["owners"][key].as_str().map(str::to_owned);
        s["step"] = json!(if count > 3
            || result
                .findings
                .iter()
                .any(|f| f.severity == plantool_contract::Severity::High)
        {
            "supervisor"
        } else {
            owner
                .as_deref()
                .filter(|o| ["backend", "frontend", "devops", "qa"].contains(o))
                .unwrap_or("supervisor")
        });
    } else {
        s["step"] = json!(match step.as_str() {
            "research" => "plan",
            "plan" => "plan_review",
            "plan_review" => "plan_gate",
            "backend" =>
                if s["areas"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!("frontend")))
                {
                    "frontend"
                } else if s["infra_changed"] == true {
                    "devops"
                } else {
                    "qa"
                },
            "frontend" =>
                if s["infra_changed"] == true {
                    "devops"
                } else {
                    "qa"
                },
            "devops" => "qa",
            "qa" => "reviewer",
            "reviewer" => "docs",
            "docs" => "pr",
            _ => "supervisor",
        });
    }
    if coding(&result.role) {
        let head = crate::git::git(worktree, &["rev-parse", "HEAD"])?;
        if let Some(base) = s["rebase_base"]
            .as_str()
            .filter(|_| result.outcome == Outcome::Done)
            .map(str::to_owned)
        {
            crate::git::git(worktree, &["merge-base", "--is-ancestor", &base, &head])?;
            tx.execute(
                "UPDATE workflow_items SET base_sha=?2 WHERE item_id=?1",
                params![result.item, base],
            )?;
            s["rebase_base"] = Value::Null;
        }

        let base: String = tx.query_row(
            "SELECT base_sha FROM workflow_items WHERE item_id=?1",
            [&result.item],
            |r| r.get(0),
        )?;
        let changes = crate::git::git(worktree, &["diff", "--name-only", &base, &head])?;
        s["infra_changed"] = json!(changes.lines().any(|p| p.starts_with(".github/")
            || p.contains("infra/")
            || p == "flake.nix"
            || p == "Cargo.toml"
            || p.ends_with("lock")));
        if (step == "backend" || step == "frontend")
            && s["step"] == "qa"
            && s["infra_changed"] == true
        {
            s["step"] = json!("devops");
        }
        let (_, root) = dispatcher::item(tx, &result.item)?;
        if root != worktree {
            crate::git::git(
                &root,
                &[
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "uploadpack.packObjectsHook=",
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    &worktree.to_string_lossy(),
                    &head,
                ],
            )?;
        }
        tx.execute(
            "UPDATE workflow_items SET head_sha=?2 WHERE item_id=?1",
            params![result.item, head],
        )?;
        if result.role == "qa" && result.outcome == Outcome::Done {
            tx.execute(
                "UPDATE workflow_items SET qa_head=?2 WHERE item_id=?1",
                params![result.item, head],
            )?;
        }
        if result.role == "reviewer" && result.outcome == Outcome::Done {
            tx.execute(
                "UPDATE workflow_items SET reviewed_head=?2 WHERE item_id=?1",
                params![result.item, head],
            )?;
        }
    }
    if let Some(RoleDetails::Docs { body }) = &result.details {
        ensure!(step == "docs", "Only Docs may write the review guide");
        tx.execute(
            "UPDATE workflow_items SET docs_body=?2 WHERE item_id=?1",
            params![result.item, body],
        )?;
    }
    for f in result
        .findings
        .iter()
        .filter(|f| f.scope == plantool_contract::FindingScope::Out)
    {
        proposal(tx, &result.item, &f.title, &f.detail)?;
    }
    save(tx, &result.item, &s)?;
    database::audit(tx, "daemon", "workflow.projected", &result.item, &s)?;
    Ok(())
}
pub fn proposal(
    tx: &Transaction<'_>,
    item: &str,
    title: &str,
    body: &str,
) -> anyhow::Result<String> {
    let id = plantool_core::sha256_hex(&format!("{item}:{title}:{body}"));
    tx.execute(
        "INSERT OR IGNORE INTO workflow_proposals(id,item_id,title,body) VALUES(?1,?2,?3,?4)",
        params![id, item, title, body],
    )?;
    dispatcher::enqueue(tx, item, "github.proposal", &json!({"id":id}))?;
    Ok(id)
}

pub fn pr_wait(home: &Path, item: &str, thread: &str, interrupt: &str) -> anyhow::Result<Value> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let s = state(&tx, item)?.context("No item graph")?;
    ensure!(
        s["step"] == "pr" && s["frozen"] != true,
        "Item is not eligible to publish a PR"
    );
    let payload = json!({"item":item,"thread_id":thread,"interrupt_id":interrupt});
    let id = dispatcher::enqueue(&tx, item, "github.pr", &payload)?;
    tx.execute("INSERT OR IGNORE INTO workflow_actions(id,item_id,kind,payload_json) VALUES(?1,?2,'github.pr',?3)",params![id,item,payload.to_string()])?;
    tx.commit()?;
    Ok(json!({"intent":id}))
}

pub fn overview(home: &Path) -> anyhow::Result<Vec<Value>> {
    let db = database::open(home)?;
    let mut q=db.prepare("SELECT w.item_id,i.title,w.state_json,w.last_error,(SELECT json_extract(outcome_json,'$.summary') FROM attempts WHERE item_id=i.id ORDER BY generation DESC LIMIT 1) FROM workflow_items w JOIN work_items i ON i.id=w.item_id ORDER BY i.created_at DESC")?;
    let rows=q.query_map([],|r|Ok(json!({"item":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"state":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(Value::Null),"error":r.get::<_,Option<String>>(3)?,"summary":r.get::<_,Option<String>>(4)?})))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn recover(home: &Path, attempt: &str, error: &str) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let item: String =
        tx.query_row("SELECT item_id FROM attempts WHERE id=?1", [attempt], |r| {
            r.get(0)
        })?;
    tx.execute("UPDATE attempts SET state='recovery_needed',ended_at=?2 WHERE id=?1 AND state IN('starting','running')",params![attempt,plantool_core::now()])?;
    if let Some(mut state) = state(&tx, &item)? {
        state["step"] = json!("escalated");
        state["frozen"] = json!(true);
        save(&tx, &item, &state)?;
        tx.execute(
            "UPDATE workflow_items SET last_error=?2 WHERE item_id=?1",
            params![item, error],
        )?;
    }
    database::audit(
        &tx,
        "daemon",
        "workflow.recovery_needed",
        attempt,
        &json!({"error":error}),
    )?;
    tx.commit()?;
    Ok(())
}
