//! Immutable content, published before any transaction references it.
use anyhow::{ensure, Context};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub fn valid(sha: &str) -> bool {
    sha.len() == 64
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn path(home: &Path, sha: &str) -> anyhow::Result<PathBuf> {
    ensure!(valid(sha), "Invalid artifact hash");
    Ok(home.join("artifacts").join(sha))
}
pub fn read(home: &Path, sha: &str) -> anyhow::Result<Vec<u8>> {
    let path = path(home, sha)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    ensure!(file.metadata()?.is_file(), "Artifact is not a regular file");
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    ensure!(hex(&data) == sha, "Artifact checksum mismatch");
    Ok(data)
}
fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn put(home: &Path, data: &[u8]) -> anyhow::Result<String> {
    let sha = hex(data);
    let dir = home.join("artifacts");
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let destination = path(home, &sha)?;
    if destination.exists() {
        ensure!(read(home, &sha)? == data, "Artifact content collision");
        return Ok(sha);
    }
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    match temp.persist_noclobber(&destination) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(read(home, &sha)? == data, "Artifact collision")
        }
        Err(e) => return Err(e.error.into()),
    }
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(sha)
}
pub fn register(db: &Connection, sha: &str, size: usize) -> anyhow::Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO artifacts(sha256,size,created_at) VALUES(?1,?2,?3)",
        params![sha, size as i64, crate::database::unix_now()],
    )?;
    Ok(())
}
pub fn sweep_orphans(home: &Path, now: i64) -> anyhow::Result<usize> {
    let db = crate::database::open(home)?;
    let dir = home.join("artifacts");
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for file in fs::read_dir(dir)? {
        let file = file?;
        let sha = file.file_name().to_string_lossy().into_owned();
        if !valid(&sha) {
            continue;
        }
        let meta = fs::symlink_metadata(file.path())?;
        if !meta.is_file() {
            continue;
        }
        let modified = meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .context("Artifact timestamp predates epoch")?
            .as_secs() as i64;
        if now.saturating_sub(modified) < 3600 {
            continue;
        }
        let mut db = crate::database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let referenced:i64=tx.query_row("SELECT (SELECT count(*) FROM revisions WHERE content_sha256=?1)+(SELECT count(*) FROM artifact_refs WHERE sha256=?1)",[&sha],|r|r.get(0))?;
        if referenced == 0 {
            fs::remove_file(file.path())?;
            tx.execute("DELETE FROM artifacts WHERE sha256=?1", [&sha])?;
            removed += 1;
        }
        tx.commit()?;
    }
    drop(db);
    Ok(removed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_objects_are_verified_and_never_overwritten() {
        let t = tempfile::tempdir().unwrap();
        let sha = put(t.path(), b"plan").unwrap();
        assert_eq!(put(t.path(), b"plan").unwrap(), sha);
        assert_eq!(read(t.path(), &sha).unwrap(), b"plan");
        fs::write(path(t.path(), &sha).unwrap(), "corrupt").unwrap();
        assert!(put(t.path(), b"plan").is_err());
        assert!(read(t.path(), "../../auth").is_err());
    }
    #[test]
    fn fresh_orphans_are_kept_and_old_unreferenced_objects_are_swept() {
        let t = tempfile::tempdir().unwrap();
        let sha = put(t.path(), b"orphan").unwrap();
        assert_eq!(
            sweep_orphans(t.path(), crate::database::unix_now()).unwrap(),
            0
        );
        assert_eq!(
            sweep_orphans(t.path(), crate::database::unix_now() + 3601).unwrap(),
            1
        );
        assert!(!path(t.path(), &sha).unwrap().exists());
    }
}
#[cfg(test)]
mod consistency_tests {
    use super::*;
    #[test]
    fn failed_revision_commit_leaves_a_sweepable_orphan_but_published_revisions_survive() {
        let t = tempfile::tempdir().unwrap();
        let store = crate::store::SessionStore::new(t.path().join("sessions/repo/item"));
        let text = "# Plan\n";
        let rev = plantool_core::DocRevision {
            kind: plantool_core::DocKind::Plan,
            sha: plantool_core::sha256_hex(text),
            content: text.into(),
            captured_at: plantool_core::now(),
        };
        assert!(store.save_revision(&rev).is_err());
        assert!(path(t.path(), &rev.sha).unwrap().is_file());
        assert_eq!(
            sweep_orphans(t.path(), crate::database::unix_now() + 3601).unwrap(),
            1
        );
        let mut session: plantool_core::Session =
            serde_json::from_str(include_str!("../../core/tests/fixtures/v0.0.14/meta.json"))
                .unwrap();
        session.repo_slug = "repo".into();
        session.slug = "item".into();
        store.save_meta(&session).unwrap();
        store.save_revision(&rev).unwrap();
        assert_eq!(
            sweep_orphans(t.path(), crate::database::unix_now() + 3601).unwrap(),
            0
        );
        assert_eq!(read(t.path(), &rev.sha).unwrap(), text.as_bytes());
    }
}
