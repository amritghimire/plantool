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
    // Stop All terminates the worker too. Resume must restore an existing
    // installation, rather than leaving every eligible graph without a worker.
    let mut warnings = vec![];
    if crate::sandbox::supported()
        && crate::worker::has_installed_worker(&state.config.home)
        && !state.runs.worker.availability().available
    {
        if let Err(error) = state.runs.worker.repair(&state).await {
            warnings.push(format!(
                "Worker could not restart: {error}. Open Settings → Worker."
            ));
        }
    }
    if let Err(error) = crate::scheduler::tick(&state.config.home, chrono::Utc::now()) {
        warnings.push(format!("Scheduling needs attention: {error}"));
    }
    if let Err(error) = crate::graph_recovery::reconcile(&state).await {
        warnings.push(format!("Workflow restoration needs attention: {error}"));
    }
    let report = crate::graph_recovery::resume_report(&state.config.home)?;
    Ok(Json(
        json!({"execution":status,"recovery":report,"warnings":warnings,"worker":state.runs.worker.availability()}),
    ))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/execution/status", get(status))
        .route("/execution/stop", post(stop))
        .route("/execution/resume", post(resume))
}
