use plantool_core::{DocKind, DocRevision, Run, Session, State};
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
    dir.join(kind.file_name())
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
    pub fn new(dir: PathBuf) -> SessionStore {
        SessionStore { dir }
    }

    pub fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn doc_path(&self, kind: DocKind) -> PathBuf {
        doc_path(&self.dir, kind)
    }

    pub fn revision_path(&self, kind: DocKind, sha: &str) -> PathBuf {
        self.dir.join("revisions").join(kind.as_str()).join(format!("{sha}.md"))
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.dir.join("runs")
    }

    pub fn run_meta_path(&self, id: &str) -> PathBuf {
        self.runs_dir().join(format!("{id}.json"))
    }

    pub fn run_log_path(&self, id: &str) -> PathBuf {
        self.runs_dir().join(format!("{id}.ndjson"))
    }

    pub fn save_meta(&self, s: &Session) -> anyhow::Result<()> {
        write_json(&self.meta_path(), s)
    }

    pub fn load_meta(&self) -> anyhow::Result<Option<Session>> {
        read_json(&self.meta_path())
    }

    pub fn save_state(&self, s: &State) -> anyhow::Result<()> {
        write_json(&self.state_path(), s)
    }

    pub fn load_state(&self) -> anyhow::Result<State> {
        Ok(read_json(&self.state_path())?.unwrap_or_default())
    }

    pub fn read_doc(&self, kind: DocKind) -> anyhow::Result<Option<String>> {
        match fs::read_to_string(self.doc_path(kind)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_revision(&self, rev: &DocRevision) -> anyhow::Result<()> {
        let path = self.revision_path(rev.kind, &rev.sha);
        if path.exists() {
            return Ok(());
        }
        write_atomic(&path, rev.content.as_bytes())?;
        Ok(())
    }

    pub fn load_revision(&self, kind: DocKind, sha: &str) -> anyhow::Result<Option<String>> {
        match fs::read_to_string(self.revision_path(kind, sha)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_run(&self, run: &Run) -> anyhow::Result<()> {
        write_json(&self.run_meta_path(&run.id), run)
    }

    pub fn load_runs(&self) -> anyhow::Result<Vec<Run>> {
        let mut runs = Vec::new();
        let dir = self.runs_dir();
        let Ok(entries) = fs::read_dir(&dir) else { return Ok(runs) };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(run) = read_json::<Run>(&p)? {
                    runs.push(run);
                }
            }
        }
        runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        Ok(runs)
    }

    pub fn remove_run(&self, id: &str) -> anyhow::Result<()> {
        for p in [self.run_meta_path(id), self.run_log_path(id)] {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub fn append_run_event(&self, id: &str, line: &str) -> anyhow::Result<()> {
        fs::create_dir_all(self.runs_dir())?;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.run_log_path(id))?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        Ok(())
    }

    pub fn read_run_events(&self, id: &str, since: u64) -> anyhow::Result<Vec<serde_json::Value>> {
        let text = match fs::read_to_string(self.run_log_path(id)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let seq = v.get("seq").and_then(|s| s.as_u64()).unwrap_or(0);
            if seq > since {
                out.push(v);
            }
        }
        Ok(out)
    }
}

pub fn list_session_dirs(home: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(repos) = fs::read_dir(sessions_dir(home)) else { return out };
    for repo in repos.flatten() {
        if !repo.path().is_dir() {
            continue;
        }
        let repo_slug = repo.file_name().to_string_lossy().to_string();
        let Ok(slugs) = fs::read_dir(repo.path()) else { continue };
        for s in slugs.flatten() {
            if s.path().join("meta.json").is_file() {
                out.push((repo_slug.clone(), s.file_name().to_string_lossy().to_string(), s.path()));
            }
        }
    }
    out.sort();
    out
}
