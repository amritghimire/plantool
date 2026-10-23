mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use common::{raw, value, Browser};
use plantool_daemon::{
    auth::Auth,
    identities::{RunCredential, RunScope},
    routes, AppState, Config, Registry,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

struct Harness {
    app: Router,
    state: AppState,
    tmp: tempfile::TempDir,
}
fn harness() -> Harness {
    #[cfg(unix)]
    let tmp = tempfile::tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let config = Arc::new(Config {
        home: home.clone(),
        port: 41300,
        version: "test".into(),
    });
    let (tx, _) = tokio::sync::mpsc::channel(1);
    let state = AppState {
        auth: Arc::new(Auth::new(&home, 41300).unwrap()),
        registry: Arc::new(Registry::load(home).unwrap()),
        config,
        runs: Arc::new(Default::default()),
        shutdown: tx,
    };
    Harness {
        app: routes::router(state.clone()),
        state,
        tmp,
    }
}
async fn session(h: &Harness, repo: &str, slug: &str) -> String {
    let root = h.tmp.path().join(repo);
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
        vec!["commit", "--allow-empty", "-qm", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
    }
    let result = value(
        raw(
            &h.app,
            "POST",
            "/api/sessions",
            json!({"repo":root,"slug":slug}),
            None,
            None,
        )
        .await,
    )
    .await;
    result["session"]["key"].as_str().unwrap().to_string()
}
fn token(credential: &RunCredential, h: &Harness) -> String {
    let mut command = tokio::process::Command::new("unused");
    credential
        .environment(h.state.config.home.clone(), 41300)
        .apply(&mut command);
    command
        .as_std()
        .get_envs()
        .find(|(k, _)| *k == "PLANTOOL_RUN_TOKEN")
        .unwrap()
        .1
        .unwrap()
        .to_str()
        .unwrap()
        .into()
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: &str,
    cookie: Option<&str>,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:41300")
        .header("origin", "http://localhost:41300")
        .header("content-type", "application/json")
        .header("x-plantool-run-token", token);
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn run_tokens_are_scoped_fail_closed_and_never_become_human() {
    let h = harness();
    let own = session(&h, "repo", "one").await;
    let other_item = session(&h, "repo", "two").await;
    let other_project = session(&h, "elsewhere", "one").await;
    let lease = h.state.runs.tokens.issue(RunScope {
        session: own.clone(),
        attempt: "attempt-one".into(),
    });
    let secret = token(&lease, &h);
    for path in [format!("/api/sessions/{own}"), "/api/sessions/one".into()] {
        let r = call(&h.app, "GET", &path, Value::Null, &secret, None).await;
        // The short name is ambiguous across projects and must not pick a target.
        assert_eq!(
            r.status(),
            if path.ends_with("/one") && path.matches('/').count() == 3 {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::OK
            }
        );
    }
    for target in [&other_item, &other_project] {
        for (method, suffix, body) in [
            ("GET", "", Value::Null),
            ("POST", "/brief", json!({"brief":"unauthorized"})),
        ] {
            assert_eq!(
                call(
                    &h.app,
                    method,
                    &format!("/api/sessions/{target}{suffix}"),
                    body,
                    &secret,
                    None
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    let own_session = h.state.registry.get_key(&own).unwrap();
    std::fs::write(
        own_session.doc_path(plantool_core::DocKind::Plan),
        "own plan",
    )
    .unwrap();
    own_session
        .capture_doc(plantool_core::DocKind::Plan)
        .unwrap();
    let sha = own_session.doc(plantool_core::DocKind::Plan).unwrap().sha;
    assert_eq!(
        call(
            &h.app,
            "GET",
            &format!("/api/sessions/{own}/docs/plan?sha={sha}"),
            Value::Null,
            &secret,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let sibling = h.state.registry.get_key(&other_item).unwrap();
    std::fs::write(
        sibling.doc_path(plantool_core::DocKind::Plan),
        "sibling plan",
    )
    .unwrap();
    for selector in [
        "../../../two/plan",
        "..%2F..%2F..%2Ftwo%2Fplan",
        "%2Ftmp%2Foutside",
        "invalid",
    ] {
        assert_eq!(
            call(
                &h.app,
                "GET",
                &format!("/api/sessions/{own}/docs/plan?sha={selector}"),
                Value::Null,
                &secret,
                None
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    let path = format!("/api/sessions/{own}/brief");
    assert_eq!(
        call(
            &h.app,
            "POST",
            &path,
            json!({"brief":"my progress"}),
            &secret,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let browser = Browser::register(&h.app, &h.state.config.home).await;
    for (method, path, body) in [
        (
            "POST",
            format!("/api/sessions/{own}/stage"),
            json!({"to":"approved"}),
        ),
        (
            "POST",
            format!("/api/sessions/{own}/where"),
            json!({"difftool":"built-in","confirm":true}),
        ),
        (
            "POST",
            "/api/settings/difftool".into(),
            json!({"value":"built-in"}),
        ),
        ("GET", "/api/auth/status".into(), Value::Null),
        ("POST", "/shutdown".into(), Value::Null),
        (
            "POST",
            format!("/api/sessions/{own}/runs"),
            json!({"provider":"codex","stage":"research"}),
        ),
        (
            "GET",
            format!("/api/sessions/{own}/runs/another-attempt/events"),
            Value::Null,
        ),
        ("GET", "/api/sessions".into(), Value::Null),
    ] {
        assert_eq!(
            call(&h.app, method, &path, body, &secret, Some(&browser.cookie))
                .await
                .status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    drop(lease);
    for token in [&secret, "invalid", ""] {
        assert_eq!(
            call(
                &h.app,
                "GET",
                &format!("/api/sessions/{own}"),
                Value::Null,
                token,
                None
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    // Missing credentials intentionally preserve anonymous Personal access.
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &path,
            json!({"brief":"external executor"}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let fresh = h.state.runs.tokens.issue(RunScope {
        session: own.clone(),
        attempt: "resumed-attempt".into(),
    });
    let fresh_secret = token(&fresh, &h);
    assert_ne!(secret, fresh_secret);
    assert_eq!(
        call(
            &h.app,
            "GET",
            &format!("/api/sessions/{own}"),
            Value::Null,
            &fresh_secret,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    h.state.runs.tokens.revoke_all();
    assert_eq!(
        call(
            &h.app,
            "GET",
            &format!("/api/sessions/{own}"),
            Value::Null,
            &fresh_secret,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[cfg(unix)]
#[tokio::test]
async fn socket_transport_is_private_agent_only_and_revokes_live_streams() {
    use futures::StreamExt;
    use std::os::unix::fs::PermissionsExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let h = harness();
    let key = session(&h, "repo", "watch").await;
    let browser = Browser::register(&h.app, &h.state.config.home).await;
    let (listener, file) = plantool_daemon::socket::bind(&h.state.config.home)
        .await
        .unwrap();
    let path = plantool_daemon::socket::path(&h.state.config.home);
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(plantool_daemon::socket::bind(&h.state.config.home)
        .await
        .is_err());
    let app = plantool_daemon::socket::router(h.state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let http = reqwest::Client::builder()
        .no_proxy()
        .unix_socket(path.clone())
        .build()
        .unwrap();
    let r = http
        .post(format!("http://localhost:41300/api/sessions/{key}/stage"))
        .header("origin", "http://localhost:41300")
        .header("cookie", browser.cookie)
        .json(&json!({"to":"approved"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let mut command = tokio::process::Command::new("sh");
    command.args(["-c", "IFS= read -r token <&0; printf '%s' \"$token\""]);
    let worker = h.state.runs.tokens.worker_command(&mut command).unwrap();
    let output = command.output().await.unwrap();
    let worker_secret = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        call(&h.app, "GET", "/health", Value::Null, &worker_secret, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        http.get("http://localhost:41300/health")
            .header("x-plantool-run-token", &worker_secret)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        http.get(format!("http://localhost:41300/api/sessions/{key}"))
            .header("x-plantool-run-token", &worker_secret)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    drop(worker);
    assert_eq!(
        http.get("http://localhost:41300/health")
            .header("x-plantool-run-token", &worker_secret)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let lease = h.state.runs.tokens.issue(RunScope {
        session: key.clone(),
        attempt: "watcher".into(),
    });
    let secret = token(&lease, &h);
    let mut request = format!("ws://localhost:41300/api/sessions/{key}/live")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("x-plantool-run-token", secret.parse().unwrap());
    let socket = tokio::net::UnixStream::connect(&path).await.unwrap();
    let (mut ws, _) = tokio_tungstenite::client_async(request, socket)
        .await
        .unwrap();
    assert!(ws
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap()
        .contains("hello"));
    drop(lease);
    let closed = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
        .await
        .unwrap();
    assert!(closed.is_none() || closed.unwrap().is_err());
    server.abort();
    drop(file);
    assert!(!path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn stale_socket_recovery_never_unlinks_regular_files_or_symlinks() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir(home.join("run")).unwrap();
    let path = plantool_daemon::socket::path(home);
    let old = std::os::unix::net::UnixListener::bind(&path).unwrap();
    drop(old);
    let (listener, file) = plantool_daemon::socket::bind(home).await.unwrap();
    drop(listener);
    drop(file);
    std::fs::write(&path, "keep").unwrap();
    assert!(plantool_daemon::socket::bind(home).await.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
    std::fs::remove_file(&path).unwrap();
    let target = home.join("target");
    std::fs::write(&target, "keep").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(plantool_daemon::socket::bind(home).await.is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
}

#[cfg(unix)]
#[tokio::test]
async fn hosted_provider_tokens_are_injected_and_revoked_on_exit_failure_or_stop() {
    use std::os::unix::fs::PermissionsExt;
    let h = harness();
    let key = session(&h, "repo", "hosted").await;
    let session = h.state.registry.get_key(&key).unwrap();
    let script = h.tmp.path().join("fake-claude");
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("PLANTOOL_CLAUDE_PATH", v),
                None => std::env::remove_var("PLANTOOL_CLAUDE_PATH"),
            }
        }
    }
    let _restore = Restore(std::env::var_os("PLANTOOL_CLAUDE_PATH"));
    std::env::set_var("PLANTOOL_CLAUDE_PATH", &script);
    let mut previous = String::new();
    for exit in [0, 1, 2] {
        let token_file = h.tmp.path().join(format!("token-{exit}"));
        let release = h.tmp.path().join(format!("release-{exit}"));
        std::fs::write(&script, format!("#!/bin/sh\nprintf '%s' \"$PLANTOOL_RUN_TOKEN\" > '{}'\ntest \"$PLANTOOL_HOME\" = '{}' || exit 3\ni=0\nwhile [ ! -f '{}' ] && [ $i -lt 1000 ]; do sleep 0.01; i=$((i+1)); done\nexit {exit}\n", token_file.display(), h.state.config.home.display(), release.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let run = h
            .state
            .runs
            .start(
                session.clone(),
                plantool_core::Provider::Claude,
                plantool_core::Stage::Researching,
                "research",
                "test".into(),
                None,
                None,
                None,
                plantool_core::PermissionMode::Ask,
                plantool_core::ImplementationMode::AllAtOnce,
                None,
            )
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !token_file.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let secret = std::fs::read_to_string(&token_file).unwrap();
        assert!(!secret.is_empty());
        assert_ne!(secret, previous);
        assert_eq!(
            call(
                &h.app,
                "GET",
                &format!("/api/sessions/{key}"),
                Value::Null,
                &secret,
                None
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert!(!serde_json::to_string(&run).unwrap().contains(&secret));
        if exit == 2 {
            h.state
                .runs
                .send(&run.id, plantool_daemon::providers::RunInput::Stop)
                .await
                .unwrap();
        } else {
            std::fs::write(release, "").unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while h.state.runs.is_live(&run.id) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            call(
                &h.app,
                "GET",
                &format!("/api/sessions/{key}"),
                Value::Null,
                &secret,
                None
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        previous = secret;
    }
    std::fs::remove_file(script).unwrap();
    let run = h
        .state
        .runs
        .start(
            session.clone(),
            plantool_core::Provider::Claude,
            plantool_core::Stage::Researching,
            "research",
            "test".into(),
            None,
            None,
            None,
            plantool_core::PermissionMode::Ask,
            plantool_core::ImplementationMode::AllAtOnce,
            None,
        )
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while h.state.runs.is_live(&run.id) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        session.run(&run.id).unwrap().status,
        plantool_core::RunStatus::Failed
    );
}

#[tokio::test]
async fn stop_all_is_browser_only_revokes_tokens_and_resume_requires_a_fresh_bound_assertion() {
    let h = harness();
    let key = session(&h, "stop-repo", "one").await;
    let lease = h.state.runs.tokens.issue(RunScope {
        session: key.clone(),
        attempt: "stop-test".into(),
    });
    let secret = token(&lease, &h);
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    assert_eq!(
        call(
            &h.app,
            "POST",
            "/api/execution/stop",
            json!({}),
            &secret,
            Some(&browser.cookie)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/execution/stop",
            json!({}),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert!(h.state.runs.execution.status().stopped);
    assert_eq!(
        call(
            &h.app,
            "GET",
            &format!("/api/sessions/{key}"),
            Value::Null,
            &secret,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let path = "/api/execution/resume";
    assert_eq!(
        raw(&h.app, "POST", path, json!({}), Some(&browser.cookie), None)
            .await
            .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let stale = browser.proof(&h.app, "POST", path, json!({})).await;
    raw(
        &h.app,
        "POST",
        "/api/execution/stop",
        json!({}),
        Some(&browser.cookie),
        None,
    )
    .await;
    assert!(!raw(
        &h.app,
        "POST",
        path,
        json!({}),
        Some(&browser.cookie),
        Some(&stale)
    )
    .await
    .status()
    .is_success());
    assert!(h.state.runs.execution.status().stopped);
    let proof = browser.proof(&h.app, "POST", path, json!({})).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            path,
            json!({}),
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert!(!h.state.runs.execution.status().stopped);
    assert!(!raw(
        &h.app,
        "POST",
        path,
        json!({}),
        Some(&browser.cookie),
        Some(&proof)
    )
    .await
    .status()
    .is_success());
}

#[tokio::test]
async fn never_sent_changes_require_stopped_execution_and_a_fresh_assertion() {
    let h = harness();
    session(&h, "policy-repo", "one").await;
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let path = "/api/settings/never-sent";
    let body = json!({"repo":h.tmp.path().join("policy-repo"),"patterns":["private/**"]});
    let proof = browser.proof(&h.app, "POST", path, body.clone()).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            path,
            body.clone(),
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    raw(
        &h.app,
        "POST",
        "/api/execution/stop",
        json!({}),
        Some(&browser.cookie),
        None,
    )
    .await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            path,
            body.clone(),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser.proof(&h.app, "POST", path, body.clone()).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            path,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        h.state
            .runs
            .execution
            .rules(&h.tmp.path().join("policy-repo").canonicalize().unwrap()),
        vec!["private/**"]
    );
}
