//! Admission, process-group ownership, and the persistent Stop All latch.
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Status {
    pub stopped: bool,
    pub generation: u64,
    pub policy_revision: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Saved {
    status: Status,
    #[serde(default)]
    rules: HashMap<PathBuf, Vec<String>>,
}
#[derive(Default)]
struct Inner {
    saved: Saved,
    home: Option<PathBuf>,
    groups: HashMap<u32, ()>,
    hooks: HashMap<u64, Arc<dyn StopHook>>,
    next_hook: u64,
    draining: usize,
    cleanup_failed: bool,
    lifecycle: Option<String>,
}
/// The worker bridge can register cancellation/lease cleanup without putting its
/// future graph protocol or lease implementation into the Phase 0 daemon.
pub trait StopHook: Send + Sync {
    fn stop(&self) -> futures::future::BoxFuture<'_, anyhow::Result<()>>;
}
#[derive(Default)]
pub struct Execution {
    inner: Mutex<Inner>,
}
pub struct Admission<'a> {
    inner: MutexGuard<'a, Inner>,
}
impl Admission<'_> {
    pub fn generation(&self) -> u64 {
        self.inner.saved.status.generation
    }
}
impl Execution {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        let saved =
            crate::database::get(&crate::database::open(home)?, "execution")?.unwrap_or_default();
        Ok(Self {
            inner: Mutex::new(Inner {
                saved,
                lifecycle: Some(lifecycle_token(home)?),
                home: Some(home.into()),
                ..Default::default()
            }),
        })
    }
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn can_shutdown(&self, headers: &axum::http::HeaderMap) -> bool {
        use sha2::{Digest, Sha256};
        let inner = self.lock();
        match (
            &inner.lifecycle,
            headers
                .get("x-plantool-lifecycle")
                .and_then(|v| v.to_str().ok()),
        ) {
            (Some(expected), Some(actual)) => {
                Sha256::digest(expected.as_bytes()) == Sha256::digest(actual.as_bytes())
            }
            _ => false,
        }
    }
    pub fn status(&self) -> Status {
        self.lock().saved.status.clone()
    }
    pub fn admit(&self) -> anyhow::Result<Admission<'_>> {
        let inner = self.lock();
        anyhow::ensure!(
            !inner.saved.status.stopped,
            "Stop All is active. Resume execution in the browser with your passkey."
        );
        Ok(Admission { inner })
    }
    pub fn admit_version(&self, expected: &Status) -> anyhow::Result<Admission<'_>> {
        let admission = self.admit()?;
        anyhow::ensure!(
            &admission.inner.saved.status == expected,
            "Stop All or sandbox policy changed while preparing the run"
        );
        Ok(admission)
    }
    pub fn rules(&self, repo: &Path) -> Vec<String> {
        self.lock()
            .saved
            .rules
            .get(repo)
            .cloned()
            .unwrap_or_default()
    }
    pub fn set_rules(
        &self,
        expected: &Status,
        repo: PathBuf,
        patterns: Vec<String>,
    ) -> anyhow::Result<()> {
        crate::sandbox::NeverSent::new(&patterns)?;
        let mut inner = self.lock();
        anyhow::ensure!(
            inner.saved.status.stopped,
            "Use Stop All before changing never-sent rules, then resume with your passkey"
        );
        anyhow::ensure!(
            &inner.saved.status == expected,
            "Security policy changed. Confirm again."
        );
        let previous = inner.saved.clone();
        inner.saved.rules.insert(repo, patterns);
        inner.saved.status.policy_revision += 1;
        if let Err(error) = persist(&inner) {
            inner.saved = previous;
            return Err(error);
        }
        Ok(())
    }
    pub fn resume(&self, expected: &Status) -> anyhow::Result<Status> {
        let mut inner = self.lock();
        anyhow::ensure!(
            &inner.saved.status == expected,
            "Execution state changed. Confirm resume again."
        );
        anyhow::ensure!(inner.saved.status.stopped, "Execution is already running");
        anyhow::ensure!(
            inner.groups.is_empty(),
            "Processes are still stopping. Try resume again."
        );
        anyhow::ensure!(
            inner.draining == 0 && !inner.cleanup_failed,
            "Stop All cleanup has not completed. Retry Stop All before resuming."
        );
        let old = inner.saved.status.clone();
        inner.saved.status.stopped = false;
        inner.saved.status.generation += 1;
        if let Err(e) = persist(&inner) {
            inner.saved.status = old;
            return Err(e);
        }
        Ok(inner.saved.status.clone())
    }
    pub async fn stop_all(&self, tokens: &crate::identities::RunTokens) -> anyhow::Result<Status> {
        let (groups, hooks, saved) = {
            let mut inner = self.lock();
            inner.saved.status.stopped = true;
            if inner.draining == 0 {
                inner.cleanup_failed = false;
            }
            inner.draining += 1;
            inner.saved.status.generation += 1;
            let saved = persist(&inner);
            // Issuance is performed under this same admission lock by RunManager.
            tokens.revoke_all();
            (
                inner.groups.keys().copied().collect::<Vec<_>>(),
                inner.hooks.values().cloned().collect::<Vec<_>>(),
                saved,
            )
        };
        let results = futures::future::join_all(hooks.iter().map(|h| async {
            tokio::time::timeout(std::time::Duration::from_secs(5), h.stop())
                .await
                .map_err(|_| anyhow::anyhow!("Worker cancellation/lease cleanup timed out"))?
        }))
        .await;
        for pid in groups {
            kill_group(pid);
        }
        let error = results.into_iter().find_map(Result::err);
        {
            let mut inner = self.lock();
            inner.draining -= 1;
            inner.cleanup_failed |= error.is_some();
        }
        if let Some(error) = error {
            return Err(error);
        }
        saved?;
        Ok(self.status())
    }
    pub fn register_hook(
        self: &Arc<Self>,
        hook: Arc<dyn StopHook>,
    ) -> anyhow::Result<HookRegistration> {
        let mut inner = self.lock();
        // Cleanup hooks may be registered while stopped; they grant no execution admission.
        let id = inner.next_hook;
        inner.next_hook += 1;
        inner.hooks.insert(id, hook);
        Ok(HookRegistration {
            execution: self.clone(),
            id,
        })
    }
    pub fn spawn(
        self: &Arc<Self>,
        generation: u64,
        command: &mut tokio::process::Command,
    ) -> anyhow::Result<ManagedChild> {
        let mut inner = self.lock();
        anyhow::ensure!(
            !inner.saved.status.stopped && inner.saved.status.generation == generation,
            "Stop All invalidated this launch"
        );
        #[cfg(unix)]
        command.process_group(0);
        command.kill_on_drop(true);
        let child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("spawned process has no PID"))?;
        inner.groups.insert(pid, ());
        Ok(ManagedChild {
            child,
            pid,
            execution: self.clone(),
        })
    }
}
fn persist(inner: &Inner) -> anyhow::Result<()> {
    if let Some(home) = &inner.home {
        let mut db = crate::database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::database::set(&tx, "execution", &inner.saved)?;
        for (repo, patterns) in &inner.saved.rules {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM projects WHERE local_checkout=?1 ORDER BY created_at LIMIT 1",
                    [repo.to_string_lossy()],
                    |r| r.get(0),
                )
                .optional()?;
            let project = existing.unwrap_or_else(|| {
                crate::git::detect_checkout(repo)
                    .map(|c| plantool_core::sha256_hex(&c.common_dir.to_string_lossy()))
                    .unwrap_or_else(|_| plantool_core::sha256_hex(&repo.to_string_lossy()))
            });
            tx.execute(
                "INSERT OR IGNORE INTO projects(id,local_checkout,created_at) VALUES(?1,?2,?3)",
                rusqlite::params![project, repo.to_string_lossy(), plantool_core::now()],
            )?;
            tx.execute("INSERT OR IGNORE INTO project_settings(project_id,version,never_sent_globs_json,effective_at,set_by) VALUES(?1,?2,?3,?4,'browser')",rusqlite::params![project,inner.saved.status.policy_revision,serde_json::to_string(patterns)?,plantool_core::now()])?;
        }
        tx.commit()?;
    }
    Ok(())
}

