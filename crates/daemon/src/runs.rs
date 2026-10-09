use crate::events::LiveEvent;
use crate::providers::{self, ProviderEvent, RunInput, RunOptions};
use crate::registry::LiveSession;
use plantool_core::{
    ChangeReview, ImplementationMode, PermissionMode, Provider, Run, RunStatus, Stage,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct Handle {
    activity: std::time::Instant,
    input: mpsc::Sender<RunInput>,
}

#[derive(Default)]
pub struct RunManager {
    handles: Mutex<HashMap<String, Handle>>,
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_string()
}

fn implementation_run(run: &Run) -> bool {
    run.task.as_deref() == Some("implement")
        || run.milestone_key.is_some()
        || (run.task.is_none() && run.stage == Stage::Implementing)
}

pub fn next_milestone_context(run: &Run) -> Run {
    let mut next = run.clone();
    next.milestone_key = None;
    next.milestone_base = None;
    next.milestone_review = None;
    next.milestone_pending = false;
    next.milestone_commit = None;
    next
}

impl RunManager {
    pub fn is_live(&self, id: &str) -> bool {
        self.handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(
        self: &Arc<Self>,
        session: Arc<LiveSession>,
        provider: Provider,
        stage: Stage,
        stage_name: &str,
        prompt: String,
        model: Option<String>,
        effort: Option<String>,
        resume: Option<String>,
        permission_mode: PermissionMode,
        implementation_mode: ImplementationMode,
        previous_run: Option<&Run>,
    ) -> anyhow::Result<Run> {
        if session.state().plan_revision_pending
            && (stage_name == "implement" || previous_run.is_some_and(implementation_run))
        {
            anyhow::bail!("accept the plan revision before continuing implementation");
        }
        let sess = session.session();
        let prompt = if prompt.contains("Current session context:") {
            prompt
        } else {
            format!("{prompt}\n\n{}", crate::prompts::context_prompt(&sess))
        };
        let selected = previous_run
            .and_then(|r| r.milestone_key.clone())
            .or_else(|| {
                let state = session.state();
                state
                    .selected_milestone
                    .filter(|key| {
                        state
                            .milestones
                            .iter()
                            .any(|m| &m.key == key && m.status == "pending")
                    })
                    .or_else(|| {
                        state
                            .milestones
                            .iter()
                            .find(|m| m.status == "pending")
                            .map(|m| m.key.clone())
                    })
            });
        let plan = session.doc(plantool_core::DocKind::Plan);
        let milestone_key = if stage_name == "implement" {
            selected
        } else {
            None
        };
        let prompt = if let Some(key) = &milestone_key {
            let tasks = plan
                .as_ref()
                .and_then(|p| {
                    plantool_core::markdown::phases(&p.content)
                        .into_iter()
                        .find(|p| &p.name == key)
                        .map(|p| p.tasks)
                })
                .unwrap_or_default();
            let scope = tasks
                .iter()
                .map(|t| format!("- [{}] {}", if t.checked { "x" } else { " " }, t.text))
                .collect::<Vec<_>>()
                .join("\n");
            let pause = crate::prompts::pause_rule_prompt(&sess.pause_rule, &sess.key());
            format!("{prompt}\n\nMilestone scope: {key}. Complete this whole phase in this turn, then finish your turn for review with the stage left at implementing. Do not build another phase in this run. When this is the final pending phase, run the full project checks before finishing. This instruction overrides task-by-task pacing.\n{scope}\nPause rule: {pause}\n")
        } else {
            prompt
        };
        let head = if milestone_key.is_some() {
            Some(crate::git::snapshot_tree(
                sess.cwd(),
                &session.store.dir.join(format!(".index-{}", short_id())),
            )?)
        } else if implementation_mode == ImplementationMode::StepByStep {
            crate::git::head_sha(sess.cwd()).ok()
        } else {
            None
        };
        let run = Run {
            milestone_key,
            plan_sha: plan.as_ref().map(|p| p.sha.clone()),
            idle_stopped: false,
            id: short_id(),
            provider,
            provider_session_id: resume.clone(),
            stage,
            implementation_mode,
            milestone_pending: previous_run.is_some_and(|r| r.milestone_pending),
            milestone_review: previous_run.and_then(|r| r.milestone_review.clone()),
            milestone_base: previous_run
                .and_then(|r| r.milestone_base.clone())
                .or_else(|| head.clone()),
            milestones_approved: previous_run.map_or(0, |r| r.milestones_approved),
            milestone_commit: previous_run.and_then(|r| r.milestone_commit.clone()),
            implementation_base: previous_run
                .and_then(|r| r.implementation_base.clone())
                .or(head),
            task: Some(stage_name.to_string()),
            cwd: sess.cwd().clone(),
            status: RunStatus::Starting,
            model: model.clone(),
            permission_mode,
            started_at: plantool_core::now(),
            ended_at: None,
            error: None,
            seq: 0,
        };
        session.upsert_run(run.clone(), |r| LiveEvent::RunStarted { run: r })?;
        if let Some(key) = &run.milestone_key {
            session.milestone_status(key, "building", &run)?;
        }
        let (in_tx, in_rx) = mpsc::channel::<RunInput>(64);
        let (ev_tx, mut ev_rx) = mpsc::channel::<ProviderEvent>(1024);
        self.handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                run.id.clone(),
                Handle {
                    input: in_tx,
                    activity: std::time::Instant::now(),
                },
            );
        let opts = RunOptions {
            cwd: run.cwd.clone(),
            prompt,
            model,
            effort,
            resume,
            executable: providers::executable_for(provider),
            writable_roots: vec![session.store.dir.clone()],
            permission_mode,
        };
        let run_id = run.id.clone();
        let manager = self.clone();
        let stage_label = stage_name.to_string();

        let auto_next = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let consumer_next = auto_next.clone();
        let consumer_session = session.clone();
        let consumer_id = run_id.clone();
        let consumer_manager = self.clone();
        let consumer = tokio::spawn(async move {
            let mut seq = 0u64;
            let mut current = consumer_session
                .run(&consumer_id)
                .unwrap_or_else(|| panic!("run {consumer_id} vanished"));
            while let Some(ev) = ev_rx.recv().await {
                if let Some(handle) = consumer_manager
                    .handles
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&consumer_id)
                {
                    handle.activity = std::time::Instant::now();
                }
                if let Some(saved) = consumer_session.run(&consumer_id) {
                    current = saved;
                }
                seq += 1;
                let status = match &ev {
                    ProviderEvent::TurnStarted { .. } => Some(RunStatus::Running),
                    ProviderEvent::Permission { .. } | ProviderEvent::InputRequest { .. } => {
                        Some(RunStatus::Waiting)
                    }
                    ProviderEvent::RequestResolved { .. } => Some(RunStatus::Running),
                    ProviderEvent::TurnCompleted { .. } => Some(RunStatus::Idle),
                    ProviderEvent::ProviderSession { .. } => None,
                    _ => None,
                };
                if let ProviderEvent::ProviderSession { session_id } = &ev {
                    current.provider_session_id = Some(session_id.clone());
                    let _ = consumer_session
                        .upsert_run(current.clone(), |r| LiveEvent::RunUpdated { run: r });
                }
                let value = serde_json::to_value(&ev).unwrap_or(serde_json::Value::Null);
                if let Err(e) = consumer_session.append_run_event(&consumer_id, seq, value) {
                    tracing::warn!("run {consumer_id}: {e}");
                }
                if let Some(s) = status {
                    if current.status != s {
                        current.status = s;
                        current.seq = seq;
                        let _ = consumer_session
                            .upsert_run(current.clone(), |r| LiveEvent::RunUpdated { run: r });
                    }
                }
                if let ProviderEvent::TurnCompleted {
                    status: turn_status,
                    ..
                } = &ev
                {
                    if turn_status == "completed"
                        && (current.implementation_mode == ImplementationMode::StepByStep
                            || current.milestone_key.is_some())
                        && consumer_session.stage() == Stage::Implementing
                    {
                        let _ = consumer_session.capture_doc(plantool_core::DocKind::Plan);
                        if let Some(key) = &current.milestone_key {
                            let complete = consumer_session
                                .doc(plantool_core::DocKind::Plan)
                                .is_some_and(|p| {
                                    plantool_core::markdown::phases(&p.content)
                                        .iter()
                                        .find(|p| &p.name == key)
                                        .is_some_and(|p| p.tasks.iter().all(|t| t.checked))
                                });
                            if !complete {
                                continue;
                            }
                            let state = consumer_session.state();
                            let no_pause = consumer_session.session().pause_rule
                                != plantool_core::PauseRule::EveryMilestone
                                && state.pause_reason.is_none()
                                && !state.plan_revision_pending;
                            if no_pause {
                                let _ =
                                    consumer_session.milestone_status(key, "completed", &current);
                                consumer_next.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = consumer_manager.send(&consumer_id, RunInput::Stop).await;
                                continue;
                            }
                            let _ = consumer_session.milestone_status(key, "review", &current);
                        }
                        current.milestone_pending = true;
                        let existing = current.milestone_review.clone();
                        let base = current
                            .milestone_base
                            .clone()
                            .unwrap_or_else(|| "HEAD".into());
                        let review_session = consumer_session.session();
                        let plan = consumer_session.doc_path(plantool_core::DocKind::Plan);
                        let opened = tokio::task::spawn_blocking(move || {
                            crate::changes::open_review(
                                &review_session,
                                &base,
                                Some(plan),
                                existing.as_ref(),
                            )
                        })
                        .await;
                        match opened {
                            Ok(Ok((review, _))) => {
                                let new_review =
                                    current.milestone_review.as_ref().and_then(review_ref)
                                        != review_ref(&review);
                                current.milestone_review = Some(review.clone());
                                consumer_session.announce_review(review.clone());
                                if new_review {
                                    if let Some(reference) = review_ref(&review) {
                                        tokio::spawn(watch_review(
                                            consumer_manager.clone(),
                                            consumer_session.clone(),
                                            consumer_id.clone(),
                                            reference.to_string(),
                                        ));
                                    }
                                }
                            }
                            Ok(Err(e)) => tracing::warn!("milestone review {consumer_id}: {e}"),
                            Err(e) => tracing::warn!("milestone review task {consumer_id}: {e}"),
                        }
                        let _ = consumer_session
                            .upsert_run(current.clone(), |r| LiveEvent::RunUpdated { run: r });
                    }
                }
            }
            seq
        });
        if run.milestone_pending {
            if let Some(reference) = run.milestone_review.as_ref().and_then(review_ref) {
                tokio::spawn(watch_review(
                    self.clone(),
                    session.clone(),
                    run.id.clone(),
                    reference.to_string(),
                ));
            }
        }

        tokio::spawn(async move {
            let result = providers::run_provider(provider, opts, in_rx, ev_tx).await;
            let _ = consumer.await;
            manager
                .handles
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&run_id);
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
            if auto_next.load(std::sync::atomic::Ordering::SeqCst) {
                let state = session.state();
                if !state.plan_revision_pending && state.pause_reason.is_none() {
                    if state.milestones.iter().any(|m| m.status == "pending") {
                        if let Some(previous) = session.run(&run_id) {
                            let context = next_milestone_context(&previous);
                            let prompt = format!("Continue the next milestone of {}. Read plantool skill and plantool skill implement. Keep the plan checkboxes current. When the final phase is complete, run all project checks before finishing your turn.", session.key);
                            let _ = manager.start(
                                session.clone(),
                                previous.provider,
                                previous.stage,
                                "implement",
                                prompt,
                                previous.model,
                                None,
                                previous.provider_session_id,
                                previous.permission_mode,
                                previous.implementation_mode,
                                Some(&context),
                            );
                        }
                    } else {
                        let _ = session
                            .set_stage(Stage::ImplementationReview, plantool_core::Actor::Agent);
                    }
                }
            }
            tracing::info!("{stage_label} run {run_id} ended");
        });
        Ok(run)
    }

    pub async fn sweep_idle(&self, session: &Arc<LiveSession>, minutes: u64) {
        if minutes == 0 {
            return;
        }
        self.sweep_idle_for(
            session,
            std::time::Duration::from_secs(minutes.saturating_mul(60)),
        )
        .await;
    }

    pub async fn sweep_idle_for(&self, session: &Arc<LiveSession>, limit: std::time::Duration) {
        for mut run in session.runs() {
            if run.status != RunStatus::Idle || run.idle_stopped {
                continue;
            }
            let expired = self
                .handles
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&run.id)
                .is_some_and(|h| h.activity.elapsed() >= limit);
            if expired {
                run.idle_stopped = true;
                if session
                    .upsert_run(run.clone(), |r| LiveEvent::RunUpdated { run: r })
                    .is_ok()
                {
                    let _ = self.send(&run.id, RunInput::Stop).await;
                }
            }
        }
    }

    pub async fn forward_comment(
        self: &Arc<Self>,
        session: Arc<LiveSession>,
        text: String,
    ) -> anyhow::Result<()> {
        let latest = session
            .runs()
            .into_iter()
            .max_by(|a, b| a.started_at.cmp(&b.started_at));
        let Some(run) = latest else { return Ok(()) };
        if session.state().plan_revision_pending && implementation_run(&run) {
            return Ok(());
        }
        if run.status == RunStatus::Idle && self.is_live(&run.id) {
            return self.send(&run.id, RunInput::Text(text)).await;
        }
        if run.idle_stopped && !self.is_live(&run.id) {
            if let Some(resume) = run.provider_session_id.clone() {
                session.verify_workspace()?;
                self.start(
                    session,
                    run.provider,
                    run.stage,
                    run.task.as_deref().unwrap_or("resume"),
                    text,
                    run.model.clone(),
                    None,
                    Some(resume),
                    run.permission_mode,
                    run.implementation_mode,
                    Some(&run),
                )?;
            }
        }
        Ok(())
    }

    pub async fn send(&self, id: &str, input: RunInput) -> anyhow::Result<()> {
        let tx = self
            .handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|h| h.input.clone());
        match tx {
            Some(tx) => tx
                .send(input)
                .await
                .map_err(|_| anyhow::anyhow!("run {id} is no longer accepting input")),
            None => anyhow::bail!("run {id} is not running"),
        }
    }

    pub fn mark_orphans(&self, session: &LiveSession) {
        for mut r in session.runs() {
            if matches!(
                r.status,
                RunStatus::Starting | RunStatus::Running | RunStatus::Waiting | RunStatus::Idle
            ) && !self.is_live(&r.id)
            {
                r.status = RunStatus::Stopped;
                r.ended_at = Some(plantool_core::now());
                r.error = Some(
                    "the daemon restarted; start a new run to resume the provider session".into(),
                );
                let _ = session.upsert_run(r, |r| LiveEvent::RunUpdated { run: r });
            }
        }
    }
}

