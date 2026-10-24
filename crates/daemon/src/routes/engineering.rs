use super::{actor_from, ApiError};
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::{json, Value};
fn human(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if actor_from(headers, state) != plantool_core::Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Browser session required".into(),
        ));
    }
    Ok(())
}
async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    if state.runs.worker.availability().available {
        let _ = state.runs.worker.call("health", None).await;
    }
    let available = state.runs.worker.availability();
    Ok(Json(
        json!({"worker":available,"items":crate::workflow::overview(&state.config.home)?,"versions":plantool_contract::Health::default(),"sandbox_supported":crate::sandbox::supported()}),
    ))
}
async fn repair(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    state
        .runs
        .worker
        .repair(&state)
        .await
        .map_err(|e| ApiError(StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    let _ =
        crate::models::gateway::verify(&state.config.home, &crate::models::gateway::HttpGateway)
            .await?;
    crate::database::audit(
        &crate::database::open(&state.config.home)?,
        "human",
        "worker.repair",
        "worker",
        &json!({"versions":plantool_contract::Health::default()}),
    )?;
    Ok(Json(json!({"worker":state.runs.worker.availability()})))
}
pub(crate) fn worker(
    state: &AppState,
    headers: &HeaderMap,
    transport: Option<&crate::socket::SocketTransport>,
) -> Result<(), ApiError> {
    if transport.is_none()
        || !state.runs.tokens.is_worker(headers)
        || headers
            .get("x-plantool-contract-version")
            .and_then(|h| h.to_str().ok())
            != Some(plantool_contract::CONTRACT_VERSION.to_string().as_str())
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Matching authenticated worker socket required".into(),
        ));
    }
    Ok(())
}
async fn health(
    State(state): State<AppState>,
    headers: HeaderMap,
    transport: Option<Extension<crate::socket::SocketTransport>>,
) -> Result<Json<plantool_contract::Health>, ApiError> {
    worker(&state, &headers, transport.as_deref())?;
    Ok(Json(plantool_contract::Health::default()))
}
pub fn browser_routes() -> Router<AppState> {
    Router::new()
        .route("/engineering-os/status", get(status))
        .route("/engineering-os/repair", post(repair))
}
pub fn internal_routes() -> Router<AppState> {
    Router::new().route("/internal/v1/health", get(health))
}
