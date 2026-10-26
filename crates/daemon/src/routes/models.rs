use super::{actor_from, ApiError};
use crate::{
    database,
    models::{gateway, proxy, router},
    AppState,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use rusqlite::params;
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
async fn settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    let db = database::open(&state.config.home)?;
    let projects = {
        let mut q = db
            .prepare("SELECT id,local_checkout,visibility FROM projects")
            .map_err(anyhow::Error::from)?;
        let rows = q
            .query_map([], |r| {
                Ok(json!({"id":r.get::<_,String>(0)?,"checkout":r.get::<_,String>(1)?,"visibility":r.get::<_,Option<String>>(2)?}))
            })
            .map_err(anyhow::Error::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(anyhow::Error::from)?;
        rows
    };
    let projects = projects
        .into_iter()
        .map(|mut project| -> anyhow::Result<Value> {
            project["allowed_providers"] =
                json!(router::allowed(&db, project["id"].as_str().unwrap())?);
            Ok(project)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut routes = {
        let mut q = db
            .prepare("SELECT route,available,model,error FROM route_health")
            .map_err(anyhow::Error::from)?;
        let rows=q.query_map([],|r|Ok(json!({"route":r.get::<_,String>(0)?,"available":r.get::<_,bool>(1)?,"model":r.get::<_,Option<String>>(2)?,"error":r.get::<_,Option<String>>(3)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
        rows
    };
    for route in &mut routes {
        route["diagnostic"] = database::get::<Value>(
            &db,
            &format!(
                "provider.diagnostic:{}",
                route["route"].as_str().unwrap_or("")
            ),
        )?
        .unwrap_or(Value::Null);
    }
    let mut diagnostic_batch =
        database::get::<Value>(&db, "provider.diagnostic_batch")?.unwrap_or(Value::Null);
    if diagnostic_batch["finished"] == false && diagnostic_batch["pid"] != std::process::id() {
        diagnostic_batch["finished"] = json!(true);
        diagnostic_batch["warnings"]=json!(["Daemon restarted; remaining live checks were not replayed. Start a new batch when ready."]);
    }
    let diagnostic_activity = {
        let mut q = db.prepare("SELECT holder_kind,'running',model_route FROM leases WHERE holder_kind LIKE 'diagnostic:%' UNION ALL SELECT id,'queued',json_object('route',route,'model',model) FROM model_queue WHERE id LIKE 'diagnostic:%'").map_err(anyhow::Error::from)?;
        let rows = q.query_map([], |r| Ok(json!({"owner":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"model":r.get::<_,String>(2)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
        rows
    };
    Ok(Json(
        json!({"diagnostic_batch":diagnostic_batch,"diagnostic_activity":diagnostic_activity,"yaml":serde_yaml::to_string(&router::config(&state.config.home)?).map_err(anyhow::Error::from)?,"projects":projects,"routes":routes}),
    ))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    yaml: String,
    project: Option<String>,
    allowed_providers: Option<Vec<String>>,
}
async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Settings>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    if !state.runs.execution.status().stopped {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Use Stop All before changing providers or chains".into(),
        ));
    }
    let cfg: router::Chains = serde_yaml::from_str(&body.yaml).map_err(anyhow::Error::from)?;
    cfg.validate()?;
    if let Some(providers) = &body.allowed_providers {
        if providers.iter().any(|p| {
            ![
                "openai",
                "openrouter",
                "ollama-cloud",
                "opencode-zen",
                "ollama",
            ]
            .contains(&p.as_str())
        }) {
            return Err(ApiError(StatusCode::BAD_REQUEST, "Unknown provider".into()));
        }
    }
    let mut db = database::open(&state.config.home)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    database::set(&tx, "model_chains", &cfg)?;
    tx.execute(
        "DELETE FROM settings WHERE key LIKE 'provider.diagnostic:%'",
        [],
    )
    .map_err(anyhow::Error::from)?;
    tx.execute("DELETE FROM route_health", [])
        .map_err(anyhow::Error::from)?;
    if let (Some(project), Some(providers)) = (&body.project, &body.allowed_providers) {
        let copied=tx.execute("INSERT INTO project_settings(project_id,version,oversight,allowed_providers_json,never_sent_globs_json,sprint_day,scan_time,timezone,retention_json,effective_at,set_by,sprint_time,review_globs_json) SELECT project_id,version+1,oversight,?2,never_sent_globs_json,sprint_day,scan_time,timezone,retention_json,?3,'browser',sprint_time,review_globs_json FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",params![project,serde_json::to_string(providers).map_err(anyhow::Error::from)?,plantool_core::now()]).map_err(anyhow::Error::from)?;
        if copied == 0 {
            tx.execute("INSERT INTO project_settings(project_id,version,allowed_providers_json,effective_at,set_by) VALUES(?1,1,?2,?3,'browser')",params![project,serde_json::to_string(providers).map_err(anyhow::Error::from)?,plantool_core::now()]).map_err(anyhow::Error::from)?;
        }
    }
    database::audit(
        &tx,
        "human",
        "models.settings",
        "global",
        &json!({"config":cfg,"project":body.project,"allowed_providers":body.allowed_providers}),
    )?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(Json(json!({"ok":true,"verification_required":true})))
}
async fn verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    Ok(Json(
        gateway::verify(&state.config.home, &gateway::HttpGateway).await?,
    ))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    route: String,
}
async fn diagnose(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Diagnostic>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    crate::models::diagnostics::run(&state, &body.route)
        .await
        .map(Json)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))
}
async fn diagnose_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    if state.runs.execution.status().stopped {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Resume execution before live checks".into(),
        ));
    }
    let routes: Vec<String> = router::config(&state.config.home)?
        .routes
        .into_keys()
        .collect();
    let generation = state.runs.execution.status().generation;
    let started = crate::database::unix_now();
    {
        let mut db = database::open(&state.config.home)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(anyhow::Error::from)?;
        let previous = database::get::<Value>(&tx, "provider.diagnostic_batch")?;
        if previous.is_some_and(|p| p["pid"] == std::process::id() && p["finished"] == false) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "A diagnostic batch is already queued or running".into(),
            ));
        }
        database::set(
            &tx,
            "provider.diagnostic_batch",
            &json!({"pid":std::process::id(),"started_at":started,"routes":routes,"completed":[],"finished":false}),
        )?;
        tx.commit().map_err(anyhow::Error::from)?;
    }
    let scheduled = routes.clone();
    tokio::spawn(async move {
        let mut completed = vec![];
        let mut warnings = vec![];
        for route in &scheduled {
            let current = state.runs.execution.status();
            if current.stopped || current.generation != generation {
                warnings.push("Execution changed; remaining tests were cancelled".to_string());
                break;
            }
            if let Err(error) = crate::models::diagnostics::run(&state, route).await {
                warnings.push(format!(
                    "{route}: {}",
                    crate::models::diagnostics::sanitized_error(&error.to_string())
                ));
            }
            completed.push(route.clone());
            if let Ok(db) = database::open(&state.config.home) {
                let _ = database::set(
                    &db,
                    "provider.diagnostic_batch",
                    &json!({"pid":std::process::id(),"started_at":started,"routes":scheduled,"completed":completed,"finished":false,"warnings":warnings}),
                );
            }
        }
        if let Ok(db) = database::open(&state.config.home) {
            let _ = database::set(
                &db,
                "provider.diagnostic_batch",
                &json!({"pid":std::process::id(),"started_at":started,"routes":scheduled,"completed":completed,"finished":true,"warnings":warnings}),
            );
        }
    });
    Ok(Json(json!({"queued":true,"routes":routes})))
}
async fn completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    authorized: Option<Extension<crate::identities::AuthorizedRun>>,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, ApiError> {
    let (owner, context) = if let Some(authorized) = authorized {
        let db = database::open(&state.config.home)?;
        let (item, role, generation): (String, String, u64) = db
            .query_row(
                "SELECT item_id,role,generation FROM attempts WHERE id=?1",
                [&authorized.scope.attempt],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(anyhow::Error::from)?;
        let key = headers
            .get("idempotency-key")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned)
            .unwrap_or_else(|| plantool_core::sha256_hex(&body.to_string()));
        (
            format!("attempt:{}", authorized.scope.attempt),
            plantool_contract::LlmContext {
                item,
                role,
                generation,
                attempt_id: Some(authorized.scope.attempt.clone()),
                idempotency_key: key,
            },
        )
    } else {
        super::engineering::worker(&state, &headers, socket.as_deref())?;
        let context: plantool_contract::LlmContext =
            serde_json::from_value(body["plantool"].clone()).map_err(anyhow::Error::from)?;
        crate::dispatcher::item(&crate::database::open(&state.config.home)?, &context.item)?;
        let owner = context
            .attempt_id
            .as_ref()
            .map(|id| format!("attempt:{id}"))
            .unwrap_or_else(|| format!("llm:{}:{}", context.item, context.idempotency_key));
        (owner, context)
    };
    let reply = proxy::complete(
        &state.config.home,
        &owner,
        &context,
        body,
        state.runs.execution.clone(),
        &gateway::HttpGateway,
        &crate::models::lease::Ollama,
    )
    .await
    .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY);
    Ok((
        status,
        [
            ("content-type", reply.content_type),
            ("cache-control", "no-store".into()),
        ],
        reply.body,
    )
        .into_response())
}
pub fn browser_routes() -> Router<AppState> {
    Router::new()
        .route("/settings/models", get(settings).post(save))
        .route("/settings/models/verify", post(verify))
        .route("/settings/models/diagnose", post(diagnose))
        .route("/settings/models/diagnose-all", post(diagnose_all))
}
pub fn proxy_routes() -> Router<AppState> {
    Router::new().route("/v1/chat/completions", post(completions))
}
