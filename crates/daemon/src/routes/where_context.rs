use super::sessions::resolve;
use super::{actor_from, bad_request, ApiError};
use crate::{git, AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use plantool_core::{Actor, ProposalKind, RunStatus};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Clone, Deserialize, Default)]
pub struct Update {
    pub base: Option<String>,
    pub branch: Option<String>,
    pub workspace: Option<PathBuf>,
    pub difftool: Option<String>,
    pub pause_rule: Option<plantool_core::PauseRule>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub reason: String,
}

async fn describe(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = s.session();
    Ok(Json(
        json!({"base":session.base,"branch":s.view().workspace_branch,"workspace":session.cwd(),"difftool":session.difftool,"proposals":s.state().proposals}),
    ))
}

async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<Update>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    if let Some(tool) = &body.difftool {
        if !["auto", "built-in", "difftool", "git-difftool"].contains(&tool.as_str()) {
            return Err(bad_request(
                "diff tool must be auto, built-in, difftool or git-difftool",
            ));
        }
    }
    if actor_from(&headers, &state) != Actor::Human && !body.confirm {
        s.propose_context(&body)?;
        return Ok(Json(
            json!({"proposed":true,"proposals":s.state().proposals}),
        ));
    }
    let old = s.session();
    let moving = body.base.is_some() || body.branch.is_some() || body.workspace.is_some();
    let mut blockers = if moving {
        git::context_precheck(
            &old,
            body.workspace.as_deref(),
            body.base.as_deref(),
            body.branch.as_deref(),
        )
    } else {
        Vec::new()
    };
    if moving && s.runs().iter().any(|r| r.milestone_pending) {
        blockers.push("A milestone is waiting for approval".into());
    }
    if moving && s.runs().iter().any(|r| r.status == RunStatus::Waiting) {
        blockers.push("An agent is waiting for permission or an answer".into());
    }
    if !blockers.is_empty() {
        return Err(ApiError(StatusCode::CONFLICT, blockers.join("; ")));
    }
    let live: Vec<_> = s
        .runs()
        .into_iter()
        .filter(|r| state.runs.is_live(&r.id))
        .collect();
    if moving {
        for run in &live {
            state
                .runs
                .send(&run.id, crate::providers::RunInput::Stop)
                .await
                .map_err(|e| bad_request(e.to_string()))?;
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while live.iter().any(|r| state.runs.is_live(&r.id)) {
            if tokio::time::Instant::now() >= deadline {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Agent has not stopped yet. Retry the switch after it stops.".into(),
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    if moving {
        let mut blockers = git::context_precheck(
            &old,
            body.workspace.as_deref(),
            body.base.as_deref(),
            body.branch.as_deref(),
        );
        if s.runs().iter().any(|r| r.milestone_pending) {
            blockers.push("A milestone became ready for approval while the agent stopped".into());
        }
        if !blockers.is_empty() {
            return Err(ApiError(StatusCode::CONFLICT, blockers.join("; ")));
        }
        let latest = s
            .runs()
            .into_iter()
            .filter(|run| {
                run.task.as_deref() == Some("implement")
                    || run.stage == plantool_core::Stage::Implementing
            })
            .max_by(|a, b| a.started_at.cmp(&b.started_at));
        if let Some(mut run) = latest {
            run.implementation_base = Some(body.base.clone().unwrap_or_else(|| old.base.clone()));
            run.milestone_base = None;
            run.milestone_review = None;
            s.upsert_run(run, |run| crate::events::LiveEvent::RunUpdated { run })?;
        }
    }
    if let Some(branch) = &body.branch {
        git::git(
            body.workspace.as_deref().unwrap_or(old.cwd()),
            &["switch", "--no-guess", branch],
        )
        .map_err(|e| bad_request(e.to_string()))?;
    }
    let session = s.apply_context(&body)?;
    if moving {
        for run in live {
            if let Some(resume) = run
                .provider_session_id
                .clone()
                .or_else(|| s.run(&run.id).and_then(|r| r.provider_session_id))
            {
                let prompt = format!("The owner changed the session context. Workspace: {}. Base: {}. Re-read `plantool session get --session {} --json` and the plan before continuing.", session.cwd().display(), session.base, s.key);
                let mut previous = run.clone();
                previous.implementation_base = Some(session.base.clone());
                previous.milestone_base = Some(session.base.clone());
                state.runs.start(
                    s.clone(),
                    run.provider,
                    run.stage,
                    run.task.as_deref().unwrap_or("resume"),
                    prompt,
                    run.model.clone(),
                    None,
                    Some(resume),
                    run.permission_mode,
                    run.implementation_mode,
                    Some(&previous),
                )?;
            }
        }
    }
    Ok(Json(
        json!({"session":session,"view":s.view(),"diff_range":format!("{}…working tree", session.base)}),
    ))
}

#[derive(Deserialize)]
struct ProposalAction {
    id: String,
    #[serde(default)]
    dismiss: bool,
}
async fn proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<ProposalAction>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "only the owner can apply or dismiss proposals".into(),
        ));
    }
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let p = s
        .state()
        .proposals
        .into_iter()
        .find(|p| p.id == body.id)
        .ok_or_else(|| bad_request("unknown proposal"))?;
    if !body.dismiss {
        let mut change = Update::default();
        match p.kind {
            ProposalKind::Base => change.base = Some(p.new),
            ProposalKind::Branch => change.branch = Some(p.new),
            ProposalKind::Workspace => change.workspace = Some(p.new.into()),
            ProposalKind::Difftool => change.difftool = Some(p.new),
            ProposalKind::PlanRevision => {
                return Err(bad_request("review and accept the plan revision from Plan"))
            }
        }
        let _ = update(
            State(state.clone()),
            headers,
            Path((repo, slug)),
            Json(change),
        )
        .await?;
    }
    s.dismiss_proposal(&body.id)?;
    Ok(Json(json!({"view":s.view()})))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/where", get(describe).post(update))
        .route("/sessions/{repo}/{slug}/proposals", post(proposal))
}