pub struct HookRegistration {
    execution: Arc<Execution>,
    id: u64,
}
impl Drop for HookRegistration {
    fn drop(&mut self) {
        self.execution.lock().hooks.remove(&self.id);
    }
}

pub struct ManagedChild {
    child: tokio::process::Child,
    pid: u32,
    execution: Arc<Execution>,
}
impl std::ops::Deref for ManagedChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}
impl std::ops::DerefMut for ManagedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}
impl ManagedChild {
    pub fn start_kill(&mut self) -> std::io::Result<()> {
        kill_group(self.pid);
        self.child.start_kill()
    }
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.start_kill()?;
        self.child.wait().await.map(|_| ())
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        kill_group(self.pid);
        self.execution.lock().groups.remove(&self.pid);
    }
}
#[cfg(unix)]
fn kill_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}
#[cfg(windows)]
fn kill_group(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
#[cfg(not(any(unix, windows)))]
fn kill_group(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stop_latch_survives_restart_and_old_admission_cannot_launch() {
        let home = tempfile::tempdir().unwrap();
        let execution = Arc::new(Execution::load(home.path()).unwrap());
        let tokens = crate::identities::RunTokens::default();
        let before = execution.status();
        execution.stop_all(&tokens).await.unwrap();
        assert!(execution.admit().is_err());
        let restarted = Execution::load(home.path()).unwrap();
        assert!(restarted.status().stopped);
        assert!(restarted.resume(&before).is_err());
        restarted.resume(&restarted.status()).unwrap();
        assert!(!Execution::load(home.path()).unwrap().status().stopped);
        assert!(restarted.admit_version(&before).is_err());
    }
    struct Hook {
        cancelled: std::sync::atomic::AtomicBool,
        released: std::sync::atomic::AtomicBool,
    }
    impl StopHook for Hook {
        fn stop(&self) -> futures::future::BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async move {
                self.cancelled
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                tokio::task::yield_now().await;
                self.released
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
        }
    }
    #[tokio::test]
    async fn stop_awaits_registered_worker_cancellation_and_lease_cleanup() {
        let execution = Arc::new(Execution::default());
        let hook = Arc::new(Hook {
            cancelled: false.into(),
            released: false.into(),
        });
        let _registration = execution.register_hook(hook.clone()).unwrap();
        execution
            .stop_all(&crate::identities::RunTokens::default())
            .await
            .unwrap();
        assert!(hook.cancelled.load(std::sync::atomic::Ordering::SeqCst));
        assert!(hook.released.load(std::sync::atomic::Ordering::SeqCst));
        execution.resume(&execution.status()).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn stop_kills_descendants_and_rejects_launches_from_old_generation() {
        let root = tempfile::tempdir().unwrap();
        let child_pid = root.path().join("grandchild");
        let execution = Arc::new(Execution::default());
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", r#"sleep 60 & echo $! > "$1"; wait"#, "sh"])
            .arg(&child_pid);
        let mut process = execution.spawn(0, &mut command).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !child_pid.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let grandchild: i32 = std::fs::read_to_string(child_pid)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        execution
            .stop_all(&crate::identities::RunTokens::default())
            .await
            .unwrap();
        assert!(!process.wait().await.unwrap().success());
        drop(process);
        execution.resume(&execution.status()).unwrap();
        assert!(execution
            .spawn(0, &mut tokio::process::Command::new("true"))
            .is_err());
        // Signal delivery and orphan reaping are asynchronous. A zombie is
        // terminated and cannot execute or keep a pipe open.
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                #[cfg(target_os = "linux")]
                let dead = std::fs::read_to_string(format!("/proc/{grandchild}/stat"))
                    .map(|stat| stat.split_whitespace().nth(2) == Some("Z"))
                    .unwrap_or(true);
                #[cfg(not(target_os = "linux"))]
                let dead = {
                    let status = std::process::Command::new("ps")
                        .args(["-o", "stat=", "-p", &grandchild.to_string()])
                        .output()
                        .unwrap();
                    let state = String::from_utf8_lossy(&status.stdout);
                    state.trim().is_empty() || state.trim().starts_with('Z')
                };
                if dead {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}

fn lifecycle_token(home: &Path) -> anyhow::Result<String> {
    use std::io::{Read, Write};
    let dir = home.join("security");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join("lifecycle-token");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            let secret = crate::config::random_token();
            file.write_all(secret.as_bytes())?;
            file.sync_all()?;
            Ok(secret)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::ensure!(
                std::fs::symlink_metadata(&path)?.is_file(),
                "Lifecycle credential must be a regular file"
            );
            let mut secret = String::new();
            std::fs::File::open(path)?
                .take(256)
                .read_to_string(&mut secret)?;
            anyhow::ensure!(
                secret.len() == 48 && secret.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid lifecycle credential"
            );
            Ok(secret)
        }
        Err(error) => Err(error.into()),
    }
}