pub(crate) fn review_ref(review: &ChangeReview) -> Option<&str> {
    match review {
        ChangeReview::Difftool { review, .. } => review.as_deref(),
        _ => None,
    }
}

pub(crate) async fn watch_review(
    manager: Arc<RunManager>,
    session: Arc<LiveSession>,
    run_id: String,
    reference: String,
) {
    let crate::changes::ChangeTool::Difftool { path } = crate::changes::detect_change_tool() else {
        return;
    };
    let mut cursor = "2000-01-01T00:00:00Z".to_string();
    while let Some(run) = session.run(&run_id) {
        if !run.milestone_pending
            || session.stage() != Stage::Implementing
            || !manager.is_live(&run_id)
            || run.milestone_review.as_ref().and_then(review_ref) != Some(reference.as_str())
        {
            break;
        }
        let output = tokio::process::Command::new(&path)
            .args([
                "review",
                "watch",
                "--review",
                &reference,
                "--since",
                &cursor,
                "--kind",
                "human",
                "--timeout",
                "10",
                "--json",
            ])
            .output()
            .await;
        match output {
            Ok(out) if out.status.success() => {
                let comments: Vec<serde_json::Value> =
                    serde_json::from_slice(&out.stdout).unwrap_or_default();
                if let Some(last) = comments
                    .last()
                    .and_then(|c| c.get("id"))
                    .and_then(|v| v.as_str())
                {
                    cursor = last.to_string();
                }
                if !comments.is_empty() {
                    if !session.run(&run_id).is_some_and(|r| r.milestone_pending) {
                        break;
                    }
                    let ids = comments
                        .iter()
                        .filter_map(|c| c.get("id").and_then(|v| v.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let message = format!("New human comments on difftool review {reference}: {ids}. Read the whole open comment board with `difftool review comment list --review {reference} --kind human --unresolved --context --json`. Address these comments in this same milestone, refresh the review, and reply where useful. Keep waiting for milestone approval; do not start the next plan ticket.");
                    if manager
                        .send(&run_id, RunInput::Text(message))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Ok(out) if out.status.code() == Some(124) => {}
            Ok(out) => {
                tracing::warn!("difftool watch {reference} exited with {}", out.status);
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
            Err(e) => {
                tracing::warn!("difftool watch {reference}: {e}");
                break;
            }
        }
    }
}
