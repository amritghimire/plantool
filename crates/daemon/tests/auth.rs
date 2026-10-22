mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use common::{raw, value, Browser};
use plantool_daemon::{
    auth::{setup_code, Auth},
    routes, AppState, Config, Registry,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
use tower::ServiceExt;
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::Url;
struct Harness {
    app: Router,
    state: AppState,
    tmp: tempfile::TempDir,
}
fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
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
async fn session(h: &Harness, slug: &str) -> String {
    let repo = h.tmp.path().join(slug);
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
        vec!["commit", "--allow-empty", "-qm", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .status()
            .unwrap()
            .success());
    }
    let result = value(
        raw(
            &h.app,
            "POST",
            "/api/sessions",
            json!({"slug":slug,"repo":repo}),
            None,
            None,
        )
        .await,
    )
    .await;
    let key = result["session"]["key"].as_str().unwrap().to_string();
    let s = h.state.registry.get_key(&key).unwrap();
    std::fs::write(
        s.doc_path(plantool_core::DocKind::Plan),
        "# Plan\n- [ ] task\n",
    )
    .unwrap();
    s.capture_doc(plantool_core::DocKind::Plan).unwrap();
    key
}
#[tokio::test]
async fn served_html_contains_no_secret_and_agents_cannot_approve() {
    let h = harness();
    let key = session(&h, "work").await;
    let path = format!("/api/sessions/{key}/stage");
    let r = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "localhost:41300")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    use http_body_util::BodyExt;
    let html =
        String::from_utf8(r.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(!html.contains("__PLANTOOL_TOKEN__"));
    assert!(!html.contains("plantool_session"));
    assert!(!html.contains("one-time"));
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    assert_eq!(
        raw(&h.app, "POST", &path, json!({"to":"approved"}), None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let proof = browser
        .proof(&h.app, "POST", &path, json!({"to":"approved"}))
        .await;
    for header in ["authorization", "x-plantool-run-token"] {
        let req = Request::builder()
            .method("POST")
            .uri(&path)
            .header("host", "localhost:41300")
            .header("origin", "http://localhost:41300")
            .header("content-type", "application/json")
            .header("cookie", &browser.cookie)
            .header("x-plantool-assertion", &proof)
            .header(header, "Bearer agent-run-token")
            .body(Body::from(r#"{"to":"approved"}"#))
            .unwrap();
        assert_eq!(
            h.app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_ne!(
        h.state.registry.get_key(&key).unwrap().stage(),
        plantool_core::Stage::Approved
    );
}
#[tokio::test]
async fn approvals_require_fresh_assertions_bound_to_action_target_and_payload() {
    let h = harness();
    let a = session(&h, "first").await;
    let b = session(&h, "second").await;
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let path = format!("/api/sessions/{a}/stage");
    let body = json!({"to":"approved"});
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &path,
            body.clone(),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser.proof(&h.app, "POST", &path, body.clone()).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &format!("/api/sessions/{b}/stage"),
            body.clone(),
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let proof = browser.proof(&h.app, "POST", &path, body.clone()).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &path,
            json!({"to":"done"}),
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let proof = browser.proof(&h.app, "POST", &path, body.clone()).await;
    let s = h.state.registry.get_key(&a).unwrap();
    std::fs::write(s.doc_path(plantool_core::DocKind::Plan), "# Changed plan\n").unwrap();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &path,
            body.clone(),
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    s.capture_doc(plantool_core::DocKind::Plan).unwrap();
    let proof = browser.proof(&h.app, "POST", &path, body.clone()).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &path,
            body.clone(),
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &path,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn first_registration_needs_setup_code_and_cannot_be_reopened() {
    let h = harness();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/start",
            json!({"code":"guessed"}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let old = setup_code(&h.state.config.home).unwrap();
    let _new = setup_code(&h.state.config.home).unwrap();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/start",
            json!({"code":old}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let browser = Browser::register(&h.app, &h.state.config.home).await;
    assert!(setup_code(&h.state.config.home).is_err());
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/start",
            json!({"code":"any"}),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let db = rusqlite::Connection::open(h.state.config.home.join("db/plantool.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM passkeys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM passkey_setup", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn login_signature_origin_and_user_verification_are_checked_and_logout_revokes_cookie() {
    let h = harness();
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let response = raw(
        &h.app,
        "POST",
        "/api/auth/logout",
        json!({}),
        Some(&browser.cookie),
        None,
    )
    .await;
    assert!(response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    assert_eq!(
        value(
            raw(
                &h.app,
                "GET",
                "/api/auth/status",
                Value::Null,
                Some(&browser.cookie),
                None
            )
            .await
        )
        .await["authenticated"],
        false
    );
    let start = value(
        raw(
            &h.app,
            "POST",
            "/api/auth/assertion/start",
            json!({}),
            None,
            None,
        )
        .await,
    )
    .await;
    let cred = browser
        .auth
        .do_authentication(
            Url::parse("http://localhost:41300").unwrap(),
            serde_json::from_value(start["options"].clone()).unwrap(),
        )
        .unwrap();
    let response = raw(
        &h.app,
        "POST",
        "/api/auth/assertion/finish",
        json!({"id":start["id"],"credential":cred}),
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let new_cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/assertion/finish",
            json!({"id":start["id"],"credential":cred}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let cross_origin = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header("host", "localhost:41300")
        .header("origin", "http://localhost:5273")
        .header("cookie", &new_cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        h.app.clone().oneshot(cross_origin).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    // A valid credential with a changed signed clientDataJSON must fail verification.
    let start = value(
        raw(
            &h.app,
            "POST",
            "/api/auth/assertion/start",
            json!({}),
            None,
            None,
        )
        .await,
    )
    .await;
    let mut cred = serde_json::to_value(
        browser
            .auth
            .do_authentication(
                Url::parse("http://localhost:41300").unwrap(),
                serde_json::from_value(start["options"].clone()).unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    cred["response"]["signature"] = json!("AAAA");
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/assertion/finish",
            json!({"id":start["id"],"credential":cred}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn adding_another_passkey_requires_existing_passkey_assertion() {
    let h = harness();
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let body = json!({"code":null});
    let proof = browser
        .proof(&h.app, "POST", "/api/auth/register/start", body.clone())
        .await;
    let start = value(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/start",
            body,
            Some(&browser.cookie),
            Some(&proof),
        )
        .await,
    )
    .await;
    let mut second = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential = second
        .do_registration(
            Url::parse("http://localhost:41300").unwrap(),
            common::registration_options(start["options"].clone()),
        )
        .unwrap();
    let result = raw(
        &h.app,
        "POST",
        "/api/auth/register/finish",
        json!({"id":start["id"],"credential":credential}),
        Some(&browser.cookie),
        None,
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let reloaded = Auth::new(Path::new(&h.state.config.home), 41300).unwrap();
    let _ = reloaded;
    let db = rusqlite::Connection::open(h.state.config.home.join("db/plantool.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM passkeys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn registration_without_user_verification_is_rejected() {
    let h = harness();
    let code = setup_code(&h.state.config.home).unwrap();
    let start = value(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/start",
            json!({"code":code}),
            None,
            None,
        )
        .await,
    )
    .await;
    let mut options = start["options"].clone();
    options["publicKey"]["authenticatorSelection"]["userVerification"] = json!("discouraged");
    let options = common::registration_options(options);
    let mut auth = WebauthnAuthenticator::new(SoftPasskey::new(false));
    let cred = auth
        .do_registration(Url::parse("http://localhost:41300").unwrap(), options)
        .unwrap();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/finish",
            json!({"id":start["id"],"credential":cred}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn expired_setup_code_and_anonymous_confirm_cannot_grant_authority() {
    let h = harness();
    let code = setup_code(&h.state.config.home).unwrap();
    let db = rusqlite::Connection::open(h.state.config.home.join("db/plantool.db")).unwrap();
    db.execute("UPDATE passkey_setup SET expires_at=0", [])
        .unwrap();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/auth/register/start",
            json!({"code":code}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    for query in ["confirm=true", "%63onfirm=%74rue"] {
        let path = format!("/api/import?repo=/tmp&{query}");
        assert_eq!(
            raw(&h.app, "POST", &path, json!({}), None, None)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}
