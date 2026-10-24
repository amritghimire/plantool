use futures::future::BoxFuture;
use plantool_contract::*;
use plantool_daemon::{
    database, dispatcher,
    github::{
        api::{Api, Reply},
        git_ops::GitOps,
        lifecycle,
        setup::Repository,
    },
    workflow,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
fn git(root: &Path, args: &[&str]) -> String {
    plantool_daemon::git::git(root, args).unwrap()
}
struct Fixture {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
    item: String,
    base: String,
    repo: Repository,
}
impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "test"]);
        git(&root, &["config", "user.email", "test@localhost"]);
        std::fs::write(root.join("src/a.rs"), "old\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        let db = database::open(&home).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,?1,'eos','public',?2)",
            params![root.to_string_lossy(), plantool_core::now()],
        )
        .unwrap();
        db.execute("INSERT INTO project_settings(project_id,version,effective_at,set_by) VALUES('p',1,?1,'test')",[plantool_core::now()]).unwrap();
        db.execute("INSERT INTO github_repositories(project_id,app_id,installation_id,repo_id,full_name,default_branch,app_bot_id) VALUES('p',1,2,42,'o/r','main',99)",[]).unwrap();
        db.execute("INSERT INTO allowlist VALUES(7,'owner','now','test')", [])
            .unwrap();
        let item =
            workflow::start(&home, "p", 1, "A scoped fix", "Fix a single file", &base).unwrap();
        Self {
            _tmp: tmp,
            home,
            root,
            item,
            base,
            repo: Repository {
                project: "p".into(),
                app_id: 1,
                installation_id: 2,
                repo_id: 42,
                full_name: "o/r".into(),
                default_branch: "main".into(),
                app_bot_id: 99,
            },
        }
    }
    fn state(&self) -> Value {
        workflow::state(&database::open(&self.home).unwrap(), &self.item)
            .unwrap()
            .unwrap()
    }
    fn run(&self, step: &str, details: Option<RoleDetails>, keys: Vec<String>) -> RoleResult {
        let state = self.state();
        assert_eq!(state["step"], step);
        let role = workflow::role(step);
        let generation = state["generation"].as_u64().unwrap() + 1;
        let request = Dispatch {
            item: self.item.clone(),
            role: role.into(),
            generation,
            inputs: json!({"step":step}),
            plan_sha: state["plan_sha"].as_str().unwrap().into(),
            policy_version: 1,
            capability: workflow::capability(role).into(),
            idempotency_key: format!("dispatch:{step}:{generation}"),
            thread_id: self.item.clone(),
            interrupt_id: format!("interrupt:{generation}"),
        };
        let attempt = dispatcher::admit(&self.home, &request, 0).unwrap();
        let db = database::open(&self.home).unwrap();
        db.execute(
            "UPDATE attempts SET state='running' WHERE id=?1",
            [&attempt.attempt_id],
        )
        .unwrap();
        let head = git(&self.root, &["rev-parse", "HEAD"]);
        db.execute(
            "UPDATE dispatches SET base_sha=?2 WHERE attempt_id=?1",
            params![attempt.attempt_id, head],
        )
        .unwrap();
        let mut result = RoleResult {
            item: self.item.clone(),
            role: role.into(),
            attempt_id: attempt.attempt_id.clone(),
            generation,
            outcome: Outcome::Done,
            summary: format!("{step} complete"),
            tasks_done: keys,
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details,
        };
        if role == "backend" {
            std::fs::write(
                self.root.join("src/a.rs"),
                if result.tasks_done.contains(&"T2".into()) {
                    "second fix\n"
                } else {
                    "fixed\n"
                },
            )
            .unwrap();
            git(&self.root, &["add", "."]);
            let message = format!(
                "Scoped fix\n\nPlanTool-Item: {}\nPlanTool-Plan: sha256:{}\nPlanTool-Attempt: {}",
                self.item, request.plan_sha, attempt.attempt_id
            );
            if !git(&self.root, &["diff", "--cached", "--name-only"]).is_empty() {
                git(&self.root, &["commit", "-qm", &message]);
            }
        }
        if workflow::coding(role) {
            result.artifacts.push(Artifact {
                kind: "commit".into(),
                reference: git(&self.root, &["rev-parse", "HEAD"]),
            });
        }
        if role == "qa" {
            let evidence = plantool_daemon::artifacts::put(
                &self.home,
                b"fake test runner: all acceptance criteria pass",
            )
            .unwrap();
            plantool_daemon::artifacts::register(&db, &evidence, 46).unwrap();
            db.execute(
                "INSERT INTO artifact_refs VALUES('attempt',?1,'qa',?2,0)",
                params![attempt.attempt_id, evidence],
            )
            .unwrap();
            result.checks.push(Check {
                name: "acceptance".into(),
                status: CheckStatus::Pass,
                evidence: format!("artifact://{evidence}"),
            });
        }
        dispatcher::report(&self.home, &result).unwrap();
        result
    }
    fn planning(&self) {
        self.run(
            "research",
            Some(RoleDetails::Research {
                markdown: "Read the default-branch source and conventions".into(),
            }),
            vec![],
        );
        self.run(
            "plan",
            Some(RoleDetails::Plan {
                markdown: "# Plan\n## Phase 1 [review]\n- [ ] T1: [backend] Fix src/a.rs\n".into(),
                acceptance_criteria: vec!["The scoped test passes".into()],
                affected_files: vec!["src/a.rs".into(), "tests/**".into(), "README.md".into()],
            }),
            vec![],
        );
        self.run("plan_review", None, vec![]);
    }
    fn approve_plan(&self) {
        let sha = self.state()["plan_sha"].as_str().unwrap().to_string();
        let mut db = database::open(&self.home).unwrap();
        let tx = db.transaction().unwrap();
        let gate = plantool_daemon::github::gates::request(
            &tx,
            &self.item,
            &sha,
            &self.item,
            "plan-interrupt",
        )
        .unwrap();
        tx.execute(
            "UPDATE gates SET state='approved',github_comment_id=10 WHERE id=?1",
            [gate],
        )
        .unwrap();
        tx.execute("UPDATE work_items SET approved_plan_revision_id=(SELECT id FROM revisions WHERE item_id=?1 AND doc_kind='plan' ORDER BY number DESC LIMIT 1),state_json=json_set(state_json,'$.plan_revision_pending',0) WHERE id=?1",[&self.item]).unwrap();
        workflow::gate_decision(&tx, &self.item, true).unwrap();
        tx.commit().unwrap();
    }
    fn ready(&self) {
        self.planning();
        self.approve_plan();
        self.run("backend", None, vec!["T1".into()]);
        self.run("qa", None, vec![]);
        let head = git(&self.root, &["rev-parse", "HEAD"]);
        self.run(
            "reviewer",
            Some(RoleDetails::Security {
                commit: head,
                passed: true,
                criteria: vec!["auth, CI, secrets and sandbox reviewed".into()],
            }),
            vec![],
        );
        self.run("docs",Some(RoleDetails::Docs{updates:None,body:"## What changed and why\nScoped fix\n## Where to look first\nsrc/a.rs\n## How to test\nRun tests\n## AI review rounds\nQA and Reviewer\n## Found during review\nNone\n## Evidence\nFake test evidence\n".into()}),vec![]);
        workflow::pr_wait(&self.home, &self.item, &self.item, "pr-interrupt").unwrap();
    }
}
#[derive(Default)]
struct Remote {
    pr: Option<Value>,
    reviews: Vec<Value>,
    checks: Vec<Value>,
    sha: Option<String>,
    calls: Vec<(String, String, Option<Value>)>,
    pushes: usize,
    merge_calls: usize,
    issues: Vec<Value>,
    lose_issue_reply: bool,
    refuse_merge: bool,
    comments: Vec<Value>,
    lose_comment_reply: bool,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Remote>>);
