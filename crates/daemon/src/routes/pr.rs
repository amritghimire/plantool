use super::sessions::resolve;
use super::{actor_from, bad_request, ApiError};
use crate::{git, AppState};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use plantool_core::{Actor, PullRequest, RunStatus, Session, Stage};
use serde::{Deserialize, Serialize};
use std::path::Path as FsPath;
use std::time::Duration;

fn human(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    if actor_from(headers, state) == Actor::Human {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            "only a human can edit or create a PR".into(),
        ))
    }
}

async fn gh(cwd: &FsPath, args: &[&str]) -> Result<String, ApiError> {
    let binary = std::env::var_os("PLANTOOL_GH_BIN").unwrap_or_else(|| "gh".into());
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(binary)
            .args(args)
            .current_dir(cwd)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| ApiError(StatusCode::GATEWAY_TIMEOUT, "GitHub CLI timed out".into()))?
    .map_err(|e| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("GitHub CLI is unavailable: {e}"),
        )
    })?;
    if !output.status.success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Serialize)]
struct Preflight {
    head: String,
    base: String,
    repository: String,
    push_remote: String,
    dirty: bool,
    changed_files: Vec<String>,
    live_run: bool,
    commits_ahead: u64,
    existing: Option<PullRequest>,
}

fn checked_workspace(session: &Session) -> Result<(String, bool, u64), ApiError> {
    let cwd = session.cwd();
    if let Some(path) = &session.worktree {
        git::verify_worktree(&session.repo, path, None).map_err(|e| bad_request(e.to_string()))?;
    } else {
        let checkout = git::detect_checkout(cwd).map_err(|e| bad_request(e.to_string()))?;
        if checkout.common_dir != session.repo.common_dir || checkout.root != session.repo.root {
            return Err(bad_request(
                "session workspace no longer matches its repository",
            ));
        }
    }
    let head = git::git(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|_| bad_request("workspace is detached from a branch"))?;
    if head == session.base {
        return Err(bad_request("the workspace is on the base branch"));
    }
    let dirty = git::is_dirty(cwd).map_err(|e| bad_request(e.to_string()))?;
    let commits = git::git(
        cwd,
        &["rev-list", "--count", &format!("{}..HEAD", session.base)],
    )
    .map_err(|e| bad_request(e.to_string()))?
    .parse::<u64>()
    .map_err(|_| bad_request("cannot count commits ahead of base"))?;
    Ok((head, dirty, commits))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPr {
    number: u64,
    url: String,
    state: String,
    #[serde(rename = "isDraft")]
    is_draft: bool,
}

fn record(pr: GhPr) -> PullRequest {
    PullRequest {
        number: pr.number,
        url: pr.url,
        state: pr.state,
        draft: pr.is_draft,
        updated_at: plantool_core::now(),
    }
}

async fn lookup(
    cwd: &FsPath,
    repo: &str,
    head: &str,
    base: &str,
) -> Result<Option<PullRequest>, ApiError> {
    let output = gh(
        cwd,
        &[
            "pr",
            "list",
            "--repo",
            repo,
            "--head",
            head,
            "--base",
            base,
            "--state",
            "all",
            "--json",
            "number,url,state,isDraft,headRefName,baseRefName",
            "--limit",
            "100",
        ],
    )
    .await?;
    let items: Vec<serde_json::Value> = serde_json::from_str(&output).map_err(|e| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("invalid GitHub response: {e}"),
        )
    })?;
    for item in items {
        if item["headRefName"] == head && item["baseRefName"] == base {
            let pr: GhPr = serde_json::from_value(item)
                .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?;
            return Ok(Some(record(pr)));
        }
    }
    Ok(None)
}

