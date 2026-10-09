use super::{actor_from, bad_request, sessions::resolve, ApiError};
use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Deserialize, Default)]
struct ExportQuery {
    #[serde(default)]
    transcripts: bool,
    #[serde(default)]
    revisions: bool,
    note: Option<String>,
}
async fn export(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Query(q): Query<ExportQuery>,
) -> Result<([(String, String); 2], Vec<u8>), ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let bytes = crate::archive::export(&s, q.transcripts, q.revisions, q.note.as_deref())?;
    Ok((
        [
            ("content-type".into(), "application/zip".into()),
            (
                "content-disposition".into(),
                format!("attachment; filename=\"{slug}.zip\""),
            ),
        ],
        bytes,
    ))
}
#[derive(Deserialize)]
struct ImportQuery {
    repo: PathBuf,
    workspace: Option<PathBuf>,
    #[serde(default)]
    confirm: bool,
}
async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
    bytes: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut bundle = crate::archive::decode(&bytes).map_err(|e| bad_request(e.to_string()))?;
    if actor_from(&headers, &state) != plantool_core::Actor::Human && !q.confirm {
        bundle.state.stage = plantool_core::Stage::PlanReview;
        bundle.state.approved = None;
    }
    let view = state
        .registry
        .import_bundle(bundle, &q.repo, q.workspace.as_deref())?;
    Ok(Json(json!({"session":view})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/export", get(export))
        .route("/import", post(import))
}