impl Api for Fake {
    fn request<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Value>,
        _: Option<String>,
    ) -> BoxFuture<'a, anyhow::Result<Reply>> {
        Box::pin(async move {
            let mut s = self.0.lock().unwrap();
            s.calls.push((method.into(), path.into(), body.clone()));
            let base = path.split('?').next().unwrap();
            let mut status = 200;
            let value = match (method, base) {
                ("GET", "/repos/o/r") => {
                    json!({"id":42,"allow_squash_merge":true,"allow_merge_commit":true,"allow_rebase_merge":false})
                }
                ("GET", p) if p.contains("/git/ref/heads/") => {
                    if let Some(sha) = &s.sha {
                        json!({"object":{"sha":sha}})
                    } else {
                        status = 404;
                        Value::Null
                    }
                }
                ("GET", "/repos/o/r/issues") => json!(s.issues),
                ("POST", "/repos/o/r/issues") => {
                    status = 201;
                    let b = body.unwrap();
                    assert!(
                        b["labels"] == json!(["plantool:proposed"])
                            || b["labels"] == json!(["plantool:sprint"])
                    );
                    let issue = json!({"number":10+s.issues.len(),"state":"open","user":{"id":99,"type":"Bot"},"title":b["title"],"body":b["body"]});
                    s.issues.push(issue.clone());
                    if s.lose_issue_reply {
                        s.lose_issue_reply = false;
                        return Err(anyhow::anyhow!("lost issue reply after effect"));
                    }
                    issue
                }
                ("GET", "/repos/o/r/pulls") => json!(s.pr.iter().cloned().collect::<Vec<_>>()),
                ("POST", "/repos/o/r/pulls") => {
                    status = 201;
                    let b = body.unwrap();
                    let p = json!({"number":2,"user":{"id":99,"type":"Bot"},"head":{"ref":b["head"],"sha":s.sha},"body":b["body"],"state":"open","merged":false,"draft":b["draft"],"node_id":"PR_NODE"});
                    s.pr = Some(p.clone());
                    p
                }
                ("POST", "/graphql") => {
                    s.pr.as_mut().unwrap()["draft"] = json!(false);
                    json!({"data":{"markPullRequestReadyForReview":{"pullRequest":{"isDraft":false}}}})
                }
                ("GET", p) if p.starts_with("/repos/o/r/issues/comments/") => {
                    let id = p.rsplit('/').next().unwrap().parse::<u64>().unwrap();
                    match s.comments.iter().find(|c| c["id"] == id) {
                        Some(c) => c.clone(),
                        None => {
                            status = 404;
                            Value::Null
                        }
                    }
                }
                ("GET", p) if p.starts_with("/repos/o/r/issues/") && p.ends_with("/comments") => {
                    let n = p.split('/').nth(5).unwrap();
                    json!(s
                        .comments
                        .iter()
                        .filter(|c| c["issue_url"].as_str().unwrap().ends_with(&format!("/{n}")))
                        .cloned()
                        .collect::<Vec<_>>())
                }
                ("POST", p) if p.starts_with("/repos/o/r/issues/") && p.ends_with("/comments") => {
                    let n = p.split('/').nth(5).unwrap();
                    let time = chrono::Utc::now().to_rfc3339();
                    let c = json!({"id":1000+s.comments.len(),"user":{"id":99,"type":"Bot"},"body":body.unwrap()["body"],"issue_url":format!("https://api.github.com/repos/o/r/issues/{n}"),"created_at":time,"updated_at":time});
                    s.comments.push(c.clone());
                    status = 201;
                    if s.lose_comment_reply {
                        s.lose_comment_reply = false;
                        return Err(anyhow::anyhow!("lost comment reply"));
                    }
                    c
                }
                ("GET", p) if p.starts_with("/repos/o/r/issues/") => {
                    let n = p.rsplit('/').next().unwrap().parse::<u64>().unwrap();
                    s.issues
                        .iter()
                        .find(|i| i["number"] == n)
                        .cloned()
                        .unwrap_or(json!({"number":n,"state":"open"}))
                }
                ("GET", "/repos/o/r/pulls/2") => s.pr.clone().unwrap(),
                ("PATCH", "/repos/o/r/pulls/2") => {
                    s.pr.as_mut().unwrap()["body"] = body.unwrap()["body"].clone();
                    s.pr.clone().unwrap()
                }
                ("GET", "/repos/o/r/pulls/2/reviews") => json!(s.reviews),
                ("GET", p) if p.ends_with("/check-runs") => json!({"check_runs":s.checks}),
                ("POST", "/repos/o/r/check-runs") => {
                    status = 201;
                    let mut c = body.unwrap();
                    c["id"] = json!(50);
                    c["app"] = json!({"id":1});
                    s.checks.push(c.clone());
                    c
                }
                ("PATCH", "/repos/o/r/check-runs/50") => {
                    let b = body.unwrap();
                    let c = s.checks.first_mut().unwrap();
                    c["status"] = b["status"].clone();
                    c["conclusion"] = b["conclusion"].clone();
                    c.clone()
                }
                ("GET", p) if p.ends_with("/status") => json!({"state":"success"}),
                ("PUT", "/repos/o/r/pulls/2/merge") => {
                    let b = body.unwrap();
                    assert_eq!(b["sha"], s.sha.clone().unwrap());
                    assert_eq!(b["merge_method"], "squash");
                    s.merge_calls += 1;
                    if s.refuse_merge {
                        return Ok(Reply {
                            status: 405,
                            body: json!({"message":"branch protection"}),
                            etag: None,
                            next: false,
                        });
                    }
                    s.pr.as_mut().unwrap()["merged"] = json!(true);
                    json!({"merged":true})
                }
                _ => {
                    status = 404;
                    Value::Null
                }
            };
            Ok(Reply {
                status,
                body: value,
                etag: None,
                next: false,
            })
        })
    }
}
struct FakeGit {
    remote: Fake,
    base: String,
}
impl GitOps for FakeGit {
    fn fetch<'a>(
        &'a self,
        _: &'a dyn Api,
        _: &'a Path,
        _: &'a str,
        _: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move { Ok(self.base.clone()) })
    }
    fn push<'a>(
        &'a self,
        _: &'a dyn Api,
        _: &'a Path,
        _: &'a str,
        _: &'a str,
        head: &'a str,
        _: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            let mut remote = self.remote.0.lock().unwrap();
            remote.pushes += 1;
            remote.sha = Some(head.into());
            if let Some(pr) = &mut remote.pr {
                pr["head"]["sha"] = json!(head);
            }
            Ok(())
        })
    }
}
#[test]
fn planning_is_authorized_before_gate_but_implementation_is_not() {
    let f = Fixture::new();
    f.planning();
    assert_eq!(f.state()["step"], "plan_gate");
    let request = Dispatch {
        item: f.item.clone(),
        role: "backend".into(),
        generation: 4,
        inputs: json!({"step":"backend"}),
        plan_sha: f.state()["plan_sha"].as_str().unwrap().into(),
        policy_version: 1,
        capability: "write".into(),
        idempotency_key: "forged".into(),
        thread_id: f.item.clone(),
        interrupt_id: "i".into(),
    };
    assert!(dispatcher::admit(&f.home, &request, 0).is_err());
    f.approve_plan();
    assert_eq!(f.state()["step"], "backend");
}
#[tokio::test]
async fn fake_item_reaches_merge_with_exact_head_and_no_replayed_push_or_pr() {
    let f = Fixture::new();
    f.ready();
    let remote = Fake::default();
    let git = FakeGit {
        remote: remote.clone(),
        base: f.base.clone(),
    };
    lifecycle::process_item(&f.home, &remote, &f.repo, &git, &f.item)
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().merge_calls, 0);
    let head = plantool_daemon::git::git(&f.root, &["rev-parse", "HEAD"]).unwrap();
    remote.0.lock().unwrap().reviews = vec![
        json!({"id":60,"user":{"id":7,"type":"User"},"state":"APPROVED","commit_id":head,"submitted_at":(chrono::Utc::now()+chrono::Duration::seconds(1)).to_rfc3339()}),
    ];
    lifecycle::process_item(&f.home, &remote, &f.repo, &git, &f.item)
        .await
        .unwrap();
    lifecycle::process_item(&f.home, &remote, &f.repo, &git, &f.item)
        .await
        .unwrap();
    let r = remote.0.lock().unwrap();
    assert_eq!(r.merge_calls, 1);
    assert_eq!(r.pushes, 1);
    assert_eq!(
        r.calls
            .iter()
            .filter(|(m, p, _)| m == "POST" && p == "/repos/o/r/pulls")
            .count(),
        1
    );
    assert_eq!(f.state()["step"], "merged");
}
#[test]
fn old_sha_bot_and_superseded_user_reviews_do_not_approve() {
    let f = Fixture::new();
    let db = database::open(&f.home).unwrap();
    let current = "a".repeat(40);
    let time = "2026-10-24T00:01:00Z";
    let mut reviews = vec![
        json!({"id":1,"user":{"id":7,"type":"User"},"state":"APPROVED","commit_id":"b".repeat(40),"submitted_at":time}),
        json!({"id":2,"user":{"id":7,"type":"Bot"},"state":"APPROVED","commit_id":current,"submitted_at":time}),
    ];
    assert!(
        lifecycle::latest_approval(&db, &reviews, &current, "2026-10-24T00:00:00Z")
            .unwrap()
            .is_none()
    );
    reviews.push(json!({"id":3,"user":{"id":7,"type":"User"},"state":"APPROVED","commit_id":current,"submitted_at":time}));
    assert!(
        lifecycle::latest_approval(&db, &reviews, &current, "2026-10-24T00:00:00Z")
            .unwrap()
            .is_some()
    );
    reviews.push(json!({"id":4,"user":{"id":7,"type":"User"},"state":"CHANGES_REQUESTED","commit_id":current,"submitted_at":time}));
    assert!(
        lifecycle::latest_approval(&db, &reviews, &current, "2026-10-24T00:00:00Z")
            .unwrap()
            .is_none()
    );
}
#[test]
fn qa_paths_and_review_policy_are_not_prompt_permissions() {
    assert!(workflow::test_path("web/src/a.test.tsx"));
    assert!(workflow::test_path("tests/a.rs"));
    assert!(!workflow::test_path("src/contested.rs"));
    let f = Fixture::new();
    assert!(!lifecycle::policy_facts(&f.home, &f.item, &f.base).unwrap());
}

