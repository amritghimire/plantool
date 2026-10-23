use super::{NeverSent, Sandbox};
use std::{path::Path, sync::Arc};
pub struct Launch {
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
            execution,
            generation,
            sandbox,
            policy,
            temp,
        }))
    }
    pub fn command(&self, exe: &Path) -> anyhow::Result<tokio::process::Command> {
        match &self.sandbox {
            Some(s) => s.command(exe),
            None => Ok(tokio::process::Command::new(exe)),
        }
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
