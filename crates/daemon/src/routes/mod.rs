pub mod archive;
pub mod changes;
pub mod comments;
pub mod live;
pub mod pr;
pub mod runs;
pub mod sessions;
pub mod settings;
pub mod where_context;

use crate::registry::RegistryError;
use crate::AppState;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use plantool_core::Actor;
use serde_json::json;

pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<RegistryError> for ApiError {
    fn from(e: RegistryError) -> Self {
        let status = match &e {
            RegistryError::NotFound(_)
            | RegistryError::NoDoc(..)
            | RegistryError::UnknownComment(_) => StatusCode::NOT_FOUND,
            RegistryError::ReviewBlocked(_) => StatusCode::CONFLICT,
            RegistryError::Ambiguous { .. } | RegistryError::RepoMismatch { .. } => {
                StatusCode::CONFLICT
            }
            RegistryError::BadSlug(_) | RegistryError::Anchor(_) => StatusCode::BAD_REQUEST,
            RegistryError::Stage(_) => StatusCode::FORBIDDEN,
            RegistryError::Git(_) => StatusCode::BAD_REQUEST,
            RegistryError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(status, e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

pub fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

pub fn actor_from(headers: &HeaderMap, state: &AppState) -> Actor {
    match headers
        .get("x-plantool-actor")
        .and_then(|v| v.to_str().ok())
    {
        Some(t) if t == state.config.token => Actor::Human,
        _ => Actor::Agent,
    }
}

pub fn author_from(headers: &HeaderMap, actor: Actor) -> String {
    headers
        .get("x-plantool-author")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| match actor {
            Actor::Human => "you".to_string(),
            Actor::Agent => "agent".to_string(),
        })
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "protocol": plantool_core::PROTOCOL_VERSION,
        "version": state.config.version,
        "pid": std::process::id(),
        "home": state.config.home,
        "sessions": state.registry.all().len(),
    }))
}

async fn shutdown(State(state): State<AppState>) -> Json<serde_json::Value> {
    let tx = state.shutdown.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = tx.send(()).await;
    });
    Json(json!({ "ok": true }))
}

async fn spa(State(state): State<AppState>, uri: Uri) -> Response {
    crate::assets::serve(&uri, &state.config.token)
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/shutdown", post(shutdown))
        .merge(sessions::routes())
        .merge(comments::routes())
        .merge(changes::routes())
        .merge(archive::routes())
        .merge(live::routes())
        .merge(runs::routes())
        .merge(settings::routes())
        .merge(where_context::routes())
        .merge(pr::routes());
    Router::new()
        .nest("/api", api)
        .route("/health", get(health))
        .route("/shutdown", post(shutdown))
        .fallback(spa)
        .layer(DefaultBodyLimit::max(crate::security::MAX_BODY_BYTES))
        .layer(axum::middleware::from_fn(crate::security::guard))
        .with_state(state)
}
