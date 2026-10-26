pub mod environment;
pub mod launch;
pub mod never_sent;
mod profile;
pub mod read_guard;

pub use never_sent::NeverSent;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Agent,
    Reviewer,
    EosAgent,
    EosReviewer,
    EosQa,
    Worker,
}
#[derive(Debug)]
pub struct Sandbox {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    profile: String,
    temp: tempfile::TempDir,
    pub policy: NeverSent,
}
#[derive(Debug)]
pub struct Paths {
    pub owner_home: PathBuf,
    pub daemon_home: PathBuf,
    pub worktree: PathBuf,
    pub docs: PathBuf,
}
pub fn supported() -> bool {
    cfg!(target_os = "macos")
}
impl Sandbox {
    pub fn required(paths: Paths, role: Role, policy: NeverSent) -> anyhow::Result<Arc<Self>> {
        if !supported() {
            anyhow::bail!("macOS sandbox-exec is required; this platform cannot run an isolated agent or worker");
        }
        let temp = tempfile::Builder::new()
            .prefix("plantool-attempt-")
            .tempdir()?;
        let profile = profile::build(&paths, temp.path(), role, &policy)?;
        // Keep provider logs/state inside the attempt. Copy only known model-auth
        // files, never whole owner configuration directories or shell hooks.
        for (source, destination) in [
            (".codex/auth.json", "codex/auth.json"),
            (".local/share/opencode/auth.json", "data/opencode/auth.json"),
            (".claude/.credentials.json", "claude/.credentials.json"),
        ] {
            if role == Role::Worker
                || (matches!(role, Role::EosAgent | Role::EosReviewer | Role::EosQa)
                    && source != ".codex/auth.json")
            {
                continue;
            }
            let source = paths.owner_home.join(source);
            if source.is_file() {
                let destination = temp.path().join(destination);
                std::fs::create_dir_all(destination.parent().unwrap())?;
                std::fs::copy(source, &destination)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o600))?;
                }
            }
        }
        for name in [
            "codex", "claude", "copilot", "config", "data", "cache", "state",
        ] {
            std::fs::create_dir_all(temp.path().join(name))?;
        }
        Ok(Arc::new(Self {
            profile,
            temp,
            policy,
        }))
    }
    pub fn temp_dir(&self) -> &Path {
        self.temp.path()
    }
    #[cfg(target_os = "macos")]
    pub fn command(&self, exe: &Path) -> anyhow::Result<tokio::process::Command> {
        let mut cmd = tokio::process::Command::new("/usr/bin/sandbox-exec");
        cmd.args(["-p", &self.profile]).arg(exe);
        Ok(cmd)
    }
    #[cfg(not(target_os = "macos"))]
    pub fn command(&self, _exe: &Path) -> anyhow::Result<tokio::process::Command> {
        anyhow::bail!("macOS sandbox-exec is unavailable on this platform")
    }
    pub fn apply_environment(&self, command: &mut tokio::process::Command) {
        // Native TLS trust discovery can query Keychain, which agents must not
        // access. Use the system's public roots; certificate verification stays on.
        #[cfg(target_os = "macos")]
        if std::env::var_os("CODEX_CA_CERTIFICATE").is_none()
            && std::env::var_os("SSL_CERT_FILE").is_none()
            && Path::new("/etc/ssl/cert.pem").is_file()
        {
            command.env("CODEX_CA_CERTIFICATE", "/etc/ssl/cert.pem");
        }
        command
            .env("TMPDIR", self.temp.path())
            .env("TMP", self.temp.path())
            .env("TEMP", self.temp.path())
            .env("PLANTOOL_SANDBOXED", "1")
            .env("CODEX_HOME", self.temp.path().join("codex"))
            .env("CLAUDE_CONFIG_DIR", self.temp.path().join("claude"))
            .env("COPILOT_HOME", self.temp.path().join("copilot"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_DATA_HOME", self.temp.path().join("data"))
            .env("XDG_CACHE_HOME", self.temp.path().join("cache"))
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
    }
}

/// Required sandbox + process-group supervision for worker bridge launches.
/// The returned owner keeps the attempt directory alive until the worker exits.
pub struct WorkerProcess {
    pub child: crate::execution::ManagedChild,
    _sandbox: Arc<Sandbox>,
}
pub fn spawn_worker(
    paths: Paths,
    policy: NeverSent,
    execution: &Arc<crate::execution::Execution>,
    exe: &Path,
    configure: impl FnOnce(&mut tokio::process::Command) -> anyhow::Result<()>,
) -> anyhow::Result<WorkerProcess> {
    let version = execution.status();
    anyhow::ensure!(!version.stopped, "Stop All is active");
    let daemon_home = paths.daemon_home.clone();
    let sandbox = Sandbox::required(paths, Role::Worker, policy)?;
    let command = sandbox.command(exe)?;
    #[cfg(unix)]
    let (mut command, _id, barrier) = crate::supervision::wrap(command)?;
    #[cfg(not(unix))]
    let mut command = command;
    environment::apply(&mut command, None);
    configure(&mut command)?;
    sandbox.apply_environment(&mut command);
    let child = execution.spawn(version.generation, &mut command)?;
    #[cfg(unix)]
    barrier.release(
        &daemon_home,
        "worker",
        child
            .id()
            .ok_or_else(|| anyhow::anyhow!("Worker PID unavailable"))?,
    )?;
    Ok(WorkerProcess {
        child,
        _sandbox: sandbox,
    })
}

#[cfg(all(test, not(target_os = "macos")))]
mod stub_tests {
    use super::*;
    #[test]
    fn required_sandbox_fails_closed_on_unsupported_platforms() {
        assert!(!supported());
        assert!(Sandbox::required(
            Paths {
                owner_home: "/unused".into(),
                daemon_home: "/unused".into(),
                worktree: "/unused".into(),
                docs: "/unused".into(),
            },
            Role::Worker,
            NeverSent::new(&[]).unwrap()
        )
        .is_err());
    }
}
