use super::sessions::resolve;
use super::{bad_request, ApiError};
use crate::providers::RunInput;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use plantool_core::{Actor, Provider, Stage};
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
    let sess = s.session();
    let prompt = crate::prompts::render(&state.config.home, stage_name, &sess, &s.store.dir, body.prompt.as_deref().filter(|p| !p.trim().is_empty()));
    let resume = body.resume_run.as_deref().and_then(|rid| s.run(rid)).and_then(|r| r.provider_session_id);
    let run = state.runs.start(s.clone(), provider, target, stage_name, prompt.clone(), body.model.clone(), resume)?;
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
}

async fn input(State(state): State<AppState>, Path((repo, slug, id)): Path<(String, String, String)>, Json(body): Json<InputBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    s.run(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown run {id}")))?;
    let msg = if let Some(p) = body.permission {
        RunInput::Permission { request_id: p.request_id, decision: p.decision }
    } else if let Some(i) = body.input {
        RunInput::Input { request_id: i.request_id, answers: i.answers }
    } else if let Some(t) = body.text.filter(|t| !t.trim().is_empty()) {
        RunInput::Text(t)
    } else {
        return Err(bad_request("give text, permission or input"));
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
        .route("/sessions/{repo}/{slug}/runs/{id}/events", get(events))
}
