use super::sessions::resolve;
use super::ApiError;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use plantool_core::{DocKind, ImplementationMode, Stage};
use serde_json::json;

async fn changes(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.session();
    let review = if review_base(&s) == "HEAD" { latest_step_run(&s).and_then(|r| r.milestone_review) } else { s.state().review };
    let base = review_base(&s);
    let c = tokio::task::spawn_blocking(move || crate::changes::stat(&session, &base, review)).await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(Json(serde_json::to_value(c).map_err(|e| anyhow::anyhow!(e))?))
}

async fn open(State(state): State<AppState>, Path((repo, slug)): Path<(String, String)>) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.session();
    let plan = s.doc_path(DocKind::Plan);
    let base = review_base(&s);
    let previous = if base == "HEAD" { latest_step_run(&s).and_then(|r| r.milestone_review) } else { s.state().review };
    let open_base = base.clone();
    let (review, message) = tokio::task::spawn_blocking(move || crate::changes::open_review(&session, &open_base, Some(plan), previous.as_ref())).await.map_err(|e| anyhow::anyhow!(e))??;
    s.set_review(review.clone())?;
    if base == "HEAD" {
        if let Some(mut run) = latest_step_run(&s).filter(|r| r.milestone_pending) {
            let needs_watch = run.milestone_review.is_none();
            run.milestone_review = Some(review.clone());
            let run_id = run.id.clone();
            s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
            if needs_watch {
                if let Some(reference) = crate::runs::review_ref(&review) {
                    tokio::spawn(crate::runs::watch_review(state.runs.clone(), s.clone(), run_id, reference.to_string()));
                }
            }
        }
    }
    let session = s.session();
    let c = tokio::task::spawn_blocking(move || crate::changes::stat(&session, &base, Some(review))).await.map_err(|e| anyhow::anyhow!(e))?;
    let mut v = serde_json::to_value(c).map_err(|e| anyhow::anyhow!(e))?;
    if let Some(m) = message {
        v["message"] = json!(m);
    }
    Ok(Json(v))
}

fn review_base(s: &crate::registry::LiveSession) -> String {
    let stage = s.stage();
    let step_mode = matches!(stage, Stage::Implementing | Stage::ImplementationReview)
        && latest_step_run(s).is_some_and(|r| r.implementation_mode == ImplementationMode::StepByStep);
    if step_mode { "HEAD".into() } else { s.session().base }
}

fn latest_step_run(s: &crate::registry::LiveSession) -> Option<plantool_core::Run> {
    s.runs().into_iter().filter(|r| r.task.as_deref() == Some("implement"))
        .max_by(|a, b| a.started_at.cmp(&b.started_at))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/changes", get(changes))
        .route("/sessions/{repo}/{slug}/changes/open", post(open))
}
