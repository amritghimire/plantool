use super::{actor_from, ApiError};
use crate::{
    database,
    github::{
        api,
        auth::{AppConfig, KeyStore, Keychain},
        setup,
    },
    AppState,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
fn mutation_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}
fn human(s: &AppState, h: &HeaderMap) -> Result<(), ApiError> {
    if actor_from(h, s) != plantool_core::Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Browser session required".into(),
        ));
    }
    Ok(())
}
pub fn snapshot(home: &std::path::Path) -> anyhow::Result<Value> {
    Ok(json!({"repositories":setup::repositories(home)?,"allowlist":setup::allowlist(home)?}))
}
async fn status(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    let db = database::open(&s.config.home)?;
    let mut value = snapshot(&s.config.home)?;
    value["status"] =
        json!({"supported":cfg!(target_os="macos"),"worker":s.runs.worker.availability()});
    let mut q = db
        .prepare("SELECT project_id,last_error,last_sync_at FROM github_repositories")
        .map_err(anyhow::Error::from)?;
    let rows=q.query_map([],|r|Ok(json!({"project":r.get::<_,String>(0)?,"error":r.get::<_,Option<String>>(1)?,"synced_at":r.get::<_,Option<String>>(2)?}))).map_err(anyhow::Error::from)?;
    value["sync"] = json!(rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(anyhow::Error::from)?);
    Ok(Json(value))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configure {
    app_id: u64,
    installation_id: u64,
    repo: String,
    checkout: PathBuf,
    private_key: Option<String>,
}
async fn configure(
    State(s): State<AppState>,
    h: HeaderMap,
    Extension(proof): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Configure>,
) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    let _guard = mutation_lock().lock().await;
    proof.recheck(&s)?;
    let cfg = AppConfig {
        app_id: body.app_id,
        installation_id: body.installation_id,
    };
    if let Some(key) = body.private_key {
        crate::github::auth::jwt(cfg.app_id, key.as_bytes(), database::unix_now())?;
        Keychain.write(cfg.app_id, key.as_bytes())?;
        database::audit(
            &database::open(&s.config.home)?,
            "human",
            "github.key_stored",
            &cfg.app_id.to_string(),
            &json!({"app_id":cfg.app_id}),
        )?;
    }
    let client = api::Client::new(cfg.clone(), Arc::new(Keychain))?;
    let value = setup::configure(&s.config.home, &client, &cfg, &body.repo, &body.checkout).await?;
    Ok(Json(value))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Allow {
    login: String,
    remove: bool,
}
async fn allow(
    State(s): State<AppState>,
    h: HeaderMap,
    Extension(proof): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Allow>,
) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    let _guard = mutation_lock().lock().await;
    proof.recheck(&s)?;
    let repos = setup::repositories(&s.config.home)?;
    let r = repos
        .first()
        .ok_or_else(|| ApiError(StatusCode::CONFLICT, "Set up the App first".into()))?;
    let c = api::Client::new(
        AppConfig {
            app_id: r.app_id,
            installation_id: r.installation_id,
        },
        Arc::new(Keychain),
    )?;
    setup::change_allowlist(&s.config.home, &c, &body.login, body.remove).await?;
    Ok(Json(json!({"ok":true})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings/github", get(status).post(configure))
        .route("/settings/github/allowlist", post(allow))
}
