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
    state.runs.worker.refresh_health().await;
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
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Retry {
    attempt_id: String,
}
async fn recovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    let facts = crate::recovery::snapshot(&state.config.home, &id)?;
    let routing_error = if facts["eligible"] == true {
        let db = crate::database::open(&state.config.home)?;
        let (project,role):(String,String)=db.query_row("SELECT i.project_id,a.role FROM work_items i JOIN attempts a ON a.item_id=i.id WHERE a.id=?1",[facts["attempt_id"].as_str().unwrap_or("")],|r|Ok((r.get(0)?,r.get(1)?))).map_err(anyhow::Error::from)?;
        crate::models::router::select(
            &state.config.home,
            &project,
            &id,
            &role,
            crate::database::unix_now(),
        )
        .err()
        .map(|e| e.to_string())
    } else {
        None
    };
    Ok(Json(
        json!({"routing_error":routing_error,"eligible":facts["eligible"],"attempt_id":facts["attempt_id"],"step":facts["step"],"message":if facts["eligible"]==true{"Retry this failed read-only step. Existing approvals and old checkpoints are preserved."}else{"Recovery requires a failed read-only step, no active/uncertain attempts, no pending external effects and no open gates. Reconcile other work manually."}}),
    ))
}
async fn retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Retry>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    if state.runs.execution.status().stopped {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Execution is stopped. Resume execution with passkey before retrying this step.".into(),
        ));
    }
    if !state.runs.worker.availability().available {
        return Err(ApiError(
            StatusCode::CONFLICT,
            format!(
                "Worker is not ready: {}. Open Settings → Worker to check health before retrying.",
                state.runs.worker.availability().message
            ),
        ));
    }
    crate::recovery::retry(&state.config.home, &id, &body.attempt_id)
        .map(Json)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))
}
pub fn browser_routes() -> Router<AppState> {
    Router::new()
        .route("/engineering-os/status", get(status))
        .route(
            "/engineering-os/items/{id}/recover",
            get(recovery).post(retry),
        )
        .route("/engineering-os/setup", get(setup_cursor).post(save_cursor))
        .route("/engineering-os/projects", get(projects))
        .route("/engineering-os/items/{id}", get(item))
        .route("/engineering-os/repair", post(repair))
}
pub fn internal_routes() -> Router<AppState> {
    Router::new().route("/internal/v1/health", get(health))
}
