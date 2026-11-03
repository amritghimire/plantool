use super::{emit, EventSink, ProviderEvent, RunInput, RunOptions};
use anyhow::Context;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;

pub const DEFAULT_MODEL: &str = "claudeAgent:claude-opus-5-5";
const STOP_MODEL: &str = "claudeAgent:claude-haiku-5-5";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Thread {
    status: String,
    link: Option<String>,
    active_run_id: Option<String>,
    #[serde(default)]
    run_count: usize,
    #[serde(default)]
    pending_request_count: usize,
}

#[derive(Deserialize)]
struct Message {
    role: String,
    text: String,
}

#[derive(Deserialize)]
struct Shown {
    thread: Thread,
    #[serde(default)]
    messages: Vec<Message>,
}

async fn t3(exe: &Path, args: &[&str], stdin: Option<&str>) -> anyhow::Result<String> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start {}", exe.display()))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(text.as_bytes()).await?;
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "t3 {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn show(exe: &Path, thread: &str) -> anyhow::Result<Shown> {
    let out = t3(
        exe,
        &["thread", "show", thread, "--messages", "1", "--json"],
        None,
    )
    .await?;
    serde_json::from_str(&out).context("could not read `t3 thread show` output")
}

// T3 resolves a project from any path inside it; plantool worktrees may live outside the repo.
fn project_dir(cwd: &Path) -> PathBuf {
    std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            PathBuf::from(String::from_utf8_lossy(&o.stdout).trim())
                .parent()
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| cwd.to_path_buf())
}

// shortcut: the t3 CLI has no interrupt and its MCP needs OAuth, so a helper thread calls the MCP
// interrupt tool for us. Replace with `t3 thread interrupt` once the CLI has it.
async fn interrupt(exe: &Path, project: &str, thread: &str) -> anyhow::Result<()> {
    let (instance, model) = STOP_MODEL.split_once(':').unwrap_or_default();
    let message = format!("Call the t3_thread_interrupt tool with threadId \"{thread}\" and stop true. Do nothing else, then reply with the tool result.");
    t3(
        exe,
        &[
            "thread",
            "start",
            project,
            "-",
            "--provider",
            instance,
            "--model",
            model,
            "--title",
            "Stop plantool run",
            "--wait",
        ],
        Some(&message),
    )
    .await
    .map(drop)
}

pub async fn run(
    opts: RunOptions,
    mut input: mpsc::Receiver<RunInput>,
    sink: EventSink,
) -> anyhow::Result<()> {
    let exe = opts.executable.clone().unwrap_or_else(|| "t3".into());
    emit(
        &sink,
        ProviderEvent::Message {
            id: "prompt".into(),
            role: "user".into(),
            content: opts.prompt.clone(),
        },
    )
    .await;
    let project = project_dir(&opts.cwd);
    let project = project.to_string_lossy();
    let mut thread = opts.resume.clone();
    let mut queued = vec![opts.prompt.clone()];
    let mut turn = 0usize;
    loop {
        if queued.is_empty() {
            match input.recv().await {
                Some(RunInput::Text(text)) => queued.push(text),
                Some(RunInput::Stop) | None => return Ok(()),
                _ => {}
            }
            continue;
        }
        let prompt = queued.remove(0);
        turn += 1;
        let turn_id = format!("t{turn}");
        emit(
            &sink,
            ProviderEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        )
        .await;
        let id = match &thread {
            Some(id) => {
                let before = show(&exe, id).await?.thread.run_count;
                t3(&exe, &["thread", "send", id, "-"], Some(&prompt)).await?;
                (id.clone(), before)
            }
            None => {
                let model = opts.model.as_deref().unwrap_or(DEFAULT_MODEL);
                let (instance, model) = model.split_once(':').with_context(|| {
                    format!("T3 model `{model}` must look like `claudeAgent:claude-opus-5-5`")
                })?;
                let id = t3(
                    &exe,
                    &[
                        "thread",
                        "start",
                        &project,
                        "-",
                        "--provider",
                        instance,
                        "--model",
                        model,
                    ],
                    Some(&prompt),
                )
                .await?;
                emit(
                    &sink,
                    ProviderEvent::ProviderSession {
                        session_id: id.clone(),
                    },
                )
                .await;
                thread = Some(id.clone());
                (id, 0)
            }
        };
        let (id, before) = id;
        let mut announced = false;
        let mut waiting = false;
        let shown = loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {
                    let shown = show(&exe, &id).await?;
                    if !announced {
                        announced = true;
                        emit(&sink, ProviderEvent::Status { label: "Running in T3 Code".into(), detail: shown.thread.link.clone() }).await;
                    }
                    if shown.thread.pending_request_count > 0 && !waiting {
                        emit(&sink, ProviderEvent::Status { label: "Waiting for your answer in T3 Code".into(), detail: shown.thread.link.clone() }).await;
                    }
                    waiting = shown.thread.pending_request_count > 0;
                    if shown.thread.active_run_id.is_none() && shown.thread.run_count > before {
                        break shown;
                    }
                }
                next = input.recv() => match next {
                    Some(RunInput::Text(text)) => queued.push(text),
                    Some(RunInput::Stop) | None => {
                        emit(&sink, ProviderEvent::Status { label: "Stopping the T3 Code thread".into(), detail: None }).await;
                        return interrupt(&exe, &project, &id).await;
                    }
                    _ => {}
                }
            }
        };
        let reply = shown
            .messages
            .into_iter()
            .rev()
            .find(|m| m.role == "assistant")
            .map(|m| m.text)
            .unwrap_or_default();
        emit(
            &sink,
            ProviderEvent::Message {
                id: format!("t3-{turn}"),
                role: "assistant".into(),
                content: reply,
            },
        )
        .await;
        let ok = shown.thread.status == "completed";
        emit(
            &sink,
            ProviderEvent::TurnCompleted {
                turn_id,
                status: if ok {
                    "completed".into()
                } else {
                    shown.thread.status.clone()
                },
                error: (!ok).then(|| format!("T3 thread ended as {}", shown.thread.status)),
            },
        )
        .await;
    }
}
