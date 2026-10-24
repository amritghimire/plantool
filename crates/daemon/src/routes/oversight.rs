use super::{actor_from, ApiError};
use crate::{database, oversight, workflow, AppState};
use anyhow::Context;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use rusqlite::params;
use serde_json::{json, Value};
macro_rules! ensure {($condition:expr,$($message:tt)*)=>{if !$condition{return Err(anyhow::anyhow!($($message)*).into())}}}
fn human(s: &AppState, h: &HeaderMap) -> Result<(), ApiError> {
    if actor_from(h, s) != plantool_core::Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Browser session required".into(),
        ));
    }
    Ok(())
}
async fn view(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    let db = database::open(&state.config.home)?;
    let mut q=db.prepare("SELECT p.id,p.repo_full_name,s.version,s.oversight,coalesce(s.sprint_day,0),s.sprint_time,coalesce(s.timezone,'UTC'),s.review_globs_json FROM projects p JOIN project_settings s ON s.project_id=p.id AND s.version=(SELECT max(version) FROM project_settings WHERE project_id=p.id) WHERE p.mode='eos'").map_err(anyhow::Error::from)?;
    let projects=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,Option<String>>(1)?,"version":r.get::<_,u64>(2)?,"preset":r.get::<_,String>(3)?,"day":r.get::<_,u32>(4)?,"time":r.get::<_,String>(5)?,"timezone":r.get::<_,String>(6)?,"review_globs":serde_json::from_str::<Value>(&r.get::<_,String>(7)?).unwrap_or(json!([]))}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
    let mut q=db.prepare("SELECT w.item_id,i.title,w.state_json FROM workflow_items w JOIN work_items i ON i.id=w.item_id WHERE json_extract(w.state_json,'$.step')='plan_gate'").map_err(anyhow::Error::from)?;
    let plans=q.query_map([],|r|Ok(json!({"item":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"state":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(json!({}))}))).map_err(anyhow::Error::from)?.collect::<Result<Vec<_>,_>>().map_err(anyhow::Error::from)?;
    Ok(Json(json!({"projects":projects,"plans":plans})))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    project: String,
    version: u64,
    preset: plantool_core::presets::Preset,
    day: u32,
    time: String,
    timezone: String,
    review_globs: Vec<String>,
}
async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Settings>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    ensure!(
        !body.review_globs.is_empty() && body.review_globs.len() <= 64,
        "Invalid review rules"
    );
    crate::scheduler::due(chrono::Utc::now(), body.day, &body.time, &body.timezone)?;
    oversight::suggested(&[], &body.review_globs)?;
    ensure!(
        state.runs.execution.status().stopped,
        "Use Stop All before changing project policy"
    );
    let mut db = database::open(&state.config.home)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    let current: u64 = tx
        .query_row(
            "SELECT max(version) FROM project_settings WHERE project_id=?1",
            [&body.project],
            |r| r.get(0),
        )
        .map_err(anyhow::Error::from)?;
    ensure!(current == body.version, "Project settings changed; reload");
    let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM work_items WHERE project_id=?1 AND kind='eos' AND github_issue_number IS NOT NULL AND closed_at IS NULL AND state NOT IN('merged','done','backlog'))",[&body.project],|r|r.get(0)).map_err(anyhow::Error::from)?;
    ensure!(
        !active,
        "Finish or replan active items before changing oversight policy"
    );
    tx.execute("INSERT INTO project_settings(project_id,version,oversight,allowed_providers_json,never_sent_globs_json,sprint_day,scan_time,timezone,retention_json,effective_at,set_by,sprint_time,review_globs_json) SELECT project_id,version+1,?2,allowed_providers_json,never_sent_globs_json,?3,scan_time,?4,retention_json,?5,'browser',?6,?7 FROM project_settings WHERE project_id=?1 AND version=?8",params![body.project,serde_json::to_value(body.preset).map_err(anyhow::Error::from)?.as_str(),body.day,body.timezone,plantool_core::now(),body.time,json!(body.review_globs).to_string(),current]).map_err(anyhow::Error::from)?;
    database::audit(
        &tx,
        "human",
        "project.policy",
        &body.project,
        &json!({"preset":body.preset,"day":body.day,"time":body.time,"timezone":body.timezone,"review_globs":body.review_globs}),
    )?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(Json(json!({"saved":true})))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Tags {
    item: String,
    hash: String,
    tags: Vec<bool>,
}
pub fn edit_tags(
    home: &std::path::Path,
    item: &str,
    hash: &str,
    tags: &[bool],
) -> anyhow::Result<String> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut s = workflow::state(&tx, item)?.context("Item missing")?;
    ensure!(
        s["step"] == "plan_gate" && s["frozen"] != true && s["preset"] == "standard",
        "Only a pending Standard plan has editable review tags"
    );
    ensure!(s["plan_sha"] == hash, "Plan revision changed");
    ensure!(!tx.query_row("SELECT EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND kind='plan' AND target_digest=?2 AND state='approved')",params![item,hash],|r|r.get::<_,bool>(0))?,"Approved tags require a replan");
    let raw = String::from_utf8(crate::artifacts::read(home, hash)?)?;
    let phases = oversight::phases(&raw)?;
    let phases = phases.as_array().unwrap();
    ensure!(phases.len() == tags.len(), "Phase list changed");
    let tasks = plantool_core::tasks::parse(&raw).map_err(anyhow::Error::msg)?;
    let mut content = String::new();
    for (index, line) in raw.lines().enumerate() {
        let heading = line
            .strip_prefix("### ")
            .or_else(|| line.strip_prefix("## "));
        let phase = heading.and_then(|h| phases.iter().position(|p| p["name"] == h));
        let task = tasks.iter().find(|t| t.line as usize == index + 1);
        if let Some(phase) = phase {
            content.push_str(line.replace("[review]", "").trim_end());
            if tags[phase] {
                content.push_str(" [review]")
            }
        } else if let Some(task) = task {
            content.push_str(line.replace("[review]", "").trim_end());
            if task.phase.is_none()
                && tags[phases
                    .iter()
                    .position(|p| p["keys"].as_array().unwrap().contains(&json!(task.key)))
                    .unwrap()]
            {
                content.push_str(" [review]")
            }
        } else {
            content.push_str(line)
        }
        content.push('\n');
    }
    let sha = crate::artifacts::put(home, content.as_bytes())?;
    if sha == hash {
        return Ok(sha);
    }
    crate::artifacts::register(&tx, &sha, content.len())?;
    let revision = format!("{item}:plan:{sha}");
    tx.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) SELECT ?1,?2,'plan',coalesce(max(number),0)+1,?3,'human',?4 FROM revisions WHERE item_id=?2 AND doc_kind='plan'",params![revision,item,sha,plantool_core::now()])?;
    for t in plantool_core::tasks::parse(&content).map_err(anyhow::Error::msg)? {
        tx.execute(
            "INSERT INTO plan_tasks VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                format!("{revision}:{}", t.key),
                revision,
                t.key,
                t.area,
                t.title,
                t.phase,
                t.review_tag
            ],
        )?;
    }
    let (thread,interrupt):(String,String)=tx.query_row("SELECT c.thread_id,c.interrupt_id FROM github_gate_context c JOIN gates g ON g.id=c.gate_id WHERE g.item_id=?1 AND g.kind='plan' AND g.state IN('open','pending') ORDER BY g.created_at DESC LIMIT 1",[item],|r|Ok((r.get(0)?,r.get(1)?)))?;
    s["plan_sha"] = json!(sha);
    s["phases"] = oversight::phases(&content)?;
    s["approved"] = json!(false);
    workflow::save(&tx, item, &s)?;
    crate::github::gates::request(&tx, item, &sha, &thread, &interrupt)?;
    database::audit(
        &tx,
        "human",
        "plan.review_tags",
        item,
        &json!({"previous":hash,"sha":sha,"tags":tags}),
    )?;
    tx.commit()?;
    Ok(sha)
}
async fn tags(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(approval): Extension<crate::auth::ApprovedAction>,
    Json(body): Json<Tags>,
) -> Result<Json<Value>, ApiError> {
    human(&state, &headers)?;
    approval.recheck(&state)?;
    Ok(Json(
        json!({"hash":edit_tags(&state.config.home,&body.item,&body.hash,&body.tags)?}),
    ))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/settings/oversight", get(view).post(save))
        .route("/settings/oversight/tags", post(tags))
}
