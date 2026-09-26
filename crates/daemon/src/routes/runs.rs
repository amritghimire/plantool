use super::sessions::resolve;
use super::{bad_request, ApiError};
use crate::providers::RunInput;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use plantool_core::{Actor, PermissionMode, Provider, Stage};
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
}

fn parse_mode(s: &str) -> Result<PermissionMode, ApiError> {
    PermissionMode::parse(s).ok_or_else(|| bad_request(format!("permission mode must be one of ask, accept-edits, auto, allow-all (got {s})")))
}

async fn start(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>, Json(body): Json<StartBody>) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let provider = Provider::parse(&body.provider).ok_or_else(|| bad_request(format!("unknown provider {}", body.provider)))?;
    let stage_name = body.stage.as_str();
    let target = match stage_name {
        "research" => Stage::Researching,
        "plan" => Stage::Planning,
        "implement" => Stage::Implementing,
        other => return Err(bad_request(format!("stage must be research, plan or implement (got {other})"))),
    };
    let current = s.stage();
    if stage_name == "implement" && !matches!(current, Stage::Approved | Stage::Implementing | Stage::ImplementationReview) {
        return Err(ApiError(StatusCode::FORBIDDEN, format!("the plan must be approved before implementation starts (stage is {current})")));
    }
    if target.index() > current.index() {
        let _ = s.set_stage(target, Actor::Agent);
    }
    if stage_name == "implement" && body.worktree.unwrap_or(true) {
        let ws = s.clone();
        tokio::task::spawn_blocking(move || ws.ensure_worktree(None)).await.map_err(|e| anyhow::anyhow!(e))??;
    }
    let sess = s.session();
    let resume_from = body.resume_run.as_deref().and_then(|rid| s.run(rid));
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
    let extra = body.prompt.as_deref().filter(|p| !p.trim().is_empty());
    let prompt = crate::prompts::render_at(&state.config.home, if resume_from.is_some() { "resume" } else { stage_name }, &sess, &s.store.dir, extra, s.stage());
    let resume = resume_from.and_then(|r| r.provider_session_id);
    let mode = body.permission_mode.as_deref().map(parse_mode).transpose()?.unwrap_or_default();
    let run = state.runs.start(s.clone(), provider, target, stage_name, prompt.clone(), body.model.clone(), resume, mode)?;
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
        .route("/sessions/{repo}/{slug}/runs/{id}/stop", post(stop))
        .route("/sessions/{repo}/{slug}/runs/{id}", delete(remove))
        .route("/sessions/{repo}/{slug}/runs/{id}/remove", post(remove))
        .route("/sessions/{repo}/{slug}/runs/{id}/events", get(events))
}
