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
    let stopped = plantool_core::Run { id: "r1".into(), provider: plantool_core::Provider::Claude, provider_session_id: None, stage: plantool_core::Stage::Researching, cwd: h.repo.clone(), status: plantool_core::RunStatus::Stopped, model: None, permission_mode: Default::default(), started_at: plantool_core::now(), ended_at: None, error: None, seq: 0 };
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
