use super::sessions::resolve;
use super::{actor_from, bad_request, ApiError};
use crate::providers::RunInput;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use plantool_core::{Actor, ImplementationMode, PermissionMode, Provider, Stage};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct StartBody {
    pub provider: String,
    pub stage: String,
    #[serde(default)]
    pub model: Option<String>,
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
    PermissionMode::parse(s).ok_or_else(|| bad_request(format!("permission mode must be one of ask, accept-edits, auto, allow-all (got {s})")))
}

async fn start(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>, Json(body): Json<StartBody>) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let provider = Provider::parse(&body.provider).ok_or_else(|| bad_request(format!("unknown provider {}", body.provider)))?;
    let stage_name = body.stage.as_str();
    let current = s.stage();
    let target = match stage_name {
        "research" => Stage::Researching,
        "plan" => Stage::Planning,
        "implement" => Stage::Implementing,
        "critique" => current,
        other => return Err(bad_request(format!("stage must be research, plan, implement or critique (got {other})"))),
    };
    if stage_name == "implement" && !matches!(current, Stage::Approved | Stage::Implementing | Stage::ImplementationReview) {
        return Err(ApiError(StatusCode::FORBIDDEN, format!("the plan must be approved before implementation starts (stage is {current})")));
    }
    let resume_from = body.resume_run.as_deref().and_then(|rid| s.run(rid));
    if body.resume_run.is_some() && resume_from.is_none() {
        return Err(bad_request("unknown run to resume"));
    }
    if let Some(r) = &resume_from {
        if r.provider != provider {
            return Err(bad_request(format!("run {} was a {} run; it can only be resumed with the same provider", r.id, r.provider.as_str())));
        }
        if r.provider_session_id.is_none() {
            return Err(bad_request(format!("run {} has no provider session to resume", r.id)));
        }
        if state.runs.is_live(&r.id) {
            return Err(ApiError(StatusCode::CONFLICT, format!("run {} is still live; talk to it in the run panel", r.id)));
        }
    }
    if stage_name == "implement" && body.worktree.unwrap_or(true) {
        let ws = s.clone();
        tokio::task::spawn_blocking(move || ws.ensure_worktree(None)).await.map_err(|e| anyhow::anyhow!(e))??;
    }
    if target.index() > current.index() {
        s.set_stage(target, Actor::Agent)?;
    }
    let sess = s.session();
    let extra = body.prompt.as_deref().filter(|p| !p.trim().is_empty());
    let implementation_mode = if stage_name == "implement" {
        resume_from.as_ref().map(|r| r.implementation_mode).unwrap_or(body.implementation_mode)
    } else {
        ImplementationMode::AllAtOnce
    };
    let previous_milestone = if implementation_mode == ImplementationMode::StepByStep {
        resume_from.clone().or_else(|| s.runs().into_iter()
            .filter(|r| r.task.as_deref() == Some("implement") && r.implementation_mode == ImplementationMode::StepByStep && r.milestone_pending)
            .max_by(|a, b| a.started_at.cmp(&b.started_at)))
    } else { None };
    let prompt_stage = if resume_from.is_some() && implementation_mode != ImplementationMode::StepByStep { "resume" } else { stage_name };
    let prompt = if let Some(reviewing) = previous_milestone.as_ref().filter(|r| r.milestone_pending) {
        crate::prompts::render_milestone_review(&sess, &s.store.dir, reviewing.milestone_review.as_ref(), extra)
    } else {
        crate::prompts::render_run(&state.config.home, prompt_stage, &sess, &s.store.dir, extra, s.stage(), implementation_mode)
    };
    let resume = resume_from.as_ref().and_then(|r| r.provider_session_id.clone());
    let mode = body.permission_mode.as_deref().map(parse_mode).transpose()?.unwrap_or_default();
    let run = state.runs.start(s.clone(), provider, target, stage_name, prompt.clone(), body.model.clone(), resume, mode, implementation_mode, previous_milestone.as_ref())?;
    Ok((StatusCode::CREATED, Json(json!({ "run": run, "prompt": prompt }))))
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

