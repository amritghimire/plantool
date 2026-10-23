//! All default GitHub tests are deterministic and make no calls to github.com.
use futures::future::BoxFuture;
use plantool_daemon::{
    artifacts, database,
    github::{
        api::{Api, Reply},
        auth::{self, AppConfig, KeyStore},
        gates,
        setup::{self, Repository},
        sync,
    },
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Default)]
struct FakeState {
    objects: HashMap<String, Value>,
    comments: Vec<Value>,
    posts: usize,
    calls: Vec<(String, String, Option<String>)>,
    not_modified: bool,
    lose_reply: bool,
}
#[derive(Default, Clone)]
struct Fake(Arc<Mutex<FakeState>>);
impl Api for Fake {
    fn request<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Value>,
        etag: Option<String>,
    ) -> BoxFuture<'a, anyhow::Result<Reply>> {
        Box::pin(async move {
            let mut s = self.0.lock().unwrap();
            s.calls.push((method.into(), path.into(), etag.clone()));
            if method == "POST" && path.contains("/comments") {
                s.posts += 1;
                let c = json!({"id":900+s.posts,"user":{"id":99,"type":"Bot"},"body":body.unwrap()["body"],"created_at":"2026-10-23T12:00:00Z"});
                s.comments.push(c.clone());
                if s.lose_reply {
                    return Err(anyhow::anyhow!("connection lost after effect"));
                }
                return Ok(Reply {
                    status: 201,
                    body: c,
                    etag: None,
                    next: false,
                });
            }
            let is_list = path.contains('?');
            let prefix = path.split('?').next().unwrap();
            let value = if path.contains("/issues/1/comments?") {
                json!(s.comments)
            } else {
                s.objects
                    .get(path)
                    .or_else(|| s.objects.get(prefix))
                    .cloned()
                    .unwrap_or_else(|| if is_list { json!([]) } else { Value::Null })
            };
            let status = if is_list && s.not_modified && etag.is_some() {
                304
            } else if value.is_null() {
                404
            } else {
                200
            };
            Ok(Reply {
                status,
                body: if status == 304 { Value::Null } else { value },
                etag: Some("\"v1\"".into()),
                next: false,
            })
        })
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    repo: Repository,
    sha: String,
    fake: Fake,
}
impl Fixture {
    fn new() -> Self {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let root = t.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let db = database::open(&home).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,?1,'eos','public','now')",
            [root.to_string_lossy()],
        )
        .unwrap();
        db.execute("INSERT INTO project_settings(project_id,version,effective_at,set_by) VALUES('p',1,'now','test')",[]).unwrap();
        db.execute("INSERT INTO github_repositories(project_id,app_id,installation_id,repo_id,full_name,default_branch,app_bot_id) VALUES('p',1,2,42,'o/r','main',99)",[]).unwrap();
        db.execute("INSERT INTO allowlist VALUES(7,'owner','now','test')", [])
            .unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,github_issue_number,created_at,repo_slug,slug,session_json) VALUES('item','p','eos','test',1,'now','repo','item','{}')",[]).unwrap();
        let plan = b"# Plan\n- [ ] T1 [backend] implement a thing\n";
        let sha = artifacts::put(&home, plan).unwrap();
        artifacts::register(&db, &sha, plan.len()).unwrap();
        db.execute(
            "INSERT INTO revisions VALUES('r1','item','plan',1,?1,'test','now')",
            [&sha],
        )
        .unwrap();
        let repo = Repository {
            project: "p".into(),
            app_id: 1,
            installation_id: 2,
            repo_id: 42,
            full_name: "o/r".into(),
            default_branch: "main".into(),
            app_bot_id: 99,
        };
        let fake = Fake::default();
        {
            let mut s = fake.0.lock().unwrap();
            s.objects
                .insert("/repos/o/r".into(), json!({"id":42,"visibility":"public"}));
            s.objects
                .insert("/repos/o/r/issues/1".into(), json!({"state":"open"}));
        }
        Self {
            _temp: t,
            home,
            root,
            repo,
            sha,
            fake,
        }
    }
    fn gate(&self) -> String {
        let mut db = database::open(&self.home).unwrap();
        let tx = db.transaction().unwrap();
        let id = gates::request(&tx, "item", &self.sha, "thread", "interrupt").unwrap();
        tx.commit().unwrap();
        id
    }
    fn command(&self, id: u64, body: String) -> Value {
        json!({"id":id,"body":body,"user":{"id":7,"type":"User"},"created_at":"2026-10-23T12:01:00Z","updated_at":"2026-10-23T12:01:00Z","issue_url":"https://api.github.com/repos/o/r/issues/1"})
    }
    fn install(&self, c: Value) {
        self.fake
            .0
            .lock()
            .unwrap()
            .objects
            .insert(format!("/repos/o/r/issues/comments/{}", c["id"]), c);
    }
    fn count(&self, table: &str) -> i64 {
        database::open(&self.home)
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}
#[tokio::test]
async fn plan_publication_is_app_authored_immutable_and_decision_resumes_once() {
    let f = Fixture::new();
    let gate = f.gate();
    gates::publish(&f.home, &f.fake, &f.repo).await.unwrap();
    gates::publish(&f.home, &f.fake, &f.repo).await.unwrap();
    assert_eq!(f.fake.0.lock().unwrap().posts, 1);
    let c = f.command(
        10,
        format!("/plantool approve-plan sha256:{}", &f.sha[..12]),
    );
    f.install(c);
    gates::comment(&f.home, &f.fake, &f.repo, 10).await.unwrap();
    gates::comment(&f.home, &f.fake, &f.repo, 10).await.unwrap();
    assert_eq!(f.count("decisions"), 1);
    let db = database::open(&f.home).unwrap();
    let state: String = db
        .query_row("SELECT state FROM gates WHERE id=?1", [gate], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "approved");
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM outbox WHERE kind='worker.resume'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let sent = f.fake.0.lock().unwrap();
    assert!(sent.comments[0]["body"].as_str().unwrap().contains(&f.sha));
    assert!(sent.calls.iter().all(|(m, _, _)| m != "PATCH"));
}
#[tokio::test]
async fn forged_edited_bot_stale_deleted_early_and_natural_language_never_approve() {
    let f = Fixture::new();
    f.gate();
    gates::publish(&f.home, &f.fake, &f.repo).await.unwrap();
    let valid = format!("/plantool approve-plan sha256:{}", &f.sha[..12]);
    for case in 0..9 {
        let mut c = f.command(10 + case, valid.clone());
        match case {
            0 => c["user"]["id"] = json!(8),
            1 => c["updated_at"] = json!("2026-10-23T12:02:00Z"),
            2 => c["user"]["type"] = json!("Bot"),
            3 => c["body"] = json!("/plantool approve-plan sha256:000000000000"),
            4 => {
                c["created_at"] = json!("2026-10-23T11:00:00Z");
                c["updated_at"] = c["created_at"].clone()
            }
            5 => c["body"] = json!("LGTM"),
            6 => c["issue_url"] = json!("https://api.github.com/repos/attacker/r/issues/1"),
            7 => c["body"] = json!(format!("{valid}\n/plantool urgent")),
            _ => {}
        }
        if case != 8 {
            f.install(c)
        }
        gates::comment(&f.home, &f.fake, &f.repo, 10 + case)
            .await
            .unwrap();
        assert_eq!(f.count("decisions"), 0, "case {case}");
    }
    // Only allowlisted near misses enqueue replies, never non-allowlisted or bots.
    let db = database::open(&f.home).unwrap();
    let replies: i64 = db
        .query_row(
            "SELECT count(*) FROM outbox WHERE kind='github.reply'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(replies, 4);
}
#[tokio::test]
async fn new_revision_and_policy_change_invalidate_pending_approval() {
    for policy in [false, true] {
        let f = Fixture::new();
        f.gate();
        gates::publish(&f.home, &f.fake, &f.repo).await.unwrap();
        let db = database::open(&f.home).unwrap();
        if policy {
            db.execute("INSERT INTO project_settings(project_id,version,effective_at,set_by) VALUES('p',2,'now','test')",[]).unwrap();
        } else {
            let sha = artifacts::put(&f.home, b"changed plan").unwrap();
            artifacts::register(&db, &sha, 12).unwrap();
            db.execute(
                "INSERT INTO revisions VALUES('r2','item','plan',2,?1,'test','now')",
                [sha],
            )
            .unwrap();
        }
        f.install(f.command(
            10,
            format!("/plantool approve-plan sha256:{}", &f.sha[..12]),
        ));
        gates::comment(&f.home, &f.fake, &f.repo, 10).await.unwrap();
        assert_eq!(f.count("decisions"), 0);
    }
}
#[tokio::test]
async fn crash_after_comment_creation_reconciles_without_duplicate_effect() {
    let f = Fixture::new();
    f.gate();
    f.fake.0.lock().unwrap().lose_reply = true;
    assert!(gates::publish(&f.home, &f.fake, &f.repo).await.is_err());
    f.fake.0.lock().unwrap().lose_reply = false;
    gates::publish(&f.home, &f.fake, &f.repo).await.unwrap();
    assert_eq!(f.fake.0.lock().unwrap().posts, 1);
}
#[tokio::test]
async fn uncertain_missing_publication_is_not_reposted() {
    let f = Fixture::new();
    f.gate();
    database::open(&f.home)
        .unwrap()
        .execute("UPDATE outbox SET attempts=1", [])
        .unwrap();
    assert!(gates::publish(&f.home, &f.fake, &f.repo).await.is_err());
    assert_eq!(f.fake.0.lock().unwrap().posts, 0);
}
#[tokio::test]
async fn publication_refuses_never_sent_contents() {
    let f = Fixture::new();
    f.gate();
    std::fs::write(f.root.join(".env"), b"implement a thing").unwrap();
    assert!(gates::publish(&f.home, &f.fake, &f.repo).await.is_err());
    assert_eq!(f.fake.0.lock().unwrap().posts, 0);
}
#[tokio::test]
async fn request_changes_is_durable_and_comment_is_globally_single_use() {
    let f = Fixture::new();
    f.gate();
    gates::publish(&f.home, &f.fake, &f.repo).await.unwrap();
    f.install(f.command(10, "/plantool request-changes explain rollback".into()));
    gates::comment(&f.home, &f.fake, &f.repo, 10).await.unwrap();
    let second = f.gate();
    database::open(&f.home)
        .unwrap()
        .execute(
            "UPDATE gates SET state='open',created_at='2026-10-23T12:00:00Z' WHERE id=?1",
            [second],
        )
        .unwrap();
    gates::comment(&f.home, &f.fake, &f.repo, 10).await.unwrap();
    assert_eq!(f.count("decisions"), 1);
}
#[tokio::test]
async fn label_actors_must_be_allowlisted_users_and_latest_event_wins() {
    let f = Fixture::new();
    for (id, user, kind, expected) in [
        (1, 8, "User", false),
        (2, 7, "Bot", false),
        (3, 7, "User", true),
    ] {
        sync::label_event(&f.home,&f.repo,1,&json!({"id":id,"event":"labeled","actor":{"id":user,"type":kind},"label":{"name":"plantool:accept"}})).unwrap();
        assert_eq!(
            sync::authorized_label(&f.home, "p", 1, "plantool:accept").unwrap(),
            expected
        )
    }
    sync::label_event(
        &f.home,
        &f.repo,
        1,
        &json!({"id":4,"event":"unlabeled","label":{"name":"plantool:accept"}}),
    )
    .unwrap();
    assert!(!sync::authorized_label(&f.home, "p", 1, "plantool:accept").unwrap());
}
#[tokio::test]
async fn polling_etags_visibility_audit_and_inbox_dedupe() {
    let f = Fixture::new();
    {
        let mut s = f.fake.0.lock().unwrap();
        s.objects.insert(
            "/repos/o/r/issues".into(),
            json!([{"id":100,"number":1,"updated_at":"2026-10-23T12:00:00Z"}]),
        );
    }
    sync::poll(&f.home, &f.fake, &f.repo).await.unwrap();
    let count = f.count("inbox");
    sync::poll(&f.home, &f.fake, &f.repo).await.unwrap();
    f.fake.0.lock().unwrap().not_modified = true;
    sync::poll(&f.home, &f.fake, &f.repo).await.unwrap();
    assert_eq!(f.count("inbox"), count);
    {
        let mut s = f.fake.0.lock().unwrap();
        assert!(s.calls.iter().any(|(_, _, tag)| tag.is_some()));
        s.objects
            .insert("/repos/o/r".into(), json!({"id":42,"visibility":"private"}));
    }
    sync::poll(&f.home, &f.fake, &f.repo).await.unwrap();
    let db = database::open(&f.home).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM audit WHERE action='github.visibility'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        plantool_daemon::models::router::allowed(&db, "p")
            .unwrap()
            .len(),
        5
    );
}
#[tokio::test]
async fn allowlist_uses_numeric_ids_and_refuses_bots_and_last_removal() {
    let f = Fixture::new();
    {
        let mut s = f.fake.0.lock().unwrap();
        s.objects.insert(
            "/users/new".into(),
            json!({"id":8,"login":"New","type":"User"}),
        );
        s.objects.insert(
            "/users/bot".into(),
            json!({"id":99,"login":"bot","type":"Bot"}),
        );
        s.objects.insert(
            "/users/owner".into(),
            json!({"id":7,"login":"owner","type":"User"}),
        );
    }
    assert!(setup::change_allowlist(&f.home, &f.fake, "owner", true)
        .await
        .is_err());
    assert!(setup::change_allowlist(&f.home, &f.fake, "bot", false)
        .await
        .is_err());
    setup::change_allowlist(&f.home, &f.fake, "new", false)
        .await
        .unwrap();
    setup::change_allowlist(&f.home, &f.fake, "owner", true)
        .await
        .unwrap();
    assert_eq!(
        setup::allowlist(&f.home).unwrap(),
        vec![json!({"id":8,"login":"New"})]
    );
}
struct MemoryKey(Vec<u8>);
impl KeyStore for MemoryKey {
    fn read(&self, _: u64) -> anyhow::Result<Vec<u8>> {
        Ok(self.0.clone())
    }
    fn write(&self, _: u64, _: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn http_client_signs_rs256_caches_short_lived_tokens_and_sends_etags() {
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
        Json, Router,
    };
    let rsa = openssl::rsa::Rsa::generate(2048).unwrap();
    let pem = rsa.private_key_to_pem().unwrap();
    let public = rsa.public_key_to_pem().unwrap();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let state = (count.clone(), Arc::new(public));
    let app=Router::new().route("/app/installations/444/access_tokens",post(|State((count,key)):State<(Arc<std::sync::atomic::AtomicUsize>,Arc<Vec<u8>>)>,headers:HeaderMap|async move{
 count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);let jwt=headers["authorization"].to_str().unwrap().strip_prefix("Bearer ").unwrap();assert_eq!(jsonwebtoken::decode_header(jwt).unwrap().alg,jsonwebtoken::Algorithm::RS256);
 let data=jsonwebtoken::decode::<Value>(jwt,&jsonwebtoken::DecodingKey::from_rsa_pem(&key).unwrap(),&jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256)).unwrap();assert_eq!(data.claims["iss"],"333");assert!(data.claims["exp"].as_i64().unwrap()-data.claims["iat"].as_i64().unwrap()<=600);
 Json(json!({"token":"test-installation","expires_at":(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339()}))
 })).route("/repos/o/r",get(|h:HeaderMap|async move{assert_eq!(h["authorization"],"Bearer test-installation");assert_eq!(h["if-none-match"],"\"tag\"");(StatusCode::NOT_MODIFIED,"")})).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = plantool_daemon::github::api::Client::with_base(
        AppConfig {
            app_id: 333,
            installation_id: 444,
        },
        Arc::new(MemoryKey(pem)),
        &format!("http://{address}"),
    )
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            client
                .request("GET", "/repos/o/r", None, Some("\"tag\"".into()))
                .await
                .unwrap()
                .status,
            304
        )
    }
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    auth::invalidate(333);
    server.abort();
}
#[test]
fn full_command_verifier_pins_snapshot_and_milestone_commits() {
    use plantool_daemon::github::commands::{self, Command, Context, Place};
    let f = Fixture::new();
    let db = database::open(&f.home).unwrap();
    let c = f.command(55, "/plantool continue".into());
    let old = "aaaaaaaaaaaa0000000000000000000000000000";
    let new = "bbbbbbbbbbbb0000000000000000000000000000";
    let mut ctx = Context {
        place: Place::DraftPr,
        opened_at: "2026-10-23T12:00:00Z",
        snapshot_at: None,
        pending_hash: None,
        gate_commit: Some(&old[..12]),
        current_commit: Some(old),
        snapshot_items: &[],
    };
    assert!(commands::verify(&db, &c, &Command::Continue, &ctx).is_ok());
    ctx.current_commit = Some(new);
    assert!(commands::verify(&db, &c, &Command::Continue, &ctx).is_err());
    ctx.place = Place::Issue;
    assert!(commands::verify(&db, &c, &Command::Continue, &ctx).is_err());
    let snapshot = [("item", f.sha.as_str(), f.sha.as_str())];
    ctx.place = Place::TrackingIssue;
    ctx.snapshot_at = Some("2026-10-23T12:00:30Z");
    ctx.snapshot_items = &snapshot;
    assert!(commands::verify(&db, &c, &Command::ApproveSprint, &ctx).is_ok());
    ctx.snapshot_at = Some("2026-10-23T12:02:00Z");
    assert!(commands::verify(&db, &c, &Command::ApproveSprint, &ctx).is_err());
    ctx.snapshot_at = Some("2026-10-23T12:00:30Z");
    let stale = [("item", f.sha.as_str(), new)];
    ctx.snapshot_items = &stale;
    assert!(commands::verify(&db, &c, &Command::ApproveItem("item".into()), &ctx).is_err());
    assert!(commands::verify(&db, &c, &Command::ApproveItem("absent".into()), &ctx).is_err());
}
#[tokio::test]
async fn failed_poll_does_not_advance_cursor_and_repository_identity_fails_closed() {
    let f = Fixture::new();
    f.fake
        .0
        .lock()
        .unwrap()
        .objects
        .insert("/repos/o/r".into(), json!({"id":999,"visibility":"public"}));
    assert!(sync::poll(&f.home, &f.fake, &f.repo).await.is_err());
    assert_eq!(f.count("github_cursors"), 0);
}
