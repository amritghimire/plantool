use super::{actor_from, ApiError};
use crate::AppState;
use axum::{
    extract::{Path, State},
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
async fn projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    Ok(Json(crate::engineering_view::overview(&state.config.home)?))
}
async fn item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    crate::engineering_view::item(&state.config.home, &id)
        .map(Json)
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SetupCursor {
    step: String,
    project: Option<String>,
}
async fn setup_cursor(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    Ok(Json(
        crate::database::get::<Value>(
            &crate::database::open(&state.config.home)?,
            "engineering.setup_cursor",
        )?
        .unwrap_or(json!({"step":"welcome","project":null})),
    ))
}
async fn save_cursor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(cursor): Json<SetupCursor>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    if ![
        "welcome", "tools", "github", "worker", "models", "policies", "ready",
    ]
    .contains(&cursor.step.as_str())
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Unknown setup step".into(),
        ));
    }
    let db = crate::database::open(&state.config.home)?;
    if let Some(project) = &cursor.project {
        let valid: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND mode='eos')",
                [project],
                |r| r.get(0),
            )
            .map_err(anyhow::Error::from)?;
        if !valid {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "Unknown Engineering OS project".into(),
            ));
        }
    }
    // Navigation preference only: this never configures policy or grants an approval.
    let value = serde_json::to_value(cursor).map_err(anyhow::Error::from)?;
    crate::database::set(&db, "engineering.setup_cursor", &value)?;
    Ok(Json(value))
}
pub fn browser_routes() -> Router<AppState> {
    Router::new()
        .route("/engineering-os/status", get(status))
        .route("/engineering-os/setup", get(setup_cursor).post(save_cursor))
        .route("/engineering-os/projects", get(projects))
        .route("/engineering-os/items/{id}", get(item))
        .route("/engineering-os/repair", post(repair))
}
pub fn internal_routes() -> Router<AppState> {
    Router::new().route("/internal/v1/health", get(health))
}
