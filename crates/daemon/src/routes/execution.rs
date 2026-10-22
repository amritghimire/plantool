use super::{actor_from, ApiError};
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use plantool_core::Actor;
use serde_json::{json, Value};
fn human(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if actor_from(headers, state) != Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Execution controls are browser-only".into(),
        ));
    }
    Ok(())
}
async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    Ok(Json(
        json!({"execution":state.runs.execution.status(),"sandbox_supported":crate::sandbox::supported()}),
    ))
}
async fn stop(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    let (result, ()) = tokio::join!(
        state.runs.execution.stop_all(&state.runs.tokens),
        state.runs.stop_inputs()
    );
    let status = result.map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({"execution":status})))
}
async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    let status = approval.execution()?.clone();
    let status = state
        .runs
        .execution
        .resume(&status)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({"execution":status})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/execution/status", get(status))
        .route("/execution/stop", post(stop))
        .route("/execution/resume", post(resume))
}
