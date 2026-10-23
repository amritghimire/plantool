use super::{NeverSent, Sandbox};
use std::{path::Path, sync::Arc};
pub struct Launch {
    #[cfg(unix)]
    barriers: std::sync::Mutex<std::collections::HashMap<String, crate::supervision::Barrier>>,
    pub attempt: Option<(std::path::PathBuf, String)>,
    pub execution: Arc<crate::execution::Execution>,
    pub generation: u64,
    pub sandbox: Option<Arc<Sandbox>>,
    pub policy: NeverSent,
    temp: Option<tempfile::TempDir>,
}
impl Launch {
    pub fn new(
        execution: Arc<crate::execution::Execution>,
        generation: u64,
        sandbox: Option<Arc<Sandbox>>,
        policy: NeverSent,
    ) -> anyhow::Result<Arc<Self>> {
        let temp = if sandbox.is_none() {
            Some(
                tempfile::Builder::new()
                    .prefix("plantool-attempt-")
                    .tempdir()?,
            )
        } else {
            None
        };
        Ok(Arc::new(Self {
            #[cfg(unix)]
            barriers: Default::default(),
            attempt: None,
            execution,
            generation,
            sandbox,
            policy,
            temp,
        }))
    }
    pub fn command(&self, exe: &Path) -> anyhow::Result<tokio::process::Command> {
        let command = match &self.sandbox {
            Some(s) => s.command(exe)?,
            None => tokio::process::Command::new(exe),
        };
        #[cfg(unix)]
        if self.attempt.is_some() {
            let (command, id, barrier) = crate::supervision::wrap(command)?;
            self.barriers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id, barrier);
            return Ok(command);
        }
        Ok(command)
    }
    pub fn release(&self, command: &tokio::process::Command, pid: u32) -> anyhow::Result<()> {
        #[cfg(unix)]
        if let Some((home, attempt)) = &self.attempt {
            let id = command
                .as_std()
                .get_args()
                .nth(2)
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("Missing launch barrier identity"))?;
            let barrier = self
                .barriers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(id)
                .ok_or_else(|| anyhow::anyhow!("Unknown launch barrier"))?;
            barrier.release(home, attempt, pid)?;
        }
        #[cfg(not(unix))]
        let _ = (command, pid);
        Ok(())
    }

    pub fn apply_environment(&self, command: &mut tokio::process::Command) {
        if let Some(s) = &self.sandbox {
            s.apply_environment(command);
        } else if let Some(temp) = &self.temp {
            command
                .env("TMPDIR", temp.path())
                .env("TMP", temp.path())
                .env("TEMP", temp.path());
        }
    }
}
