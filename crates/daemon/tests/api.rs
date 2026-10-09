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
    let st = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
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
    let config = Arc::new(Config {
        home: home.clone(),
        port: 0,
        token: "tok".into(),
        version: "test".into(),
    });
    let registry = Arc::new(Registry::load(home).unwrap());
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let state = AppState {
        registry,
        config,
        runs: Arc::new(plantool_daemon::runs::RunManager::default()),
        shutdown: tx,
    };
    let app = routes::router(state.clone());
    Harness {
        app,
        state,
        _tmp: tmp,
        repo,
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    human: bool,
    host: &str,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, host);
    if human {
        req = req.header("x-plantool-actor", "tok");
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes)
        .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string()));
    (status, v)
}

#[tokio::test]
async fn uploads_a_file_into_the_session_and_rejects_empty_files() {
    let h = harness();
    let (status, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "upload", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let key = created["session"]["key"].as_str().unwrap();
    let path = format!("/api/sessions/{key}/attachments");
    let upload = |bytes: &'static [u8]| {
        Request::builder()
            .method("POST")
            .uri(&path)
            .header(header::HOST, "127.0.0.1")
            .header("x-plantool-actor", "tok")
            .header("x-plantool-filename", "notes / plan.txt")
            .body(Body::from(bytes))
            .unwrap()
    };
    let response = h
        .app
        .clone()
        .oneshot(upload(b"review context"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let saved = std::path::PathBuf::from(body["path"].as_str().unwrap());
    assert_eq!(std::fs::read(&saved).unwrap(), b"review context");
    assert!(saved
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with("notes___plan.txt"));
    assert_eq!(
        h.app.clone().oneshot(upload(b"")).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn full_session_flow() {
    let h = harness();
    let app = &h.app;
    let (st, v) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let key = v["session"]["key"].as_str().unwrap().to_string();
    assert_eq!(key, "repo/fix-it");

    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/docs/plan/path"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let plan_path = std::path::PathBuf::from(v["path"].as_str().unwrap());
    assert!(!v["exists"].as_bool().unwrap());

    let (st, _) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/docs/plan"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    std::fs::write(
        &plan_path,
        "# Plan\n\nalpha\nbeta\n\n### Phase 1: X\n- [ ] one\n- [x] two\n",
    )
    .unwrap();
    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/docs/plan/touch"),
        Some(json!({})),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["changed"].as_bool().unwrap());

    let (_, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(v["state"]["stage"], "plan-review");
    assert_eq!(v["docs"][1]["progress"][0]["done"], 1);

    let live = h.state.registry.get_key(&key).unwrap();
    let mut rx = live.tx.subscribe();
    let batch = json!({ "comments": [
        { "doc": "plan", "match": "alpha", "body": "one" },
        { "doc": "plan", "line": 4, "body": "two" }
    ]});
    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/comments/batch"),
        Some(batch),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["comments"].as_array().unwrap().len(), 2);
    assert_eq!(v["comments"][0]["kind"], "agent");
    let ev = rx.try_recv().unwrap();
    assert!(matches!(
        ev.event,
        plantool_daemon::events::LiveEvent::CommentsAdded { .. }
    ));
    assert!(rx.try_recv().is_err(), "one broadcast for one batch");

    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/comments"),
        Some(json!({ "doc": "plan", "match": "nope", "body": "x" })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    std::fs::write(&plan_path, "# Plan\n\nintro\nalpha\nBETA\n").unwrap();
    let (_, _) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/docs/plan/touch"),
        Some(json!({})),
        false,
        "127.0.0.1",
    )
    .await;
    let (_, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/comments"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    let comments = v["comments"].as_array().unwrap();
    assert_eq!(comments[0]["anchor"]["line"], 4);
    assert_eq!(comments[0]["anchor"]["outdated"], false);
    assert_eq!(comments[1]["anchor"]["outdated"], true);

    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/stage"),
        Some(json!({ "to": "approved" })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/stage"),
        Some(json!({ "to": "approved" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["actor"], "human");
    let (st, _) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/stage"),
        Some(json!({ "to": "implementing" })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/changes"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["base"], "main");

    let (st, v) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
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
    assert_eq!(
        v["session"]["session"]["brief"],
        "Token scope names are confusing"
    );

    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/prompt/next"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["stage"], "research");
    let prompt = v["prompt"].as_str().unwrap();
    assert!(prompt.contains("plantool skill research"), "{prompt}");
    assert!(
        prompt.contains("What the user wants:\nToken scope names are confusing"),
        "{prompt}"
    );
    assert!(prompt.contains("/research.md"), "{prompt}");

    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/prompt/plan?extra=check%20issue%2013162"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["prompt"]
        .as_str()
        .unwrap()
        .contains("Additional instructions from the user:\ncheck issue 13162"));

    let (st, _) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/prompt/bogus"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let live = h.state.registry.get_key(&key).unwrap();
    let mut rx = live.tx.subscribe();
    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/brief"),
        Some(json!({ "brief": "Rename the scopes" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["brief"], "Rename the scopes");
    assert!(matches!(
        rx.try_recv().unwrap().event,
        plantool_daemon::events::LiveEvent::SessionUpdated { .. }
    ));
    let (_, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(v["session"]["brief"], "Rename the scopes");
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(live.store.meta_path()).unwrap()).unwrap();
    assert_eq!(meta["brief"], "Rename the scopes");

    let (_, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/prompt/research"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert!(v["prompt"].as_str().unwrap().contains("Rename the scopes"));

    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/brief"),
        Some(json!({ "brief": "   " })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["brief"].is_null());
    let (_, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/prompt/research"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert!(!v["prompt"]
        .as_str()
        .unwrap()
        .contains("What the user wants"));

    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/runs"),
        Some(json!({ "provider": "claude", "stage": "research", "permission_mode": "sometimes" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    let (st, _) = call(
        app,
        "DELETE",
        &format!("/api/sessions/{key}/runs/nope"),
        None,
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let stopped = plantool_core::Run {
        idle_stopped: false,
        milestone_key: None,
        plan_sha: None,
        id: "r1".into(),
        provider: plantool_core::Provider::Claude,
        provider_session_id: None,
        stage: plantool_core::Stage::Researching,
        implementation_mode: Default::default(),
        milestone_pending: false,
        milestone_review: None,
        milestone_base: None,
        milestones_approved: 0,
        milestone_commit: None,
        implementation_base: None,
        task: None,
        cwd: h.repo.clone(),
        status: plantool_core::RunStatus::Stopped,
        model: None,
        permission_mode: Default::default(),
        started_at: plantool_core::now(),
        ended_at: None,
        error: None,
        seq: 0,
    };
    live.upsert_run(stopped, |r| {
        plantool_daemon::events::LiveEvent::RunStarted { run: r }
    })
    .unwrap();
    live.append_run_event("r1", 1, json!({ "type": "status", "label": "x" }))
        .unwrap();
    assert!(live.store.run_log_path("r1").is_file());
    let (st, v) = call(
        app,
        "DELETE",
        &format!("/api/sessions/{key}/runs/r1"),
        None,
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(!live.store.run_meta_path("r1").exists());
    assert!(!live.store.run_log_path("r1").exists());
    let (_, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(v["runs"].as_array().unwrap().len(), 0);
    assert_eq!(v["session"]["created_in"], json!(h.repo));

    let (st, v) = call(app, "GET", "/api/repos", None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v["repos"][0]["root"],
        json!(std::fs::canonicalize(&h.repo).unwrap())
    );
    assert_eq!(v["repos"][0]["sessions"], 1);

    let (st, v) = call(
        app,
        "DELETE",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/worktree"),
        Some(json!({})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let wt = std::path::PathBuf::from(v["worktree"].as_str().unwrap());
    assert!(wt.join("a.txt").is_file() || wt.is_dir());
    let mut rx = live.tx.subscribe();
    let (st, v) = call(
        app,
        "DELETE",
        &format!("/api/sessions/{key}?worktree=true"),
        None,
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(matches!(
        rx.try_recv().unwrap().event,
        plantool_daemon::events::LiveEvent::SessionRemoved { .. }
    ));
    assert!(!live.store.dir.exists());
    assert!(!wt.exists());
    let (st, _) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert!(h.state.registry.get_key(&key).is_none());
}

#[tokio::test]
async fn step_mode_previews_one_ticket_and_reviews_uncommitted_changes() {
    let h = harness();
    let app = &h.app;
    let (_, v) = call(
        app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "steps", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    let key = v["session"]["key"].as_str().unwrap().to_string();
    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/prompt/implement?implementation_mode=step-by-step"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["prompt"]
        .as_str()
        .unwrap()
        .contains("Work on one plan phase"));

    let live = h.state.registry.get_key(&key).unwrap();
    live.set_stage(
        plantool_core::Stage::Implementing,
        plantool_core::Actor::Human,
    )
    .unwrap();
    let first = plantool_daemon::git::head_sha(&h.repo).unwrap();
    // milestone 1 was committed by approval; milestone 2 is in progress
    std::fs::write(h.repo.join("a.txt"), "hi\nmilestone one\n").unwrap();
    let base = plantool_daemon::git::commit_all(&h.repo, "Milestone 1: first").unwrap();
    std::fs::write(h.repo.join("b.txt"), "milestone two\n").unwrap();
    let run = plantool_core::Run {
        idle_stopped: false,
        milestone_key: None,
        plan_sha: None,
        id: "step-run".into(),
        provider: plantool_core::Provider::Claude,
        provider_session_id: None,
        stage: plantool_core::Stage::Implementing,
        implementation_mode: plantool_core::ImplementationMode::StepByStep,
        milestone_pending: true,
        milestone_review: None,
        milestone_base: Some(base.clone()),
        milestones_approved: 1,
        milestone_commit: None,
        implementation_base: Some(first.clone()),
        task: Some("implement".into()),
        cwd: h.repo.clone(),
        status: plantool_core::RunStatus::Idle,
        model: None,
        permission_mode: Default::default(),
        started_at: plantool_core::now(),
        ended_at: None,
        error: None,
        seq: 0,
    };
    live.upsert_run(run, |r| plantool_daemon::events::LiveEvent::RunStarted {
        run: r,
    })
    .unwrap();
    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/changes"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["scope"], "step");
    assert_eq!(v["base"], base);
    assert_eq!(v["label"], "since milestone 1 was approved");
    assert_eq!(v["step_available"], true);
    let paths: Vec<&str> = v["stat"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec!["b.txt"],
        "only the current milestone is in the step scope"
    );
    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/changes?scope=all"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["scope"], "all");
    assert_eq!(v["base"], first);
    assert_eq!(v["label"], "in the whole implementation");
    let mut paths: Vec<&str> = v["stat"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        vec!["a.txt", "b.txt"],
        "the whole implementation diffs against the base: {v}"
    );
    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/changes/file?path=b.txt"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        v["diff"].as_str().unwrap().contains("+milestone two"),
        "{v}"
    );
    assert_eq!(v["truncated"], false);
    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/changes/file?path=a.txt&scope=all"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        v["diff"].as_str().unwrap().contains("+milestone one"),
        "{v}"
    );
    let (st, _) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/changes/file?path=../x"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert_ne!(first, base);
    let (st, v) = call(app, "GET", &format!("/api/sessions/{key}/prompt/implement?implementation_mode=step-by-step&resume_run=step-run"), None, false, "127.0.0.1").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["prompt"]
        .as_str()
        .unwrap()
        .contains("Do not start another plan task yet"));
    let (st, _) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/runs/step-run/milestone/approve"),
        Some(json!({})),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(
        app,
        "GET",
        &format!("/api/sessions/{key}/runs/step-run/milestone"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["subject"], "Milestone 2");
    assert_eq!(v["dirty"], true);
    assert_eq!(v["live"], false);
    let (st, v) = call(
        app,
        "POST",
        &format!("/api/sessions/{key}/runs/step-run/milestone/approve"),
        Some(json!({ "message": "custom subject" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap_or_default().contains("not live"),
        "{v}"
    );
    assert!(
        plantool_daemon::git::is_dirty(&h.repo).unwrap(),
        "a refused approval must not commit"
    );
    assert_eq!(live.run("step-run").unwrap().milestones_approved, 1);
}

#[tokio::test]
async fn rejects_foreign_host_and_origin() {
    let h = harness();
    let (st, _) = call(&h.app, "GET", "/health", None, false, "evil.example").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let req = Request::builder()
        .uri("/health")
        .header(header::HOST, "127.0.0.1")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        h.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let (st, _) = call(&h.app, "GET", "/health", None, false, "localhost:41200").await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn bad_slug_and_missing_repo() {
    let h = harness();
    let (st, _) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "../x", "cwd": h.repo })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "ok", "cwd": h._tmp.path() })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, _) = call(
        &h.app,
        "GET",
        "/api/sessions/nope",
        None,
        false,
        "127.0.0.1",
    )
    .await;
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
    let (st, _) = call(
        app,
        "POST",
        "/api/settings/worktree-dir",
        Some(body.clone()),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(
        app,
        "POST",
        "/api/settings/worktree-dir",
        Some(body),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["repo"]["value"], "../{repo}-worktrees/{slug}");
    assert_eq!(v["effective"], "../{repo}-worktrees/{slug}");
    let example = repo.parent().unwrap().join("repo-worktrees").join("<slug>");
    assert_eq!(v["example"], json!(example));

    let (_, v) = call(
        app,
        "GET",
        &format!("{q}&preview=trees"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(v["example"], json!(repo.join("trees").join("<slug>")));

    let (st, v) = call(
        app,
        "POST",
        "/api/settings/worktree-dir",
        Some(json!({ "scope": "repo", "repo": repo, "value": null })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["repo"]["value"], Value::Null);

    let (st, _) = call(
        app,
        "POST",
        "/api/settings/worktree-dir",
        Some(json!({ "scope": "repo", "value": "x" })),
        true,
        "127.0.0.1",
    )
    .await;
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
    let route = format!("/api/sessions/{key}/pr/commit");
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "Add change" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "not at implementation review yet"
    );
    live.set_stage(
        plantool_core::Stage::ImplementationReview,
        plantool_core::Actor::Human,
    )
    .unwrap();

    let (status, draft) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(draft["exists"], false);
    assert!(
        draft["message"]
            .as_str()
            .unwrap()
            .contains("Session: fix-it"),
        "{draft}"
    );
    std::fs::write(
        live.store.dir.join("commit-draft.md"),
        "Add the change\n\nSession: repo/fix-it\n",
    )
    .unwrap();
    let (_, draft) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(draft["exists"], true);
    assert_eq!(draft["message"], "Add the change\n\nSession: repo/fix-it");

    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "Add change" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "clean tree");
    std::fs::write(wt.join("change.txt"), "change").unwrap();
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "Add change" })),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "agents cannot commit");
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "  " })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, result) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "Add the change\n\nSession: repo/fix-it" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let head = plantool_daemon::git::head_sha(&wt).unwrap();
    assert_eq!(result["sha"], head);
    assert!(!plantool_daemon::git::is_dirty(&wt).unwrap());
    assert_eq!(
        plantool_daemon::git::git(&wt, &["rev-list", "--count", "main..HEAD"]).unwrap(),
        "1"
    );
    assert_eq!(
        plantool_daemon::git::git(&wt, &["log", "-1", "--format=%s"]).unwrap(),
        "Add the change"
    );
    assert!(!live.store.dir.join("commit-draft.md").exists());

    git(&wt, &["checkout", "-q", "--detach"]);
    std::fs::write(wt.join("more.txt"), "more").unwrap();
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "More" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "detached HEAD");
}

#[cfg(unix)]
#[tokio::test]
async fn pr_commit_streams_progress_blocks_a_second_commit_and_cancels() {
    use std::os::unix::fs::PermissionsExt;

    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({ "slug": "fix-it", "cwd": h.repo, "worktree": true })),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap().to_string();
    let live = h.state.registry.get_key(&key).unwrap();
    live.set_stage(
        plantool_core::Stage::ImplementationReview,
        plantool_core::Actor::Human,
    )
    .unwrap();
    let wt = live.session().worktree.unwrap();
    let hook = h.repo.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'ruff....Passed'\nsleep 30 &\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let route = format!("/api/sessions/{key}/pr/commit");
    let mut events = live.tx.subscribe();

    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "Add change" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "clean tree");
    assert!(
        events.try_recv().is_err(),
        "a clean tree announces no commit"
    );

    std::fs::write(wt.join("change.txt"), "change").unwrap();
    let app = h.app.clone();
    let first_route = route.clone();
    let first = tokio::spawn(async move {
        call(
            &app,
            "POST",
            &first_route,
            Some(json!({ "message": "Add change" })),
            true,
            "127.0.0.1",
        )
        .await
    });
    let running = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let msg = serde_json::to_value(events.recv().await.unwrap()).unwrap();
            if msg["type"] == "commit-progress"
                && msg["commit"]["lines"]
                    .as_array()
                    .is_some_and(|l| !l.is_empty())
            {
                return msg;
            }
        }
    })
    .await
    .expect("the hook's output arrives while it runs");
    assert_eq!(running["commit"]["phase"], "hooks");
    assert_eq!(running["commit"]["scope"]["kind"], "pr");
    assert_eq!(running["commit"]["lines"][0], "ruff....Passed");

    let (_, view) = call(
        &h.app,
        "GET",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(
        view["commit"]["phase"], "hooks",
        "a reload sees the running commit"
    );
    let (status, busy) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({ "message": "Again" })),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        busy["error"].as_str().unwrap().contains("already running"),
        "{busy}"
    );

    let cancel = format!("/api/sessions/{key}/commit/cancel");
    let (status, _) = call(&h.app, "POST", &cancel, None, false, "127.0.0.1").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "agents cannot cancel");
    let (status, _) = call(&h.app, "POST", &cancel, None, true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, result) = tokio::time::timeout(std::time::Duration::from_secs(10), first)
        .await
        .expect("cancel stops the hook")
        .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(result["error"], "commit cancelled");

    let (_, view) = call(
        &h.app,
        "GET",
        &format!("/api/sessions/{key}"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert!(view.get("commit").is_none(), "{view}");
    assert_eq!(
        plantool_daemon::git::git(&wt, &["rev-list", "--count", "main..HEAD"]).unwrap(),
        "0"
    );
    assert_eq!(
        plantool_daemon::git::git(&wt, &["diff", "--cached", "--name-only"]).unwrap(),
        "change.txt"
    );
    let (status, _) = call(&h.app, "POST", &cancel, None, true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CONFLICT, "nothing left to cancel");
}

#[tokio::test]
async fn scoped_comments_gate_approval_and_record_override() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"review","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    std::fs::write(
        live.doc_path(plantool_core::DocKind::Plan),
        "# Plan\n\n## Risks\n- [ ] Task\n",
    )
    .unwrap();
    live.capture_doc(plantool_core::DocKind::Plan).unwrap();
    let prefix = format!("/api/sessions/{key}");
    let (status, comment) = call(
        &h.app,
        "POST",
        &format!("{prefix}/comments"),
        Some(json!({"doc":"plan","scope":"document","type":"blocker","body":"Missing risk"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(comment["comments"][0]["anchor"]["scope"], "document");
    let (status, _) = call(
        &h.app,
        "POST",
        &format!("{prefix}/stage"),
        Some(json!({"to":"approved"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_ne!(status, StatusCode::OK);
    let (status, _) = call(
        &h.app,
        "POST",
        &format!("{prefix}/stage"),
        Some(json!({"to":"approved","override_reason":"Accepted risk"})),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &h.app,
        "POST",
        &format!("{prefix}/stage"),
        Some(json!({"to":"approved","override_reason":"Accepted risk"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let state = live.state();
    let approved = state.approved.unwrap();
    assert_eq!(approved.override_reason.as_deref(), Some("Accepted risk"));
    assert_eq!(approved.open_blockers.len(), 1);
    assert_eq!(
        approved.sha,
        live.doc(plantool_core::DocKind::Plan).unwrap().sha
    );
    assert!(!state.activity.is_empty());
}

#[tokio::test]
async fn context_changes_require_owner_or_explicit_confirmation_and_check_dirty_tree() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"context","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let path = format!("/api/sessions/{key}/where");
    let (status, proposed) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"base":"HEAD","reason":"Compare current work"})),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(proposed["proposed"], true);
    let live = h.state.registry.get_key(key).unwrap();
    assert_eq!(live.session().base, "main");
    let (status, changed) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"base":"HEAD","confirm":true})),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(live.session().base, "HEAD");
    git(&h.repo, &["branch", "feature"]);
    let (status, changed) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"branch":"feature"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changed["view"]["branch_now"], "feature");
    let (_, prompt) = call(
        &h.app,
        "GET",
        &format!("/api/sessions/{key}/prompt/plan"),
        None,
        false,
        "127.0.0.1",
    )
    .await;
    assert!(prompt["prompt"]
        .as_str()
        .unwrap()
        .contains("Branch: feature"));
    assert!(prompt["prompt"].as_str().unwrap().contains("Base: HEAD"));
    std::fs::write(h.repo.join("a.txt"), "dirty\n").unwrap();
    let (status, _) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"base":"main"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(live.session().base, "HEAD");
    let (status, _) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"difftool":"built-in"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(live.session().difftool.as_deref(), Some("built-in"));
}

#[tokio::test]
async fn archives_round_trip_with_new_paths_and_collision_names() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"portable","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let original = "# Plan\n\n## Summary\nPortable session\n\n### Phase 1\n- [ ] Work\n";
    std::fs::write(live.doc_path(plantool_core::DocKind::Plan), original).unwrap();
    live.capture_doc(plantool_core::DocKind::Plan).unwrap();
    live.set_stage(plantool_core::Stage::Approved, plantool_core::Actor::Human)
        .unwrap();
    let approved = live.state().approved.unwrap().sha;
    let updated = original.replace("[ ]", "[x]");
    std::fs::write(live.doc_path(plantool_core::DocKind::Plan), &updated).unwrap();
    live.capture_doc(plantool_core::DocKind::Plan).unwrap();
    live.mark_viewed(plantool_core::DocKind::Plan, &approved)
        .unwrap();
    let bytes =
        plantool_daemon::archive::export(&live, false, false, Some("Shareable result")).unwrap();
    let bundle = plantool_daemon::archive::decode(&bytes).unwrap();
    assert!(bundle.transcripts.is_empty());
    assert!(bundle.revisions.iter().any(|r| r.sha == approved));
    let collision = h
        .state
        .registry
        .import_bundle(bundle, &h.repo, None)
        .unwrap();
    assert!(collision.key.ends_with("portable-import-1"));
    let other = h._tmp.path().join("other-repo");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/import?repo={}", other.display()))
        .header(header::HOST, "127.0.0.1")
        .header("x-plantool-actor", "tok")
        .header(header::CONTENT_TYPE, "application/zip")
        .body(Body::from(bytes))
        .unwrap();
    let response = h.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let imported: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        imported["session"]["session"]["repo"]["root"],
        other.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    assert_eq!(imported["session"]["state"]["approved"]["sha"], approved);
    let imported_key = imported["session"]["key"].as_str().unwrap();
    let reloaded = h.state.registry.get_key(imported_key).unwrap();
    assert_eq!(
        reloaded.doc(plantool_core::DocKind::Plan).unwrap().content,
        updated
    );
}

#[cfg(unix)]
#[tokio::test]
async fn idle_provider_stops_and_resumes_on_owner_comment() {
    let _provider_lock = PROVIDER_TEST_LOCK.lock().await;
    use std::os::unix::fs::PermissionsExt;
    let h = harness();
    let fake = h._tmp.path().join("fake-claude");
    std::fs::write(&fake, "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"fake-session\"}'\nwhile IFS= read -r line; do\n case \"$line\" in\n *'\"type\":\"user\"'*) printf '%s\\n' '{\"type\":\"result\",\"subtype\":\"success\"}' ;;\n esac\ndone\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("PLANTOOL_CLAUDE_PATH", &fake);
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            std::env::remove_var("PLANTOOL_CLAUDE_PATH");
        }
    }
    let _restore = Restore;
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"idle","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    std::fs::write(live.doc_path(plantool_core::DocKind::Plan), "# Plan\n").unwrap();
    live.capture_doc(plantool_core::DocKind::Plan).unwrap();
    let run = h
        .state
        .runs
        .start(
            live.clone(),
            plantool_core::Provider::Claude,
            plantool_core::Stage::Planning,
            "assist",
            "Read feedback".into(),
            None,
            None,
            None,
            plantool_core::PermissionMode::Ask,
            plantool_core::ImplementationMode::AllAtOnce,
            None,
        )
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while live.run(&run.id).unwrap().status != plantool_core::RunStatus::Idle {
        assert!(
            tokio::time::Instant::now() < deadline,
            "fake provider did not become idle"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    h.state.runs.sweep_idle(&live, 0).await;
    assert!(h.state.runs.is_live(&run.id));
    h.state
        .runs
        .sweep_idle_for(&live, std::time::Duration::ZERO)
        .await;
    while h.state.runs.is_live(&run.id) {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(live.run(&run.id).unwrap().idle_stopped);
    assert_eq!(
        live.run(&run.id).unwrap().provider_session_id.as_deref(),
        Some("fake-session")
    );
    let (status, _) = call(
        &h.app,
        "POST",
        &format!("/api/sessions/{key}/comments"),
        Some(json!({"doc":"plan","scope":"document","body":"Continue with this feedback"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let next = live
        .runs()
        .into_iter()
        .find(|r| r.id != run.id)
        .expect("owner comment resumes provider");
    assert_eq!(next.provider_session_id.as_deref(), Some("fake-session"));
    let _ = h
        .state
        .runs
        .send(&next.id, plantool_daemon::providers::RunInput::Stop)
        .await;
}

static PROVIDER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(unix)]
#[tokio::test]
async fn phase_runs_have_separate_diff_windows_and_owner_checkpoint_approval() {
    use std::os::unix::fs::PermissionsExt;
    let _provider_lock = PROVIDER_TEST_LOCK.lock().await;
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"phases","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let plan = live.doc_path(plantool_core::DocKind::Plan);
    std::fs::write(
        &plan,
        "# Plan\n\n## Tasks\n### Phase 1: First\n- [ ] One\n### Phase 2: Second\n- [ ] Two\n",
    )
    .unwrap();
    live.capture_doc(plantool_core::DocKind::Plan).unwrap();
    live.set_stage(plantool_core::Stage::Approved, plantool_core::Actor::Human)
        .unwrap();
    live.set_stage(
        plantool_core::Stage::Implementing,
        plantool_core::Actor::Agent,
    )
    .unwrap();
    live.apply_context(&plantool_daemon::routes::where_context::Update {
        difftool: Some("built-in".into()),
        pause_rule: Some(plantool_core::PauseRule::EveryMilestone),
        ..Default::default()
    })
    .unwrap();
    let fake = h._tmp.path().join("phase-claude");
    let path = serde_json::to_string(&plan.to_string_lossy()).unwrap();
    let script = format!("#!/usr/bin/env python3\nimport sys,json,pathlib\np=pathlib.Path({path})\nprint(json.dumps({{'type':'system','subtype':'init','session_id':'phase-session'}}),flush=True)\nfor raw in sys.stdin:\n msg=json.loads(raw)\n if msg.get('type')=='user':\n  text=p.read_text().replace('- [ ]','- [x]',1)\n  p.write_text(text)\n  pathlib.Path('a.txt').write_text('completed '+str(text.count('[x]'))+'\\n')\n  print(json.dumps({{'type':'result','subtype':'success'}}),flush=True)\n");
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("PLANTOOL_CLAUDE_PATH", &fake);
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            std::env::remove_var("PLANTOOL_CLAUDE_PATH");
        }
    }
    let _restore = Restore;
    let first = h
        .state
        .runs
        .start(
            live.clone(),
            plantool_core::Provider::Claude,
            plantool_core::Stage::Implementing,
            "implement",
            "Build phase".into(),
            None,
            None,
            None,
            plantool_core::PermissionMode::Ask,
            plantool_core::ImplementationMode::StepByStep,
            None,
        )
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !live.run(&first.id).unwrap().milestone_pending {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(live.state().milestones[0].completed_tasks, vec!["One"]);
    assert_eq!(live.state().milestones[1].status, "pending");
    let path = format!("/api/sessions/{key}/runs/{}/milestone/approve", first.id);
    let (status, _) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"commit":false})),
        false,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    live.apply_context(&plantool_daemon::routes::where_context::Update {
        pause_rule: Some(plantool_core::PauseRule::NoPauses),
        ..Default::default()
    })
    .unwrap();
    let (status, response) = call(
        &h.app,
        "POST",
        &path,
        Some(json!({"commit":false})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    while live.stage() != plantool_core::Stage::ImplementationReview {
        assert!(
            tokio::time::Instant::now() < deadline,
            "next phase did not finish"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let runs = live.runs();
    assert_eq!(runs.len(), 2);
    let second = runs.iter().find(|r| r.id != first.id).unwrap();
    assert_eq!(second.milestone_key.as_deref(), Some("Phase 2: Second"));
    assert_ne!(first.milestone_base, second.milestone_base);
    let diff = plantool_daemon::git::diff_file(
        &h.repo,
        second.milestone_base.as_deref().unwrap(),
        "a.txt",
    )
    .unwrap();
    assert!(diff.contains("-completed 1"));
    assert!(diff.contains("+completed 2"));
    assert_eq!(first.implementation_base, second.implementation_base);
    let whole = plantool_daemon::routes::changes::resolve_window(
        &live,
        Some(plantool_daemon::changes::ChangeScope::All),
    );
    let whole_diff = plantool_daemon::git::diff_file(&h.repo, &whole.base, "a.txt").unwrap();
    assert!(whole_diff.contains("-hi"));
    assert!(whole_diff.contains("+completed 2"));

    assert_eq!(live.state().milestones[0].status, "approved");
    assert_eq!(live.state().milestones[1].status, "completed");
}

#[tokio::test]
async fn revised_plan_requires_review_and_reapproval_clears_pending() {
    use plantool_core::{Actor, DocKind, Stage};
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"revision","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    let path = live.doc_path(DocKind::Plan);
    let plan = "# Plan\n### Phase 1\n- [ ] First\n";
    std::fs::write(&path, plan).unwrap();
    live.capture_doc(DocKind::Plan).unwrap();
    live.set_stage(Stage::Approved, Actor::Human).unwrap();
    let old_sha = live.state().approved.unwrap().sha;
    std::fs::write(&path, format!("{plan}- [ ] Added\n")).unwrap();
    live.capture_doc(DocKind::Plan).unwrap();
    assert!(live.state().plan_revision_pending);
    assert!(live.set_stage(Stage::Implementing, Actor::Agent).is_err());
    let route = format!("/api/sessions/{key}/stage");
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({"to":"approved","plan_sha":old_sha})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let current = live.doc(DocKind::Plan).unwrap().sha;
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({"to":"approved","plan_sha":current})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!live.state().plan_revision_pending);
    assert_eq!(live.state().approved.unwrap().sha, current);
    let stored = plantool_daemon::store::SessionStore::new(live.store.dir.clone());
    assert!(!stored.load_state().unwrap().plan_revision_pending);
    live.set_stage(Stage::Implementing, Actor::Agent).unwrap();
}

#[tokio::test]
async fn code_comments_follow_a_changed_file_and_reject_stale_lines() {
    let h = harness();
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"code-review","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let route = format!("/api/sessions/{key}/comments");
    let (status, _) = call(&h.app, "POST", &route, Some(json!({"doc":"plan","body":"Check this","code":{"path":"a.txt","side":"new","line":1,"context":"wrong"}})), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(&h.app, "POST", &route, Some(json!({"doc":"plan","body":"Check this","code":{"path":"a.txt","side":"new","line":1,"context":"hi"}})), true, "127.0.0.1").await;
    assert_eq!(status, StatusCode::CREATED);
    std::fs::write(h.repo.join("a.txt"), "new line\nhi\n").unwrap();
    git(&h.repo, &["add", "a.txt"]);
    git(&h.repo, &["commit", "-q", "-m", "Move reviewed line"]);
    let (_, comments) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(comments["comments"][0]["anchor"]["code"]["line"], 2);
    assert_eq!(comments["comments"][0]["anchor"]["outdated"], false);
    std::fs::write(h.repo.join("a.txt"), "removed\n").unwrap();
    let (_, comments) = call(&h.app, "GET", &route, None, false, "127.0.0.1").await;
    assert_eq!(comments["comments"][0]["anchor"]["outdated"], true);
}

#[tokio::test]
async fn accepting_scope_changes_reopens_finished_phases() {
    use plantool_core::{Actor, DocKind, Stage};
    for status in ["approved", "completed"] {
        for changed in [
            "- [x] Done\n- [ ] Added\n",
            "- [ ] Reworded\n",
            "- [ ] Done\n",
        ] {
            let h = harness();
            let (_, created) = call(
                &h.app,
                "POST",
                "/api/sessions",
                Some(json!({"slug":"reopen","repo":h.repo})),
                false,
                "127.0.0.1",
            )
            .await;
            let live = h
                .state
                .registry
                .get_key(created["session"]["key"].as_str().unwrap())
                .unwrap();
            let path = live.doc_path(DocKind::Plan);
            std::fs::write(
                &path,
                "# Plan\n### Phase 1\n- [x] Done\n### Phase 2\n- [ ] Later\n",
            )
            .unwrap();
            live.capture_doc(DocKind::Plan).unwrap();
            live.set_stage(Stage::Approved, Actor::Human).unwrap();
            let run: plantool_core::Run =
                serde_json::from_str(include_str!("../../core/tests/fixtures/v0.0.14/run.json"))
                    .unwrap();
            live.milestone_status("Phase 1", status, &run).unwrap();
            std::fs::write(
                &path,
                format!("# Plan\n### Phase 1\n{changed}### Phase 2\n- [ ] Later\n"),
            )
            .unwrap();
            live.capture_doc(DocKind::Plan).unwrap();
            assert!(live.state().plan_revision_pending);
            assert_eq!(live.state().milestones[0].status, status);
            live.set_stage(Stage::Approved, Actor::Human).unwrap();
            let first = live.state().milestones[0].clone();
            assert_eq!(first.status, "pending", "{status}: {changed}");
            let expected = if changed.starts_with("- [x]") {
                vec!["Done".to_string()]
            } else {
                vec![]
            };
            assert_eq!(first.completed_tasks, expected);
            assert!(first.run_id.is_none());
            assert!(first.base.is_none());
            assert!(first.head.is_none());
            live.select_milestone("Phase 1").unwrap();
        }
    }
}

#[tokio::test]
async fn owner_comments_do_not_bypass_revision_review_for_idle_runs() {
    use plantool_core::{Actor, DocKind, Stage};
    use std::os::unix::fs::PermissionsExt;
    let _provider_lock = PROVIDER_TEST_LOCK.lock().await;
    let h = harness();
    let fake = h._tmp.path().join("gated-claude");
    let inputs = h._tmp.path().join("inputs");
    let path = serde_json::to_string(&inputs.to_string_lossy()).unwrap();
    std::fs::write(&fake, format!("#!/usr/bin/env python3\nimport sys,json,pathlib\np=pathlib.Path({path})\nprint(json.dumps({{'type':'system','subtype':'init','session_id':'gated-session'}}),flush=True)\nfor raw in sys.stdin:\n msg=json.loads(raw)\n if msg.get('type')=='user':\n  with p.open('a') as f: f.write('turn\\n')\n  print(json.dumps({{'type':'result','subtype':'success'}}),flush=True)\n")).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let prior = std::env::var_os("PLANTOOL_CLAUDE_PATH");
    std::env::set_var("PLANTOOL_CLAUDE_PATH", &fake);
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("PLANTOOL_CLAUDE_PATH", value),
                None => std::env::remove_var("PLANTOOL_CLAUDE_PATH"),
            }
        }
    }
    let _restore = Restore(prior);
    let (_, created) = call(
        &h.app,
        "POST",
        "/api/sessions",
        Some(json!({"slug":"gated-feedback","repo":h.repo})),
        false,
        "127.0.0.1",
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = h.state.registry.get_key(key).unwrap();
    std::fs::write(live.doc_path(DocKind::Plan), "# Plan\n").unwrap();
    live.capture_doc(DocKind::Plan).unwrap();
    live.set_stage(Stage::Approved, Actor::Human).unwrap();
    live.set_stage(Stage::Implementing, Actor::Agent).unwrap();
    let run = h
        .state
        .runs
        .start(
            live.clone(),
            plantool_core::Provider::Claude,
            Stage::Implementing,
            "implement",
            "Start".into(),
            None,
            None,
            None,
            plantool_core::PermissionMode::Ask,
            plantool_core::ImplementationMode::AllAtOnce,
            None,
        )
        .unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while live.run(&run.id).unwrap().status != plantool_core::RunStatus::Idle {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    live.propose_revision("Scope needs review".into()).unwrap();
    let route = format!("/api/sessions/{key}/comments");
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({"doc":"plan","scope":"document","body":"Discuss this revision"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(std::fs::read_to_string(&inputs).unwrap().lines().count(), 1);
    assert!(h
        .state
        .runs
        .start(
            live.clone(),
            run.provider,
            run.stage,
            "resume",
            "Bypass".into(),
            None,
            None,
            run.provider_session_id.clone(),
            run.permission_mode,
            run.implementation_mode,
            Some(&live.run(&run.id).unwrap())
        )
        .is_err());
    h.state
        .runs
        .sweep_idle_for(&live, std::time::Duration::ZERO)
        .await;
    while h.state.runs.is_live(&run.id) {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(live.run(&run.id).unwrap().idle_stopped);
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({"doc":"plan","scope":"document","body":"More review feedback"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(live.runs().len(), 1);
    assert_eq!(live.state().comments.len(), 2);
    live.set_stage(Stage::Approved, Actor::Human).unwrap();
    let (status, _) = call(
        &h.app,
        "POST",
        &route,
        Some(json!({"doc":"plan","scope":"document","body":"Accepted, continue"})),
        true,
        "127.0.0.1",
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let next = live.runs().into_iter().find(|r| r.id != run.id).unwrap();
    assert_eq!(next.provider_session_id.as_deref(), Some("gated-session"));
    let _ = h
        .state
        .runs
        .send(&next.id, plantool_daemon::providers::RunInput::Stop)
        .await;
}
