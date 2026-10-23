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
    db.busy_timeout(Duration::from_millis(5000))?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.pragma_update(None, "foreign_keys", true)?;
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    ensure!(version <= 1, "Database is newer than this PlanTool version");
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
    Ok(db)
}
/// Run before opening auth or the registry. No old JSON is imported.
pub fn initialize(home: &Path) -> anyhow::Result<()> {
    if path(home).exists() {
        open(home)?;
        return Ok(());
    }
    let old = home.join("sessions");
    let legacy = old.is_dir()
        && fs::read_dir(&old)?.filter_map(Result::ok).any(|repo| {
            fs::read_dir(repo.path()).ok().is_some_and(|items| {
                items
                    .filter_map(Result::ok)
                    .any(|item| item.path().join("meta.json").is_file())
            })
        });
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
