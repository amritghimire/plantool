//! Lazy worker lifecycle. Failure is visible to the owner and never blocks Personal.
use crate::{database, AppState};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../worker"]
#[include = "pyproject.toml"]
#[include = "uv.lock"]
#[include = "plantool_worker/*.py"]
#[include = "plantool_worker/versions.json"]
struct WorkerFiles;
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../agents"]
struct AgentFiles;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Availability {
    pub available: bool,
    pub message: String,
}
impl Default for Availability {
    fn default() -> Self {
        Self {
            available: false,
            message: "Engineering OS unavailable: worker has not been set up".into(),
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("Worker returned {status}: {detail}")]
pub struct CallError {
    pub status: u16,
    pub detail: String,
}
#[derive(Default)]
pub struct WorkerManager {
    state: Mutex<Availability>,
    #[cfg(unix)]
    running: tokio::sync::Mutex<Option<Running>>,
    home: Mutex<Option<PathBuf>>,
}
#[cfg(unix)]
struct Running {
    process: crate::sandbox::WorkerProcess,
    _credential: crate::identities::WorkerCredential,
    client: reqwest::Client,
}
impl WorkerManager {
    pub fn availability(&self) -> Availability {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn unavailable(&self, error: impl std::fmt::Display) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Availability {
            available: false,
            message: format!("Engineering OS unavailable: {error}"),
        };
    }
    pub async fn repair(&self, state: &AppState) -> anyhow::Result<()> {
        *self.home.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.config.home.clone());
        if !crate::sandbox::supported() {
            self.unavailable("macOS sandbox-exec is required");
            anyhow::bail!("macOS sandbox-exec is required")
        }
        #[cfg(unix)]
        {
            let result = self.launch(state).await;
            if let Err(e) = &result {
                if !e
                    .downcast_ref::<CallError>()
                    .is_some_and(|e| matches!(e.status, 400 | 404 | 409 | 422))
                {
                    self.unavailable(e)
                }
            }
            result
        }
        #[cfg(not(unix))]
        {
            anyhow::bail!("Unix sockets are required")
        }
    }
    #[cfg(unix)]
    async fn launch(&self, state: &AppState) -> anyhow::Result<()> {
        let mut running = self.running.lock().await;
        if let Some(mut old) = running.take() {
            let _ = old.process.child.kill().await;
        }
        let home = &state.config.home;
        if crate::database::get::<bool>(&crate::database::open(home)?, "model_recovery_fence")?
            .unwrap_or(false)
            || crate::database::get::<bool>(&crate::database::open(home)?, "worker_recovery_fence")?
                .unwrap_or(false)
        {
            crate::supervision::reconcile(home)?;
        }
        anyhow::ensure!(
            !crate::database::get::<bool>(&crate::database::open(home)?, "model_recovery_fence")?
                .unwrap_or(false),
            "An old process group is unresolved; stop it before repairing"
        );
        anyhow::ensure!(
            !crate::database::get::<bool>(&crate::database::open(home)?, "worker_recovery_fence")?
                .unwrap_or(false),
            "An old worker process is unresolved; stop it before repairing"
        );
        let dir = unpack(home, &state.config.version)?;
        let python = if let Some(path) = option_env!("PLANTOOL_PACKAGED_WORKER_PYTHON") {
            PathBuf::from(path)
        } else {
            let uv = bootstrap(home).await?;
            let mut command = tokio::process::Command::new(uv);
            crate::sandbox::environment::apply(&mut command, None);
            command
                .args(["sync", "--frozen", "--no-dev", "--python", "3.12"])
                .current_dir(&dir)
                .env("UV_CACHE_DIR", home.join("cache/uv"))
                .env("UV_PYTHON_INSTALL_DIR", home.join("python"))
                .env("UV_PROJECT_ENVIRONMENT", dir.join(".venv"))
                .env("UV_NO_PROGRESS", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let mut child = state
                .runs
                .execution
                .spawn(state.runs.execution.status().generation, &mut command)?;
            let status = tokio::time::timeout(Duration::from_secs(300), child.wait()).await??;
            anyhow::ensure!(
                status.success(),
                "uv sync --frozen failed; check network and repair"
            );
            dir.join(".venv/bin/python")
        };
        std::fs::create_dir_all(home.join("worker-state"))?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                home.join("worker-state"),
                std::fs::Permissions::from_mode(0o700),
            )?;
        }
        std::fs::create_dir_all(home.join("run"))?;
        let socket = home.join("run/worker.sock");
        if let Ok(meta) = std::fs::symlink_metadata(&socket) {
            use std::os::unix::fs::FileTypeExt;
            anyhow::ensure!(
                meta.file_type().is_socket(),
                "Refusing to replace a non-socket worker path"
            );
            std::fs::remove_file(&socket)?;
        }
        let mut credential = None;
        let process = crate::sandbox::spawn_worker(
            crate::sandbox::Paths {
                owner_home: dirs_home()?,
                daemon_home: home.clone(),
                worktree: dir.clone(),
                docs: dir.join("docs"),
            },
            crate::sandbox::NeverSent::new(&[])?,
            &state.runs.execution,
            &python,
            |cmd| {
                cmd.args([
                    "-m",
                    "uvicorn",
                    "plantool_worker.app:runtime_app",
                    "--factory",
                    "--uds",
                ])
                .arg(&socket)
                .arg("--no-access-log")
                .arg("--log-level")
                .arg("warning")
                .current_dir(&dir)
                .env("PLANTOOL_HOME", home)
                .env("PYTHONPATH", &dir)
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
                credential = Some(state.runs.tokens.worker_command(cmd)?);
                Ok(())
            },
        )?;
        let credential = credential.unwrap();
        let client = worker_client(home, credential.token())?;
        let mut process = process;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = process.child.try_wait()? {
                anyhow::bail!("Worker exited during startup ({status}); repair is required")
            }
            if let Ok(response) = client
                .get("http://localhost/internal/v1/health")
                .send()
                .await
            {
                if response.status().is_success() {
                    let health: plantool_contract::Health = response.json().await?;
                    anyhow::ensure!(
                        health == plantool_contract::Health::default(),
                        "Worker contract/graph version mismatch"
                    );
                    break;
                }
            }
            anyhow::ensure!(
                tokio::time::Instant::now() < deadline,
                "Worker health check timed out"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        *running = Some(Running {
            process,
            _credential: credential,
            client,
        });
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Availability {
            available: true,
            message: "Engineering OS available".into(),
        };
        Ok(())
    }
    pub async fn call(
        &self,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        #[cfg(unix)]
        {
            let result = async {
                let client = {
                    let mut running = self.running.lock().await;
                    let worker = running
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("Worker is not running"))?;
                    anyhow::ensure!(
                        worker.process.child.try_wait()?.is_none(),
                        "Worker exited; repair required"
                    );
                    worker.client.clone()
                };
                let url = format!("http://localhost/internal/v1/{path}");
                let request = if let Some(body) = body {
                    client.post(url).json(body)
                } else {
                    client.get(url)
                };
                let request = if path == "role" {
                    request.timeout(Duration::from_secs(180))
                } else {
                    request
                };
                let response = request.send().await?;
                let status = response.status();
                if !status.is_success() {
                    let detail = response.text().await?;
                    return Err(CallError {
                        status: status.as_u16(),
                        detail,
                    }
                    .into());
                }
                Ok(response.json().await?)
            }
            .await;
            if let Err(e) = &result {
                if !e
                    .downcast_ref::<CallError>()
                    .is_some_and(|e| matches!(e.status, 400 | 404 | 409 | 422))
                {
                    self.unavailable(e)
                }
            }
            result
        }
        #[cfg(not(unix))]
        {
            let _ = (path, body);
            anyhow::bail!("Worker unavailable on this platform")
        }
    }
    pub fn for_home(home: &Path) -> Self {
        Self {
            home: Mutex::new(Some(home.into())),
            ..Default::default()
        }
    }
    pub async fn stop(&self) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            let mut running = self.running.lock().await;
            let home = self.home.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(home) = home {
                let threads = {
                    let db = database::open(&home)?;
                    let mut q=db.prepare("SELECT DISTINCT thread_id FROM dispatches UNION SELECT DISTINCT json_extract(payload_json,'$.thread_id') FROM outbox WHERE kind LIKE 'worker.%' AND json_extract(payload_json,'$.thread_id') IS NOT NULL")?;
                    let rows = q
                        .query_map([], |r| r.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    rows
                };
                for thread in threads {
                    let payload = serde_json::json!({"thread_id":thread,"idempotency_key":format!("stop:{thread}:{}",database::unix_now())});
                    let id = {
                        let mut db = database::open(&home)?;
                        let tx =
                            db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                        let id =
                            crate::dispatcher::enqueue(&tx, &thread, "worker.cancel", &payload)?;
                        tx.commit()?;
                        id
                    };
                    if let Some(worker) = running.as_ref() {
                        if let Ok(response) = worker
                            .client
                            .post("http://localhost/internal/v1/cancel")
                            .json(&payload)
                            .send()
                            .await
                        {
                            if response.status().is_success() {
                                database::finish_intent(
                                    &home,
                                    &id,
                                    &serde_json::json!({"cancelled":true}),
                                )?;
                            }
                        }
                    }
                }
            }
            if let Some(mut worker) = running.take() {
                let _ = worker.process.child.kill().await;
            }
        }
        self.unavailable("execution stopped");
        Ok(())
    }
}
impl crate::execution::StopHook for WorkerManager {
    fn stop(&self) -> futures::future::BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(self.stop())
    }
}
fn dirs_home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Owner home unavailable"))
}
#[cfg(unix)]
fn worker_client(home: &Path, token: &str) -> anyhow::Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    let mut token = reqwest::header::HeaderValue::from_str(token)?;
    token.set_sensitive(true);
    headers.insert("x-plantool-worker-token", token);
    headers.insert(
        "x-plantool-contract-version",
        plantool_contract::CONTRACT_VERSION.into(),
    );
    Ok(reqwest::Client::builder()
        .no_proxy()
        .unix_socket(home.join("run/worker.sock"))
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()?)
}
pub fn unpack(home: &Path, version: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !version.is_empty()
            && version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')),
        "Invalid version"
    );
    let dir = home.join("worker").join(version);
    std::fs::create_dir_all(&dir)?;
    for name in WorkerFiles::iter() {
        let file = WorkerFiles::get(&name).unwrap();
        let path = dir.join(name.as_ref());
        if std::fs::read(&path).ok().as_deref() != Some(file.data.as_ref()) {
            crate::store::write_atomic(&path, &file.data)?;
        }
    }
    for name in AgentFiles::iter() {
        let file = AgentFiles::get(&name).unwrap();
        crate::store::write_atomic(&dir.join("agents").join(name.as_ref()), &file.data)?;
    }
    Ok(dir)
}
fn verify_checksum(bytes: &[u8], expected: &str) -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    anyhow::ensure!(actual == expected, "Pinned uv checksum mismatch");
    Ok(())
}
async fn bootstrap(home: &Path) -> anyhow::Result<PathBuf> {
    let checks: serde_json::Value = serde_json::from_str(include_str!("uv-checksums.json"))?;
    let target = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };
    anyhow::ensure!(
        cfg!(target_os = "macos"),
        "uv bootstrap is supported only on macOS"
    );
    let version = checks["version"].as_str().unwrap();
    let root = home.join("tools").join(format!("uv-{version}-{target}"));
    let archive = root.with_extension("tar.gz");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let bytes = match std::fs::read(&archive) {
        Ok(data) => data,
        Err(_) => client
            .get(format!(
                "https://github.com/astral-sh/uv/releases/download/{version}/uv-{target}.tar.gz"
            ))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec(),
    };
    verify_checksum(&bytes, checks["targets"][target].as_str().unwrap())?;
    std::fs::create_dir_all(&root)?;
    crate::store::write_atomic(&archive, &bytes)?;
    // Extract only the verified uv executable, not arbitrary archive paths.
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]));
    let expected = format!("uv-{target}/uv");
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new(&expected) {
            use std::io::Read;
            anyhow::ensure!(
                entry.header().entry_type().is_file(),
                "uv archive entry must be regular"
            );
            let mut exe = Vec::new();
            entry.read_to_end(&mut exe)?;
            let path = root.join("uv");
            crate::store::write_atomic(&path, &exe)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
            }
            return Ok(path);
        }
    }
    anyhow::bail!("Verified uv archive did not contain uv")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_worker_and_checksum_validation() {
        let t = tempfile::tempdir().unwrap();
        let dir = unpack(t.path(), "test").unwrap();
        assert!(dir.join("uv.lock").is_file());
        assert!(dir.join("plantool_worker/models.py").is_file());
        assert!(verify_checksum(b"tampered", &"0".repeat(64)).is_err());
        assert!(unpack(t.path(), "../escape").is_err());
    }
}
