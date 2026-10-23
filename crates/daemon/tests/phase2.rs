use plantool_contract::{Dispatch, Outcome, RoleResult};
use plantool_core::{Actor, DocKind, Stage};
use plantool_daemon::{
    database, dispatcher,
    models::{gateway, lease, proxy, router},
    Registry,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    item: String,
    project: String,
    sha: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "test"],
            vec!["config", "user.email", "t@t"],
            vec!["commit", "--allow-empty", "-qm", "init"],
        ] {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success());
        }
        let registry = Registry::load(home.clone()).unwrap();
        let (live, _) = registry
            .create(serde_json::from_value(json!({"repo":root,"slug":"work"})).unwrap())
            .unwrap();
        std::fs::write(
            live.doc_path(DocKind::Plan),
            "# Plan\n### Phase A\n- [ ] T1: Build it\n",
        )
        .unwrap();
        live.capture_doc(DocKind::Plan).unwrap();
        live.set_stage(Stage::Approved, Actor::Human).unwrap();
        let sha = live.doc(DocKind::Plan).unwrap().sha;
        let item = live.store.key();
        let db = database::open(&home).unwrap();
        let project: String = db
            .query_row(
                "SELECT project_id FROM work_items WHERE id=?1",
                [&item],
                |r| r.get(0),
            )
            .unwrap();
        db.execute("UPDATE projects SET mode='eos' WHERE id=?1", [&project])
            .unwrap();
        db.execute("UPDATE work_items SET kind='eos' WHERE id=?1", [&item])
            .unwrap();
        Self {
            _temp: temp,
            home,
            root,
            item,
            project,
            sha,
        }
    }
    fn request(&self, generation: u64) -> Dispatch {
        Dispatch {
            item: self.item.clone(),
            role: "backend".into(),
            generation,
            inputs: json!({}),
            plan_sha: self.sha.clone(),
            policy_version: 0,
            capability: "write".into(),
            idempotency_key: format!("dispatch:{generation}"),
            thread_id: "project:item".into(),
            interrupt_id: "interrupt-1".into(),
        }
    }
    fn result(&self, id: &str, generation: u64) -> RoleResult {
        RoleResult {
            item: self.item.clone(),
            role: "backend".into(),
            attempt_id: id.into(),
            generation,
            outcome: Outcome::Done,
            summary: "done".into(),
            tasks_done: vec!["T1".into()],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
        }
    }
}
#[test]
fn durable_dispatch_validates_authority_and_never_duplicates() {
    let f = Fixture::new();
    let request = f.request(1);
    let reply = dispatcher::admit(&f.home, &request, 0).unwrap();
    assert_eq!(reply, dispatcher::admit(&f.home, &request, 0).unwrap());
    let mut changed = request.clone();
    changed.inputs = json!({"changed":true});
    assert!(dispatcher::admit(&f.home, &changed, 0).is_err());
    let db = database::open(&f.home).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM outbox WHERE kind='attempt.dispatch'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    let mut denied = f.request(2);
    denied.plan_sha = "0".repeat(64);
    assert!(dispatcher::admit(&f.home, &denied, 0).is_err());
    let result = f.result(&reply.attempt_id, 1);
    dispatcher::report(&f.home, &result).unwrap();
    dispatcher::report(&f.home, &result).unwrap();
    let mut changed = result.clone();
    changed.summary = "changed".into();
    assert!(dispatcher::report(&f.home, &changed).is_err());
    let next = dispatcher::admit(&f.home, &f.request(2), 0).unwrap();
    let stale = f.result(&next.attempt_id, 1);
    assert!(dispatcher::report(&f.home, &stale).is_err());
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM outbox WHERE kind='worker.resume'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn crash_boundaries_reconcile_without_relaunch_or_duplicate_resume() {
    for phase in ["queued", "starting", "running", "result"] {
        let f = Fixture::new();
        let reply = dispatcher::admit(&f.home, &f.request(1), 0).unwrap();
        let db = database::open(&f.home).unwrap();
        if phase != "queued" {
            db.execute(
                "UPDATE attempts SET state=?2 WHERE id=?1",
                params![
                    reply.attempt_id,
                    if phase == "result" { "running" } else { phase }
                ],
            )
            .unwrap();
        }
        if phase == "result" {
            dispatcher::report(&f.home, &f.result(&reply.attempt_id, 1)).unwrap();
        }
        dispatcher::reconcile(&f.home).unwrap();
        dispatcher::reconcile(&f.home).unwrap();
        let state: String = db
            .query_row(
                "SELECT state FROM attempts WHERE id=?1",
                [&reply.attempt_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            state,
            match phase {
                "queued" => "queued",
                "result" => "completed",
                _ => "recovery_needed",
            }
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.resume'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            u64::from(phase != "queued")
        );
    }
}
#[derive(Default)]
struct FakeLifecycle(Mutex<Vec<String>>);
impl lease::Lifecycle for FakeLifecycle {
    fn unload<'a>(&'a self, model: &'a str) -> futures::future::BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(model.into());
            Ok(())
        })
    }
}
fn model(name: &str) -> lease::Model {
    lease::Model {
        route: name.into(),
        provider: "ollama".into(),
        model: name.into(),
    }
}
#[tokio::test]
async fn global_lease_serializes_and_unloads_before_switch() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().to_path_buf();
    let lifecycle = Arc::new(FakeLifecycle::default());
    let execution = Arc::new(plantool_daemon::execution::Execution::default());
    let active = Arc::new(AtomicUsize::new(0));
    let max = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for i in 0..12 {
        let (home, lifecycle, execution, active, max) = (
            home.clone(),
            lifecycle.clone(),
            execution.clone(),
            active.clone(),
            max.clone(),
        );
        tasks.push(tokio::spawn(async move {
            let grant = lease::acquire(
                &home,
                &format!("call-{i}"),
                None,
                model(if i % 2 == 0 { "qwen" } else { "devstral" }),
                false,
                i64::MAX,
                execution,
                lifecycle.as_ref(),
            )
            .await
            .unwrap();
            let n = active.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(n, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(4)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            drop(grant);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(max.load(Ordering::SeqCst), 1);
    assert!(!lifecycle.0.lock().unwrap().is_empty());
    assert!(lease::current(&home).unwrap().is_none());
    let first = lease::acquire(
        &home,
        "first",
        None,
        model("first"),
        false,
        0,
        execution.clone(),
        lifecycle.as_ref(),
    )
    .await
    .unwrap();
    drop(first);
    let next = lease::acquire(
        &home,
        "next",
        None,
        model("next"),
        false,
        0,
        execution,
        lifecycle.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(lifecycle.0.lock().unwrap().last().unwrap(), "first");
    drop(next);
}
struct FakeGateway {
    calls: AtomicUsize,
    status: u16,
    catalog: Vec<String>,
}
impl gateway::Gateway for FakeGateway {
    fn models<'a>(
        &'a self,
        _: &'a str,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async { Ok(self.catalog.clone()) })
    }
    fn chat<'a>(
        &'a self,
        _: &'a str,
        body: Value,
    ) -> futures::future::BoxFuture<'a, anyhow::Result<gateway::Reply>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(!body.to_string().contains("never-send-me"));
            Ok(gateway::Reply {
                status: self.status,
                body: if self.status == 429 {
                    "{\"error\":\"quota\"}".into()
                } else {
                    "{\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}"
                        .into()
                },
                content_type: "application/json".into(),
                reset: Some(database::unix_now() + 30),
            })
        })
    }
}
fn verified(f: &Fixture) {
    let cfg = router::config(&f.home).unwrap();
    let db = database::open(&f.home).unwrap();
    for (name, route) in cfg.routes {
        db.execute(
            "INSERT INTO route_health(route,available,model) VALUES(?1,1,?2)",
            params![name, route.model],
        )
        .unwrap();
    }
}
#[test]
fn chain_budget_quota_reset_diversity_and_allowlist() {
    let f = Fixture::new();
    verified(&f);
    let now = database::unix_now();
    let first = router::select(&f.home, &f.project, &f.item, "backend", now).unwrap();
    assert_eq!(first.name, "codex-sol");
    router::exhaust(&f.home, &first, Some(now + 10), now).unwrap();
    let second = router::select(&f.home, &f.project, &f.item, "backend", now).unwrap();
    assert_ne!(second.name, first.name);
    assert_eq!(
        router::select(&f.home, &f.project, &f.item, "backend", now + 11)
            .unwrap()
            .name,
        first.name
    );
    let db = database::open(&f.home).unwrap();
    db.execute("INSERT INTO project_settings(project_id,version,allowed_providers_json,effective_at,set_by) VALUES(?1,1,'[\"ollama\"]','now','test')",[&f.project]).unwrap();
    assert_eq!(
        router::select(&f.home, &f.project, &f.item, "backend", now)
            .unwrap()
            .route
            .provider,
        "ollama"
    );
    db.execute(
        "INSERT INTO role_families VALUES(?1,'backend','qwen')",
        [&f.item],
    )
    .unwrap();
    let qa = router::select(&f.home, &f.project, &f.item, "reviewer", now).unwrap();
    assert!(qa.same_family_warning); // only local-qwen38 remains in this role chain
}
#[tokio::test]
async fn proxy_blocks_never_sent_files_and_caches_replay_without_provider_call() {
    let f = Fixture::new();
    verified(&f);
    let db = database::open(&f.home).unwrap();
    db.execute("INSERT INTO project_settings(project_id,version,allowed_providers_json,effective_at,set_by) VALUES(?1,1,'[\"ollama\"]','now','test')",[&f.project]).unwrap();
    std::fs::write(f.root.join(".env"), "never-send-me").unwrap();
    let gateway = FakeGateway {
        calls: AtomicUsize::new(0),
        status: 200,
        catalog: vec![],
    };
    let life = FakeLifecycle::default();
    let execution = Arc::new(plantool_daemon::execution::Execution::default());
    let context = plantool_contract::LlmContext {
        item: f.item.clone(),
        role: "cto".into(),
        generation: 0,
        attempt_id: None,
        idempotency_key: "one".into(),
    };
    assert!(proxy::complete(
        &f.home,
        "call:one",
        &context,
        json!({"messages":[{"role":"user","content":"never-send-me"}]}),
        execution.clone(),
        &gateway,
        &life
    )
    .await
    .is_err());
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    let body = json!({"messages":[{"role":"user","content":"safe"}]});
    let first = proxy::complete(
        &f.home,
        "call:one",
        &context,
        body.clone(),
        execution.clone(),
        &gateway,
        &life,
    )
    .await
    .unwrap();
    let replay = proxy::complete(
        &f.home, "call:one", &context, body, execution, &gateway, &life,
    )
    .await
    .unwrap();
    assert_eq!(first.body, replay.body);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn setup_marks_missing_slugs_unavailable_without_substitution() {
    let f = Fixture::new();
    let gateway = FakeGateway {
        calls: AtomicUsize::new(0),
        status: 200,
        catalog: vec!["devstral-small-2".into()],
    };
    gateway::verify(&f.home, &gateway).await.unwrap();
    let db = database::open(&f.home).unwrap();
    let available: bool = db
        .query_row(
            "SELECT available FROM route_health WHERE route='codex-sol'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!available);
    let local: bool = db
        .query_row(
            "SELECT available FROM route_health WHERE route='local-devstral'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(local);
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn outbox_crash_after_external_effect_retries_without_duplicate_push_pr_or_resume() {
    for kind in ["fake.push", "fake.pr", "worker.resume"] {
        let f = Fixture::new();
        let mut db = database::open(&f.home).unwrap();
        let tx = db.transaction().unwrap();
        dispatcher::enqueue(&tx, &f.item, kind, &json!({"idempotency_key":"stable"})).unwrap();
        tx.commit().unwrap();
        let seen = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let first = dispatcher::deliver_pending(&f.home, &[kind], |_, body| {
            let seen = seen.clone();
            let calls = calls.clone();
            async move {
                if seen
                    .lock()
                    .unwrap()
                    .insert(body["idempotency_key"].as_str().unwrap().to_string())
                {
                    calls.fetch_add(1, Ordering::SeqCst);
                }
                anyhow::bail!("Crash after external effect before local acknowledgement")
            }
        })
        .await;
        assert!(first.is_err());
        dispatcher::deliver_pending(&f.home, &[kind], |_, body| {
            let seen = seen.clone();
            let calls = calls.clone();
            async move {
                if seen
                    .lock()
                    .unwrap()
                    .insert(body["idempotency_key"].as_str().unwrap().to_string())
                {
                    calls.fetch_add(1, Ordering::SeqCst);
                }
                Ok(json!({"ok":true}))
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            dispatcher::deliver_pending(&f.home, &[kind], |_, _| async {
                panic!("Acknowledged effect was delivered again")
            })
            .await
            .unwrap(),
            0
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn launch_barrier_prevents_effects_before_identity_commit_and_recovery_only_fences() {
    use plantool_daemon::{
        execution::Execution,
        sandbox::{launch::Launch, NeverSent},
        supervision,
    };
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    database::initialize(&home).unwrap();
    let marker = t.path().join("effect");
    let execution = Arc::new(Execution::default());
    let mut launch = Launch::new(execution.clone(), 0, None, NeverSent::new(&[]).unwrap()).unwrap();
    Arc::get_mut(&mut launch).unwrap().attempt = Some((home.clone(), "fake-model".into()));
    let mut cmd = launch.command(std::path::Path::new("/bin/sh")).unwrap();
    cmd.args(["-c", "printf effect > \"$1\"; exec /bin/sleep 30", "fake"])
        .arg(&marker);
    let mut child = execution.spawn(0, &mut cmd).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    assert!(!marker.exists());
    launch.release(&cmd, child.id().unwrap()).unwrap();
    for _ in 0..30 {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(marker.exists());
    supervision::reconcile(&home).unwrap();
    assert_eq!(
        database::get::<bool>(&database::open(&home).unwrap(), "model_recovery_fence").unwrap(),
        Some(true)
    );
    assert!(lease::acquire(
        &home,
        "new",
        None,
        model("new"),
        false,
        0,
        execution.clone(),
        &FakeLifecycle::default()
    )
    .await
    .is_err());
    // Only the live owned handle is terminated; recovery never signals stored PIDs.
    child.kill().await.unwrap();
    drop(child);
    supervision::reconcile(&home).unwrap();
    assert_eq!(
        database::get::<bool>(&database::open(&home).unwrap(), "model_recovery_fence").unwrap(),
        Some(false)
    );
    let marker = t.path().join("must-not-run");
    let mut launch = Launch::new(execution.clone(), 0, None, NeverSent::new(&[]).unwrap()).unwrap();
    Arc::get_mut(&mut launch).unwrap().attempt = Some((home, "before-record".into()));
    let mut cmd = launch
        .command(std::path::Path::new("/usr/bin/touch"))
        .unwrap();
    cmd.arg(&marker);
    let mut child = execution.spawn(0, &mut cmd).unwrap();
    drop(launch);
    assert!(!child.wait().await.unwrap().success());
    assert!(!marker.exists());
}
#[tokio::test]
async fn quota_error_moves_down_and_reset_moves_back_up_at_next_attempt() {
    let f = Fixture::new();
    verified(&f);
    let db = database::open(&f.home).unwrap();
    db.execute("INSERT INTO project_settings(project_id,version,allowed_providers_json,effective_at,set_by) VALUES(?1,1,'[\"openrouter\",\"ollama\"]','now','test')",[&f.project]).unwrap();
    let now = database::unix_now();
    let higher = router::select(&f.home, &f.project, &f.item, "cto", now).unwrap();
    let gateway = FakeGateway {
        calls: AtomicUsize::new(0),
        status: 429,
        catalog: vec![],
    };
    let life = FakeLifecycle::default();
    let ctx = plantool_contract::LlmContext {
        item: f.item.clone(),
        role: "cto".into(),
        generation: 0,
        attempt_id: None,
        idempotency_key: "quota".into(),
    };
    let reply = proxy::complete(
        &f.home,
        "quota",
        &ctx,
        json!({"messages":[]}),
        Arc::new(Default::default()),
        &gateway,
        &life,
    )
    .await
    .unwrap();
    assert_eq!(reply.status, 429);
    let lower = router::select(&f.home, &f.project, &f.item, "cto", now).unwrap();
    assert_ne!(lower.name, higher.name);
    assert_eq!(
        router::select(&f.home, &f.project, &f.item, "cto", now + 31)
            .unwrap()
            .name,
        higher.name
    );
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn quota_detection_does_not_treat_model_discussion_as_provider_exhaustion() {
    assert!(!router::quota(
        200,
        r#"{"choices":[{"message":{"content":"Explain API quota limits"}}]}"#
    ));
    assert!(router::quota(
        200,
        r#"{"error":{"code":"insufficient_quota"}}"#
    ));
    assert!(router::quota(429, ""));
}
