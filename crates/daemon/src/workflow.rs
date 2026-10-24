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
        "plan" | "sprint_plan" | "triage" | "scan_pm" | "route_finding" => "pm",
        "scan_devops" => "devops",
        "scan_qa" => "qa",
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
    let mut state = state(&db, item)?.context("Item graph is not initialized")?;
    state["head_sha"] = json!(db.query_row(
        "SELECT head_sha FROM workflow_items WHERE item_id=?1",
        [item],
        |r| r.get::<_, String>(0)
    )?);
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
    if step == "discussion" {
        ensure!(
            request.role == s["discussion"]["speaker"]
                && request.inputs["step"] == "discussion"
                && request.capability == "read"
                && request.plan_sha == s["plan_sha"].as_str().unwrap_or(""),
            "Discussion scope/turn mismatch"
        );
        return Ok(true);
    }
    ensure!(
        role(step) == request.role && request.inputs["step"] == step,
        "Graph step/role mismatch"
    );
    ensure!(
        request.capability
            == if crate::loop_engine::read_step(step) {
                "read"
            } else {
                capability(&request.role)
            },
        "Role capability mismatch"
    );
    ensure!(
        request.plan_sha == s["plan_sha"].as_str().unwrap_or(""),
        "Graph plan changed"
    );
    Ok(crate::loop_engine::read_step(step)
        || [
            "research",
            "plan",
            "plan_review",
            "supervisor",
            "sprint_plan",
        ]
        .contains(&step))
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
    let discussion: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM channels WHERE project_id=?1)",
        [project],
        |r| r.get(0),
    )?;
    let mut state = json!({"item":item,"step":"research","generation":0,"sequence":0,"plan_sha":"","approved":false,"areas":[],"owners":{},"retries":{},"supervisor_rounds":0,"frozen":false,"issue_body":body});
    let intake: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM loop_intake WHERE item_id=?1)",
        [&item],
        |r| r.get(0),
    )?;
    if intake {
        state["step"] = json!("triage");
        state["urgent"] = json!(tx.query_row(
            "SELECT urgent FROM loop_intake WHERE item_id=?1",
            [&item],
            |r| r.get::<_, bool>(0)
        )?);
        state["issue_number"] = json!(issue);
    }
    state["discuss_enabled"] = json!(discussion);
    state["preset"] = serde_json::to_value(crate::oversight::preset(&tx, project)?)?;
    state["review_globs"] = json!(crate::oversight::rules(&tx, project)?);
    tx.execute("INSERT INTO work_items(id,project_id,kind,title,github_issue_number,state,created_at,repo_slug,slug,session_json) VALUES(?1,?2,'eos',?3,?4,'research',?5,?2,?6,'{}')",params![item,project,title,issue,plantool_core::now(),format!("issue-{issue}")])?;
    if state["urgent"] == true {
        tx.execute(
            "UPDATE work_items SET priority='urgent' WHERE id=?1",
            [&item],
        )?;
    }
    tx.execute(
        "INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES(?1,?2,?3,?3)",
        params![item, state.to_string(), base],
    )?;
    if discussion {
        crate::workspace::item_thread(&tx, &item)?;
    }
    if self::state(&tx, &format!("{project}:project"))?.is_some() {
        crate::scheduler::event(
            &tx,
            project,
            "issue_new",
            &json!({"item":item}),
            &format!("issue:{issue}"),
        )?;
    }
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
    if approved {
        crate::oversight::initialize(s);
    }
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
    if approved && s["phases"].as_array().is_some_and(|p| !p.is_empty()) {
        s["step"] = json!(crate::oversight::first_step(&s["areas"]));
    }
}
pub fn research_decision(tx: &Transaction<'_>, item: &str, approved: bool) -> anyhow::Result<()> {
    let mut s = state(tx, item)?.context("Research graph missing")?;
    ensure!(s["step"] == "research_gate", "Not waiting for research");
    s["research_approved"] = json!(approved);
    if !approved {
        s["step"] = json!("research");
    } else if s["discuss_enabled"] == true {
        let summary = s["research_summary"]
            .as_str()
            .unwrap_or("Discuss research")
            .to_owned();
        crate::workspace::begin(tx, item, &mut s, "plan", &summary)?;
    } else {
        s["step"] = json!("plan");
    }
    save(tx, item, &s)
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
    if crate::loop_engine::read_step(s["step"].as_str().unwrap_or("")) {
        return crate::loop_engine::validate(result, &s);
    }
    if s["project"] == true && s["step"] == "sprint_plan" {
        if result.outcome == Outcome::Done {
            crate::scheduler::validate(&s, result)?;
        }
        return Ok(());
    }
    if s["step"] == "discussion" {
        if result.outcome == Outcome::Done {
            crate::workspace::validate(&s, result)?;
        }
        return Ok(());
    }
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
                        .is_some_and(|a| ["backend", "frontend", "devops", "qa", "docs", "docs"]
                            .contains(&a)),
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
            let Some(RoleDetails::Docs { body, .. }) = &result.details else {
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
    s["phase_restart"] = json!(false);
    s["sequence"] = json!(s["sequence"].as_u64().unwrap_or(0) + 1);
    if crate::loop_engine::read_step(&step) {
        crate::loop_engine::record(tx, result, &mut s)?;
        save(tx, &result.item, &s)?;
        return Ok(());
    }
    if s["project"] == true && step == "sprint_plan" {
        crate::scheduler::record(result, &mut s)?;
        save(tx, &result.item, &s)?;
        return Ok(());
    }
    if step == "discussion" {
        crate::workspace::record(tx, result, &mut s)?;
        save(tx, &result.item, &s)?;
        return Ok(());
    }
    if let Some(commit) = result.artifacts.iter().rev().find(|a| a.kind == "commit") {
        tx.execute(
            "UPDATE workflow_items SET head_sha=?2 WHERE item_id=?1",
            params![result.item, commit.reference],
        )?;
    }
    if let Some(RoleDetails::Research { markdown }) = &result.details {
        ensure!(
            ["research", "plan_review"].contains(&step.as_str()),
            "Only CTO steps may publish research"
        );
        let sha = crate::artifacts::put(home, markdown.as_bytes())?;
        if step == "research" {
            s["research_sha"] = json!(sha);
            s["research_summary"] = json!(result.summary);
        }
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
        let project = dispatcher::item(tx, &result.item)?.0;
        let preset = crate::oversight::preset(tx, &project)?;
        let marked = crate::oversight::mark_plan(
            markdown,
            preset,
            affected_files,
            &crate::oversight::rules(tx, &project)?,
        )?;
        let normalized = if preset != plantool_core::presets::Preset::Minimal {
            crate::oversight::normalize_phases(&marked)
        } else {
            marked
        };
        let markdown = &normalized;
        if preset != plantool_core::presets::Preset::Minimal {
            s["phases"] = crate::oversight::phases(markdown)?;
        } else {
            s["phases"] = json!([])
        }
        s["preset"] = serde_json::to_value(preset)?;
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
        // Progress belongs to the task wording and area in its approved revision.
        // A replan must never inherit completion for a changed or removed task.
        tx.execute("UPDATE task_progress SET status='pending',updated_at=?3 WHERE item_id=?1 AND NOT EXISTS(SELECT 1 FROM plan_tasks n JOIN plan_tasks o ON o.task_key=n.task_key AND o.title=n.title AND o.phase IS n.phase AND o.area IS n.area JOIN work_items i ON i.approved_plan_revision_id=o.revision_id WHERE i.id=?1 AND n.revision_id=?2 AND n.task_key=task_progress.task_key)",params![result.item,revision,plantool_core::now()])?;
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
                    && owner.as_deref().is_some_and(|o| {
                        ["backend", "frontend", "devops", "qa", "docs"].contains(&o)
                    }) =>
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
                .filter(|o| ["backend", "frontend", "devops", "qa", "docs"].contains(o))
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
            if s["rebase_restart"] == true {
                crate::oversight::initialize(&mut s);
                s["step"] = json!("qa");
                s["phase_restart"] = json!(true);
                s["rebase_restart"] = json!(false);
                s["last_milestone_sha"] = Value::Null;
                tx.execute("UPDATE gates SET state='superseded',closed_at=?2 WHERE item_id=?1 AND kind='milestone'",params![result.item,plantool_core::now()])?;
            }
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
    s["docs_changed"] = json!(false);
    if let Some(RoleDetails::Docs {
        updates: Some(updates),
        ..
    }) = &result.details
    {
        ensure!(
            step == "docs",
            "Only Docs may propose documentation updates"
        );
        if !updates.is_empty() {
            let (_, root) = dispatcher::item(tx, &result.item)?;
            let head: String = tx.query_row(
                "SELECT head_sha FROM workflow_items WHERE item_id=?1",
                [&result.item],
                |r| r.get(0),
            )?;
            std::fs::create_dir_all(home.join("worktrees"))?;
            let temp = tempfile::tempdir_in(home.join("worktrees"))?;
            let checkout = temp.path().join("docs");
            crate::git::git(
                &root,
                &[
                    "clone",
                    "--no-hardlinks",
                    "--no-checkout",
                    &root.to_string_lossy(),
                    &checkout.to_string_lossy(),
                ],
            )?;
            crate::git::git(&checkout, &["checkout", "--detach", &head])?;
            let rules = crate::execution::Execution::load(home)?.rules(&root);
            let policy = crate::sandbox::NeverSent::new(&rules)?;
            for update in updates {
                ensure!(
                    ["README.md", "CHANGELOG.md"].contains(&update.path.as_str()),
                    "Docs updates are limited to README.md and CHANGELOG.md"
                );
                ensure!(
                    s["affected_files"]
                        .as_array()
                        .is_some_and(|p| p.contains(&json!(update.path))),
                    "Documentation path is outside approved scope"
                );
                policy.check(Path::new(""), Path::new(&update.path))?;
                crate::models::proxy::check_payload(
                    &root,
                    &policy,
                    &json!({"content":update.content}),
                )?;
                let file = checkout.join(&update.path);
                ensure!(
                    !file
                        .symlink_metadata()
                        .is_ok_and(|m| m.file_type().is_symlink()),
                    "Documentation target is a symlink"
                );
                std::fs::write(file, &update.content)?;
            }
            crate::git::git(&checkout, &["add", "--", "README.md", "CHANGELOG.md"]).or_else(
                |_| {
                    let paths = updates.iter().map(|u| u.path.as_str()).collect::<Vec<_>>();
                    let mut args = vec!["add", "--"];
                    args.extend(paths);
                    crate::git::git(&checkout, &args)
                },
            )?;
            if !crate::git::git(&checkout, &["diff", "--cached", "--name-only"])?.is_empty() {
                let message=format!("Docs: approved documentation updates\n\nPlanTool-Item: {}\nPlanTool-Plan: sha256:{}\nPlanTool-Attempt: {}",result.item,s["plan_sha"].as_str().unwrap(),result.attempt_id);
                crate::git::git(
                    &checkout,
                    &[
                        "-c",
                        "core.hooksPath=/dev/null",
                        "-c",
                        "user.name=PlanTool",
                        "-c",
                        "user.email=plantool@localhost",
                        "commit",
                        "-m",
                        &message,
                    ],
                )?;
                let next = crate::git::git(&checkout, &["rev-parse", "HEAD"])?;
                crate::git::git(
                    &root,
                    &[
                        "fetch",
                        "--no-tags",
                        "--no-write-fetch-head",
                        &checkout.to_string_lossy(),
                        &next,
                    ],
                )?;
                tx.execute("UPDATE workflow_items SET head_sha=?2,qa_head=NULL,reviewed_head=NULL WHERE item_id=?1",params![result.item,next])?;
                s["docs_changed"] = json!(true);
                let rounds = s["docs_rounds"].as_u64().unwrap_or(0) + 1;
                s["docs_rounds"] = json!(rounds);
                s["step"] = json!(if rounds <= 2 { "qa" } else { "supervisor" });
            }
        }
    }
    if let Some(RoleDetails::Docs { body, .. }) = &result.details {
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
    if s["step"] == "pr" && result.outcome == Outcome::Done {
        crate::oversight::boundary(tx, &result.item, &mut s)?;
    }
    if step == "research"
        && result.outcome == Outcome::Done
        && s["preset"] == "strict"
        && s["step"] == "plan"
    {
        s["step"] = json!("research_gate");
    }
    if step == "research"
        && result.outcome == Outcome::Done
        && s["discuss_enabled"] == true
        && s["step"] == "plan"
    {
        crate::workspace::begin(tx, &result.item, &mut s, "plan", &result.summary)?;
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
    let (project, _) = dispatcher::item(tx, item)?;
    let base: String = tx.query_row(
        "SELECT head_sha FROM workflow_items WHERE item_id=?1",
        [item],
        |r| r.get(0),
    )?;
    crate::loop_engine::start_aux(
        tx,
        &project,
        &format!("{project}:finding:{id}"),
        "route_finding",
        &json!({"proposal_id":id,"source_item":item,"title":title,"body":body}),
        &base,
    )?;
    Ok(id)
}

pub fn pr_wait(home: &Path, item: &str, thread: &str, interrupt: &str) -> anyhow::Result<Value> {
    pr_request(
        home,
        &plantool_contract::WorkflowPr {
            item: item.into(),
            thread_id: thread.into(),
            interrupt_id: interrupt.into(),
            idempotency_key: format!("{thread}:{interrupt}"),
        },
    )
}
pub fn pr_request(home: &Path, request: &plantool_contract::WorkflowPr) -> anyhow::Result<Value> {
    let digest = plantool_core::sha256_hex(&serde_json::to_string(request)?);
    let value = database::inbox_once(home, "worker.pr", &request.idempotency_key, |tx| {
        let s = state(tx, &request.item)?.context("No item graph")?;
        ensure!(
            s["step"] == "pr" && s["frozen"] != true,
            "Item is not eligible to publish a PR"
        );
        ensure!(
            !request.thread_id.is_empty() && !request.interrupt_id.is_empty(),
            "Waiting graph identity required"
        );
        let payload = json!({"item":request.item,"thread_id":request.thread_id,"interrupt_id":request.interrupt_id});
        let id = dispatcher::enqueue(tx, &request.item, "github.pr", &payload)?;
        tx.execute("INSERT OR IGNORE INTO workflow_actions(id,item_id,kind,payload_json) VALUES(?1,?2,'github.pr',?3)",params![id,request.item,payload.to_string()])?;
        Ok(json!({"intent":id,"digest":digest}))
    })?;
    ensure!(
        value["digest"] == digest,
        "PR idempotency key reused for changed input"
    );
    Ok(value)
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