#[tokio::test]
async fn changes_requested_returns_to_the_owning_implementation_loop() {
    let f = Fixture::new();
    f.ready();
    let remote = Fake::default();
    let transport = FakeGit {
        remote: remote.clone(),
        base: f.base.clone(),
    };
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    let head = git(&f.root, &["rev-parse", "HEAD"]);
    remote.0.lock().unwrap().reviews = vec![
        json!({"id":61,"user":{"id":7,"type":"User"},"state":"CHANGES_REQUESTED","commit_id":head,"body":"Fix the approved edge case","submitted_at":(chrono::Utc::now()+chrono::Duration::seconds(1)).to_rfc3339()}),
    ];
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "backend");
    assert_eq!(remote.0.lock().unwrap().merge_calls, 0);
}
#[tokio::test]
async fn a_frozen_item_performs_no_app_write_or_push() {
    let f = Fixture::new();
    f.ready();
    let mut s = f.state();
    s["frozen"] = json!(true);
    let mut db = database::open(&f.home).unwrap();
    let tx = db.transaction().unwrap();
    workflow::save(&tx, &f.item, &s).unwrap();
    tx.commit().unwrap();
    let remote = Fake::default();
    let transport = FakeGit {
        remote: remote.clone(),
        base: f.base.clone(),
    };
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    assert!(remote.0.lock().unwrap().calls.is_empty());
}
#[tokio::test]
async fn main_movement_rebases_and_invalidates_qa_and_review_evidence() {
    let f = Fixture::new();
    f.ready();
    git(&f.root, &["checkout", "--detach", &f.base]);
    std::fs::write(f.root.join("upstream.txt"), "new main\n").unwrap();
    git(&f.root, &["add", "."]);
    git(&f.root, &["commit", "-qm", "upstream"]);
    let next = git(&f.root, &["rev-parse", "HEAD"]);
    let remote = Fake::default();
    let transport = FakeGit {
        remote: remote.clone(),
        base: next.clone(),
    };
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "qa");
    assert_eq!(remote.0.lock().unwrap().pushes, 0);
    let db = database::open(&f.home).unwrap();
    let (head, qa, review): (String, Option<String>, Option<String>) = db
        .query_row(
            "SELECT head_sha,qa_head,reviewed_head FROM workflow_items WHERE item_id=?1",
            [&f.item],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    git(&f.root, &["merge-base", "--is-ancestor", &next, &head]);
    assert!(qa.is_none() && review.is_none());
    assert!(!lifecycle::policy_facts(&f.home, &f.item, &head).unwrap());
}
#[tokio::test]
async fn rebase_conflicts_return_to_an_implementer_without_publishing() {
    let f = Fixture::new();
    f.ready();
    git(&f.root, &["checkout", "--detach", &f.base]);
    std::fs::write(f.root.join("src/a.rs"), "upstream conflict\n").unwrap();
    git(&f.root, &["add", "."]);
    git(&f.root, &["commit", "-qm", "upstream"]);
    let next = git(&f.root, &["rev-parse", "HEAD"]);
    let remote = Fake::default();
    let transport = FakeGit {
        remote: remote.clone(),
        base: next.clone(),
    };
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "backend");
    assert_eq!(f.state()["rebase_base"], next);
    assert_eq!(remote.0.lock().unwrap().pushes, 0);
}
#[test]
fn default_branch_instructions_do_not_read_a_worktree_replacement() {
    let f = Fixture::new();
    std::fs::write(f.root.join("AGENTS.md"), "trusted default conventions").unwrap();
    git(&f.root, &["add", "."]);
    git(&f.root, &["commit", "-qm", "conventions"]);
    std::fs::write(f.root.join("AGENTS.md"), "malicious live replacement").unwrap();
    let reference = plantool_daemon::instructions::default_ref(&f.root).unwrap();
    assert_eq!(
        plantool_daemon::instructions::default_file(&f.root, &reference, "AGENTS.md")
            .unwrap()
            .unwrap(),
        "trusted default conventions"
    );
}
#[test]
fn approval_digest_binds_acceptance_criteria_and_file_scope() {
    let f = Fixture::new();
    f.planning();
    let plan = f.state()["plan_sha"].as_str().unwrap().to_string();
    let bytes = plantool_daemon::artifacts::read(&f.home, &plan).unwrap();
    let plan = String::from_utf8(bytes).unwrap();
    assert!(plan.contains("Bound acceptance criteria") && plan.contains("Bound affected files"));
}
#[tokio::test]
async fn proposals_are_filed_once_after_a_lost_reply_without_changing_code() {
    let f = Fixture::new();
    let mut db = database::open(&f.home).unwrap();
    let tx = db.transaction().unwrap();
    workflow::proposal(
        &tx,
        &f.item,
        "Separate follow-up",
        "An out-of-scope change belongs in its own plan",
    )
    .unwrap();
    tx.commit().unwrap();
    let remote = Fake::default();
    remote.0.lock().unwrap().lose_issue_reply = true;
    assert!(lifecycle::proposals(&f.home, &remote, &f.repo)
        .await
        .is_err());
    lifecycle::proposals(&f.home, &remote, &f.repo)
        .await
        .unwrap();
    lifecycle::proposals(&f.home, &remote, &f.repo)
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().issues.len(), 1);
    assert_eq!(git(&f.root, &["rev-parse", "HEAD"]), f.base);
}
#[tokio::test]
async fn a_merge_refusal_is_shown_and_not_automatically_retried() {
    let f = Fixture::new();
    f.ready();
    let remote = Fake::default();
    let transport = FakeGit {
        remote: remote.clone(),
        base: f.base.clone(),
    };
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    let head = git(&f.root, &["rev-parse", "HEAD"]);
    {
        let mut r = remote.0.lock().unwrap();
        r.refuse_merge = true;
        r.reviews = vec![
            json!({"id":60,"user":{"id":7,"type":"User"},"state":"APPROVED","commit_id":head,"submitted_at":(chrono::Utc::now()+chrono::Duration::seconds(1)).to_rfc3339()}),
        ];
    }
    assert!(
        lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
            .await
            .is_err()
    );
    lifecycle::process_item(&f.home, &remote, &f.repo, &transport, &f.item)
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().merge_calls, 1);
}
#[test]
fn retry_budget_exhaustion_goes_to_supervisor_and_escalation_freezes() {
    let f = Fixture::new();
    f.planning();
    f.approve_plan();
    for round in 0..4 {
        f.run("backend", None, vec!["T1".into()]);
        let s = f.state();
        let generation = s["generation"].as_u64().unwrap() + 1;
        let request = Dispatch {
            item: f.item.clone(),
            role: "qa".into(),
            generation,
            inputs: json!({"step":"qa"}),
            plan_sha: s["plan_sha"].as_str().unwrap().into(),
            policy_version: 1,
            capability: "tests".into(),
            idempotency_key: format!("qa-fault:{round}"),
            thread_id: f.item.clone(),
            interrupt_id: format!("qa:{round}"),
        };
        let attempt = dispatcher::admit(&f.home, &request, 0).unwrap();
        database::open(&f.home)
            .unwrap()
            .execute(
                "UPDATE attempts SET state='running' WHERE id=?1",
                [&attempt.attempt_id],
            )
            .unwrap();
        let result = RoleResult {
            item: f.item.clone(),
            role: "qa".into(),
            attempt_id: attempt.attempt_id,
            generation,
            outcome: Outcome::NeedsFix,
            summary: "An approved acceptance criterion failed".into(),
            tasks_done: vec![],
            artifacts: vec![Artifact {
                kind: "commit".into(),
                reference: git(&f.root, &["rev-parse", "HEAD"]),
            }],
            checks: vec![],
            findings: vec![Finding {
                severity: Severity::Medium,
                scope: FindingScope::In,
                title: "Edge case".into(),
                detail: "Fix within T1".into(),
                file: Some("src/a.rs".into()),
                line: Some(1),
                task_key: Some("T1".into()),
            }],
            next: None,
            review_guide: None,
            details: None,
        };
        dispatcher::report(&f.home, &result).unwrap();
        assert_eq!(
            f.state()["step"],
            if round < 3 { "backend" } else { "supervisor" }
        );
    }
    f.run(
        "supervisor",
        Some(RoleDetails::Strategy {
            action: "escalate".into(),
            owner: None,
            task_key: Some("T1".into()),
            reason: "Retry budget exhausted".into(),
        }),
        vec![],
    );
    assert_eq!(f.state()["frozen"], true);
}

