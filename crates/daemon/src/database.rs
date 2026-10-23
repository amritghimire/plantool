//! Single daemon-owned SQLite database. Worker databases are never opened here.
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn path(home: &Path) -> PathBuf {
    home.join("db/plantool.db")
}
pub fn open(home: &Path) -> anyhow::Result<Connection> {
    fs::create_dir_all(home.join("db"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(home.join("db"), fs::Permissions::from_mode(0o700))?;
    }
    let mut db = Connection::open(path(home))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path(home), fs::Permissions::from_mode(0o600))?;
    }
    db.busy_timeout(Duration::from_millis(5000))?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "foreign_keys", true)?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(version <= 3, "Database is newer than this PlanTool version");
    if version == 0 {
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // A second connection may have waited for the same initial migration.
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 0 {
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            tx.pragma_update(None, "user_version", 1)?;
        }
        tx.commit()?;
    }
    if version < 2 {
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 2 {
            tx.execute_batch(include_str!("../migrations/002_worker.sql"))?;
            tx.pragma_update(None, "user_version", 2)?;
        }
        tx.commit()?;
    }
    if version < 3 {
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if current < 3 {
            tx.execute_batch(include_str!("../migrations/003_github.sql"))?;
            tx.pragma_update(None, "user_version", 3)?;
        }
        tx.commit()?;
    }
    Ok(db)
}
/// Run before opening auth or the registry. No old JSON is imported.
pub fn initialize(home: &Path) -> anyhow::Result<()> {
    if path(home).exists() {
        let db = Connection::open(path(home))?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version >= 1 {
            open(home)?;
            return Ok(());
        }
    }
    let old = home.join("sessions");
    let legacy = old.is_dir();
    let backup = if legacy {
        let name = home
            .file_name()
            .context("PlanTool home has no directory name")?
            .to_string_lossy();
        let parent = home.parent().context("PlanTool home has no parent")?;
        let target = parent.join(format!("{name}.v0-backup-{}", unix_now()));
        ensure!(
            !target.exists(),
            "Backup destination already exists: {}",
            target.display()
        );
        fs::rename(home, &target)
            .with_context(|| format!("Backing up old PlanTool home to {}", target.display()))?;
        Some(target)
    } else {
        None
    };
    let db = open(home)?;
    if let Some(backup) = backup {
        set(
            &db,
            "clean_break_banner",
            &serde_json::json!({"backup":backup}),
        )?;
    }
    Ok(())
}
pub fn get<T: serde::de::DeserializeOwned>(
    db: &Connection,
    key: &str,
) -> anyhow::Result<Option<T>> {
    let s: Option<String> = db
        .query_row("SELECT value_json FROM settings WHERE key=?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    s.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
pub fn set(db: &Connection, key: &str, value: &impl serde::Serialize) -> anyhow::Result<()> {
    db.execute("INSERT INTO settings(key,value_json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",params![key,serde_json::to_string(value)?])?;
    Ok(())
}
pub fn audit(
    db: &Connection,
    actor: &str,
    action: &str,
    subject: &str,
    metadata: &serde_json::Value,
) -> anyhow::Result<()> {
    db.execute("INSERT INTO audit(at,actor_kind,action,subject,digest,metadata_json) VALUES(?1,?2,?3,?4,?5,?6)",params![plantool_core::now(),actor,action,subject,plantool_core::sha256_hex(&metadata.to_string()),metadata.to_string()])?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_is_versioned_relational_and_audit_is_append_only() {
        let t = tempfile::tempdir().unwrap();
        let db = open(t.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.pragma_query_value(None, "busy_timeout", |r| r.get::<_, i64>(0))
                .unwrap(),
            5000
        );
        assert_eq!(
            db.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        audit(&db, "test", "test", "test", &serde_json::json!({})).unwrap();
        assert!(db.execute("DELETE FROM audit", []).is_err());
        assert!(db.execute("UPDATE audit SET action='changed'", []).is_err());
        assert!(db
            .execute(
                "INSERT INTO task_progress VALUES('missing','T1','done',NULL,'now')",
                []
            )
            .is_err());
        assert!(db.execute("INSERT INTO leases(id) VALUES(2)", []).is_err());
    }
    #[test]
    fn clean_break_backs_up_whole_home_once_without_import() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("plantool");
        fs::create_dir_all(home.join("sessions/repo/old")).unwrap();
        fs::write(home.join("sessions/repo/old/meta.json"), "{}").unwrap();
        fs::write(home.join("auth-fixture"), "old").unwrap();
        initialize(&home).unwrap();
        let db = open(&home).unwrap();
        let banner: serde_json::Value = get(&db, "clean_break_banner").unwrap().unwrap();
        let backup = PathBuf::from(banner["backup"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(backup.join("auth-fixture")).unwrap(),
            "old"
        );
        assert!(!home.join("sessions").exists());
        assert_eq!(
            db.query_row("SELECT count(*) FROM work_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        initialize(&home).unwrap();
        assert_eq!(
            get::<serde_json::Value>(&db, "clean_break_banner").unwrap(),
            Some(banner)
        );
    }
}

/// An interrupted Personal attempt cannot silently become a successful run.
pub fn reconcile_personal(home: &Path) -> anyhow::Result<()> {
    let mut db = open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let ids = {
        let mut q = tx.prepare(
            "SELECT id,item_id FROM attempts WHERE state IN('starting','running','waiting','idle') AND item_id IN(SELECT id FROM work_items WHERE kind='personal')",
        )?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, item) in ids {
        tx.execute("UPDATE attempts SET state='recovery_needed',ended_at=?2,run_json=json_set(run_json,'$.status','failed','$.ended_at',?2,'$.error','Recovery needed: daemon stopped before this attempt completed') WHERE id=?1",params![id,plantool_core::now()])?;
        tx.execute(
            "UPDATE work_items SET outcome='recovery_needed' WHERE id=?1",
            [item],
        )?;
        audit(
            &tx,
            "daemon",
            "attempt.recovery_needed",
            &id,
            &serde_json::json!({}),
        )?;
    }
    tx.commit()?;
    Ok(())
}
/// Record the owner's Personal publication intent before invoking external tools.
pub fn record_intent(
    home: &Path,
    aggregate: &str,
    kind: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<String> {
    let key = plantool_core::sha256_hex(&format!("{aggregate}\n{kind}\n{payload}"));
    let mut db = open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let inserted=tx.execute("INSERT OR IGNORE INTO outbox(id,aggregate_id,kind,payload_json,created_at,idempotency_key) VALUES(?1,?2,?3,?4,?5,?1)",params![key,aggregate,kind,payload.to_string(),plantool_core::now()])?;
    tx.execute("UPDATE outbox SET attempts=attempts+1 WHERE id=?1", [&key])?;
    if inserted > 0 {
        audit(&tx, "human", "external.intent", aggregate, payload)?;
    }
    tx.commit()?;
    Ok(key)
}
pub fn finish_intent(home: &Path, id: &str, result: &serde_json::Value) -> anyhow::Result<()> {
    let mut db = open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    ensure!(
        tx.execute(
            "UPDATE outbox SET delivered_at=?2,result_json=?3 WHERE id=?1",
            params![id, plantool_core::now(), result.to_string()]
        )? == 1,
        "Unknown external intent"
    );
    audit(&tx, "daemon", "external.delivered", id, result)?;
    tx.commit()?;
    Ok(())
}
/// Domain writes and receipt commit together; replay never repeats the callback.
pub fn inbox_once(
    home: &Path,
    source: &str,
    event: &str,
    apply: impl FnOnce(&rusqlite::Transaction<'_>) -> anyhow::Result<serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    let mut db = open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let prior: Option<String> = tx
        .query_row(
            "SELECT result FROM inbox WHERE source=?1 AND external_event_id=?2",
            params![source, event],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prior) = prior {
        return Ok(serde_json::from_str(&prior)?);
    }
    let result = apply(&tx)?;
    tx.execute(
        "INSERT INTO inbox(source,external_event_id,processed_at,result) VALUES(?1,?2,?3,?4)",
        params![source, event, plantool_core::now(), result.to_string()],
    )?;
    tx.commit()?;
    Ok(result)
}
#[cfg(test)]
mod consistency_tests {
    use super::*;
    #[test]
    fn inbox_replays_do_not_repeat_domain_writes_and_errors_rollback() {
        let t = tempfile::tempdir().unwrap();
        let result = inbox_once(t.path(), "test", "one", |tx| {
            set(tx, "counter", &1)?;
            Ok(serde_json::json!({"count":1}))
        })
        .unwrap();
        assert_eq!(
            inbox_once(t.path(), "test", "one", |_| panic!("replayed callback")).unwrap(),
            result
        );
        assert!(inbox_once(t.path(), "test", "two", |tx| {
            set(tx, "counter", &2)?;
            anyhow::bail!("abort")
        })
        .is_err());
        assert_eq!(
            get::<i64>(&open(t.path()).unwrap(), "counter").unwrap(),
            Some(1)
        );
    }
    #[test]
    fn intents_are_durable_and_retries_share_identity() {
        let t = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({"head":"abc"});
        let id = record_intent(t.path(), "repo/item", "personal.push", &payload).unwrap();
        let same = record_intent(t.path(), "repo/item", "personal.push", &payload).unwrap();
        assert_eq!(id, same);
        let db = open(t.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT attempts FROM outbox WHERE id=?1", [&id], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        finish_intent(t.path(), &id, &serde_json::json!({"ok":true})).unwrap();
        assert!(db
            .query_row("SELECT delivered_at IS NOT NULL FROM outbox", [], |r| r
                .get::<_, bool>(0))
            .unwrap());
    }
}
/// Read-back on a Personal publication retry resolves earlier uncertain intents.
pub fn reconcile_pr_intents(
    home: &Path,
    item: &str,
    repository: &str,
    head: &str,
    result: &serde_json::Value,
) -> anyhow::Result<()> {
    let mut db = open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute("UPDATE outbox SET delivered_at=?4,result_json=?5 WHERE aggregate_id=?1 AND kind='personal.pr.create' AND delivered_at IS NULL AND json_extract(payload_json,'$.repository')=?2 AND json_extract(payload_json,'$.head')=?3",params![item,repository,head,plantool_core::now(),result.to_string()])?;
    tx.commit()?;
    Ok(())
}
