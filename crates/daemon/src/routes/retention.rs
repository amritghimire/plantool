use super::{actor_from, sessions::resolve, ApiError};
use crate::{
    auth::ApprovedAction,
    retention::{self, Policy},
    AppState,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Extension, Json, Router,
};
use serde_json::{json, Value};
fn human(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if actor_from(headers, state) != plantool_core::Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Retention settings are browser-only".into(),
        ));
    }
    Ok(())
}
async fn settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Policy>, ApiError> {
    human(&state, &headers)?;
    Ok(Json(retention::policy(&state.config.home)?))
}
async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<ApprovedAction>,
    Json(policy): Json<Policy>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    retention::save_policy(&state.config.home, &policy)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(serde::Deserialize)]
struct Pin {
    pinned: bool,
}
async fn pin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug, id)): Path<(String, String, String)>,
    Extension(approval): Extension<ApprovedAction>,
    Json(body): Json<Pin>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    s.pin_run(&id, body.pinned)?;
    Ok(Json(json!({"ok":true,"pinned":body.pinned})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings/retention", get(settings).post(save))
        .route(
            "/sessions/{repo}/{slug}/runs/{id}/pin",
            axum::routing::post(pin),
        )
}
