use super::{actor_from, bad_request, ApiError};
use crate::git;
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use plantool_core::Actor;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub struct WorktreeDirQuery {
    #[serde(default)]
    pub repo: Option<PathBuf>,
    #[serde(default)]
    pub preview: Option<String>,
}

#[derive(Deserialize)]
pub struct WorktreeDirBody {
    pub scope: String,
    #[serde(default)]
    pub repo: Option<PathBuf>,
    #[serde(default)]
    pub value: Option<String>,
}

fn main_root(repo: &Path) -> Result<PathBuf, ApiError> {
    let checkout = git::detect_checkout(repo).map_err(|e| bad_request(e.to_string()))?;
    Ok(git::main_root(&checkout.common_dir))
}

fn describe(state: &AppState, repo: Option<&Path>, preview: Option<&str>) -> Result<serde_json::Value, ApiError> {
    let root = repo.map(main_root).transpose()?;
    let cwd = root.clone().unwrap_or_else(|| state.config.home.clone());
    let global = git::worktree_dir_setting(&cwd, true);
    let local = root.as_deref().and_then(|r| git::worktree_dir_setting(r, false));
    let effective = match &root {
        Some(r) => git::worktree_dir_template(r),
        None => global.clone().unwrap_or_else(|| git::DEFAULT_WORKTREE_DIR.to_string()),
    };
    let template = preview.map(str::trim).filter(|p| !p.is_empty()).unwrap_or(&effective);
    let example = root.as_deref().map(|r| git::resolve_worktree_dir(r, template, "<slug>"));
    Ok(json!({
        "key": git::WORKTREE_DIR_KEY,
        "default": git::DEFAULT_WORKTREE_DIR,
        "global": global,
        "repo": root.as_ref().map(|r| json!({ "root": r, "value": local })),
        "effective": effective,
        "example": example,
    }))
}

async fn get_worktree_dir(State(state): State<AppState>, Query(q): Query<WorktreeDirQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    tokio::task::spawn_blocking(move || describe(&state, q.repo.as_deref(), q.preview.as_deref()).map(Json)).await.map_err(|e| anyhow::anyhow!(e))?
}

async fn set_worktree_dir(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<WorktreeDirBody>) -> Result<Json<serde_json::Value>, ApiError> {
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(StatusCode::FORBIDDEN, "only a human can change settings; use the browser or git config".into()));
    }
    let global = match body.scope.as_str() {
        "global" => true,
        "repo" => false,
        other => return Err(bad_request(format!("unknown scope {other}; use global or repo"))),
    };
    tokio::task::spawn_blocking(move || {
        let root = body.repo.as_deref().map(main_root).transpose()?;
        let cwd = match (&root, global) {
            (Some(r), _) => r.clone(),
            (None, true) => state.config.home.clone(),
            (None, false) => return Err(bad_request("repo is required for the repo scope")),
        };
        git::set_worktree_dir_setting(&cwd, global, body.value.as_deref()).map_err(|e| bad_request(e.to_string()))?;
        describe(&state, root.as_deref(), None).map(Json)
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/settings/worktree-dir", get(get_worktree_dir).post(set_worktree_dir))
}