#[test]
fn docs_updates_are_scoped_and_reset_validation_for_the_new_head() {
    let f = Fixture::new();
    f.ready();
    assert!(!workflow::coding("docs"));
    let db = database::open(&f.home).unwrap();
    db.execute("UPDATE workflow_items SET state_json=json_set(state_json,'$.step','docs') WHERE item_id=?1",[&f.item]).unwrap();
    let old: String = db
        .query_row(
            "SELECT head_sha FROM workflow_items WHERE item_id=?1",
            [&f.item],
            |r| r.get(0),
        )
        .unwrap();
    f.run("docs", Some(RoleDetails::Docs { body: "## What changed and why\nScoped documentation\n## Where to look first\nREADME.md\n## How to test\nRun tests\n## AI review rounds\nQA and Reviewer\n## Found during review\nNone\n## Evidence\nTest evidence\n".into(), updates:Some(vec![DocUpdate {path:"README.md".into(), content:"Documented behavior\n".into()}]) }), vec![]);
    assert_eq!(f.state()["step"], "qa");
    let (head, qa, review): (String, Option<String>, Option<String>) = db
        .query_row(
            "SELECT head_sha,qa_head,reviewed_head FROM workflow_items WHERE item_id=?1",
            [&f.item],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_ne!(old, head);
    assert!(qa.is_none() && review.is_none());
    assert_eq!(
        git(&f.root, &["show", &format!("{head}:README.md")]),
        "Documented behavior"
    );
    assert!(
        !f.root.join("README.md").exists(),
        "Owner checkout remains untouched"
    );
}

#[test]
fn supervisor_replan_creates_a_new_revision_and_resets_changed_task_progress() {
    let f = Fixture::new();
    f.ready();
    let old = f.state()["plan_sha"].clone();
    let db = database::open(&f.home).unwrap();
    db.execute("UPDATE workflow_items SET state_json=json_set(state_json,'$.step','supervisor') WHERE item_id=?1",[&f.item]).unwrap();
    f.run(
        "supervisor",
        Some(RoleDetails::Strategy {
            action: "replan".into(),
            owner: None,
            task_key: None,
            reason: "Acceptance criterion needs a different implementation".into(),
        }),
        vec![],
    );
    assert_eq!(f.state()["step"], "plan");
    assert_eq!(f.state()["approved"], false);
    f.run(
        "plan",
        Some(RoleDetails::Plan {
            markdown:
                "# Plan\n## Phase 1 [review]\n- [ ] T1: [backend] Reworded implementation task\n"
                    .into(),
            acceptance_criteria: vec!["New scoped criterion".into()],
            affected_files: vec!["src/a.rs".into()],
        }),
        vec![],
    );
    assert_ne!(f.state()["plan_sha"], old);
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM revisions WHERE item_id=?1 AND doc_kind='plan'",
            [&f.item],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    let progress: String = db
        .query_row(
            "SELECT status FROM task_progress WHERE item_id=?1 AND task_key='T1'",
            [&f.item],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(progress, "pending");
    f.run("plan_review", None, vec![]);
    assert_eq!(f.state()["step"], "plan_gate");
    assert_eq!(f.state()["approved"], false);
}

fn phase4_preset(f: &Fixture, preset: &str) {
    let db = database::open(&f.home).unwrap();
    db.execute(
        "UPDATE project_settings SET oversight=?1 WHERE project_id='p'",
        [preset],
    )
    .unwrap();
    db.execute(
        "UPDATE workflow_items SET state_json=json_set(state_json,'$.preset',?2) WHERE item_id=?1",
        params![f.item, preset],
    )
    .unwrap();
}
fn phase4_command(remote: &Fake, number: u64, body: &str) -> u64 {
    let mut r = remote.0.lock().unwrap();
    let id = 2000 + r.comments.len() as u64;
    let time = (chrono::Utc::now() + chrono::Duration::seconds(2)).to_rfc3339();
    r.comments.push(json!({"id":id,"user":{"id":7,"type":"User"},"body":body,"issue_url":format!("https://api.github.com/repos/o/r/issues/{number}"),"created_at":time,"updated_at":time}));
    id
}
fn phase4_plan_gate(f: &Fixture, item: &str) -> String {
    let mut db = database::open(&f.home).unwrap();
    let tx = db.transaction().unwrap();
    let s = workflow::state(&tx, item).unwrap().unwrap();
    let g = plantool_daemon::github::gates::request(
        &tx,
        item,
        s["plan_sha"].as_str().unwrap(),
        item,
        "plan-interrupt",
    )
    .unwrap();
    tx.execute("UPDATE gates SET state='open',created_at='2026-10-01T00:00:00Z',github_comment_id=50 WHERE id=?1",[&g]).unwrap();
    tx.commit().unwrap();
    g
}
fn phase4_docs() -> RoleDetails {
    RoleDetails::Docs{updates:None,body:"## What changed and why\nScoped fix\n## Where to look first\nsrc/a.rs\n## How to test\nRun focused tests\n## AI review rounds\nQA and Reviewer\n## Found during review\nNone\n## Evidence\nActual fake-runner artifacts\n".into()}
}
fn phase4_finish_phase(f: &Fixture, key: &str) {
    f.run("backend", None, vec![key.into()]);
    f.run("qa", None, vec![]);
    f.run(
        "reviewer",
        Some(RoleDetails::Security {
            commit: git(&f.root, &["rev-parse", "HEAD"]),
            passed: true,
            criteria: vec!["auth, sandbox, state and scope checked".into()],
        }),
        vec![],
    );
    f.run("docs", Some(phase4_docs()), vec![]);
}
fn phase4_milestone(f: &Fixture) -> String {
    let s = f.state();
    let mut db = database::open(&f.home).unwrap();
    let tx = db.transaction().unwrap();
    let g = plantool_daemon::github::milestones::request(
        &tx,
        &Gate {
            item: f.item.clone(),
            kind: "milestone".into(),
            target_digest: s["plan_sha"].as_str().unwrap().into(),
            target_sha: Some(git(&f.root, &["rev-parse", "HEAD"])),
            idempotency_key: format!("milestone:{}", s["sequence"]),
            thread_id: Some(f.item.clone()),
            interrupt_id: Some(format!("milestone:{}", s["sequence"])),
            summary: None,
            discussion_thread_id: None,
        },
    )
    .unwrap();
    tx.commit().unwrap();
    g
}
#[tokio::test]
async fn phase4_strict_research_is_a_separate_current_hash_owner_gate() {
    let f = Fixture::new();
    phase4_preset(&f, "strict");
    f.run(
        "research",
        Some(RoleDetails::Research {
            markdown: "Inspected architecture, risks and interfaces".into(),
        }),
        vec![],
    );
    assert_eq!(f.state()["step"], "research_gate");
    let mut db = database::open(&f.home).unwrap();
    let tx = db.transaction().unwrap();
    let sha = f.state()["research_sha"].as_str().unwrap().to_string();
    plantool_daemon::github::gates::request_revision(
        &tx,
        &f.item,
        &sha,
        &f.item,
        "research-interrupt",
        "research",
    )
    .unwrap();
    tx.commit().unwrap();
    let remote = Fake::default();
    plantool_daemon::github::gates::publish(&f.home, &remote, &f.repo)
        .await
        .unwrap();
    let issue: u64 = db
        .query_row(
            "SELECT github_issue_number FROM work_items WHERE id=?1",
            [&f.item],
            |r| r.get(0),
        )
        .unwrap();
    let wrong = phase4_command(
        &remote,
        issue,
        &format!("/plantool approve-plan sha256:{}", &sha[..12]),
    );
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, wrong)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "research_gate");
    let good = phase4_command(
        &remote,
        issue,
        &format!("/plantool approve-research sha256:{}", &sha[..12]),
    );
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, good)
        .await
        .unwrap();
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, good)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "plan");
    assert_eq!(f.state()["research_approved"], true);
    assert_eq!(
        db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(db
        .query_row(
            "SELECT approved_plan_revision_id FROM work_items WHERE id=?1",
            [&f.item],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn phase4_milestone_draft_head_binding_and_final_ready_keep_both_owner_gates() {
    let f = Fixture::new();
    phase4_preset(&f, "standard");
    f.planning();
    f.approve_plan();
    phase4_finish_phase(&f, "T1");
    assert_eq!(f.state()["step"], "milestone");
    let gate = phase4_milestone(&f);
    let remote = Fake::default();
    let git = FakeGit {
        remote: remote.clone(),
        base: f.base.clone(),
    };
    remote.0.lock().unwrap().lose_comment_reply = true;
    assert!(
        plantool_daemon::github::milestones::publish(&f.home, &remote, &f.repo, &git)
            .await
            .is_err()
    );
    plantool_daemon::github::milestones::publish(&f.home, &remote, &f.repo, &git)
        .await
        .unwrap();
    plantool_daemon::github::milestones::publish(&f.home, &remote, &f.repo, &git)
        .await
        .unwrap();
    {
        let r = remote.0.lock().unwrap();
        assert_eq!(r.pr.as_ref().unwrap()["draft"], true);
        assert_eq!(r.pushes, 1);
        assert_eq!(r.comments.len(), 1);
        assert!(r.comments[0]["body"]
            .as_str()
            .unwrap()
            .contains("/compare/"));
    }
    let head = plantool_daemon::git::git(&f.root, &["rev-parse", "HEAD"]).unwrap();
    remote.0.lock().unwrap().pr.as_mut().unwrap()["head"]["sha"] = json!("f".repeat(40));
    let stale = phase4_command(&remote, 2, "/plantool continue");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, stale)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "milestone");
    remote.0.lock().unwrap().pr.as_mut().unwrap()["head"]["sha"] = json!(head);
    let good = phase4_command(&remote, 2, "/plantool continue");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, good)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "pr");
    assert_eq!(
        database::open(&f.home)
            .unwrap()
            .query_row("SELECT state FROM gates WHERE id=?1", [gate], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "approved"
    );
    workflow::pr_wait(&f.home, &f.item, &f.item, "pr-final").unwrap();
    lifecycle::process_item(&f.home, &remote, &f.repo, &git, &f.item)
        .await
        .unwrap();
    assert_eq!(
        remote.0.lock().unwrap().pr.as_ref().unwrap()["draft"],
        false
    );
    assert_eq!(remote.0.lock().unwrap().merge_calls, 0);
    remote.0.lock().unwrap().reviews = vec![
        json!({"id":9000,"user":{"id":7,"type":"User"},"state":"APPROVED","commit_id":head,"submitted_at":(chrono::Utc::now()+chrono::Duration::seconds(3)).to_rfc3339()}),
    ];
    lifecycle::process_item(&f.home, &remote, &f.repo, &git, &f.item)
        .await
        .unwrap();
    assert_eq!(remote.0.lock().unwrap().merge_calls, 1);
}

