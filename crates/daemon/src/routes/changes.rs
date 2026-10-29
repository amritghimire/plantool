use super::sessions::resolve;
use super::{bad_request, ApiError};
use crate::changes::{ChangeScope, Window};
use crate::registry::LiveSession;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use plantool_core::{DocKind, ImplementationMode, Stage};
use serde::Deserialize;
use serde_json::json;

const MAX_FILE_DIFF: usize = 400_000;

#[derive(Deserialize, Default)]
pub struct ScopeQuery {
    #[serde(default)]
    pub scope: Option<ChangeScope>,
}

#[derive(Deserialize)]
pub struct FileQuery {
    pub path: String,
    #[serde(default)]
    pub scope: Option<ChangeScope>,
}

async fn changes(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Query(q): Query<ScopeQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    s.refresh_code_anchors()?;
    let session = s.session();
    let window = resolve_window(&s, q.scope);
    let review = window.review.clone();
    let c = tokio::task::spawn_blocking(move || crate::changes::stat(&session, &window))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut response = serde_json::to_value(&c).map_err(|e| anyhow::anyhow!(e))?;
    let plan = s.doc(DocKind::Plan);
    let planned = plan
        .as_ref()
        .map(|p| plantool_core::markdown::affected_files(&p.content))
        .unwrap_or_default();
    let outside: Vec<_> = c
        .stat
        .iter()
        .filter(|f| {
            !planned.iter().any(|p| {
                p == &f.path
                    || p.strip_suffix('*')
                        .is_some_and(|prefix| f.path.starts_with(prefix))
            })
        })
        .map(|f| f.path.clone())
        .collect();
    let checked = plan.as_ref().is_some_and(|p| {
        plantool_core::markdown::checkboxes(&p.content)
            .iter()
            .any(|t| t.checked)
    });
    let state = s.state();
    response["drift"] = json!({"checked":!planned.is_empty(),"planned_files":planned,"outside_files":outside,"ticks_without_diff":checked && c.stat.is_empty(),"plan_changed":state.plan_change.is_some_and(|c| c != plantool_core::markdown::PlanChange::Unchanged)});
    response["milestones"] = json!(state.milestones);
    response["difftool_open_comments"] = match &review {
        Some(review) => crate::changes::unresolved_human_comments(review)
            .ok()
            .map_or(serde_json::Value::Null, |count| json!(count)),
        None => serde_json::Value::Null,
    };
    Ok(Json(response))
}

async fn open(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    body: Option<Json<ScopeQuery>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.session();
    let plan = s.doc_path(DocKind::Plan);
    let mut window = resolve_window(&s, body.and_then(|b| b.scope));
    let previous = window.review.clone();
    let open_base = window.base.clone();
    let opened = tokio::task::spawn_blocking(move || {
        crate::changes::open_review(&session, &open_base, Some(plan), previous.as_ref())
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    let (review, message) = match opened {
        Ok(result) => result,
        Err(error) => {
            let mut changes = crate::changes::stat(&s.session(), &window);
            changes.tool = "none";
            changes.message = Some(format!("{error}. Use the built-in diff below."));
            return Ok(Json(
                serde_json::to_value(changes).map_err(|e| anyhow::anyhow!(e))?,
            ));
        }
    };
    match window.scope {
        ChangeScope::All => s.set_review(review.clone())?,
        ChangeScope::Step => {
            s.announce_review(review.clone());
            if let Some(mut run) = latest_step_run(&s).filter(|r| r.milestone_pending) {
                let needs_watch = run.milestone_review.is_none();
                run.milestone_review = Some(review.clone());
                let run_id = run.id.clone();
                s.upsert_run(run, |r| crate::events::LiveEvent::RunUpdated { run: r })?;
                if needs_watch {
                    if let Some(reference) = crate::runs::review_ref(&review) {
                        tokio::spawn(crate::runs::watch_review(
                            state.runs.clone(),
                            s.clone(),
                            run_id,
                            reference.to_string(),
                        ));
                    }
                }
            }
        }
    }
    window.review = Some(review);
    let session = s.session();
    let c = tokio::task::spawn_blocking(move || crate::changes::stat(&session, &window))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut v = serde_json::to_value(c).map_err(|e| anyhow::anyhow!(e))?;
    if let Some(m) = message {
        v["message"] = json!(m);
    }
    Ok(Json(v))
}

async fn file(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Query(q): Query<FileQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let path = q.path.trim().to_string();
    if path.is_empty() || path.starts_with('/') || path.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(bad_request("path must be relative to the checkout"));
    }
    let window = resolve_window(&s, q.scope);
    let cwd = s.session().cwd().clone();
    let diff_path = path.clone();
    let diff = tokio::task::spawn_blocking(move || {
        crate::git::diff_file(&cwd, &window.base, &diff_path).map_err(anyhow::Error::from)
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))??;
    let truncated = diff.len() > MAX_FILE_DIFF;
    let diff = if truncated {
        let mut end = MAX_FILE_DIFF;
        while !diff.is_char_boundary(end) {
            end -= 1;
        }
        diff[..end].to_string()
    } else {
        diff
    };
    Ok(Json(
        json!({ "path": path, "diff": diff, "truncated": truncated }),
    ))
}

/// Pick the diff window for a scope. Step scope is only available during a step-by-step
/// implementation; asking for it elsewhere falls back to the whole implementation.
pub fn resolve_window(s: &LiveSession, requested: Option<ChangeScope>) -> Window {
    let step_run = latest_step_run(s).filter(|r| {
        (r.implementation_mode == ImplementationMode::StepByStep || r.milestone_key.is_some())
            && matches!(s.stage(), Stage::Implementing | Stage::ImplementationReview)
    });
    let step_available = step_run.is_some();
    let scope = match requested {
        Some(ChangeScope::All) => ChangeScope::All,
        Some(ChangeScope::Step) | None if step_available => ChangeScope::Step,
        _ => ChangeScope::All,
    };
    match (scope, step_run) {
        (ChangeScope::Step, Some(run)) => {
            let base = run.milestone_base.clone().unwrap_or_else(|| "HEAD".into());
            let label = if run.milestones_approved > 0 {
                format!("since milestone {} was approved", run.milestones_approved)
            } else {
                "since this implementation started".to_string()
            };
            Window {
                scope,
                base,
                label,
                step_available,
                review: run.milestone_review,
            }
        }
        (_, Some(run)) if run.implementation_base.is_some() => {
            let base = run.implementation_base.unwrap_or_default();
            Window {
                scope: ChangeScope::All,
                base,
                label: "in the whole implementation".to_string(),
                step_available,
                review: s.state().review,
            }
        }
        _ => {
            let base = s.session().base;
            Window {
                scope: ChangeScope::All,
                label: format!("against {base}"),
                base,
                step_available,
                review: s.state().review,
            }
        }
    }
}

fn latest_step_run(s: &LiveSession) -> Option<plantool_core::Run> {
    s.runs()
        .into_iter()
        .filter(|r| r.task.as_deref() == Some("implement"))
        .max_by(|a, b| a.started_at.cmp(&b.started_at))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/changes", get(changes))
        .route("/sessions/{repo}/{slug}/changes/open", post(open))
        .route("/sessions/{repo}/{slug}/changes/file", get(file))
}
