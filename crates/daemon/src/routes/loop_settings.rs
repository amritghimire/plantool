//! Browser-only loop controls. Mutations require a fresh action-bound assertion.
use super::{actor_from, ApiError};
use crate::{database, AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use rusqlite::params;
use serde_json::{json, Value};
fn human(s: &AppState, h: &HeaderMap) -> Result<(), ApiError> {
    if actor_from(h, s) != plantool_core::Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Browser session required".into(),
        ));
    }
    Ok(())
}
async fn view(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    let db = database::open(&s.config.home)?;
    let mut q=db.prepare("SELECT p.id,p.repo_full_name,coalesce(l.version,1),coalesce(l.scan_day,(coalesce(ps.sprint_day,0)+6)%7),coalesce(l.scan_time,'08:00'),coalesce(l.nix_enabled,0) FROM projects p JOIN project_settings ps ON ps.project_id=p.id AND ps.version=(SELECT max(version) FROM project_settings WHERE project_id=p.id) LEFT JOIN loop_settings l ON l.project_id=p.id WHERE p.mode='eos'").map_err(anyhow::Error::from)?;
    let projects=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,Option<String>>(1)?,"version":r.get::<_,u64>(2)?,"scan_day":r.get::<_,u32>(3)?,"scan_time":r.get::<_,String>(4)?,"nix_enabled":r.get::<_,bool>(5)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
    let mut q=db.prepare("SELECT i.id,i.title,d.reason,d.reset_at,d.active FROM loop_declines d JOIN work_items i ON i.id=d.item_id WHERE i.closed_at IS NULL").map_err(anyhow::Error::from)?;
    let declines=q.query_map([],|r|Ok(json!({"item":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"reason":r.get::<_,String>(2)?,"reset_at":r.get::<_,String>(3)?,"active":r.get::<_,bool>(4)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
    let mut q = db
        .prepare(
            "SELECT id,body,created_at FROM loop_notifications ORDER BY created_at DESC LIMIT 100",
        )
        .map_err(anyhow::Error::from)?;
    let notifications=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"body":r.get::<_,String>(1)?,"created_at":r.get::<_,String>(2)?}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
    Ok(Json(
        json!({"projects":projects,"declines":declines,"notifications":notifications}),
    ))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    project: String,
    version: u64,
    scan_day: u32,
    scan_time: String,
    nix_enabled: bool,
}
async fn save(
    State(s): State<AppState>,
    h: HeaderMap,
    Extension(a): Extension<crate::auth::ApprovedAction>,
    Json(b): Json<Settings>,
) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    a.recheck(&s)?;
    if b.scan_day >= 7 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid scan day".into()));
    }
    chrono::NaiveTime::parse_from_str(&b.scan_time, "%H:%M").map_err(anyhow::Error::from)?;
    if !s.runs.execution.status().stopped {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Use Stop All before changing loop settings".into(),
        ));
    }
    let mut db = database::open(&s.config.home)?;
    let tx = db.transaction().map_err(anyhow::Error::from)?;
    let mode: String = tx
        .query_row("SELECT mode FROM projects WHERE id=?1", [&b.project], |r| {
            r.get(0)
        })
        .map_err(anyhow::Error::from)?;
    if mode != "eos" {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Engineering OS project required".into(),
        ));
    }
    tx.execute(
        "INSERT OR IGNORE INTO loop_settings(project_id) VALUES(?1)",
        [&b.project],
    )
    .map_err(anyhow::Error::from)?;
    let changed=tx.execute("UPDATE loop_settings SET version=version+1,scan_day=?3,scan_time=?4,nix_enabled=?5 WHERE project_id=?1 AND version=?2",params![b.project,b.version,b.scan_day,b.scan_time,b.nix_enabled]).map_err(anyhow::Error::from)?;
    if changed != 1 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Settings changed; reload".into(),
        ));
    }
    database::audit(
        &tx,
        "human",
        "loop.settings",
        &b.project,
        &json!({"scan_day":b.scan_day,"scan_time":b.scan_time,"nix_enabled":b.nix_enabled,"version":b.version+1}),
    )?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(Json(json!({"saved":true})))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    item: String,
}
async fn override_decline(
    State(s): State<AppState>,
    h: HeaderMap,
    Extension(a): Extension<crate::auth::ApprovedAction>,
    Json(b): Json<Override>,
) -> Result<Json<Value>, ApiError> {
    human(&s, &h)?;
    a.recheck(&s)?;
    crate::loop_engine::override_decline(&s.config.home, &b.item)?;
    Ok(Json(json!({"triage":true})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings/loop", get(view).post(save))
        .route("/settings/loop/override", post(override_decline))
}
