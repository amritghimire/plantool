//! Retain permanent records; prune only disposable payload references.
use crate::{artifacts, database, store::SessionStore};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Policy {
    pub transcript_days: u32,
    pub evidence_days: u32,
    pub disk_cap_bytes: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            transcript_days: 30,
            evidence_days: 90,
            disk_cap_bytes: 20 * 1024 * 1024 * 1024,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.transcript_days <= 36500 && self.evidence_days <= 36500,
            "Retention must be at most 100 years"
        );
        anyhow::ensure!(
            self.disk_cap_bytes > 0 && self.disk_cap_bytes <= i64::MAX as u64,
            "Invalid disk cap"
        );
        Ok(())
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub transcripts_removed: usize,
    pub evidence_removed: usize,
    pub bytes_remaining: u64,
    pub over_cap: bool,
    pub cleaned_items: Vec<String>,
    pub warnings: Vec<String>,
}
pub fn policy(home: &Path) -> anyhow::Result<Policy> {
    Ok(database::get(&database::open(home)?, "retention_policy")?.unwrap_or_default())
}
pub fn save_policy(home: &Path, value: &Policy) -> anyhow::Result<()> {
    value.validate()?;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    database::set(&tx, "retention_policy", value)?;
    database::audit(
        &tx,
        "human",
        "retention.policy",
        "global",
        &serde_json::to_value(value)?,
    )?;
    tx.commit()?;
    Ok(())
}
fn size(home: &Path) -> anyhow::Result<u64> {
    fn walk(path: &Path) -> anyhow::Result<u64> {
        if !path.exists() {
            return Ok(0);
        }
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_symlink() {
            return Ok(0);
        }
        if meta.is_file() {
            return Ok(meta.len());
        }
        let mut total = 0;
        for child in std::fs::read_dir(path)? {
            total += walk(&child?.path())?
        }
        Ok(total)
    }
    walk(home)
}
fn remove_transcript(home: &Path, id: &str) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let pinned: bool = tx.query_row(
        "SELECT coalesce((SELECT pinned FROM runs_summary WHERE attempt_id=?1),0)",
        [id],
        |r| r.get(0),
    )?;
    if !pinned {
        tx.execute(
            "UPDATE proxy_calls SET state='expired' WHERE attempt_id=?1 AND state='completed'",
            [id],
        )?;
        tx.execute("DELETE FROM run_events WHERE attempt_id=?1", [id])?;
        tx.execute(
            "DELETE FROM artifact_refs WHERE owner_type IN('transcript','trace') AND owner_id=?1",
            [id],
        )?;
        database::audit(
            &tx,
            "daemon",
            "retention.transcript",
            id,
            &serde_json::json!({}),
        )?;
    }
    tx.commit()?;
    Ok(())
}
/// Delete only after references were committed away. Serialize with publishers.
fn collect_unreferenced(home: &Path) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let hashes = {
        let mut q=tx.prepare("SELECT sha256 FROM artifacts a WHERE NOT EXISTS(SELECT 1 FROM revisions WHERE content_sha256=a.sha256) AND NOT EXISTS(SELECT 1 FROM artifact_refs WHERE sha256=a.sha256)")?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for sha in hashes {
        let file = artifacts::path(home, &sha)?;
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        };
        tx.execute("DELETE FROM artifacts WHERE sha256=?1", [sha])?;
    }
    tx.commit()?;
    Ok(())
}
pub fn run(home: &Path, now: i64) -> anyhow::Result<Report> {
    let policy = policy(home)?;
    let db = database::open(home)?;
    let mut report = Report::default();
    let old_runs = {
        let mut q=db.prepare("SELECT a.id FROM attempts a JOIN work_items i ON i.id=a.item_id JOIN runs_summary s ON s.attempt_id=a.id WHERE s.pinned=0 AND i.closed_at IS NOT NULL AND i.closed_at<=?1 AND a.state NOT IN('starting','running','waiting','idle') AND EXISTS(SELECT 1 FROM artifact_refs r WHERE r.owner_type IN('transcript','trace') AND r.owner_id=a.id) ORDER BY a.started_at")?;
        let rows = q
            .query_map([now - i64::from(policy.transcript_days) * 86400], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for id in old_runs {
        remove_transcript(home, &id)?;
        report.transcripts_removed += 1;
    }
    let evidence = {
        let mut q=db.prepare("SELECT r.owner_id,r.kind FROM artifact_refs r JOIN work_items i ON i.id=r.owner_id WHERE r.owner_type='evidence' AND i.merged_at IS NOT NULL AND i.merged_at<=?1 AND coalesce(i.outcome,'') NOT IN('failed','recovery_needed') AND NOT EXISTS(SELECT 1 FROM attempts a JOIN runs_summary s ON s.attempt_id=a.id WHERE a.item_id=i.id AND s.pinned=1)")?;
        let rows = q
            .query_map([now - i64::from(policy.evidence_days) * 86400], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (item, kind) in evidence {
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let deleted=tx.execute("DELETE FROM artifact_refs WHERE owner_type='evidence' AND owner_id=?1 AND kind=?2 AND NOT EXISTS(SELECT 1 FROM attempts a JOIN runs_summary s ON s.attempt_id=a.id WHERE a.item_id=?1 AND s.pinned=1)",params![item,kind])?;
        database::audit(
            &tx,
            "daemon",
            "retention.evidence",
            &item,
            &serde_json::json!({"kind":kind}),
        )?;
        tx.commit()?;
        report.evidence_removed += deleted;
        if deleted > 0 {
            if let Some(name) = kind.strip_prefix("attachment:") {
                if !name.is_empty()
                    && name != "."
                    && name != ".."
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
                {
                    let active: Option<(String,String)> = db.query_row("SELECT repo_slug,slug FROM work_items WHERE id=?1 AND deleted_at IS NULL", [&item], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                    if let Some((repo, slug)) = active {
                        let file = crate::store::session_dir(home, &repo, &slug)
                            .join("attachments")
                            .join(name);
                        match std::fs::remove_file(file) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => report.warnings.push(e.to_string()),
                        }
                    }
                }
            }
        }
    }
    collect_unreferenced(home)?;
    let mut bytes = size(home)?;
    if bytes > policy.disk_cap_bytes {
        let candidates = {
            let mut q=db.prepare("SELECT a.id FROM attempts a JOIN runs_summary s ON s.attempt_id=a.id WHERE s.pinned=0 AND a.state NOT IN('starting','running','waiting','idle') AND EXISTS(SELECT 1 FROM artifact_refs r WHERE r.owner_type IN('transcript','trace') AND r.owner_id=a.id) ORDER BY a.started_at,a.id")?;
            let rows = q
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for id in candidates {
            if bytes <= policy.disk_cap_bytes {
                break;
            }
            remove_transcript(home, &id)?;
            collect_unreferenced(home)?;
            bytes = size(home)?;
            report.transcripts_removed += 1;
        }
    }
    let items = {
        let mut q=db.prepare("SELECT id,session_json FROM work_items i WHERE (i.merged_at IS NOT NULL OR i.state='declined') AND i.deleted_at IS NULL AND NOT EXISTS(SELECT 1 FROM attempts a WHERE a.item_id=i.id AND a.state IN('starting','running','waiting','idle'))")?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (_id, json) in items {
        let session: plantool_core::Session = serde_json::from_str(&json)?;
        if session.worktree.is_none() {
            continue;
        }
        match cleanup_worktree(home, &session) {
            Ok(()) => report.cleaned_items.push(session.key()),
            Err(e) => report.warnings.push(format!("{}: {e}", session.key())),
        }
    }
    report.bytes_remaining = size(home)?;
    report.over_cap = report.bytes_remaining > policy.disk_cap_bytes;
    if report.over_cap {
        report.warnings.push("Disk cap remains exceeded: permanent records, pinned runs, active runs and protected evidence are retained".into());
    }
    Ok(report)
}
fn cleanup_worktree(home: &Path, session: &plantool_core::Session) -> anyhow::Result<()> {
    let Some(worktree) = &session.worktree else {
        return Ok(());
    };
    if worktree.exists() {
        crate::git::verify_worktree(&session.repo, worktree, Some(&session.slug))?;
        anyhow::ensure!(
            worktree.canonicalize()? != session.repo.root.canonicalize()?,
            "Refusing to remove the main checkout"
        );
        anyhow::ensure!(
            crate::git::git(worktree, &["status", "--porcelain"])?.is_empty(),
            "Worktree has uncommitted changes; automatic cleanup paused"
        );
        let patch = crate::git::git(worktree, &["diff", &session.base, "HEAD"])?;
        let store = SessionStore::new(crate::store::session_dir(
            home,
            &session.repo_slug,
            &session.slug,
        ));
        store.save_evidence("worktree-final.patch", patch.as_bytes())?;
        crate::git::git(
            &session.repo.root,
            &["worktree", "remove", &worktree.to_string_lossy()],
        )?;
    }
    // Only the known per-item branch, never a base/default branch or an adopted branch.
    if session.slug != session.base && session.slug != session.repo.branch {
        let exists = crate::git::git(
            &session.repo.root,
            &[
                "show-ref",
                "--verify",
                &format!("refs/heads/{}", session.slug),
            ],
        )
        .is_ok();
        if exists {
            crate::git::git(&session.repo.root, &["branch", "-D", &session.slug])?;
        }
    }
    let mut saved = session.clone();
    saved.worktree = None;
    let store = SessionStore::new(crate::store::session_dir(
        home,
        &session.repo_slug,
        &session.slug,
    ));
    store.save_meta(&saved)?;
    Ok(())
}
pub fn set_pin(home: &Path, item: &str, id: &str, pinned: bool) -> anyhow::Result<()> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND item_id=?2 AND hidden=0)",
        params![id, item],
        |r| r.get(0),
    )?;
    anyhow::ensure!(exists, "Run not found");
    tx.execute(
        "UPDATE runs_summary SET pinned=?2 WHERE attempt_id=?1",
        params![id, pinned],
    )?;
    database::audit(
        &tx,
        "human",
        "run.pin",
        id,
        &serde_json::json!({"pinned":pinned}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn pinned(home: &Path, id: &str) -> anyhow::Result<bool> {
    Ok(database::open(home)?
        .query_row(
            "SELECT pinned FROM runs_summary WHERE attempt_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}
#[cfg(test)]
mod tests {
    use super::*;
    use plantool_core::{DocKind, DocRevision, Run, RunStatus, Session, Stage, State};
    fn fixture() -> (tempfile::TempDir, SessionStore, Session) {
        let t = tempfile::tempdir().unwrap();
        let store = SessionStore::new(t.path().join("sessions/repo/item"));
        let mut s: Session =
            serde_json::from_str(include_str!("../../core/tests/fixtures/v0.0.14/meta.json"))
                .unwrap();
        s.repo_slug = "repo".into();
        s.slug = "item".into();
        s.worktree = None;
        s.repo.root = t.path().join("repo");
        s.repo.common_dir = s.repo.root.join(".git");
        store.save_meta(&s).unwrap();
        store.save_state(&State::default()).unwrap();
        (t, store, s)
    }
    fn saved_run(store: &SessionStore, id: &str, started: &str, payload: usize) {
        let mut r: Run =
            serde_json::from_str(include_str!("../../core/tests/fixtures/v0.0.14/run.json"))
                .unwrap();
        r.id = id.into();
        r.started_at = started.into();
        r.status = RunStatus::Stopped;
        r.ended_at = Some(plantool_core::now());
        store.save_run(&r).unwrap();
        store.append_run_event(id,&serde_json::json!({"seq":1,"event":{"text":format!("{id}{}","x".repeat(payload))}}).to_string()).unwrap();
    }
    fn close(store: &SessionStore) {
        store
            .save_state(&State {
                stage: Stage::Done,
                ..State::default()
            })
            .unwrap();
    }
    #[test]
    fn transcript_age_and_pins_never_remove_permanent_revisions_or_summaries() {
        let (t, store, _) = fixture();
        saved_run(&store, "old", "2024-01-01T00:00:00Z", 10);
        saved_run(&store, "pinned", "2024-01-01T00:00:01Z", 10);
        set_pin(t.path(), &store.key(), "pinned", true).unwrap();
        close(&store);
        let text = "# Approved plan\n";
        let sha = plantool_core::sha256_hex(text);
        store
            .save_revision(&DocRevision {
                kind: DocKind::Plan,
                sha: sha.clone(),
                content: text.into(),
                captured_at: plantool_core::now(),
            })
            .unwrap();
        let now = database::unix_now();
        assert_eq!(
            run(t.path(), now + 29 * 86400).unwrap().transcripts_removed,
            0
        );
        let report = run(t.path(), now + 31 * 86400).unwrap();
        assert_eq!(report.transcripts_removed, 1);
        assert!(store.read_run_events("old", 0).unwrap().is_empty());
        assert!(!store.read_run_events("pinned", 0).unwrap().is_empty());
        assert_eq!(artifacts::read(t.path(), &sha).unwrap(), text.as_bytes());
        let db = database::open(t.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM runs_summary", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert!(db.execute("DELETE FROM audit", []).is_err());
    }
    #[test]
    fn evidence_expires_after_merge_but_failed_and_recovery_items_are_preserved() {
        for outcome in [None, Some("failed"), Some("recovery_needed")] {
            let (t, store, mut s) = fixture();
            let sha = store
                .save_evidence("test-result", b"local evidence")
                .unwrap();
            s.pull_request = Some(plantool_core::PullRequest {
                number: 1,
                url: "https://github.com/test/repo/pull/1".into(),
                state: "MERGED".into(),
                draft: false,
                updated_at: plantool_core::now(),
            });
            store.save_meta(&s).unwrap();
            if let Some(outcome) = outcome {
                store
                    .db()
                    .unwrap()
                    .execute("UPDATE work_items SET outcome=?1", [outcome])
                    .unwrap();
            }
            let now = database::unix_now();
            assert_eq!(run(t.path(), now + 89 * 86400).unwrap().evidence_removed, 0);
            let report = run(t.path(), now + 91 * 86400).unwrap();
            assert_eq!(report.evidence_removed, usize::from(outcome.is_none()));
            assert_eq!(
                artifacts::path(t.path(), &sha).unwrap().exists(),
                outcome.is_some()
            );
            assert_eq!(
                store
                    .db()
                    .unwrap()
                    .query_row("SELECT count(*) FROM external_bindings", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
    #[test]
    fn cap_discards_oldest_unpinned_transcripts_first() {
        let (t, store, _) = fixture();
        saved_run(&store, "pinned", "2023-01-01T00:00:00Z", 1_000_000);
        saved_run(&store, "old", "2024-01-01T00:00:00Z", 1_000_000);
        saved_run(&store, "new", "2025-01-01T00:00:00Z", 1_000_000);
        set_pin(t.path(), &store.key(), "pinned", true).unwrap();
        let cap = size(t.path()).unwrap() - 500_000;
        save_policy(
            t.path(),
            &Policy {
                disk_cap_bytes: cap,
                ..Default::default()
            },
        )
        .unwrap();
        let report = run(t.path(), database::unix_now()).unwrap();
        assert_eq!(report.transcripts_removed, 1);
        assert!(store.read_run_events("old", 0).unwrap().is_empty());
        assert!(!store.read_run_events("new", 0).unwrap().is_empty());
        assert!(!store.read_run_events("pinned", 0).unwrap().is_empty());
    }
    #[test]
    fn restart_marks_unknown_outcomes_for_recovery() {
        let (t, store, _) = fixture();
        saved_run(&store, "interrupted", "2024-01-01T00:00:00Z", 10);
        store
            .db()
            .unwrap()
            .execute(
                "UPDATE attempts SET state='running' WHERE id='interrupted'",
                [],
            )
            .unwrap();
        database::reconcile_personal(t.path()).unwrap();
        let runs = store.load_runs().unwrap();
        assert_eq!(runs[0].status, RunStatus::Failed);
        assert!(runs[0].error.as_ref().unwrap().contains("Recovery needed"));
        assert_eq!(
            store
                .db()
                .unwrap()
                .query_row("SELECT outcome FROM work_items", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "recovery_needed"
        );
    }
    #[test]
    fn cleanup_removes_only_clean_owned_worktrees_and_local_branches() {
        for dirty in [false, true] {
            let (t, store, mut s) = fixture();
            std::fs::create_dir_all(&s.repo.root).unwrap();
            for args in [
                vec!["init", "-q", "-b", "main"],
                vec!["config", "user.email", "t@t"],
                vec!["config", "user.name", "t"],
                vec!["commit", "--allow-empty", "-qm", "initial"],
            ] {
                assert!(std::process::Command::new("git")
                    .current_dir(&s.repo.root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success());
            }
            s.repo = crate::git::detect_checkout(&s.repo.root).unwrap();
            s.base = "main".into();
            let worktree = t.path().join("worktree");
            crate::git::worktree_add(&s.repo, &worktree, "item", "main").unwrap();
            if dirty {
                std::fs::write(worktree.join("unsaved"), "owner work").unwrap();
            }
            s.worktree = Some(worktree.clone());
            s.pull_request = Some(plantool_core::PullRequest {
                number: 1,
                url: "https://github.com/test/repo/pull/1".into(),
                state: "MERGED".into(),
                draft: false,
                updated_at: plantool_core::now(),
            });
            store.save_meta(&s).unwrap();
            let report = run(t.path(), database::unix_now()).unwrap();
            assert_eq!(worktree.exists(), dirty);
            assert_eq!(report.cleaned_items.len(), usize::from(!dirty));
            assert_eq!(
                crate::git::git(&s.repo.root, &["branch", "--show-current"]).unwrap(),
                "main"
            );
            if dirty {
                assert!(!report.warnings.is_empty());
            } else {
                assert!(crate::git::git(
                    &s.repo.root,
                    &["show-ref", "--verify", "refs/heads/item"]
                )
                .is_err());
            }
        }
    }
}
