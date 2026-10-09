use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use plantool_daemon::config::Config;
use plantool_daemon::{routes, AppState, Registry};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

struct Harness {
    app: axum::Router,
    state: AppState,
    _tmp: tempfile::TempDir,
    repo: std::path::PathBuf,
}

fn git(cwd: &Path, args: &[&str]) {
    let st = std::process::Command::new("git").args(args).current_dir(cwd).status().unwrap();
    assert!(st.success(), "git {args:?}");
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("a.txt"), "hi\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let home = tmp.path().join("home");
    let config = Arc::new(Config { home: home.clone(), port: 0, token: "tok".into(), version: "test".into() });
    let registry = Arc::new(Registry::load(home).unwrap());
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let state = AppState { registry, config, runs: Arc::new(plantool_daemon::runs::RunManager::default()), shutdown: tx };
    let app = routes::router(state.clone());
    Harness { app, state, _tmp: tmp, repo }
}

async fn call(app: &axum::Router, method: &str, path: &str, body: Option<Value>, human: bool, host: &str) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(path).header(header::HOST, host);
    if human {
        req = req.header("x-plantool-actor", "tok");
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()));
    (status, v)
}

#[tokio::test]
async fn uploads_a_file_into_the_session_and_rejects_empty_files() {
    let h = harness();
    let (status, created) = call(&h.app, "POST", "/api/sessions", Some(json!({ "slug": "upload", "cwd": h.repo })), false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CREATED);
    let key = created["session"]["key"].as_str().unwrap();
    let path = format!("/api/sessions/{key}/attachments");
    let upload = |bytes: &'static [u8]| Request::builder().method("POST").uri(&path).header(header::HOST, "127.0.0.1")
        .header("x-plantool-actor", "tok").header("x-plantool-filename", "notes / plan.txt")
        .body(Body::from(bytes)).unwrap();
    let response = h.app.clone().oneshot(upload(b"review context")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let saved = std::path::PathBuf::from(body["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&saved).unwrap(), b"review context");
    assert!(saved.file_name().unwrap().to_string_lossy().ends_with("notes___plan.txt"));
    assert_eq!(h.app.clone().oneshot(upload(b"")).await.unwrap().status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn full_session_flow() {
    let h = harness();
    let app = &h.app;
    let (st, v) = call(app, "POST", "/api/sessions", Some(json!({ "slug": "fix-it", "cwd": h.repo })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let key = v["session"]["key"].as_str().unwrap().to_string();
    assert_eq!(key, "repo/fix-it");

    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/docs/plan/path"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK);
    let plan_path = std::path::PathBuf::from(v["path"].as_str().unwrap());
    assert!(!v["exists"].as_bool().unwrap());

    let (st, _) = call(app, "GET", &format!("/api/sessions/{key}/docs/plan"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    std::fs::write(&plan_path, "# Plan\n\nalpha\nbeta\n\n### Phase 1: X\n- [ ] one\n- [x] two\n").unwrap();
    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/docs/plan/touch"), Some(json!({})), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["changed"].as_bool().unwrap());

    let (_, v) = call(app, "GET", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert_eq!(v["state"]["stage"], "plan-review");
    assert_eq!(v["docs"][1]["progress"][0]["done"], 1);

    let live = h.state.registry.get_key(&key).unwrap();
    let mut rx = live.tx.subscribe();
    let batch = json!({ "comments": [
        { "doc": "plan", "match": "alpha", "body": "one" },
        { "doc": "plan", "line": 4, "body": "two" }
    ]});
    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/comments/batch"), Some(batch), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["comments"].as_array().unwrap().len(), 2);
    assert_eq!(v["comments"][0]["kind"], "agent");
    let ev = rx.try_recv().unwrap();
    assert!(matches!(ev.event, plantool_daemon::events::LiveEvent::CommentsAdded { .. }));
    assert!(rx.try_recv().is_err(), "one broadcast for one batch");

    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/comments"), Some(json!({ "doc": "plan", "match": "nope", "body": "x" })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    std::fs::write(&plan_path, "# Plan\n\nintro\nalpha\nBETA\n").unwrap();
    let (_, _) = call(app, "POST", &format!("/api/sessions/{key}/docs/plan/touch"), Some(json!({})), false, "127.0.0.1").await;
    let (_, v) = call(app, "GET", &format!("/api/sessions/{key}/comments"), None, false, "127.0.0.1").await;
    let comments = v["comments"].as_array().unwrap();
    assert_eq!(comments[0]["anchor"]["line"], 4);
    assert_eq!(comments[0]["anchor"]["outdated"], false);
    assert_eq!(comments[1]["anchor"]["outdated"], true);

    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/stage"), Some(json!({ "to": "approved" })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/stage"), Some(json!({ "to": "approved" })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["actor"], "human");
    let (st, _) = call(app, "POST", &format!("/api/sessions/{key}/stage"), Some(json!({ "to": "implementing" })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK);

    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/changes"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["base"], "main");

    let (st, v) = call(app, "POST", "/api/sessions", Some(json!({ "slug": "fix-it", "cwd": h.repo })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["created"], false);
}

#[tokio::test]
async fn brief_and_prompt() {
    let h = harness();
    let app = &h.app;
    let (st, v) = call(app, "POST", "/api/sessions", Some(json!({ "slug": "scopes", "cwd": h.repo, "brief": "  Token scope names are confusing  " })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let key = v["session"]["key"].as_str().unwrap().to_string();
    assert_eq!(v["session"]["session"]["brief"], "Token scope names are confusing");

    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/next"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["stage"], "research");
    let prompt = v["prompt"].as_str().unwrap();
    assert!(prompt.contains("plantool skill research"), "{prompt}");
    assert!(prompt.contains("What the user wants:\nToken scope names are confusing"), "{prompt}");
    assert!(prompt.contains("/research.md"), "{prompt}");

    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/plan?extra=check%20issue%2013162"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["prompt"].as_str().unwrap().contains("Additional instructions from the user:\ncheck issue 13162"));

    let (st, _) = call(app, "GET", &format!("/api/sessions/{key}/prompt/bogus"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let live = h.state.registry.get_key(&key).unwrap();
    let mut rx = live.tx.subscribe();
    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/brief"), Some(json!({ "brief": "Rename the scopes" })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["brief"], "Rename the scopes");
    assert!(matches!(rx.try_recv().unwrap().event, plantool_daemon::events::LiveEvent::SessionUpdated { .. }));
    let (_, v) = call(app, "GET", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert_eq!(v["session"]["brief"], "Rename the scopes");
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(live.store.meta_path()).unwrap()).unwrap();
    assert_eq!(meta["brief"], "Rename the scopes");

    let (_, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/research"), None, false, "127.0.0.1").await;
    assert!(v["prompt"].as_str().unwrap().contains("Rename the scopes"));

    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/brief"), Some(json!({ "brief": "   " })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["brief"].is_null());
    let (_, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/research"), None, false, "127.0.0.1").await;
    assert!(!v["prompt"].as_str().unwrap().contains("What the user wants"));

    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/runs"), Some(json!({ "provider": "claude", "stage": "research", "permission_mode": "sometimes" })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    let (st, _) = call(app, "DELETE", &format!("/api/sessions/{key}/runs/nope"), None, true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let stopped = plantool_core::Run { id: "r1".into(), provider: plantool_core::Provider::Claude, provider_session_id: None, stage: plantool_core::Stage::Researching, implementation_mode: Default::default(), milestone_pending: false, milestone_review: None, milestone_base: None, milestones_approved: 0, milestone_commit: None, implementation_base: None, task: None, cwd: h.repo.clone(), status: plantool_core::RunStatus::Stopped, model: None, permission_mode: Default::default(), started_at: plantool_core::now(), ended_at: None, error: None, seq: 0 };
    live.upsert_run(stopped, |r| plantool_daemon::events::LiveEvent::RunStarted { run: r }).unwrap();
    live.append_run_event("r1", 1, json!({ "type": "status", "label": "x" })).unwrap();
    assert!(live.store.run_log_path("r1").is_file());
    let (st, v) = call(app, "DELETE", &format!("/api/sessions/{key}/runs/r1"), None, true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(!live.store.run_meta_path("r1").exists());
    assert!(!live.store.run_log_path("r1").exists());
    let (_, v) = call(app, "GET", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert_eq!(v["runs"].as_array().unwrap().len(), 0);
    assert_eq!(v["session"]["created_in"], json!(h.repo));

    let (st, v) = call(app, "GET", "/api/repos", None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["repos"][0]["root"], json!(std::fs::canonicalize(&h.repo).unwrap()));
    assert_eq!(v["repos"][0]["sessions"], 1);

    let (st, v) = call(app, "DELETE", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/worktree"), Some(json!({})), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let wt = std::path::PathBuf::from(v["worktree"].as_str().unwrap());
    assert!(wt.join("a.txt").is_file() || wt.is_dir());
    let mut rx = live.tx.subscribe();
    let (st, v) = call(app, "DELETE", &format!("/api/sessions/{key}?worktree=true"), None, true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(matches!(rx.try_recv().unwrap().event, plantool_daemon::events::LiveEvent::SessionRemoved { .. }));
    assert!(!live.store.dir.exists());
    assert!(!wt.exists());
    let (st, _) = call(app, "GET", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert!(h.state.registry.get_key(&key).is_none());
}

#[tokio::test]
async fn step_mode_previews_one_ticket_and_reviews_uncommitted_changes() {
    let h = harness();
    let app = &h.app;
    let (_, v) = call(app, "POST", "/api/sessions", Some(json!({ "slug": "steps", "cwd": h.repo })), false, "127.0.0.1").await;
    let key = v["session"]["key"].as_str().unwrap().to_string();
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/implement?implementation_mode=step-by-step"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["prompt"].as_str().unwrap().contains("exactly one unchecked plan ticket"));

    let live = h.state.registry.get_key(&key).unwrap();
    live.set_stage(plantool_core::Stage::Implementing, plantool_core::Actor::Human).unwrap();
    let first = plantool_daemon::git::head_sha(&h.repo).unwrap();
    // milestone 1 was committed by approval; milestone 2 is in progress
    std::fs::write(h.repo.join("a.txt"), "hi\nmilestone one\n").unwrap();
    let base = plantool_daemon::git::commit_all(&h.repo, "Milestone 1: first").unwrap();
    std::fs::write(h.repo.join("b.txt"), "milestone two\n").unwrap();
    let run = plantool_core::Run {
        id: "step-run".into(), provider: plantool_core::Provider::Claude,
        provider_session_id: None, stage: plantool_core::Stage::Implementing,
        implementation_mode: plantool_core::ImplementationMode::StepByStep,
        milestone_pending: true, milestone_review: None,
        milestone_base: Some(base.clone()), milestones_approved: 1, milestone_commit: None, implementation_base: Some(first.clone()),
        task: Some("implement".into()), cwd: h.repo.clone(), status: plantool_core::RunStatus::Idle,
        model: None, permission_mode: Default::default(), started_at: plantool_core::now(),
        ended_at: None, error: None, seq: 0,
    };
    live.upsert_run(run, |r| plantool_daemon::events::LiveEvent::RunStarted { run: r }).unwrap();
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/changes"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["scope"], "step");
    assert_eq!(v["base"], base);
    assert_eq!(v["label"], "since milestone 1 was approved");
    assert_eq!(v["step_available"], true);
    let paths: Vec<&str> = v["stat"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["b.txt"], "only the current milestone is in the step scope");
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/changes?scope=all"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["scope"], "all");
    assert_eq!(v["base"], first);
    assert_eq!(v["label"], "in the whole implementation");
    let mut paths: Vec<&str> = v["stat"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    paths.sort();
    assert_eq!(paths, vec!["a.txt", "b.txt"], "the whole implementation diffs against the base: {v}");
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/changes/file?path=b.txt"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["diff"].as_str().unwrap().contains("+milestone two"), "{v}");
    assert_eq!(v["truncated"], false);
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/changes/file?path=a.txt&scope=all"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["diff"].as_str().unwrap().contains("+milestone one"), "{v}");
    let (st, _) = call(app, "GET", &format!("/api/sessions/{key}/changes/file?path=../x"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_ne!(first, base);
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/implement?implementation_mode=step-by-step&resume_run=step-run"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["prompt"].as_str().unwrap().contains("Do not start another plan ticket yet"));
    let (st, _) = call(app, "POST", &format!("/api/sessions/{key}/runs/step-run/milestone/approve"), Some(json!({})), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/runs/step-run/milestone"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["subject"], "Milestone 2");
    assert_eq!(v["dirty"], true);
    assert_eq!(v["live"], false);
    let (st, v) = call(app, "POST", &format!("/api/sessions/{key}/runs/step-run/milestone/approve"), Some(json!({ "message": "custom subject" })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap_or_default().contains("not live"), "{v}");
    assert!(plantool_daemon::git::is_dirty(&h.repo).unwrap(), "a refused approval must not commit");
    assert_eq!(live.run("step-run").unwrap().milestones_approved, 1);
}

#[tokio::test]
async fn rejects_foreign_host_and_origin() {
    let h = harness();
    let (st, _) = call(&h.app, "GET", "/health", None, false, "evil.example").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let req = Request::builder().uri("/health").header(header::HOST, "127.0.0.1").header(header::ORIGIN, "http://evil.example").body(Body::empty()).unwrap();
    assert_eq!(h.app.clone().oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
    let (st, _) = call(&h.app, "GET", "/health", None, false, "localhost:41200").await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn bad_slug_and_missing_repo() {
    let h = harness();
    let (st, _) = call(&h.app, "POST", "/api/sessions", Some(json!({ "slug": "../x", "cwd": h.repo })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = call(&h.app, "POST", "/api/sessions", Some(json!({ "slug": "ok", "cwd": h._tmp.path() })), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, _) = call(&h.app, "GET", "/api/sessions/nope", None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn worktree_dir_setting_per_repo() {
    let h = harness();
    let app = &h.app;
    let repo = h.repo.canonicalize().unwrap();
    let q = format!("/api/settings/worktree-dir?repo={}", repo.display());
    let (st, v) = call(app, "GET", &q, None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["repo"]["value"], Value::Null);

    let body = json!({ "scope": "repo", "repo": repo, "value": "../{repo}-worktrees/{slug}" });
    let (st, _) = call(app, "POST", "/api/settings/worktree-dir", Some(body.clone()), false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(app, "POST", "/api/settings/worktree-dir", Some(body), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["repo"]["value"], "../{repo}-worktrees/{slug}");
    assert_eq!(v["effective"], "../{repo}-worktrees/{slug}");
    let example = repo.parent().unwrap().join("repo-worktrees").join("<slug>");
    assert_eq!(v["example"], json!(example));

    let (_, v) = call(app, "GET", &format!("{q}&preview=trees"), None, false, "127.0.0.1").await;
    assert_eq!(v["example"], json!(repo.join("trees").join("<slug>")));

    let (st, v) = call(app, "POST", "/api/settings/worktree-dir", Some(json!({ "scope": "repo", "repo": repo, "value": null })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["repo"]["value"], Value::Null);

    let (st, _) = call(app, "POST", "/api/settings/worktree-dir", Some(json!({ "scope": "repo", "value": "x" })), true, "127.0.0.1").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workspace_adoption_checks_identity_and_missing_paths() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let wt = h.repo.parent().unwrap().join("external-wt");
    git(
        &h.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "fix-it",
            wt.to_str().unwrap(),
            "main",
        ],
    );
    let canonical_wt = wt.canonicalize().unwrap();
    let (status, candidates) = call(
        &h.app,
        "GET",
        &format!("/api/sessions/{key}/workspace/candidates"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        candidates["candidates"][0]["root"],
        canonical_wt.to_str().unwrap()
    );
    let route = format!("/api/sessions/{key}/workspace/adopt");
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "path": wt })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, adopted) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "path": wt })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{adopted}");
    assert_eq!(
        adopted["session"]["worktree"],
        canonical_wt.to_str().unwrap()
    );
    git(&h.repo, &["worktree", "remove", "-f", wt.to_str().unwrap()]);
    let (status, check) = call(
        &h.app,
        "GET",
        &format!("/api/sessions/{key}/workspace/candidates"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(check["missing"], true);
    let (status, _) = call(
        &h.app,
        "POST",
        &format!("/api/sessions/{key}/runs"),
        Some(json!({ "provider": "codex", "stage": "assist", "prompt": "check this" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn recorded_old_worktree_path_survives_reload() {
    let h = harness();
    let old = h.repo.join(".worktree").join("fix-it");
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo, "worktree": true, "worktree_dir": old })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(
        created["session"]["session"]["worktree"],
        old.to_str().unwrap()
    );
    let reloaded = Registry::load(h.state.config.home.clone()).unwrap();
    assert_eq!(
        reloaded
            .get_key("repo/fix-it")
            .unwrap()
            .session()
            .worktree
            .as_deref(),
        Some(old.as_path())
    );
}

#[tokio::test]
async fn pr_drafts_require_human_and_old_meta_loads() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let mut old: Value =
        serde_json::from_str(&std::fs::read_to_string(live.store.meta_path()).unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("pull_request");
    let parsed: plantool_core::Session = serde_json::from_value(old).unwrap();
    assert!(parsed.pull_request.is_none());
    let route = format!("/api/sessions/{key}/pr/draft");
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "title": "Fix thing", "body": "Why it matters" })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "title": "Fix thing", "body": "Why it matters" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, draft) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(draft["title"], "Fix thing");
    let (status, _) = call(&h.app, "POST", &format!("/api/sessions/{key}/pr"), Some(json!({ "head": "fix-it", "repository": "owner/repo", "title": "x", "body": "x", "draft": true })), false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[cfg(unix)]
#[tokio::test]
async fn pr_create_error_links_pr_found_on_retry() {
    use std::os::unix::fs::PermissionsExt;
    let h = harness();
    let bare = h._tmp.path().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare"]);
    git(
        &h.repo,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo, "worktree": true })),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let wt = live.session().worktree.unwrap();
    std::fs::write(wt.join("change.txt"), "change").unwrap();
    git(&wt, &["add", "change.txt"]);
    git(&wt, &["commit", "-q", "-m", "change"]);
    live.set_stage(
        plantool_core::Stage::ImplementationReview,
        plantool_core::Actor::Human,
    )
    .unwrap();
    let marker = h._tmp.path().join("created");
    let script = h._tmp.path().join("gh-stub");
    std::fs::write(&script, format!(r#"#!/bin/sh
if [ "$1 $2" = "repo view" ]; then echo 'owner/repo'; exit 0; fi
if [ "$1 $2" = "pr list" ]; then
  if [ -e '{}' ]; then echo '[{{"number":23,"url":"https://github.com/owner/repo/pull/23","state":"OPEN","isDraft":true,"headRefName":"fix-it","baseRefName":"main"}}]'; else echo '[]'; fi
  exit 0
fi
if [ "$1 $2" = "pr create" ]; then touch '{}'; echo 'request timed out' >&2; exit 1; fi
exit 1
"#, marker.display(), marker.display())).unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    std::env::set_var("PLANTOOL_GH_BIN", &script);
    let (status, preview) = call(
        &h.app,
        "GET",
        &format!("/api/sessions/{key}/pr/preview"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["head"], "fix-it");
    assert_eq!(preview["commits_ahead"], 1);
    let (status, result) = call(&h.app, "POST", &format!("/api/sessions/{key}/pr"), Some(json!({ "head": "fix-it", "repository": "owner/repo", "title": "Fix it", "body": "Why it matters", "draft": true })), true, "127.0.0.1").await;
    std::env::remove_var("PLANTOOL_GH_BIN");
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["pull_request"]["number"], 23);
    let saved: plantool_core::Session =
        serde_json::from_str(&std::fs::read_to_string(live.store.meta_path()).unwrap()).unwrap();
    assert_eq!(saved.pull_request.unwrap().number, 23);
}

#[tokio::test]
async fn dropped_session_cannot_be_recreated_by_late_pr_refresh() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let dir = live.store.dir.clone();
    h.state.registry.remove(key, false).unwrap();
    assert!(!dir.exists());
    let result = live.set_pull_request(plantool_core::PullRequest {
        number: 23,
        url: "https://github.com/owner/repo/pull/23".into(),
        state: "OPEN".into(),
        draft: false,
        updated_at: plantool_core::now(),
    });
    assert!(result.is_err());
    assert!(!dir.exists());
}

#[tokio::test]
async fn pr_commit_from_dialog() {
    let h = harness();
    let (_, created) = call(&h.app, "POST", "/api/sessions", Some(json!({ "slug": "fix-it", "cwd": h.repo, "worktree": true })), false, "127.0.0.1").await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let wt = live.session().worktree.unwrap();
    let route = format!("/api/sessions/{key}/pr/commit");
    let (status, _) = call(&h.app, "POST", &route, Some(json!({ "message": "Add change" })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not at implementation review yet");
    live.set_stage(plantool_core::Stage::ImplementationReview, plantool_core::Actor::Human).unwrap();

    let (status, draft) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(draft["exists"], false);
    assert!(draft["message"].as_str().unwrap().contains("Session: fix-it"), "{draft}");
    std::fs::write(live.store.dir.join("commit-draft.md"), "Add the change\n\nSession: repo/fix-it\n").unwrap();
    let (_, draft) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(draft["exists"], true);
    assert_eq!(draft["message"], "Add the change\n\nSession: repo/fix-it");

    let (status, _) = call(&h.app, "POST", &route, Some(json!({ "message": "Add change" })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CONFLICT, "clean tree");
    std::fs::write(wt.join("change.txt"), "change").unwrap();
    let (status, _) = call(&h.app, "POST", &route, Some(json!({ "message": "Add change" })), false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "agents cannot commit");
    let (status, _) = call(&h.app, "POST", &route, Some(json!({ "message": "  " })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, result) = call(&h.app, "POST", &route, Some(json!({ "message": "Add the change\n\nSession: repo/fix-it" })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let head = plantool_daemon::git::head_sha(&wt).unwrap();
    assert_eq!(result["sha"], head);
    assert!(!plantool_daemon::git::is_dirty(&wt).unwrap());
    assert_eq!(plantool_daemon::git::git(&wt, &["rev-list", "--count", "main..HEAD"]).unwrap(), "1");
    assert_eq!(plantool_daemon::git::git(&wt, &["log", "-1", "--format=%s"]).unwrap(), "Add the change");
    assert!(!live.store.dir.join("commit-draft.md").exists());

    git(&wt, &["checkout", "-q", "--detach"]);
    std::fs::write(wt.join("more.txt"), "more").unwrap();
    let (status, _) = call(&h.app, "POST", &route, Some(json!({ "message": "More" })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "detached HEAD");
}

#[cfg(unix)]
#[tokio::test]
async fn pr_commit_streams_progress_blocks_a_second_commit_and_cancels() {
    use std::os::unix::fs::PermissionsExt;

    let h = harness();
    let (_, created) = call(&h.app, "POST", "/api/sessions", Some(json!({ "slug": "fix-it", "cwd": h.repo, "worktree": true })), false, "127.0.0.1").await;
    let key = created["session"]["key"].as_str().unwrap().to_string();
    let live = h.state.registry.get_key(&key).unwrap();
    live.set_stage(plantool_core::Stage::ImplementationReview, plantool_core::Actor::Human).unwrap();
    let wt = live.session().worktree.unwrap();
    let hook = h.repo.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho 'ruff....Passed'\nsleep 30 &\nwait\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let route = format!("/api/sessions/{key}/pr/commit");
    let mut events = live.tx.subscribe();

    let (status, _) = call(&h.app, "POST", &route, Some(json!({ "message": "Add change" })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CONFLICT, "clean tree");
    assert!(events.try_recv().is_err(), "a clean tree announces no commit");

    std::fs::write(wt.join("change.txt"), "change").unwrap();
    let app = h.app.clone();
    let first_route = route.clone();
    let first = tokio::spawn(async move { call(&app, "POST", &first_route, Some(json!({ "message": "Add change" })), true, "127.0.0.1").await });
    let running = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let msg = serde_json::to_value(events.recv().await.unwrap()).unwrap();
            if msg["type"] == "commit-progress" && msg["commit"]["lines"].as_array().is_some_and(|l| !l.is_empty()) {
                return msg;
            }
        }
    })
    .await
    .expect("the hook's output arrives while it runs");
    assert_eq!(running["commit"]["phase"], "hooks");
    assert_eq!(running["commit"]["scope"]["kind"], "pr");
    assert_eq!(running["commit"]["lines"][0], "ruff....Passed");

    let (_, view) = call(&h.app, "GET", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert_eq!(view["commit"]["phase"], "hooks", "a reload sees the running commit");
    let (status, busy) = call(&h.app, "POST", &route, Some(json!({ "message": "Again" })), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(busy["error"].as_str().unwrap().contains("already running"), "{busy}");

    let cancel = format!("/api/sessions/{key}/commit/cancel");
    let (status, _) = call(&h.app, "POST", &cancel, None, false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "agents cannot cancel");
    let (status, _) = call(&h.app, "POST", &cancel, None, true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, result) = tokio::time::timeout(std::time::Duration::from_secs(10), first).await.expect("cancel stops the hook").unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(result["error"], "commit cancelled");

    let (_, view) = call(&h.app, "GET", &format!("/api/sessions/{key}"), None, false, "127.0.0.1").await;
    assert!(view.get("commit").is_none(), "{view}");
    assert_eq!(plantool_daemon::git::git(&wt, &["rev-list", "--count", "main..HEAD"]).unwrap(), "0");
    assert_eq!(plantool_daemon::git::git(&wt, &["diff", "--cached", "--name-only"]).unwrap(), "change.txt");
    let (status, _) = call(&h.app, "POST", &cancel, None, true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CONFLICT, "nothing left to cancel");
}
