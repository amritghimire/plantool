use super::sessions::resolve;
use super::{actor_from, bad_request, ApiError};
use crate::providers::RunInput;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use plantool_core::{
    Actor, CommitScope, DocKind, ImplementationMode, PermissionMode, Provider, Stage,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct StartBody {
    #[serde(default)]
    pub pause_rule: Option<plantool_core::PauseRule>,
    #[serde(default)]
    pub milestone: Option<String>,
    pub provider: String,
    pub stage: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub resume_run: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// For implement runs: work in a git worktree (default true). Ignored for other stages.
    #[serde(default)]
    pub worktree: Option<bool>,
    #[serde(default)]
    pub implementation_mode: ImplementationMode,
}

fn parse_mode(s: &str) -> Result<PermissionMode, ApiError> {
    PermissionMode::parse(s).ok_or_else(|| {
        bad_request(format!(
            "permission mode must be one of ask, accept-edits, auto, allow-all (got {s})"
        ))
    })
}

async fn start(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<StartBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let provider = Provider::parse(&body.provider)
        .ok_or_else(|| bad_request(format!("unknown provider {}", body.provider)))?;
    if provider == Provider::Ollama {
        let model = body
            .model
            .as_deref()
            .filter(|m| !m.is_empty())
            .ok_or_else(|| bad_request("choose an installed Ollama model"))?;
        let installed = tokio::task::spawn_blocking(crate::providers::ollama_models)
            .await
            .map_err(|e| bad_request(e.to_string()))?
            .map_err(bad_request)?;
        if !installed.iter().any(|option| option.id == model) {
            return Err(bad_request(format!(
                "Ollama model {model} is not installed"
            )));
        }
    }
    if let Some(effort) = body.effort.as_deref() {
        if !matches!(effort, "low" | "medium" | "high" | "xhigh" | "max") {
            return Err(bad_request("unknown reasoning effort"));
        }
    }
    if body.stage == "implement" && s.state().plan_revision_pending {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "review and accept the plan revision before starting another milestone".into(),
        ));
    }
    let stage_name = body.stage.as_str();
    let current = s.stage();
    let target = match stage_name {
        "research" => Stage::Researching,
        "plan" => Stage::Planning,
        "implement" => Stage::Implementing,
        "critique" => current,
        "assist" | "draft-pr" => current,
        other => return Err(bad_request(format!("unknown run task {other}"))),
    };
    if stage_name == "draft-pr" && !matches!(current, Stage::ImplementationReview | Stage::Done) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "PR drafting is available at implementation review or done".into(),
        ));
    }
    if stage_name == "assist" && body.prompt.as_deref().is_none_or(|p| p.trim().is_empty()) {
        return Err(bad_request("give the agent a request"));
    }
    if stage_name == "implement"
        && !matches!(
            current,
            Stage::Approved | Stage::Implementing | Stage::ImplementationReview
        )
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            format!("the plan must be approved before implementation starts (stage is {current})"),
        ));
    }
    if let Some(key) = &body.milestone {
        if stage_name != "implement" {
            return Err(bad_request("milestones apply to implementation runs"));
        }
        if s.runs().iter().any(|r| state.runs.is_live(&r.id)) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "finish or stop the current run before choosing another milestone".into(),
            ));
        }
        s.select_milestone(key)?;
    }
    if stage_name == "implement" {
        let rule = body.pause_rule.clone().or_else(|| {
            (body.implementation_mode == ImplementationMode::StepByStep
                && s.session().pause_rule == plantool_core::PauseRule::NoPauses)
                .then_some(plantool_core::PauseRule::EveryMilestone)
        });
        if let Some(rule) = rule {
            s.apply_context(&super::where_context::Update {
                pause_rule: Some(rule),
                ..Default::default()
            })?;
        }
    }
    let resume_from = body.resume_run.as_deref().and_then(|rid| s.run(rid));
    if body.resume_run.is_some() && resume_from.is_none() {
        return Err(bad_request("unknown run to resume"));
    }
    if let Some(r) = &resume_from {
        if r.provider != provider {
            return Err(bad_request(format!(
                "run {} was a {} run; it can only be resumed with the same provider",
                r.id,
                r.provider.as_str()
            )));
        }
        if r.provider_session_id.is_none() {
            return Err(bad_request(format!(
                "run {} has no provider session to resume",
                r.id
            )));
        }
        if state.runs.is_live(&r.id) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                format!("run {} is still live; talk to it in the run panel", r.id),
            ));
        }
    }
    if stage_name == "implement" && body.worktree.unwrap_or(true) {
        let ws = s.clone();
        tokio::task::spawn_blocking(move || ws.ensure_worktree(None))
            .await
            .map_err(|e| anyhow::anyhow!(e))??;
    }
    s.verify_workspace()?;
    if target.index() > current.index() {
        s.set_stage(target, Actor::Agent)?;
    }
    let sess = s.session();
    let extra = body.prompt.as_deref().filter(|p| !p.trim().is_empty());
    let implementation_mode = if stage_name == "implement" {
        resume_from
            .as_ref()
            .map(|r| r.implementation_mode)
            .unwrap_or(body.implementation_mode)
    } else {
        ImplementationMode::AllAtOnce
    };
    let previous_milestone = if implementation_mode == ImplementationMode::StepByStep {
        resume_from.clone().or_else(|| {
            s.runs()
                .into_iter()
                .filter(|r| {
                    r.task.as_deref() == Some("implement")
                        && r.implementation_mode == ImplementationMode::StepByStep
                        && r.milestone_pending
                })
                .max_by(|a, b| a.started_at.cmp(&b.started_at))
        })
    } else {
        None
    };
    let prompt_stage = if resume_from.is_some()
        && implementation_mode != ImplementationMode::StepByStep
        && !matches!(stage_name, "assist" | "draft-pr")
    {
        "resume"
    } else {
        stage_name
    };
    let prompt =
        if let Some(reviewing) = previous_milestone.as_ref().filter(|r| r.milestone_pending) {
            crate::prompts::render_milestone_review(
                &sess,
                &s.store.dir,
                reviewing.milestone_review.as_ref(),
                extra,
            )
        } else {
            crate::prompts::render_run(
                &state.config.home,
                prompt_stage,
                &sess,
                &s.store.dir,
                extra,
                s.stage(),
                implementation_mode,
            )
        };
    let resume = resume_from
        .as_ref()
        .and_then(|r| r.provider_session_id.clone());
    let mut mode = body
        .permission_mode
        .as_deref()
        .map(parse_mode)
        .transpose()?
        .unwrap_or_default();
    if matches!(provider, Provider::Copilot | Provider::Ollama) && mode == PermissionMode::Ask {
        mode = PermissionMode::AcceptEdits;
    }
    if provider == Provider::Ollama
        && stage_name != "assist"
        && !matches!(mode, PermissionMode::Auto | PermissionMode::AllowAll)
    {
        return Err(bad_request("Ollama stage runs need Auto or Allow all permissions so OpenCode can use plantool commands"));
    }
    let run = state.runs.start(
        s.clone(),
        provider,
        target,
        stage_name,
        prompt.clone(),
        body.model.clone(),
        body.effort.clone(),
        resume,
        mode,
        implementation_mode,
        previous_milestone.as_ref(),
    )?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "run": run, "prompt": prompt })),
    ))
}

