use super::{actor_from, ApiError};
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
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
async fn view(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    Ok(Json(crate::workspace::snapshot(&state.config.home)?))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    thread: String,
    body: String,
    idempotency_key: String,
}
async fn reply(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Reply>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    let id = crate::workspace::owner_reply(
        &state.config.home,
        &body.thread,
        &body.body,
        &body.idempotency_key,
    )?;
    Ok(Json(json!({"id":id,"authority":"context_only"})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workspace", get(view))
        .route("/workspace/reply", post(reply))
}