async fn input(State(state): State<AppState>, Path((repo, slug, id)): Path<(String, String, String)>, Json(body): Json<InputBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let mut run = s.run(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    let msg = if let Some(m) = body.permission_mode.as_deref() {
        let mode = parse_mode(m)?;
        state.runs.send(&id, RunInput::PermissionMode(mode)).await.map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
        run.permission_mode = mode;
        s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
        return Ok(Json(json!({ "ok": true, "permission_mode": mode })));
    } else if let Some(p) = body.permission {
        RunInput::Permission { request_id: p.request_id, decision: p.decision }
    } else if let Some(i) = body.input {
        RunInput::Input { request_id: i.request_id, answers: i.answers }
    } else if let Some(t) = body.text.filter(|t| !t.trim().is_empty()) {
        RunInput::Text(t)
    } else {
        return Err(bad_request("give text, permission, input or permission_mode"));
    };
    state.runs.send(&id, msg).await.map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

async fn approve_milestone(State(state): State<AppState>, headers: HeaderMap, Path((repo, slug, id)): Path<(String, String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(StatusCode::FORBIDDEN, "only a human can approve a milestone in the browser".into()));
    }
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let mut run = s.run(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    if run.implementation_mode != ImplementationMode::StepByStep || !run.milestone_pending || run.status != plantool_core::RunStatus::Idle {
        return Err(ApiError(StatusCode::CONFLICT, "the milestone is not ready for approval".into()));
    }
    if let Some(review) = run.milestone_review.clone() {
        let open = tokio::task::spawn_blocking(move || crate::changes::unresolved_human_comments(&review)).await.map_err(|e| anyhow::anyhow!(e))??;
        if open > 0 {
            return Err(ApiError(StatusCode::CONFLICT, format!("{open} human difftool comment(s) are still open; resolve them in difftool before approving")));
        }
    }
    let previous = run.clone();
    run.milestone_pending = false;
    run.milestone_review = None;
    s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
    if let Err(e) = state.runs.send(&id, RunInput::Text("This milestone is approved. If unchecked plan tickets remain, implement exactly the next one and pause for review again. If every ticket is complete, run the full project checks and move to implementation-review.".into())).await {
        s.upsert_run(previous, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
        return Err(ApiError(StatusCode::CONFLICT, e.to_string()));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn stop(State(state): State<AppState>, Path((repo, slug, id)): Path<(String, String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    s.run(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    match state.runs.send(&id, RunInput::Stop).await {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(_) => Ok(Json(json!({ "ok": true, "note": "run was not live" }))),
    }
}

async fn remove(State(state): State<AppState>, Path((repo, slug, id)): Path<(String, String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let run = s.run(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    if state.runs.is_live(&id) || matches!(run.status, plantool_core::RunStatus::Starting | plantool_core::RunStatus::Running | plantool_core::RunStatus::Waiting | plantool_core::RunStatus::Idle) && state.runs.is_live(&id) {
        return Err(ApiError(StatusCode::CONFLICT, format!("run {id} is still live; stop it first")));
    }
    s.remove_run(&id)?;
    Ok(Json(json!({ "ok": true, "removed": id })))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    pub since: u64,
}

async fn events(State(state): State<AppState>, Path((repo, slug, id)): Path<(String, String, String)>, Query(q): Query<EventsQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let run = s.run(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    let events = s.store.read_run_events(&id, q.since)?;
    Ok(Json(json!({ "run": run, "events": events })))
}

async fn providers() -> Json<serde_json::Value> {
    let list = tokio::task::spawn_blocking(crate::providers::discover).await.unwrap_or_default();
    Json(json!({ "providers": list }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/providers", get(providers))
        .route("/sessions/{repo}/{slug}/runs", post(start))
        .route("/sessions/{repo}/{slug}/runs/{id}/input", post(input))
        .route("/sessions/{repo}/{slug}/runs/{id}/milestone/approve", post(approve_milestone))
        .route("/sessions/{repo}/{slug}/runs/{id}/stop", post(stop))
        .route("/sessions/{repo}/{slug}/runs/{id}", delete(remove))
        .route("/sessions/{repo}/{slug}/runs/{id}/remove", post(remove))
        .route("/sessions/{repo}/{slug}/runs/{id}/events", get(events))
}