async fn preflight_session(session: &Session) -> Result<Preflight, ApiError> {
    let (head, dirty, commits_ahead) = checked_workspace(session)?;
    let cwd = session.cwd();
    let push_remote = "origin".to_string();
    git::git(cwd, &["remote", "get-url", &push_remote])
        .map_err(|_| bad_request("origin remote is missing"))?;
    let repository = gh(
        cwd,
        &[
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ],
    )
    .await?;
    let existing = lookup(cwd, &repository, &head, &session.base).await?;
    let changed_files = if dirty {
        git::changed_paths(cwd).map_err(|e| bad_request(e.to_string()))?
    } else {
        Vec::new()
    };
    Ok(Preflight {
        head,
        base: session.base.clone(),
        repository,
        push_remote,
        dirty,
        changed_files,
        live_run: false,
        commits_ahead,
        existing,
    })
}

fn require_pr_stage(s: &crate::registry::LiveSession) -> Result<(), ApiError> {
    if matches!(s.stage(), Stage::ImplementationReview | Stage::Done) {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::FORBIDDEN,
            "PRs are available at implementation review or done".into(),
        ))
    }
}

async fn preview(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    require_pr_stage(&s)?;
    let mut info = preflight_session(&s.session()).await?;
    info.live_run = s.runs().iter().any(|run| {
        matches!(
            run.status,
            RunStatus::Starting | RunStatus::Running | RunStatus::Waiting
        ) && state.runs.is_live(&run.id)
    });
    Ok(Json(serde_json::to_value(info).map_err(|e| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

fn draft_path(s: &crate::registry::LiveSession) -> std::path::PathBuf {
    s.store.dir.join("pr-draft.md")
}

fn parse_draft(content: &str) -> (String, String) {
    let mut lines = content.lines();
    let title = lines
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('#')
        .trim()
        .to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (title, body)
}

async fn get_draft(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let content = tokio::fs::read_to_string(draft_path(&s))
        .await
        .unwrap_or_default();
    let (title, body) = parse_draft(&content);
    Ok(Json(
        serde_json::json!({ "title": title, "body": body, "exists": !content.is_empty() }),
    ))
}

#[derive(Deserialize)]
struct DraftBody {
    title: String,
    body: String,
}

async fn save_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<DraftBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    human(&headers, &state)?;
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    if body.title.trim().is_empty() || body.body.trim().is_empty() {
        return Err(bad_request("PR title and body are required"));
    }
    let content = format!("# {}\n\n{}\n", body.title.trim(), body.body.trim());
    crate::store::write_atomic(&draft_path(&s), content.as_bytes())
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "saved": true })))
}

fn commit_draft_path(s: &crate::registry::LiveSession) -> std::path::PathBuf {
    s.store.dir.join("commit-draft.md")
}

async fn get_commit_draft(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let content = tokio::fs::read_to_string(commit_draft_path(&s))
        .await
        .unwrap_or_default();
    let draft = content.trim();
    let message = if draft.is_empty() {
        let session = s.session();
        format!("{}\n\nSession: {}", session.title, session.slug)
    } else {
        draft.to_string()
    };
    Ok(Json(
        serde_json::json!({ "message": message, "exists": !draft.is_empty() }),
    ))
}

#[derive(Deserialize)]
struct CommitBody {
    message: String,
}

async fn commit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<CommitBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    human(&headers, &state)?;
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    require_pr_stage(&s)?;
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Err(bad_request("a commit message is required"));
    }
    let session = s.session();
    let (_, dirty, _) = checked_workspace(&session)?;
    if !dirty {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "nothing to commit; refresh the checks".into(),
        ));
    }
    let cwd = session.cwd().clone();
    let outcome = tokio::task::spawn_blocking(move || git::commit_milestone(&cwd, &message, false))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| ApiError(StatusCode::CONFLICT, format!("could not commit: {e}")))?;
    let (sha, rewritten) = match outcome {
        git::CommitOutcome::Committed(sha) => (sha, Vec::new()),
        git::CommitOutcome::HookRewrote {
            sha: Some(sha),
            files,
        } => (sha, files),
        git::CommitOutcome::HookRewrote { sha: None, files } => {
            let shown: Vec<&str> = files.iter().take(5).map(String::as_str).collect();
            let more = if files.len() > 5 {
                format!(" and {} more", files.len() - 5)
            } else {
                String::new()
            };
            return Err(ApiError(
                StatusCode::CONFLICT,
                format!(
                    "git hooks rewrote {} file(s): {}{more}. They are staged; press Commit again to include them.",
                    files.len(),
                    shown.join(", ")
                ),
            ));
        }
    };
    let _ = tokio::fs::remove_file(commit_draft_path(&s)).await;
    Ok(Json(
        serde_json::json!({ "sha": sha, "rewritten": rewritten }),
    ))
}

