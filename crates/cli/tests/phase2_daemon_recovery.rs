#![cfg(unix)]
//! Real SIGKILL/restart of an owned daemon, with durable fake attempts and no models.
use plantool_contract::{Dispatch, Outcome, RoleResult};
use plantool_core::{Actor, DocKind, Stage};
use plantool_daemon::{database, dispatcher, Registry};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};
struct OwnedDaemon(Child);
impl Drop for OwnedDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(home: &Path, owner: &Path, port: u16) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_plantool"));
    command
        .arg("serve")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", owner)
        .env("PLANTOOL_HOME", home)
        .env("PLANTOOL_PORT", port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}
fn launch(home: &Path, owner: &Path) -> OwnedDaemon {
    let claim = std::net::TcpListener::bind(("127.0.0.1", 41300))
        .expect("Port 41300 must be free for the isolated daemon test");
    drop(claim);
    let mut daemon = OwnedDaemon(command(home, owner, 41300).spawn().unwrap());
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(200))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(response) = client.get("http://localhost:41300/health").send() {
            if let Ok(value) = response.json::<Value>() {
                assert_eq!(value["pid"].as_u64(), Some(u64::from(daemon.0.id())));
                return daemon;
            }
        }
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "Daemon exited before health"
        );
        assert!(Instant::now() < deadline, "Daemon health timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn daemon_death_reconciles_once_and_a_second_daemon_cannot_mutate_the_same_home() {
    for completed in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let owner = temp.path().join("owner");
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&owner).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "test"],
            vec!["config", "user.email", "t@t"],
            vec!["commit", "--allow-empty", "-qm", "init"],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&root)
                .env("HOME", &owner)
                .status()
                .unwrap()
                .success());
        }
        let registry = Registry::load(home.clone()).unwrap();
        let (live, _) = registry
            .create(serde_json::from_value(json!({"repo":root,"slug":"recovery"})).unwrap())
            .unwrap();
        std::fs::write(
            live.doc_path(DocKind::Plan),
            "# Plan\n- [ ] T1: Test recovery\n",
        )
        .unwrap();
        live.capture_doc(DocKind::Plan).unwrap();
        live.set_stage(Stage::Approved, Actor::Human).unwrap();
        let item = live.store.key();
        let sha = live.doc(DocKind::Plan).unwrap().sha;
        let db = database::open(&home).unwrap();
        db.execute("UPDATE projects SET mode='eos'", []).unwrap();
        db.execute("UPDATE work_items SET kind='eos'", []).unwrap();
        let request = Dispatch {
            item: item.clone(),
            role: "backend".into(),
            generation: 1,
            inputs: json!({}),
            plan_sha: sha,
            policy_version: 0,
            capability: "write".into(),
            idempotency_key: "dispatch-one".into(),
            thread_id: "project:item".into(),
            interrupt_id: "interrupt-one".into(),
        };
        let reply = dispatcher::admit(&home, &request, 0).unwrap();
        db.execute(
            "UPDATE attempts SET state='running' WHERE id=?1",
            [&reply.attempt_id],
        )
        .unwrap();
        if completed {
            dispatcher::report(
                &home,
                &RoleResult {
                    item,
                    role: "backend".into(),
                    attempt_id: reply.attempt_id.clone(),
                    generation: 1,
                    outcome: Outcome::Done,
                    summary: "fake verified result".into(),
                    tasks_done: vec![],
                    artifacts: vec![],
                    checks: vec![],
                    findings: vec![],
                    next: None,
                    review_guide: None,
                },
            )
            .unwrap();
        }
        drop(registry);
        drop(live);
        drop(db);
        let mut daemon = launch(&home, &owner);
        let second = command(&home, &owner, 41301).status().unwrap();
        assert!(!second.success());
        daemon.0.kill().unwrap();
        daemon.0.wait().unwrap();
        drop(daemon);
        let _restarted = launch(&home, &owner);
        let db = database::open(&home).unwrap();
        let state: String = db
            .query_row(
                "SELECT state FROM attempts WHERE id=?1",
                [&reply.attempt_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            state,
            if completed {
                "completed"
            } else {
                "recovery_needed"
            }
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.resume'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert!(!home.join("worker").exists());
    }
}
