//! A durable, global lease shared by Personal coding runs and Engineering OS.
use crate::database;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Model {
    pub route: String,
    pub provider: String,
    pub model: String,
}
pub trait Lifecycle: Send + Sync {
    fn unload<'a>(&'a self, model: &'a str) -> futures::future::BoxFuture<'a, anyhow::Result<()>>;
}
pub struct Ollama;
impl Lifecycle for Ollama {
    fn unload<'a>(&'a self, model: &'a str) -> futures::future::BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(15))
                .build()?
                .post(format!(
                    "{}/api/generate",
                    crate::providers::ollama_base_url()
                ))
                .json(&serde_json::json!({"model":model,"keep_alive":0}))
                .send()
                .await?
                .error_for_status()?;
            Ok(())
        })
    }
}
struct Waiting {
    home: PathBuf,
    id: String,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        if let Ok(db) = database::open(&self.home) {
            let _ = db.execute("DELETE FROM model_queue WHERE id=?1", [&self.id]);
        }
    }
}
pub struct Lease {
    home: PathBuf,
    pub owner: String,
    pub model: Model,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(db) = database::open(&self.home) {
            let _ = db.execute(
                "DELETE FROM leases WHERE id=1 AND holder_kind=?1",
                [&self.owner],
            );
        }
    }
}
pub fn owns(home: &Path, owner: &str) -> anyhow::Result<bool> {
    Ok(database::open(home)?.query_row(
        "SELECT EXISTS(SELECT 1 FROM leases WHERE id=1 AND holder_kind=?1)",
        [owner],
        |r| r.get(0),
    )?)
}
pub fn current(home: &Path) -> anyhow::Result<Option<String>> {
    Ok(database::open(home)?
        .query_row("SELECT holder_kind FROM leases WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?)
}
pub async fn acquire(
    home: &Path,
    owner: &str,
    attempt: Option<&str>,
    model: Model,
    urgent: bool,
    sprint_order: i64,
    execution: Arc<crate::execution::Execution>,
    lifecycle: &dyn Lifecycle,
) -> anyhow::Result<Lease> {
    anyhow::ensure!(
        !database::get::<bool>(&database::open(home)?, "model_recovery_fence")?.unwrap_or(false),
        "An old model process is unresolved; execution is fenced"
    );
    let generation = execution.status().generation;
    anyhow::ensure!(!execution.status().stopped, "Stop All is active");
    let waiting = Waiting {
        home: home.into(),
        id: owner.into(),
    };
    database::open(home)?.execute("INSERT INTO model_queue(id,attempt_id,route,provider,model,urgent,sprint_order,enqueued_at,execution_generation) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![owner,attempt,model.route,model.provider,model.model,urgent,sprint_order,database::unix_now(),generation])?;
    loop {
        if let Some(attempt) = attempt {
            let cancelled: bool = database::open(home)?.query_row(
                "SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND state='cancelled')",
                [attempt],
                |r| r.get(0),
            )?;
            anyhow::ensure!(!cancelled, "Work item cancelled while queued");
        }
        let status = execution.status();
        anyhow::ensure!(
            !status.stopped && status.generation == generation,
            "Stop All invalidated queued model work"
        );
        let previous = {
            let mut db = database::open(home)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let held: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM leases WHERE id=1)", [], |r| {
                    r.get(0)
                })?;
            let previous: Option<Model> = database::get(&tx, "loaded_model")?;
            let first: Option<String> = if !held {
                let preferred = previous.as_ref().map(|m| m.model.as_str()).unwrap_or("");
                // Urgency and sprint order win. Model affinity only reorders peers and
                // is disabled once a peer has waited ten minutes.
                tx.query_row("SELECT id FROM model_queue WHERE (urgent,sprint_order)=(SELECT urgent,sprint_order FROM model_queue ORDER BY urgent DESC,sprint_order,enqueued_at,rowid LIMIT 1) ORDER BY CASE WHEN ?2-(SELECT min(enqueued_at) FROM model_queue)>=600 THEN 0 WHEN model=?1 THEN 0 ELSE 1 END,enqueued_at,rowid LIMIT 1",params![preferred,database::unix_now()],|r|r.get::<_,String>(0)).optional()?
            } else {
                None
            };
            if first.as_deref() == Some(owner) {
                tx.execute("INSERT INTO leases(id,holder_attempt_id,holder_kind,model_route,acquired_at,expires_at) VALUES(1,?1,?2,?3,?4,?5)",params![attempt,owner,serde_json::to_string(&model)?,plantool_core::now(),database::unix_now()+2700])?;
                tx.execute("DELETE FROM model_queue WHERE id=?1", [owner])?;
                tx.commit()?;
                Some(previous)
            } else {
                tx.commit()?;
                None
            }
        };
        if let Some(previous) = previous {
            let lease = Lease {
                home: home.into(),
                owner: owner.into(),
                model: model.clone(),
            };
            if let Some(previous) = previous.filter(|p| {
                p.provider == "ollama" && (p.model != model.model || model.provider != "ollama")
            }) {
                lifecycle.unload(&previous.model).await?;
            }
            let status = execution.status();
            anyhow::ensure!(
                !status.stopped && status.generation == generation,
                "Execution stopped before loading the model"
            );
            database::set(&database::open(home)?, "loaded_model", &model)?;
            drop(waiting);
            return Ok(lease);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