fn phase4_seed_sprint(f: &Fixture) -> String {
    f.planning();
    phase4_plan_gate(f, &f.item);
    let first = f.state();
    let oldrev:String=database::open(&f.home).unwrap().query_row("SELECT id FROM revisions WHERE item_id=?1 AND doc_kind='plan' ORDER BY number DESC LIMIT 1",[&f.item],|r|r.get(0)).unwrap();
    for n in [2, 3] {
        let item = workflow::start(
            &f.home,
            "p",
            n,
            &format!("Item {n}"),
            "Scoped change",
            &f.base,
        )
        .unwrap();
        let db = database::open(&f.home).unwrap();
        let revision = format!("{item}:plan:{}", first["plan_sha"].as_str().unwrap());
        db.execute("INSERT INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) SELECT ?1,?2,doc_kind,number,content_sha256,created_by,created_at FROM revisions WHERE id=?3",params![revision,item,oldrev]).unwrap();
        db.execute("INSERT INTO plan_tasks SELECT ?1||':'||task_key,?1,task_key,area,title,phase,review_tag FROM plan_tasks WHERE revision_id=?2",params![revision,oldrev]).unwrap();
        let mut state = first.clone();
        state["item"] = json!(item);
        state["generation"] = json!(0);
        state["sequence"] = json!(0);
        db.execute(
            "UPDATE workflow_items SET state_json=?2 WHERE item_id=?1",
            params![item, state.to_string()],
        )
        .unwrap();
        phase4_plan_gate(f, &item);
    }
    let project = plantool_daemon::scheduler::ensure_project(&f.home, "p").unwrap();
    plantool_daemon::scheduler::wait(
        &f.home,
        &ProjectWait {
            item: project.clone(),
            thread_id: project.clone(),
            interrupt_id: "due-wait".into(),
            kind: "project".into(),
            idempotency_key: "due-wait".into(),
        },
    )
    .unwrap();
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-19T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    plantool_daemon::scheduler::tick(&f.home, now).unwrap();
    let db = database::open(&f.home).unwrap();
    let event: String = db
        .query_row(
            "SELECT id FROM project_events WHERE kind='sprint_planning_due'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    plantool_daemon::scheduler::ack(
        &f.home,
        &EventAck {
            project: "p".into(),
            event_id: event,
            next_step: Some("sprint_plan".into()),
            idempotency_key: "due-ack".into(),
        },
    )
    .unwrap();
    let dispatch = Dispatch {
        item: project.clone(),
        role: "pm".into(),
        generation: 1,
        inputs: json!({"step":"sprint_plan"}),
        plan_sha: "".into(),
        policy_version: 1,
        capability: "read".into(),
        idempotency_key: "pm-sprint".into(),
        thread_id: project.clone(),
        interrupt_id: "pm-turn".into(),
    };
    let attempt = dispatcher::admit(&f.home, &dispatch, 0).unwrap();
    db.execute(
        "UPDATE attempts SET state='running' WHERE id=?1",
        [&attempt.attempt_id],
    )
    .unwrap();
    dispatcher::report(
        &f.home,
        &RoleResult {
            item: project.clone(),
            role: "pm".into(),
            attempt_id: attempt.attempt_id,
            generation: 1,
            outcome: Outcome::Done,
            summary: "Three bounded items based on inspected candidates".into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: Some(RoleDetails::Sprint {
                items: vec![f.item.clone(), "p:2".into(), "p:3".into()]
                    .into_iter()
                    .map(|item| SprintChoice {
                        item,
                        estimate: "one focused change; PM-sized".into(),
                    })
                    .collect(),
                notes: "No imposed point ceiling; test prerequisites are available".into(),
            }),
        },
    )
    .unwrap();
    plantool_daemon::scheduler::publish(
        &f.home,
        &WorkflowPr {
            item: project.clone(),
            thread_id: project.clone(),
            interrupt_id: "publish".into(),
            idempotency_key: "publish".into(),
        },
    )
    .unwrap();
    project
}
fn phase4_release(f: &Fixture, project: &str, key: &str) {
    let db = database::open(&f.home).unwrap();
    let s = workflow::state(&db, project).unwrap().unwrap();
    plantool_daemon::scheduler::wait(
        &f.home,
        &ProjectWait {
            item: project.into(),
            thread_id: project.into(),
            interrupt_id: key.into(),
            kind: "sprint".into(),
            idempotency_key: key.into(),
        },
    )
    .unwrap();
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-19T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    plantool_daemon::scheduler::tick(&f.home, now).unwrap();
    let event:String=db.query_row("SELECT id FROM project_events WHERE kind='sprint_decision' AND delivered_at IS NOT NULL ORDER BY rowid DESC LIMIT 1",[],|r|r.get(0)).unwrap();
    plantool_daemon::scheduler::ack(
        &f.home,
        &EventAck {
            project: "p".into(),
            event_id: event,
            next_step: Some("start_items".into()),
            idempotency_key: format!("ack:{key}"),
        },
    )
    .unwrap();
    plantool_daemon::scheduler::start_items(
        &f.home,
        &WorkflowPr {
            item: project.into(),
            thread_id: project.into(),
            interrupt_id: key.into(),
            idempotency_key: format!("start:{key}"),
        },
    )
    .unwrap();
    assert!(s["step"] == "await_sprint_gate");
}
#[tokio::test]
async fn phase4_three_item_sprint_starts_only_current_snapshot_approvals_and_never_replays() {
    let f = Fixture::new();
    let project = phase4_seed_sprint(&f);
    let remote = Fake::default();
    remote.0.lock().unwrap().lose_comment_reply = true;
    assert!(
        plantool_daemon::github::sprints::publish(&f.home, &remote, &f.repo)
            .await
            .is_err()
    );
    plantool_daemon::github::sprints::publish(&f.home, &remote, &f.repo)
        .await
        .unwrap();
    let db = database::open(&f.home).unwrap();
    let number: u64 = db
        .query_row("SELECT tracking_issue_number FROM sprints", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        remote.0.lock().unwrap().issues[0]["title"],
        "Sprint 2026-W43"
    );
    assert_eq!(remote.0.lock().unwrap().comments.len(), 1);
    let bot = phase4_command(&remote, number, "/plantool approve-sprint");
    {
        let mut r = remote.0.lock().unwrap();
        r.comments.iter_mut().find(|c| c["id"] == bot).unwrap()["user"]["type"] = json!("Bot");
    }
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, bot)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "plan_gate");
    let one = phase4_command(&remote, number, "/plantool approve 2");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, one)
        .await
        .unwrap();
    assert_eq!(
        workflow::state(&db, "p:2").unwrap().unwrap()["step"],
        "plan_gate",
        "A comment alone does not dispatch before the graph start_items node"
    );
    phase4_release(&f, &project, "release-one");
    assert_eq!(
        workflow::state(&db, "p:2").unwrap().unwrap()["step"],
        "backend"
    );
    assert_eq!(f.state()["step"], "plan_gate");
    let all = phase4_command(&remote, number, "/plantool approve-sprint");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, all)
        .await
        .unwrap();
    phase4_release(&f, &project, "release-all");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, all)
        .await
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM decisions WHERE decision='approved'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM decision_targets", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(db.query_row("SELECT count(*) FROM outbox WHERE kind='worker.resume' AND json_extract(payload_json,'$.idempotency_key') LIKE 'sprint-start:%'",[],|r|r.get::<_,i64>(0)).unwrap(),3);
    assert_eq!(
        workflow::state(&db, &project).unwrap().unwrap()["step"],
        "project_wait"
    );
}
#[test]
fn phase4_unapproved_items_return_to_backlog_and_scheduler_wait_replay_is_idempotent() {
    let f = Fixture::new();
    let project = phase4_seed_sprint(&f);
    let db = database::open(&f.home).unwrap();
    let before: i64 = db
        .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))
        .unwrap();
    plantool_daemon::scheduler::wait(
        &f.home,
        &ProjectWait {
            item: project.clone(),
            thread_id: project,
            interrupt_id: "due-wait".into(),
            kind: "project".into(),
            idempotency_key: "due-wait".into(),
        },
    )
    .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
    let mut db = database::open(&f.home).unwrap();
    let tx = db.transaction().unwrap();
    plantool_daemon::scheduler::expire(&tx, "p", "p:2026-W44").unwrap();
    tx.commit().unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM work_items WHERE state='backlog'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn phase4_review_tag_edit_is_a_new_immutable_pending_revision() {
    let f = Fixture::new();
    phase4_preset(&f, "standard");
    f.planning();
    phase4_plan_gate(&f, &f.item);
    let old = f.state()["plan_sha"].as_str().unwrap().to_string();
    let new =
        plantool_daemon::routes::oversight::edit_tags(&f.home, &f.item, &old, &[false]).unwrap();
    assert_ne!(old, new);
    assert_eq!(f.state()["phases"][0]["tagged"], false);
    let db = database::open(&f.home).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM revisions WHERE item_id=?1 AND doc_kind='plan'",
            [&f.item],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert!(
        plantool_daemon::routes::oversight::edit_tags(&f.home, &f.item, &old, &[true]).is_err()
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    f.approve_plan();
    assert!(
        plantool_daemon::routes::oversight::edit_tags(&f.home, &f.item, &new, &[true]).is_err()
    );
}

#[tokio::test]
async fn phase4_two_phase_milestones_require_fresh_continue_for_each_head() {
    let f = Fixture::new();
    phase4_preset(&f, "standard");
    f.run(
        "research",
        Some(RoleDetails::Research {
            markdown: "Inspect the two bounded steps".into(),
        }),
        vec![],
    );
    f.run("plan",Some(RoleDetails::Plan{markdown:"# Plan\n### Phase 1 [review]\n- [ ] T1: [backend] First fix\n### Phase 2 [review]\n- [ ] T2: [backend] Second fix\n".into(),acceptance_criteria:vec!["AC1 T1 and AC2 T2 pass focused tests".into()],affected_files:vec!["src/a.rs".into(),"tests/**".into()]}),vec![]);
    f.run("plan_review", None, vec![]);
    f.approve_plan();
    phase4_finish_phase(&f, "T1");
    assert_eq!(f.state()["phase_index"], 0);
    assert!(!plantool_daemon::oversight::active_key(&f.state(), "T2"));
    phase4_milestone(&f);
    let remote = Fake::default();
    let transport = FakeGit {
        remote: remote.clone(),
        base: f.base.clone(),
    };
    plantool_daemon::github::milestones::publish(&f.home, &remote, &f.repo, &transport)
        .await
        .unwrap();
    let first = phase4_command(&remote, 2, "/plantool continue");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, first)
        .await
        .unwrap();
    assert_eq!(f.state()["phase_index"], 1);
    assert_eq!(f.state()["step"], "backend");
    phase4_finish_phase(&f, "T2");
    phase4_milestone(&f);
    plantool_daemon::github::milestones::publish(&f.home, &remote, &f.repo, &transport)
        .await
        .unwrap();
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, first)
        .await
        .unwrap();
    assert_eq!(
        f.state()["step"],
        "milestone",
        "The earlier comment cannot authorize the second stop"
    );
    {
        let r = remote.0.lock().unwrap();
        assert_eq!(r.pushes, 2);
        assert_eq!(
            r.comments
                .iter()
                .filter(|c| c["user"]["type"] == "Bot")
                .count(),
            2
        );
        assert!(r
            .comments
            .iter()
            .any(|c| c["body"].as_str().unwrap().contains("Milestone 2/2")));
        assert_eq!(r.pr.as_ref().unwrap()["draft"], true);
    }
    let next = phase4_command(&remote, 2, "/plantool continue");
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, next)
        .await
        .unwrap();
    assert_eq!(f.state()["step"], "pr");
}
#[tokio::test]
async fn phase4_sprint_revision_refresh_posts_a_new_snapshot_and_rejects_old_commands() {
    let f = Fixture::new();
    phase4_preset(&f, "standard");
    phase4_seed_sprint(&f);
    let remote = Fake::default();
    plantool_daemon::github::sprints::publish(&f.home, &remote, &f.repo)
        .await
        .unwrap();
    let db = database::open(&f.home).unwrap();
    let number: u64 = db
        .query_row("SELECT tracking_issue_number FROM sprints", [], |r| {
            r.get(0)
        })
        .unwrap();
    let stale = phase4_command(&remote, number, "/plantool approve-sprint");
    {
        let mut r = remote.0.lock().unwrap();
        let old = r.comments[0]["created_at"].clone();
        let c = r.comments.iter_mut().find(|c| c["id"] == stale).unwrap();
        c["created_at"] = old.clone();
        c["updated_at"] = old;
    }
    let hash = f.state()["plan_sha"].as_str().unwrap().to_owned();
    plantool_daemon::routes::oversight::edit_tags(&f.home, &f.item, &hash, &[false]).unwrap();
    plantool_daemon::github::sprints::refresh(&f.home, &f.repo).unwrap();
    plantool_daemon::github::sprints::publish(&f.home, &remote, &f.repo)
        .await
        .unwrap();
    assert_eq!(
        remote
            .0
            .lock()
            .unwrap()
            .comments
            .iter()
            .filter(|c| c["user"]["type"] == "Bot")
            .count(),
        2
    );
    plantool_daemon::github::gates::comment(&f.home, &remote, &f.repo, stale)
        .await
        .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}
