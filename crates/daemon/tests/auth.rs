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
        "# Plan\n- [ ] T1: task\n",
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

#[tokio::test]
async fn deletion_aliases_cannot_bypass_passkey_confirmation() {
    let h = harness();
    let key = session(&h, "aliases").await;
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let comments = format!("/api/sessions/{key}/comments");
    let created = value(
        raw(
            &h.app,
            "POST",
            &comments,
            json!({"doc":"plan","line":1,"body":"Owner blocker","type":"blocker"}),
            Some(&browser.cookie),
            None,
        )
        .await,
    )
    .await;
    let id = created["comments"][0]["id"].clone();
    let body = json!({"ids":[id]});
    assert_eq!(
        raw(&h.app, "DELETE", &comments, body.clone(), None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        raw(
            &h.app,
            "DELETE",
            &comments,
            body.clone(),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser
        .proof(&h.app, "DELETE", &comments, body.clone())
        .await;
    assert_eq!(
        raw(
            &h.app,
            "DELETE",
            &comments,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let remove = format!("/api/sessions/{key}/remove?worktree=true");
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &remove,
            Value::Null,
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    assert!(h.state.registry.get_key(&key).is_some());
    let proof = browser.proof(&h.app, "POST", &remove, Value::Null).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &remove,
            Value::Null,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn run_policy_changes_require_passkeys_and_failed_starts_do_not_change_pause_rules() {
    let h = harness();
    let key = session(&h, "policy").await;
    let route = format!("/api/sessions/{key}/runs");
    let s = h.state.registry.get_key(&key).unwrap();
    s.set_stage(plantool_core::Stage::Approved, plantool_core::Actor::Human)
        .unwrap();
    for policy in [
        json!({"permission_mode":"allow-all"}),
        json!({"pause_rule":{"mode":"no-pauses"}}),
        json!({"permission_mode":"auto","resume_run":"missing"}),
    ] {
        let mut body = json!({"provider":"codex","stage":"implement","worktree":false});
        body.as_object_mut()
            .unwrap()
            .extend(policy.as_object().unwrap().clone());
        assert_eq!(
            raw(&h.app, "POST", &route, body, None, None).await.status(),
            StatusCode::FORBIDDEN
        );
    }
    let safe = json!({"provider":"codex","stage":"implement","permission_mode":"ask","resume_run":"missing","worktree":false});
    assert_eq!(
        raw(&h.app, "POST", &route, safe, None, None).await.status(),
        StatusCode::BAD_REQUEST,
        "ordinary safe starts reach request validation"
    );
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let body = json!({"provider":"codex","stage":"implement","pause_rule":{"mode":"every-milestone"},"resume_run":"missing","worktree":false});
    let before = serde_json::to_value(s.session().pause_rule).unwrap();
    let proof = browser.proof(&h.app, "POST", &route, body.clone()).await;
    let response = raw(
        &h.app,
        "POST",
        &route,
        body,
        Some(&browser.cookie),
        Some(&proof),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::to_value(s.session().pause_rule).unwrap(),
        before
    );
}

#[tokio::test]
async fn query_strings_and_untracked_name_boundaries_cannot_weaken_workspace_binding() {
    let h = harness();
    let key = session(&h, "workspace").await;
    let s = h.state.registry.get_key(&key).unwrap();
    let cwd = s.session().cwd().clone();
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    for endpoint in ["pr?x=1", "pr/commit?x=1"] {
        let route = format!("/api/sessions/{key}/{endpoint}");
        let body = json!({"message":"commit","head":"workspace","repository":"owner/repo","title":"title","body":"body","draft":true});
        std::fs::write(cwd.join("a"), "bc").unwrap();
        let proof = browser.proof(&h.app, "POST", &route, body.clone()).await;
        std::fs::remove_file(cwd.join("a")).unwrap();
        std::fs::write(cwd.join("ab"), "c").unwrap();
        assert_eq!(
            raw(
                &h.app,
                "POST",
                &route,
                body,
                Some(&browser.cookie),
                Some(&proof)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        std::fs::remove_file(cwd.join("ab")).unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_targets_are_signed_without_following_links() {
    use std::os::unix::fs::symlink;
    let h = harness();
    let key = session(&h, "symlinks").await;
    let s = h.state.registry.get_key(&key).unwrap();
    let cwd = s.session().cwd().clone();
    symlink("missing-file", cwd.join("dangling")).unwrap();
    symlink(".", cwd.join("directory-link")).unwrap();
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let route = format!("/api/sessions/{key}/pr/commit?query=yes");
    let body = json!({"message":"links"});
    let proof = browser.proof(&h.app, "POST", &route, body.clone()).await;
    std::fs::remove_file(cwd.join("dangling")).unwrap();
    symlink("different-missing-file", cwd.join("dangling")).unwrap();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &route,
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
async fn provider_output_does_not_invalidate_unrelated_confirmations() {
    let h = harness();
    let key = session(&h, "output").await;
    let s = h.state.registry.get_key(&key).unwrap();
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let route = format!("/api/sessions/{key}/where");
    let body = json!({"difftool":"built-in"});
    let proof = browser.proof(&h.app, "POST", &route, body.clone()).await;
    let run = serde_json::from_value(json!({"id":"background","provider":"codex","stage":"new","cwd":s.session().cwd(),"status":"running","started_at":plantool_core::now()})).unwrap();
    s.upsert_run(run, |run| plantool_daemon::events::LiveEvent::RunStarted {
        run,
    })
    .unwrap();
    s.append_run_event(
        "background",
        1,
        json!({"type":"text","text":"ordinary output"}),
    )
    .unwrap();
    assert_eq!(
        raw(
            &h.app,
            "POST",
            &route,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[cfg(unix)]
#[tokio::test]
async fn pr_publication_rechecks_after_preflight_and_pushes_the_signed_sha() {
    use std::os::unix::fs::PermissionsExt;
    fn git(cwd: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().into()
    }
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("PLANTOOL_GH_BIN", value),
                None => std::env::remove_var("PLANTOOL_GH_BIN"),
            }
        }
    }
    let _restore = Restore(std::env::var_os("PLANTOOL_GH_BIN"));
    for during_preflight in [true, false] {
        let h = harness();
        let key = session(&h, "publish").await;
        let live = h.state.registry.get_key(&key).unwrap();
        let cwd = live.session().cwd().clone();
        git(&cwd, &["switch", "-qc", "feature"]);
        git(&cwd, &["commit", "--allow-empty", "-qm", "approved"]);
        let approved = git(&cwd, &["rev-parse", "HEAD"]);
        live.set_stage(
            plantool_core::Stage::ImplementationReview,
            plantool_core::Actor::Human,
        )
        .unwrap();
        let bare = h.tmp.path().join("remote.git");
        std::fs::create_dir(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare"]);
        git(&cwd, &["remote", "add", "origin", bare.to_str().unwrap()]);
        let marker = h.tmp.path().join("created");
        let script = h.tmp.path().join("gh-stub");
        let race = if during_preflight {
            "git -c core.hooksPath=/dev/null commit --allow-empty -qm concurrent"
        } else {
            ":"
        };
        std::fs::write(&script, format!(r#"#!/bin/sh
if [ "$1 $2" = "repo view" ]; then {race}; echo owner/repo; exit 0; fi
if [ "$1 $2" = "pr list" ]; then
if [ -e '{}' ]; then echo '[{{"number":23,"url":"https://github.com/owner/repo/pull/23","state":"OPEN","isDraft":true,"headRefName":"feature","baseRefName":"main"}}]'; else echo '[]'; fi
exit 0
fi
if [ "$1 $2" = "pr create" ]; then touch '{}'; exit 0; fi
exit 1
"#, marker.display(), marker.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("PLANTOOL_GH_BIN", &script);
        if !during_preflight {
            let hook = cwd.join(".git/hooks/pre-push");
            std::fs::write(
                &hook,
                "#!/bin/sh\ngit -c core.hooksPath=/dev/null commit --allow-empty -qm concurrent\n",
            )
            .unwrap();
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut browser = Browser::register(&h.app, &h.state.config.home).await;
        let path = format!("/api/sessions/{key}/pr");
        let body = json!({"head":"feature","repository":"owner/repo","title":"Approved","body":"Reviewed","draft":true});
        let proof = browser.proof(&h.app, "POST", &path, body.clone()).await;
        let response = raw(
            &h.app,
            "POST",
            &path,
            body,
            Some(&browser.cookie),
            Some(&proof),
        )
        .await;
        let status = response.status();
        let result = value(response).await;
        assert_ne!(git(&cwd, &["rev-parse", "HEAD"]), approved);
        if during_preflight {
            assert_eq!(status, StatusCode::CONFLICT, "{result}");
            assert!(!marker.exists());
            let output = std::process::Command::new("git")
                .args(["show-ref", "--verify", "refs/heads/feature"])
                .current_dir(&bare)
                .output()
                .unwrap();
            assert!(!output.status.success());
        } else {
            assert_eq!(status, StatusCode::OK, "{result}");
            assert_eq!(git(&bare, &["rev-parse", "refs/heads/feature"]), approved);
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn bootstrap_database_write_requires_the_owner_socket_credential() {
    let mut h = harness();
    h.state.runs = Arc::new(plantool_daemon::runs::RunManager::load(&h.state.config.home).unwrap());
    let app = routes::router(h.state.clone());
    let socket = plantool_daemon::socket::router(h.state.clone());
    let credential =
        std::fs::read_to_string(h.state.config.home.join("security/lifecycle-token")).unwrap();
    let request = |credential: &str, origin: bool| {
        let mut request = Request::builder()
            .method("POST")
            .uri("/owner/setup-code")
            .header("host", "localhost:41300")
            .header("x-plantool-lifecycle", credential);
        if origin {
            request = request.header("origin", "http://localhost:41300");
        }
        request.body(Body::empty()).unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request(&credential, false))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        socket
            .clone()
            .oneshot(request("forged", false))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        socket
            .clone()
            .oneshot(request(&credential, true))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = socket
        .clone()
        .oneshot(request(&credential, false))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(value(response).await["code"].as_str().is_some());
    Browser::register(&app, &h.state.config.home).await;
    assert!(!socket
        .oneshot(request(&credential, false))
        .await
        .unwrap()
        .status()
        .is_success());
}
#[cfg(unix)]
#[tokio::test]
async fn worker_identity_is_socket_only_and_repair_needs_a_fresh_bound_assertion() {
    let h = harness();
    let socket = plantool_daemon::socket::router(h.state.clone());
    let mut command = tokio::process::Command::new("/bin/sh");
    command.args(["-c", "IFS= read -r value; printf '%s' \"$value\""]);
    let _credential = h.state.runs.tokens.worker_command(&mut command).unwrap();
    let secret = String::from_utf8(command.output().await.unwrap().stdout).unwrap();
    let request = || {
        Request::builder()
            .uri("/internal/v1/health")
            .header("host", "localhost:41300")
            .header("x-plantool-run-token", &secret)
            .header(
                "x-plantool-contract-version",
                plantool_contract::CONTRACT_VERSION.to_string(),
            )
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        h.app.clone().oneshot(request()).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        socket.oneshot(request()).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        raw(
            &h.app,
            "GET",
            "/internal/v1/health",
            json!(null),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
    let path = "/api/engineering-os/repair";
    assert_eq!(
        raw(&h.app, "POST", path, json!({}), Some(&browser.cookie), None)
            .await
            .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser.proof(&h.app, "POST", path, json!({})).await;
    h.state
        .runs
        .execution
        .stop_all(&h.state.runs.tokens)
        .await
        .unwrap();
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
        StatusCode::FORBIDDEN
    );
    let config = "/api/settings/models";
    assert_eq!(
        raw(&h.app, "GET", config, json!(null), None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let settings = value(
        raw(
            &h.app,
            "GET",
            config,
            json!(null),
            Some(&browser.cookie),
            None,
        )
        .await,
    )
    .await;
    let body = json!({"yaml":settings["yaml"],"project":null,"allowed_providers":null});
    assert_eq!(
        raw(
            &h.app,
            "POST",
            config,
            body.clone(),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser.proof(&h.app, "POST", config, body.clone()).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            config,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn github_settings_are_browser_only_and_assertions_bind_the_allowlist() {
    let h = harness();
    let path = "/api/settings/github/allowlist";
    let body = json!({"login":"amritghimire","remove":false});
    assert_eq!(
        raw(
            &h.app,
            "GET",
            "/api/settings/github",
            json!(null),
            None,
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let mut browser = Browser::register(&h.app, &h.state.config.home).await;
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
    let db = plantool_daemon::database::open(&h.state.config.home).unwrap();
    db.execute("INSERT INTO allowlist VALUES(7,'other','now','test')", [])
        .unwrap();
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
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn github_preflight_refresh_requires_a_fresh_assertion() {
    let h = harness();
    let browser = Browser::register(&h.app, &h.state.config.home).await;
    assert_eq!(
        raw(
            &h.app,
            "POST",
            "/api/settings/github/preflight",
            json!({"project":"p"}),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
}
