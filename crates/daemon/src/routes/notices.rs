use super::{actor_from, ApiError};
use crate::{database, AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
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
async fn notice(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    let db = database::open(&state.config.home)?;
    Ok(Json(
        json!({"backup":database::get::<Value>(&db,"clean_break_banner")?.and_then(|v|v["backup"].as_str().map(str::to_string))}),
    ))
}
async fn dismiss(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    database::set(
        &database::open(&state.config.home)?,
        "clean_break_banner",
        &Value::Null,
    )?;
    Ok(Json(json!({"ok":true})))
}
pub fn routes() -> Router<AppState> {
    Router::new().route("/notices/clean-break", get(notice).post(dismiss))
}