#[derive(Deserialize)]
struct CreateBody {
    head: String,
    repository: String,
    title: String,
    body: String,
    draft: bool,
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<CreateBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    human(&headers, &state)?;
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    require_pr_stage(&s)?;
    let session = s.session();
    let info = preflight_session(&session).await?;
    if info.head != body.head || info.repository != body.repository {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "workspace branch or target repository changed; refresh the preview".into(),
        ));
    }
    if info.existing.is_none() && (body.title.trim().is_empty() || body.body.trim().is_empty()) {
        return Err(bad_request("PR title and body are required"));
    }
    if info.commits_ahead == 0 && info.existing.is_none() {
        return Err(bad_request(
            "there are no commits ahead of base; uncommitted changes cannot appear in a PR",
        ));
    }
    let pr = if let Some(existing) = info.existing {
        existing
    } else {
        let cwd = session.cwd();
        let refspec = format!("HEAD:refs/heads/{}", info.head);
        git::git(cwd, &["push", "-u", &info.push_remote, &refspec])
            .map_err(|e| bad_request(format!("push failed: {e}")))?;
        let body_path = s.store.dir.join("pr-body.md");
        crate::store::write_atomic(&body_path, body.body.trim().as_bytes())
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let body_file = body_path.to_string_lossy().to_string();
        let mut args = vec![
            "pr",
            "create",
            "--repo",
            &info.repository,
            "--head",
            &info.head,
            "--base",
            &info.base,
            "--title",
            body.title.trim(),
            "--body-file",
            &body_file,
        ];
        if body.draft {
            args.push("--draft");
        }
        if let Err(error) = gh(cwd, &args).await {
            if let Some(found) = lookup(cwd, &info.repository, &info.head, &info.base).await? {
                found
            } else {
                return Err(error);
            }
        } else {
            lookup(cwd, &info.repository, &info.head, &info.base)
                .await?
                .ok_or_else(|| {
                    ApiError(
                        StatusCode::BAD_GATEWAY,
                        "PR was created but could not be read back; refresh before retrying".into(),
                    )
                })?
        }
    };
    let session = s.set_pull_request(pr.clone())?;
    Ok(Json(
        serde_json::json!({ "pull_request": pr, "session": session }),
    ))
}

async fn refresh(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let session = refresh_saved(&s).await?;
    let pr = session.pull_request.clone();
    Ok(Json(
        serde_json::json!({ "pull_request": pr, "session": session }),
    ))
}

pub async fn refresh_saved(s: &crate::registry::LiveSession) -> Result<Session, ApiError> {
    let old = s.session();
    let current = old
        .pull_request
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "this session has no PR".into()))?;
    let output = gh(
        &old.repo.root,
        &[
            "pr",
            "view",
            &current.url,
            "--json",
            "number,url,state,isDraft",
        ],
    )
    .await?;
    let pr = record(
        serde_json::from_str(&output)
            .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e.to_string()))?,
    );
    Ok(s.set_pull_request(pr)?)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/pr/preview", get(preview))
        .route(
            "/sessions/{repo}/{slug}/pr/draft",
            get(get_draft).post(save_draft),
        )
        .route(
            "/sessions/{repo}/{slug}/pr/commit",
            get(get_commit_draft).post(commit),
        )
        .route("/sessions/{repo}/{slug}/pr", post(create))
        .route("/sessions/{repo}/{slug}/pr/refresh", post(refresh))
}
