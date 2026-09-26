use crate::events::LiveEvent;
use crate::providers::{self, ProviderEvent, RunInput, RunOptions};
use crate::registry::LiveSession;
use plantool_core::{Provider, Run, RunStatus, Stage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct Handle {
    input: mpsc::Sender<RunInput>,
}

#[derive(Default)]
pub struct RunManager {
    handles: Mutex<HashMap<String, Handle>>,
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_string()
}

impl RunManager {
    pub fn is_live(&self, id: &str) -> bool {
        self.handles.lock().unwrap_or_else(|e| e.into_inner()).contains_key(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(self: &Arc<Self>, session: Arc<LiveSession>, provider: Provider, stage: Stage, stage_name: &str, prompt: String, model: Option<String>, resume: Option<String>) -> anyhow::Result<Run> {
        let sess = session.session();
        let run = Run {
            id: short_id(),
            provider,
            provider_session_id: resume.clone(),
            stage,
            cwd: sess.cwd().clone(),
            status: RunStatus::Starting,
            model: model.clone(),
            started_at: plantool_core::now(),
            ended_at: None,
            error: None,
            seq: 0,
        };
        session.upsert_run(run.clone(), |r| LiveEvent::RunStarted { run: r })?;
        let (in_tx, in_rx) = mpsc::channel::<RunInput>(64);
        let (ev_tx, mut ev_rx) = mpsc::channel::<ProviderEvent>(1024);
        self.handles.lock().unwrap_or_else(|e| e.into_inner()).insert(run.id.clone(), Handle { input: in_tx });
        let opts = RunOptions { cwd: run.cwd.clone(), prompt, model, resume, executable: providers::executable_for(provider), writable_roots: vec![session.store.dir.clone()] };
        let run_id = run.id.clone();
        let manager = self.clone();
        let stage_label = stage_name.to_string();

        let consumer_session = session.clone();
        let consumer_id = run_id.clone();
        let consumer = tokio::spawn(async move {
            let mut seq = 0u64;
            let mut current = consumer_session.run(&consumer_id).unwrap_or_else(|| panic!("run {consumer_id} vanished"));
            while let Some(ev) = ev_rx.recv().await {
                seq += 1;
                let status = match &ev {
                    ProviderEvent::TurnStarted { .. } => Some(RunStatus::Running),
                    ProviderEvent::Permission { .. } | ProviderEvent::InputRequest { .. } => Some(RunStatus::Waiting),
                    ProviderEvent::RequestResolved { .. } => Some(RunStatus::Running),
                    ProviderEvent::TurnCompleted { .. } => Some(RunStatus::Idle),
                    ProviderEvent::ProviderSession { .. } => None,
                    _ => None,
                };
                if let ProviderEvent::ProviderSession { session_id } = &ev {
                    current.provider_session_id = Some(session_id.clone());
                    let _ = consumer_session.upsert_run(current.clone(), |r| LiveEvent::RunUpdated { run: r });
                }
                let value = serde_json::to_value(&ev).unwrap_or(serde_json::Value::Null);
                if let Err(e) = consumer_session.append_run_event(&consumer_id, seq, value) {
                    tracing::warn!("run {consumer_id}: {e}");
                }
                if let Some(s) = status {
                    if current.status != s {
                        current.status = s;
                        current.seq = seq;
                        let _ = consumer_session.upsert_run(current.clone(), |r| LiveEvent::RunUpdated { run: r });
                    }
                }
            }
            seq
        });

        tokio::spawn(async move {
            let result = providers::run_provider(provider, opts, in_rx, ev_tx).await;
            let _ = consumer.await;
            manager.handles.lock().unwrap_or_else(|e| e.into_inner()).remove(&run_id);
            if let Some(mut r) = session.run(&run_id) {
                r.ended_at = Some(plantool_core::now());
                match result {
                    Ok(()) => r.status = RunStatus::Stopped,
                    Err(e) => {
                        r.status = RunStatus::Failed;
                        r.error = Some(e.to_string());
                    }
                }
                let _ = session.upsert_run(r, |r| LiveEvent::RunEnded { run: r });
            }
            tracing::info!("{stage_label} run {run_id} ended");
        });
        Ok(run)
    }

    pub async fn send(&self, id: &str, input: RunInput) -> anyhow::Result<()> {
        let tx = self.handles.lock().unwrap_or_else(|e| e.into_inner()).get(id).map(|h| h.input.clone());
        match tx {
            Some(tx) => tx.send(input).await.map_err(|_| anyhow::anyhow!("run {id} is no longer accepting input")),
            None => anyhow::bail!("run {id} is not running"),
        }
    }

    pub fn mark_orphans(&self, session: &LiveSession) {
        for mut r in session.runs() {
            if matches!(r.status, RunStatus::Starting | RunStatus::Running | RunStatus::Waiting | RunStatus::Idle) && !self.is_live(&r.id) {
                r.status = RunStatus::Stopped;
                r.ended_at = Some(plantool_core::now());
                r.error = Some("the daemon restarted; start a new run to resume the provider session".into());
                let _ = session.upsert_run(r, |r| LiveEvent::RunUpdated { run: r });
            }
        }
    }
}