#[derive(Deserialize)]
pub struct PermissionBody {
    pub request_id: String,
    pub decision: String,
}

#[derive(Deserialize)]
pub struct InputAnswers {
    pub request_id: String,
    #[serde(default)]
    pub answers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct InputBody {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub permission: Option<PermissionBody>,
    #[serde(default)]
    pub input: Option<InputAnswers>,
    #[serde(default)]
    pub permission_mode: Option<String>,
}

async fn input(
    State(state): State<AppState>,
    Path((repo, slug, id)): Path<(String, String, String)>,
    Json(body): Json<InputBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let mut run = s
        .run(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    let msg = if let Some(m) = body.permission_mode.as_deref() {
        let mut mode = parse_mode(m)?;
        if mode == plantool_core::PermissionMode::Ask
            && matches!(
                run.provider,
                plantool_core::Provider::Copilot | plantool_core::Provider::Ollama
            )
        {
            mode = plantool_core::PermissionMode::AcceptEdits;
        }
        state
            .runs
            .send(&id, RunInput::PermissionMode(mode))
            .await
            .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
        run.permission_mode = mode;
        s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
        return Ok(Json(json!({ "ok": true, "permission_mode": mode })));
    } else if let Some(p) = body.permission {
        RunInput::Permission {
            request_id: p.request_id,
            decision: p.decision,
        }
    } else if let Some(i) = body.input {
        RunInput::Input {
            request_id: i.request_id,
            answers: i.answers,
        }
    } else if let Some(t) = body.text.filter(|t| !t.trim().is_empty()) {
        if s.state().plan_revision_pending && run.task.as_deref() == Some("implement") {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "accept the plan revision before continuing implementation".into(),
            ));
        }
        s.pause(None)?;
        RunInput::Text(t)
    } else {
        return Err(bad_request(
            "give text, permission, input or permission_mode",
        ));
    };
    state
        .runs
        .send(&id, msg)
        .await
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ApproveBody {
    /// Commit the worktree as the milestone before the agent continues (default true).
    #[serde(default = "default_true")]
    pub commit: bool,
    /// Commit message; empty or missing means the suggested one.
    #[serde(default)]
    pub message: Option<String>,
}

fn default_true() -> bool {
    true
}

/// The text of the last ticked checkbox in the plan, used as the milestone commit subject.
fn last_checked_ticket(plan: &str) -> Option<String> {
    plantool_core::markdown::checkboxes(plan)
        .into_iter()
        .rev()
        .find(|c| c.checked)
        .map(|c| c.text)
}

/// The commit subject offered for the next milestone of a run.
fn suggested_subject(s: &crate::registry::LiveSession, run: &plantool_core::Run) -> String {
    let n = run.milestones_approved + 1;
    match s
        .doc(DocKind::Plan)
        .and_then(|d| last_checked_ticket(&d.content))
    {
        Some(ticket) => format!("Milestone {n}: {ticket}"),
        None => format!("Milestone {n}"),
    }
}

async fn milestone(
    State(state): State<AppState>,
    Path((repo, slug, id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let run = s
        .run(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    let subject = suggested_subject(&s, &run);
    let cwd = run.cwd.clone();
    let dirty = tokio::task::spawn_blocking(move || crate::git::is_dirty(&cwd))
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .unwrap_or(false);
    Ok(Json(
        json!({ "subject": subject, "dirty": dirty, "milestone": run.milestones_approved + 1, "pending": run.milestone_pending, "live": state.runs.is_live(&id) }),
    ))
}

async fn approve_milestone(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug, id)): Path<(String, String, String)>,
    body: Option<Json<ApproveBody>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "only a human can approve a milestone in the browser".into(),
        ));
    }
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let mut run = s
        .run(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    if (run.implementation_mode != ImplementationMode::StepByStep && run.milestone_key.is_none())
        || !run.milestone_pending
        || !matches!(
            run.status,
            plantool_core::RunStatus::Idle | plantool_core::RunStatus::Stopped
        )
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "the milestone is not ready for approval".into(),
        ));
    }
    if !state.runs.is_live(&id) && run.milestone_key.is_none() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!("run {id} is not live; resume it before approving"),
        ));
    }
    if s.state().plan_revision_pending {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "accept the plan revision before approving a milestone".into(),
        ));
    }
    if s.state()
        .comments
        .iter()
        .any(|c| !c.resolved && c.parent.is_none())
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "resolve the session comments before approving the milestone".into(),
        ));
    }
    if let Some(review) = run.milestone_review.clone() {
        let open =
            tokio::task::spawn_blocking(move || crate::changes::unresolved_human_comments(&review))
                .await
                .map_err(|e| anyhow::anyhow!(e))??;
        if open > 0 {
            return Err(ApiError(StatusCode::CONFLICT, format!("{open} human difftool comment(s) are still open; resolve them in difftool before approving")));
        }
    }
    let (commit, message) = body.map_or((true, None), |Json(b)| (b.commit, b.message));
    let n = run.milestones_approved + 1;
    let subject = match message
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
    {
        Some(m) => m,
        None => suggested_subject(&s, &run),
    };
    let cwd = run.cwd.clone();
    let held = run.milestone_commit.clone();
    let check = cwd.clone();
    let pending = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<bool>> {
        if !commit || !crate::git::is_dirty(&check)? {
            return Ok(None);
        }
        Ok(Some(held.is_some_and(|h| {
            crate::git::head_sha(&check).ok().as_deref() == Some(h.as_str())
        })))
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?
    .map_err(|e| {
        ApiError(
            StatusCode::CONFLICT,
            format!("could not commit the milestone: {e}"),
        )
    })?;
    let outcome = match pending {
        Some(amend) => Some(
            crate::commits::run_commit(
                s.clone(),
                CommitScope::Milestone { run_id: id.clone() },
                cwd,
                subject,
                amend,
            )
            .await
            .map_err(|e| {
                ApiError(
                    StatusCode::CONFLICT,
                    e.message("could not commit the milestone"),
                )
            })?,
        ),
        None => None,
    };
    let (sha, committed) = match outcome {
        None => (
            crate::git::head_sha(&run.cwd).map_err(|e| anyhow::anyhow!(e))?,
            false,
        ),
        Some(crate::git::CommitOutcome::Committed(sha)) => (sha, true),
        Some(crate::git::CommitOutcome::HookRewrote { sha, files }) => {
            if sha.is_some() && run.milestone_commit != sha {
                run.milestone_commit = sha;
                s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
            }
            let shown: Vec<&str> = files.iter().take(5).map(String::as_str).collect();
            let more = if files.len() > 5 {
                format!(" and {} more", files.len() - 5)
            } else {
                String::new()
            };
            return Err(ApiError(StatusCode::CONFLICT, format!("git hooks rewrote {} file(s): {}{more}. The rewritten files are staged; review them in the Changes tab and approve again to include them in the milestone commit.", files.len(), shown.join(", "))));
        }
    };
    let previous = run.clone();
    run.milestone_pending = false;
    run.milestone_review = None;
    run.milestone_base = Some(sha.clone());
    run.milestone_commit = None;
    run.milestones_approved = n;
    s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
    let note = if committed {
        format!(
            "This milestone is approved and committed as {}. Do not amend that commit. ",
            &sha[..sha.len().min(12)]
        )
    } else {
        "This milestone is approved. ".to_string()
    };
    if let Some(key) = &previous.milestone_key {
        s.milestone_status(key, "approved", &previous)?;
        s.pause(None)?;
        if state.runs.is_live(&id) {
            let _ = state.runs.send(&id, RunInput::Stop).await;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            while state.runs.is_live(&id) && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            if state.runs.is_live(&id) {
                return Err(ApiError(StatusCode::CONFLICT, "Milestone approved; the previous run is still stopping. Start the next phase once it stops.".into()));
            }
        }
        if s.state().milestones.iter().any(|m| m.status == "pending") {
            let mut context = crate::runs::next_milestone_context(&previous);
            context.milestones_approved = n;
            state.runs.start(s.clone(), previous.provider, previous.stage, "implement", format!("{note}Continue the next phase. Read the plan and implementation skill; run the full checks after the final phase."), previous.model.clone(), None, previous.provider_session_id.clone(), previous.permission_mode, previous.implementation_mode, Some(&context))?;
        } else {
            s.set_stage(Stage::ImplementationReview, Actor::Agent)?;
        }
        return Ok(Json(json!({"ok":true,"committed":committed,"sha":sha})));
    }
    if let Err(e) = state.runs.send(&id, RunInput::Text(format!("{note}If unchecked plan tasks remain, implement exactly the next one and pause for review again. If every task is complete, run the full project checks and move to implementation-review."))).await {
        s.upsert_run(previous, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
        return Err(ApiError(StatusCode::CONFLICT, e.to_string()));
    }
    Ok(Json(
        json!({ "ok": true, "committed": committed, "sha": sha }),
    ))
}

async fn stop(
    State(state): State<AppState>,
    Path((repo, slug, id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    s.run(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    match state.runs.send(&id, RunInput::Stop).await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(_) => Ok(Json(json!({ "ok": true, "note": "run was not live" }))),
    }
}

async fn remove(
    State(state): State<AppState>,
    Path((repo, slug, id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let run = s
        .run(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    if state.runs.is_live(&id)
        || matches!(
            run.status,
            plantool_core::RunStatus::Starting
                | plantool_core::RunStatus::Running
                | plantool_core::RunStatus::Waiting
                | plantool_core::RunStatus::Idle
        ) && state.runs.is_live(&id)
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!("run {id} is still live; stop it first"),
        ));
    }
    s.remove_run(&id)?;
    Ok(Json(json!({ "ok": true, "removed": id })))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    pub since: u64,
}

async fn events(
    State(state): State<AppState>,
    Path((repo, slug, id)): Path<(String, String, String)>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let run = s
        .run(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    let events = s.store.read_run_events(&id, q.since)?;
    Ok(Json(json!({ "run": run, "events": events })))
}

async fn providers() -> Json<serde_json::Value> {
    let list = tokio::task::spawn_blocking(crate::providers::discover)
        .await
        .unwrap_or_default();
    Json(json!({ "providers": list }))
}

async fn upload_attachment(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    if body.is_empty() || body.len() > 10 * 1024 * 1024 {
        return Err(bad_request("Choose a file between 1 byte and 10 MB"));
    }
    let original = headers
        .get("x-plantool-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("attachment");
    let safe: String = original
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let name = format!(
        "{}-{}",
        uuid::Uuid::new_v4().simple(),
        if safe.is_empty() { "attachment" } else { &safe }
    );
    let dir = s.store.dir.join("attachments");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(anyhow::Error::from)?;
    let path = dir.join(name);
    tokio::fs::write(&path, body)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(Json(json!({ "name": original, "path": path })))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/providers", get(providers))
        .route(
            "/sessions/{repo}/{slug}/attachments",
            post(upload_attachment),
        )
        .route("/sessions/{repo}/{slug}/runs", post(start))
        .route("/sessions/{repo}/{slug}/runs/{id}/input", post(input))
        .route(
            "/sessions/{repo}/{slug}/runs/{id}/milestone",
            get(milestone),
        )
        .route(
            "/sessions/{repo}/{slug}/runs/{id}/milestone/approve",
            post(approve_milestone),
        )
        .route("/sessions/{repo}/{slug}/runs/{id}/stop", post(stop))
        .route("/sessions/{repo}/{slug}/runs/{id}", delete(remove))
        .route("/sessions/{repo}/{slug}/runs/{id}/remove", post(remove))
        .route("/sessions/{repo}/{slug}/runs/{id}/events", get(events))
}
