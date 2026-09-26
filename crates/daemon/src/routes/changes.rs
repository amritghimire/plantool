use super::sessions::resolve;
use super::ApiError;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use plantool_core::DocKind;
use serde_json::json;

async fn changes(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.session();
    let review = s.state().review;
    let c = tokio::task::spawn_blocking(move || crate::changes::stat(&session, review)).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(Json(serde_json::to_value(c).map_err(|e| anyhow::anyhow!(e))?))
}

async fn open(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.session();
    let plan = s.doc_path(DocKind::Plan);
    let existing = s.state().review;
    let (review, message) = tokio::task::spawn_blocking(move || crate::changes::open_review(&session, Some(plan), existing.as_ref())).await.map_err(|e| anyhow::anyhow!(e))??;
    s.set_review(review.clone())?;
    let session = s.session();
    let c = tokio::task::spawn_blocking(move || crate::changes::stat(&session, Some(review))).await.map_err(|e| anyhow::anyhow!(e))?;
    let mut v = serde_json::to_value(c).map_err(|e| anyhow::anyhow!(e))?;
    if let Some(m) = message {
        v["message"] = json!(m);
    }
    Ok(Json(v))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/changes", get(changes))
        .route("/sessions/{repo}/{slug}/changes/open", post(open))
}
