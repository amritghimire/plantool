use super::{actor_from, bad_request, ApiError};
use crate::registry::{CreateSession, SessionView};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use plantool_core::Actor;
use axum::{Json, Router};
use plantool_core::{DocKind, Stage};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
}

async fn list(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Result<Json<Vec<SessionView>>, ApiError> {
    let stage = match q.stage.as_deref() {
        Some(s) => Some(Stage::parse(s).ok_or_else(|| bad_request(format!("unknown stage {s}")))?),
        None => None,
    };
    Ok(Json(state.registry.list(q.repo.as_deref(), stage)))
}

async fn create(State(state): State<AppState>, Json(intent): Json<CreateSession>) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let reg = state.registry.clone();
    let (live, created) = tokio::task::spawn_blocking(move || reg.create(intent)).await.map_err(|e| anyhow::anyhow!(e))??;
    let view = live.view();
    let status = if created { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(json!({ "created": created, "session": view, "url": format!("http://127.0.0.1:{}{}", state.config.port, view.url_path) }))))
}

#[derive(Deserialize)]
pub struct RefQuery {
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

pub fn resolve(state: &AppState, reference: &str, cwd: Option<&std::path::Path>) -> Result<std::sync::Arc<crate::registry::LiveSession>, ApiError> {
    Ok(state.registry.resolve(reference, cwd)?)
}

async fn get_one(State(state): State<AppState>, Path(reference): Path<String>, Query(q): Query<RefQuery>) -> Result<Json<SessionView>, ApiError> {
    let s = resolve(&state, &reference, q.cwd.as_deref())?;
    Ok(Json(s.view()))
}

#[derive(Deserialize)]
pub struct RemoveQuery {
    #[serde(default)]
    pub worktree: bool,
}

async fn remove(State(state): State<AppState>, headers: HeaderMap, Path((repo, slug)): Path<(String, String)>, Query(q): Query<RemoveQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(StatusCode::FORBIDDEN, "only a human can drop a session; use the browser".into()));
    }
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    if s.runs().iter().any(|r| state.runs.is_live(&r.id)) {
        return Err(ApiError(StatusCode::CONFLICT, "a run is still live; stop it first".into()));
    }
    let key = s.key.clone();
    let reg = state.registry.clone();
    let session = tokio::task::spawn_blocking(move || reg.remove(&key, q.worktree)).await.map_err(|e| anyhow::anyhow!(e))??;
    Ok(Json(json!({ "removed": session.key(), "worktree_removed": q.worktree && session.worktree.is_some() })))
}

async fn repos(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "repos": state.registry.repos() }))
}

async fn get_by_key(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>) -> Result<Json<SessionView>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    Ok(Json(s.view()))
}

#[derive(Deserialize)]
pub struct StageBody {
    pub to: String,
}

async fn set_stage(State(state): State<AppState>, headers: HeaderMap, Path((repo, slug)): Path<(String, String)>, Json(body): Json<StageBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let to = Stage::parse(&body.to).ok_or_else(|| bad_request(format!("unknown stage {}", body.to)))?;
    let actor = actor_from(&headers, &state);
    let stage = s.set_stage(to, actor)?;
    Ok(Json(json!({ "stage": stage, "actor": actor })))
}

fn parse_kind(kind: &str) -> Result<DocKind, ApiError> {
    DocKind::parse(kind).ok_or_else(|| bad_request(format!("unknown doc kind {kind}")))
}

#[derive(Deserialize)]
pub struct DocQuery {
    #[serde(default)]
    pub sha: Option<String>,
}

async fn get_doc(State(state): State<AppState>, Path((repo, slug, kind)): Path<(String, String, String)>, Query(q): Query<DocQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let kind = parse_kind(&kind)?;
    let rev = match &q.sha {
        Some(sha) => s.doc_revision(kind, sha)?,
        None => s.doc(kind),
    };
    let path = s.doc_path(kind);
    match rev {
        Some(r) => Ok(Json(json!({
            "kind": r.kind, "sha": r.sha, "content": r.content, "captured_at": r.captured_at, "path": path,
            "lines": plantool_core::anchor::line_count(&r.content),
            "headings": plantool_core::markdown::headings(&r.content),
            "checkboxes": plantool_core::markdown::checkboxes(&r.content),
            "progress": plantool_core::markdown::progress(&r.content),
        }))),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no {kind} document yet; write it to {}", path.display()))),
    }
}

