use plantool_core::{DocKind, DocRevision, Run, Session, State};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
    pub protocol: u32,
    pub version: String,
}

pub fn sessions_dir(home: &Path) -> PathBuf {
    home.join("sessions")
}

pub fn session_dir(home: &Path, repo_slug: &str, slug: &str) -> PathBuf {
    sessions_dir(home).join(repo_slug).join(slug)
}

pub fn doc_path(dir: &Path, kind: DocKind) -> PathBuf {
    dir.join("docs").join(kind.file_name())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic(path, &bytes)?;
    Ok(())
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn write_daemon_info(home: &Path, info: &DaemonInfo) -> anyhow::Result<()> {
    write_json(&home.join("daemon.json"), info)
}

pub fn read_daemon_info(home: &Path) -> anyhow::Result<Option<DaemonInfo>> {
    read_json(&home.join("daemon.json"))
}

pub fn remove_daemon_info(home: &Path) -> std::io::Result<()> {
    match fs::remove_file(home.join("daemon.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub struct SessionStore {
    pub dir: PathBuf,
}
impl SessionStore {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(dir.join("docs"));
        Self { dir }
    }
    pub fn home(&self) -> &Path {
        self.dir
            .ancestors()
            .nth(3)
            .expect("session path under home/sessions/project/item")
    }
    pub fn key(&self) -> String {
        format!(
            "{}/{}",
            self.dir
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            self.dir.file_name().unwrap().to_string_lossy()
        )
    }
    pub fn db(&self) -> anyhow::Result<rusqlite::Connection> {
        crate::database::open(self.home())
    }
    pub fn doc_path(&self, kind: DocKind) -> PathBuf {
        self.dir.join("docs").join(kind.file_name())
    }
    pub fn save_meta(&self, s: &Session) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let project = plantool_core::sha256_hex(&s.repo.common_dir.to_string_lossy());
        tx.execute(
            "INSERT OR IGNORE INTO projects(id,local_checkout,created_at) VALUES(?1,?2,?3)",
            params![project, s.repo.root.to_string_lossy(), s.created_at],
        )?;
        let changed=tx.execute("INSERT INTO work_items(id,project_id,title,created_at,repo_slug,slug,session_json) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET title=excluded.title,session_json=excluded.session_json WHERE work_items.deleted_at IS NULL",params![self.key(),project,s.title,s.created_at,s.repo_slug,s.slug,serde_json::to_string(s)?])?;
        anyhow::ensure!(changed == 1, "Session has been removed");
        crate::database::audit(
            &tx,
            "daemon",
            "session.save",
            &self.key(),
            &serde_json::to_value(s)?,
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn load_meta(&self) -> anyhow::Result<Option<Session>> {
        let db = self.db()?;
        let s: Option<String> = db
            .query_row(
                "SELECT session_json FROM work_items WHERE id=?1 AND deleted_at IS NULL",
                [self.key()],
                |r| r.get(0),
            )
            .optional()?;
        s.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn save_state(&self, s: &State) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT state_json FROM work_items WHERE id=?1 AND deleted_at IS NULL",
                [self.key()],
                |r| r.get(0),
            )
            .optional()?;
        anyhow::ensure!(previous.is_some(), "Session has been removed");
        let old: serde_json::Value = serde_json::from_str(previous.as_deref().unwrap())?;
        let approval = if let Some(a) = &s.approved {
            tx.query_row("SELECT id FROM revisions WHERE item_id=?1 AND doc_kind='plan' AND content_sha256=?2",params![self.key(),a.sha],|r|r.get::<_,String>(0)).optional()?
        } else {
            None
        };
        let now = crate::database::unix_now();
        let closed = (s.stage == plantool_core::Stage::Done).then_some(now);
        tx.execute("UPDATE work_items SET state=?2,state_json=?3,approved_plan_revision_id=?4,closed_at=coalesce(closed_at,?5) WHERE id=?1",params![self.key(),s.stage.to_string(),serde_json::to_string(s)?,approval,closed])?;
        if let Some(a) = &s.approved {
            if old["approved"]["sha"].as_str() != Some(&a.sha)
                || old["approved"]["at"].as_str() != Some(&a.at)
            {
                let id = uuid::Uuid::new_v4().to_string();
                tx.execute("INSERT INTO gates(id,item_id,kind,target_digest,target_sha,state,created_at,closed_at) VALUES(?1,?2,'plan',?3,?3,'approved',?4,?4)",params![id,self.key(),a.sha,a.at])?;
                tx.execute("INSERT INTO decisions(id,gate_id,source,source_event_id,decision,created_at) VALUES(?1,?1,'plantool',?1,'approved',?2)",params![id,a.at])?;
            }
        }
        crate::database::audit(
            &tx,
            "daemon",
            "state.save",
            &self.key(),
            &serde_json::to_value(s)?,
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn load_state(&self) -> anyhow::Result<State> {
        let s: Option<String> = self
            .db()?
            .query_row(
                "SELECT state_json FROM work_items WHERE id=?1",
                [self.key()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match s {
            Some(s) if s != "{}" => serde_json::from_str(&s)?,
            _ => State::default(),
        })
    }
    pub fn read_doc(&self, kind: DocKind) -> anyhow::Result<Option<String>> {
        match read_live_document(&self.doc_path(kind)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save_revision(&self, rev: &DocRevision) -> anyhow::Result<()> {
        anyhow::ensure!(
            plantool_core::sha256_hex(&rev.content) == rev.sha,
            "Revision checksum mismatch"
        );
        let sha = crate::artifacts::put(self.home(), rev.content.as_bytes())?;
        let mut db = self.db()?;
        let tx = db.transaction()?;
        crate::artifacts::register(&tx, &sha, rev.content.len())?;
        let id = format!("{}:{}:{}", self.key(), rev.kind, sha);
        tx.execute("INSERT OR IGNORE INTO revisions(id,item_id,doc_kind,number,content_sha256,created_by,created_at) SELECT ?1,?2,?3,coalesce(max(number),0)+1,?4,'agent',?5 FROM revisions WHERE item_id=?2 AND doc_kind=?3",params![id,self.key(),rev.kind.as_str(),sha,rev.captured_at])?;
        tx.commit()?;
        Ok(())
    }
    pub fn load_revision(&self, kind: DocKind, sha: &str) -> anyhow::Result<Option<String>> {
        let exists:bool=self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM revisions WHERE item_id=?1 AND doc_kind=?2 AND content_sha256=?3)",params![self.key(),kind.as_str(),sha],|r|r.get(0))?;
        if !exists {
            return Ok(None);
        }
        Ok(Some(String::from_utf8(crate::artifacts::read(
            self.home(),
            sha,
        )?)?))
    }
    pub fn revision_shas(&self, kind: DocKind) -> anyhow::Result<Vec<String>> {
        let db = self.db()?;
        let mut q = db.prepare(
            "SELECT content_sha256 FROM revisions WHERE item_id=?1 AND doc_kind=?2 ORDER BY number",
        )?;
        let rows = q
            .query_map(params![self.key(), kind.as_str()], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }
    pub fn save_run(&self, run: &Run) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let state = serde_json::to_value(run.status)?
            .as_str()
            .unwrap()
            .to_string();
        tx.execute("INSERT INTO attempts(id,item_id,role,generation,model_route,state,worktree,started_at,ended_at,outcome_json,idempotency_key,run_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?1,?11) ON CONFLICT(id) DO UPDATE SET state=excluded.state,ended_at=excluded.ended_at,outcome_json=excluded.outcome_json,run_json=excluded.run_json",params![run.id,self.key(),run.task.as_deref().unwrap_or("personal"),run.execution_generation,run.provider.as_str(),state,run.cwd.to_string_lossy(),run.started_at,run.ended_at,serde_json::to_string(&run.error)?,serde_json::to_string(run)?])?;
        tx.execute("INSERT INTO runs_summary(attempt_id,provider,model) VALUES(?1,?2,?3) ON CONFLICT(attempt_id) DO UPDATE SET model=excluded.model",params![run.id,run.provider.as_str(),run.model])?;
        tx.commit()?;
        Ok(())
    }
    pub fn load_runs(&self) -> anyhow::Result<Vec<Run>> {
        let db = self.db()?;
        let mut q = db.prepare(
            "SELECT run_json FROM attempts WHERE item_id=?1 AND hidden=0 ORDER BY started_at",
        )?;
        let rows = q
            .query_map([self.key()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.iter()
            .map(|s| serde_json::from_str(s).map_err(Into::into))
            .collect()
    }
    pub fn remove_run(&self, id: &str) -> anyhow::Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        tx.execute(
            "UPDATE attempts SET hidden=1 WHERE id=?1 AND item_id=?2",
            params![id, self.key()],
        )?;
        tx.execute("DELETE FROM run_events WHERE attempt_id=?1", [id])?;
        tx.execute(
            "DELETE FROM artifact_refs WHERE owner_type='transcript' AND owner_id=?1",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn append_run_event(&self, id: &str, line: &str) -> anyhow::Result<()> {
        let value: serde_json::Value = serde_json::from_str(line)?;
        let seq = value["seq"].as_u64().unwrap_or(0);
        let bytes = format!("{line}\n");
        let sha = crate::artifacts::put(self.home(), bytes.as_bytes())?;
        let mut db = self.db()?;
        let tx = db.transaction()?;
        crate::artifacts::register(&tx, &sha, bytes.len())?;
        tx.execute(
            "INSERT INTO run_events(attempt_id,seq,content_sha256) VALUES(?1,?2,?3)",
            params![id, seq, sha],
        )?;
        tx.execute("INSERT INTO artifact_refs(owner_type,owner_id,kind,sha256,created_at) VALUES('transcript',?1,?2,?3,?4)",params![id,seq.to_string(),sha,crate::database::unix_now()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn read_run_events(&self, id: &str, since: u64) -> anyhow::Result<Vec<serde_json::Value>> {
        let db = self.db()?;
        let mut q = db.prepare(
            "SELECT content_sha256 FROM run_events WHERE attempt_id=?1 AND seq>?2 ORDER BY seq",
        )?;
        let hashes = q
            .query_map(params![id, since], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        hashes
            .into_iter()
            .map(|sha| {
                Ok(serde_json::from_slice(&crate::artifacts::read(
                    self.home(),
                    &sha,
                )?)?)
            })
            .collect()
    }
    pub fn transcript(&self, id: &str) -> anyhow::Result<String> {
        Ok(self
            .read_run_events(id, 0)?
            .iter()
            .map(|e| format!("{}\n", e))
            .collect())
    }
    pub fn remove(&self) -> anyhow::Result<()> {
        let db = self.db()?;
        db.execute(
            "UPDATE work_items SET deleted_at=?2 WHERE id=?1",
            params![self.key(), crate::database::unix_now()],
        )?;
        Ok(())
    }
}
pub fn list_session_dirs(home: &Path) -> anyhow::Result<Vec<(String, String, PathBuf)>> {
    let db = crate::database::open(home)?;
    let mut q =
        db.prepare("SELECT repo_slug,slug FROM work_items WHERE deleted_at IS NULL ORDER BY id")?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(repo, slug)| {
            let dir = session_dir(home, &repo, &slug);
            (repo, slug, dir)
        })
        .collect())
}

// A live draft must not redirect the daemon into a protected file. Opening with
// O_NOFOLLOW closes the symlink check/use race; hard-linked drafts are rejected.
fn read_live_document(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::Error::other("Live documents cannot be symlinks"));
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other(
            "Live document must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(std::io::Error::other("Live documents cannot be hard links"));
        }
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

#[cfg(all(test, unix))]
mod live_document_security_tests {
    use super::*;
    #[test]
    fn live_documents_cannot_redirect_daemon_reads() {
        let t = tempfile::tempdir().unwrap();
        let secret = t.path().join("secret");
        fs::write(&secret, "private").unwrap();
        let doc = t.path().join("plan.md");
        std::os::unix::fs::symlink(&secret, &doc).unwrap();
        assert!(read_live_document(&doc).is_err());
        fs::remove_file(&doc).unwrap();
        fs::hard_link(&secret, &doc).unwrap();
        assert!(read_live_document(&doc).is_err());
        fs::remove_file(&doc).unwrap();
        fs::write(&doc, "plan").unwrap();
        assert_eq!(read_live_document(&doc).unwrap(), "plan");
    }
}
