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
    extract::{Path, State},
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
    Ok(
        json!({"repositories":setup::repositories(home)?,"allowlist":setup::allowlist(home)?,"credential_revision":database::get::<u64>(&database::open(home)?,"github_credential_revision")?.unwrap_or(0)}),
    )
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
    if let Some(repositories) = value["repositories"].as_array_mut() {
        for repo in repositories {
            if let Some(project) = repo["project"].as_str().map(str::to_owned) {
                repo["preflight"] =
                    database::get::<Value>(&db, &format!("github_preflight:{project}"))?
                        .unwrap_or(Value::Null);
                repo["visibility"] = db
                    .query_row(
                        "SELECT visibility FROM projects WHERE id=?1",
                        [project],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .map_err(anyhow::Error::from)?
                    .map(Value::String)
                    .unwrap_or(Value::Null);
            }
        }
    }
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
        let mut db = database::open(&s.config.home)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)?;
        let revision = database::get::<u64>(&tx, "github_credential_revision")?.unwrap_or(0) + 1;
        database::set(&tx, "github_credential_revision", &revision)?;
        database::audit(
            &tx,
            "human",
            "github.key_stored",
            &cfg.app_id.to_string(),
            &json!({"app_id":cfg.app_id}),
        )?;
        tx.commit().map_err(anyhow::Error::from)?;
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preflight {
    project: String,
}
async fn preflight(
    State(s): State<AppState>,
    h: HeaderMap,
    Extension(proof): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Preflight>,
) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    let _guard = mutation_lock().lock().await;
    proof.recheck(&s)?;
    let repo = setup::repositories(&s.config.home)?
        .into_iter()
        .find(|r| r.project == body.project)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "GitHub project not found".into()))?;
    let client = api::Client::new(
        AppConfig {
            app_id: repo.app_id,
            installation_id: repo.installation_id,
        },
        Arc::new(Keychain),
    )?;
    Ok(Json(
        setup::refresh_preflight(&s.config.home, &client, &repo).await?,
    ))
}
async fn plan(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    let db = database::open(&s.config.home)?;
    let (sha,summary,thread,gate):(String,String,Option<String>,Value)=db.query_row("SELECT r.content_sha256,c.summary,c.discussion_thread_id,json_object('state',g.state,'number',r.number,'hash',r.content_sha256) FROM github_gate_context c JOIN gates g ON g.id=c.gate_id JOIN revisions r ON r.id=c.revision_id WHERE c.gate_id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,serde_json::from_str(&r.get::<_,String>(3)?).unwrap_or(Value::Null)))).map_err(|_|ApiError(StatusCode::NOT_FOUND,"Plan publication not found".into()))?;
    let messages = if let Some(thread) = thread {
        let mut q = db
            .prepare(
                "SELECT sender_role,body FROM messages WHERE thread_id=?1 ORDER BY created_at,id",
            )
            .map_err(anyhow::Error::from)?;
        let rows = q
            .query_map([thread], |r| {
                Ok(json!({"role":r.get::<_,String>(0)?,"body":r.get::<_,String>(1)?}))
            })
            .map_err(anyhow::Error::from)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(anyhow::Error::from)?
    } else {
        vec![]
    };
    Ok(Json(
        json!({"gate":gate,"summary":summary,"plan":String::from_utf8(crate::artifacts::read(&s.config.home,&sha)?).map_err(anyhow::Error::from)?,"messages":messages}),
    ))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/github/plans/{id}", get(plan))
        .route("/settings/github", get(status).post(configure))
        .route("/settings/github/allowlist", post(allow))
        .route("/settings/github/preflight", post(preflight))
}