async fn doc_path(State(state): State<AppState>, Path((repo, slug, kind)): Path<(String, String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let kind = parse_kind(&kind)?;
    let path = s.doc_path(kind);
    Ok(Json(json!({ "kind": kind, "path": path, "exists": path.is_file() })))
}

async fn touch_doc(State(state): State<AppState>, Path((repo, slug, kind)): Path<(String, String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let kind = parse_kind(&kind)?;
    let captured = s.capture_doc(kind)?;
    let current = s.doc(kind);
    Ok(Json(json!({ "kind": kind, "changed": captured.is_some(), "sha": current.map(|d| d.sha), "path": s.doc_path(kind) })))
}

#[derive(Deserialize)]
pub struct BriefBody {
    #[serde(default)]
    pub brief: Option<String>,
}

async fn set_brief(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>, Json(body): Json<BriefBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.set_brief(body.brief)?;
    Ok(Json(json!({ "brief": session.brief, "session": session })))
}

#[derive(Deserialize)]
pub struct WorktreeBody {
    #[serde(default)]
    pub dir: Option<PathBuf>,
}

async fn worktree(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>, Json(body): Json<WorktreeBody>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let created = s.session().worktree.is_none();
    let session = tokio::task::spawn_blocking(move || s.ensure_worktree(body.dir)).await.map_err(|e| anyhow::anyhow!(e))??;
    Ok(Json(json!({ "created": created, "worktree": session.worktree, "session": session })))
}

#[derive(Deserialize)]
pub struct PromptQuery {
    #[serde(default)]
    pub extra: Option<String>,
}

async fn prompt(State(state): State<AppState>, Path((repo, slug, stage)): Path<(String, String, String)>, Query(q): Query<PromptQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let stage = match stage.as_str() {
        "next" => crate::prompts::next_stage(s.stage()).ok_or_else(|| bad_request(format!("nothing left to prompt for; the stage is {}", s.stage())))?,
        st if crate::prompts::STAGES.contains(&st) => st,
        other => return Err(bad_request(format!("stage must be research, plan, implement, review or next (got {other})"))),
    };
    let text = crate::prompts::render_at(&state.config.home, stage, &s.session(), &s.store.dir, q.extra.as_deref(), s.stage());
    Ok(Json(json!({ "stage": stage, "prompt": text, "session_stage": s.stage() })))
}

async fn navigate(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>, Json(target): Json<crate::events::NavTarget>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let mut target = target;
    if let Some(cid) = &target.comment {
        let c = s.comment(cid).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown comment {cid}")))?;
        target.doc = Some(c.doc);
        target.line = Some(c.anchor.line);
    }
    let viewers = s.navigate(target.clone());
    Ok(Json(json!({ "target": target, "viewers": viewers })))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions", get(list).post(create))
        .route("/sessions/{reference}", get(get_one))
        .route("/repos", get(repos))
        .route("/sessions/{repo}/{slug}", get(get_by_key).delete(remove))
        .route("/sessions/{repo}/{slug}/remove", post(remove))
        .route("/sessions/{repo}/{slug}/stage", post(set_stage))
        .route("/sessions/{repo}/{slug}/docs/{kind}", get(get_doc))
        .route("/sessions/{repo}/{slug}/docs/{kind}/path", get(doc_path))
        .route("/sessions/{repo}/{slug}/docs/{kind}/touch", post(touch_doc))
        .route("/sessions/{repo}/{slug}/navigate", post(navigate))
        .route("/sessions/{repo}/{slug}/brief", post(set_brief))
        .route("/sessions/{repo}/{slug}/worktree", post(worktree))
        .route("/sessions/{repo}/{slug}/prompt/{stage}", get(prompt))
}
