mod common;
use axum::http::StatusCode;
use common::{raw, value, Browser};
use plantool_core::DocKind;
use plantool_daemon::{auth::Auth, database, routes, AppState, Config, Registry};
use serde_json::json;
use std::sync::Arc;
#[tokio::test]
async fn personal_approvals_pin_immutable_content_and_progress_survives_restart() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let repo = t.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["config", "user.name", "test"],
        vec!["commit", "--allow-empty", "-qm", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .current_dir(&repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let registry = Arc::new(Registry::load(home.clone()).unwrap());
    let (tx, _) = tokio::sync::mpsc::channel(1);
    let state = AppState {
        registry: registry.clone(),
        config: Arc::new(Config {
            home: home.clone(),
            port: 41300,
            version: "test".into(),
        }),
        auth: Arc::new(Auth::new(&home, 41300).unwrap()),
        runs: Arc::new(Default::default()),
        shutdown: tx,
    };
    let app = routes::router(state.clone());
    let created = value(
        raw(
            &app,
            "POST",
            "/api/sessions",
            json!({"repo":repo,"slug":"work"}),
            None,
            None,
        )
        .await,
    )
    .await;
    let key = created["session"]["key"].as_str().unwrap();
    let live = registry.get_key(key).unwrap();
    std::fs::write(
        live.doc_path(DocKind::Research),
        "# Research\nValidated locally.\n",
    )
    .unwrap();
    live.capture_doc(DocKind::Research).unwrap();
    let original = "# Plan\n### Build\n- [ ] T1: Build it\n- [ ] T2: Test it\n";
    std::fs::write(live.doc_path(DocKind::Plan), original).unwrap();
    live.capture_doc(DocKind::Plan).unwrap();
    let sha = live.doc(DocKind::Plan).unwrap().sha;
    let mut browser = Browser::register(&app, &home).await;
    let stage = format!("/api/sessions/{key}/stage");
    let body = json!({"to":"approved","plan_sha":sha});
    let proof = browser.proof(&app, "POST", &stage, body.clone()).await;
    assert_eq!(
        raw(
            &app,
            "POST",
            &stage,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        raw(
            &app,
            "POST",
            &stage,
            json!({"to":"implementing"}),
            None,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let task = format!("/api/sessions/{key}/tasks/T1/done");
    assert_eq!(
        raw(&app, "POST", &task, json!({}), None, None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(live.state().approved.unwrap().sha, sha);
    assert_eq!(
        live.doc_revision(DocKind::Plan, &sha)
            .unwrap()
            .unwrap()
            .content,
        original
    );
    assert_eq!(
        plantool_daemon::artifacts::read(&home, &sha).unwrap(),
        original.as_bytes()
    );
    std::fs::write(
        live.doc_path(DocKind::Plan),
        original.replace("[ ] T1", "[x] T1"),
    )
    .unwrap();
    assert!(live.capture_doc(DocKind::Plan).unwrap().is_none());
    assert!(!live.state().plan_revision_pending);
    let doc = value(
        raw(
            &app,
            "GET",
            &format!("/api/sessions/{key}/docs/plan"),
            json!(null),
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(doc["sha"], sha);
    assert!(doc["display_content"].as_str().unwrap().contains("[x] T1"));
    assert_eq!(doc["content"], original);
    let restarted = Registry::load(home.clone()).unwrap();
    let restored = restarted.get_key(key).unwrap();
    assert_eq!(
        restored.store.task_progress().unwrap().get("T1"),
        Some(&true)
    );
    std::fs::write(
        live.doc_path(DocKind::Plan),
        original.replace("Build it", "Build something else"),
    )
    .unwrap();
    live.capture_doc(DocKind::Plan).unwrap();
    assert!(live.state().plan_revision_pending);
    assert_eq!(
        raw(&app, "POST", &task, json!({}), None, None)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let next = live.doc(DocKind::Plan).unwrap().sha;
    let body = json!({"to":"approved","plan_sha":next});
    let proof = browser.proof(&app, "POST", &stage, body.clone()).await;
    assert_eq!(
        raw(
            &app,
            "POST",
            &stage,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(live.store.task_progress().unwrap().get("T1"), Some(&false));
    let mut run: plantool_core::Run =
        serde_json::from_str(include_str!("../../core/tests/fixtures/v0.0.14/run.json")).unwrap();
    run.id = "pin-fixture".into();
    live.upsert_run(run.clone(), |run| {
        plantool_daemon::events::LiveEvent::RunStarted { run }
    })
    .unwrap();
    let pin = format!("/api/sessions/{key}/runs/pin-fixture/pin");
    let body = json!({"pinned":true});
    assert_eq!(
        raw(&app, "POST", &pin, body.clone(), None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        raw(
            &app,
            "POST",
            &pin,
            body.clone(),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser.proof(&app, "POST", &pin, body.clone()).await;
    assert_eq!(
        raw(
            &app,
            "POST",
            &pin,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    live.upsert_run(run, |run| plantool_daemon::events::LiveEvent::RunUpdated {
        run,
    })
    .unwrap();
    assert!(live.run("pin-fixture").unwrap().pinned);
    let retention = "/api/settings/retention";
    let body = json!({"transcript_days":45,"evidence_days":90,"disk_cap_bytes":21474836480u64});
    assert_eq!(
        raw(
            &app,
            "POST",
            retention,
            body.clone(),
            Some(&browser.cookie),
            None
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    let proof = browser.proof(&app, "POST", retention, body.clone()).await;
    assert_eq!(
        raw(
            &app,
            "POST",
            retention,
            body,
            Some(&browser.cookie),
            Some(&proof)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        plantool_daemon::retention::policy(&home)
            .unwrap()
            .transcript_days,
        45
    );
    let db = database::open(&home).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM plan_tasks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert!(db
        .execute("UPDATE revisions SET content_sha256='invalid'", [])
        .is_err());
    assert!(!live.store.dir.join("meta.json").exists());
    assert!(!live.store.dir.join("state.json").exists());
}
