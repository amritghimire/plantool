use super::{sessions::resolve, ApiError};
use crate::{identities::AuthorizedRun, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Extension, Json, Router,
};
use serde_json::{json, Value};
async fn done(
    State(state): State<AppState>,
    Path((repo, slug, key)): Path<(String, String, String)>,
    run: Option<Extension<AuthorizedRun>>,
) -> Result<Json<Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let status = state.runs.execution.status();
    if status.stopped {
        return Err(ApiError(StatusCode::CONFLICT, "Stop All is active".into()));
    }
    s.complete_task(&key, run.as_ref().map(|r| r.0.scope.attempt.as_str()))
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(
        json!({"ok":true,"task_progress":s.store.task_progress()?}),
    ))
}
pub fn routes() -> Router<AppState> {
    Router::new().route("/sessions/{repo}/{slug}/tasks/{key}/done", post(done))
}
