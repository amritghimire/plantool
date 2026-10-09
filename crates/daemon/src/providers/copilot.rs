use super::{emit, EventSink, ProviderEvent, RunInput, RunOptions};
use anyhow::Context;
use plantool_core::PermissionMode;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

pub async fn run(opts: RunOptions, mut input: mpsc::Receiver<RunInput>, sink: EventSink) -> anyhow::Result<()> {
    let session_id = opts.resume.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    emit(&sink, ProviderEvent::ProviderSession { session_id: session_id.clone() }).await;
    emit(&sink, ProviderEvent::Message { id: "prompt".into(), role: "user".into(), content: opts.prompt.clone() }).await;
    let mut queued = vec![opts.prompt.clone()];
    let mut turn = 0usize;
    let mut mode = opts.permission_mode;
    loop {
        if queued.is_empty() {
            match input.recv().await {
                Some(RunInput::Text(text)) => queued.push(text),
                Some(RunInput::PermissionMode(next)) => mode = next,
                Some(RunInput::Stop) | None => return Ok(()),
                _ => {}
            }
            continue;
        }
        let prompt = queued.remove(0);
        turn += 1;
        let turn_id = format!("t{turn}");
        emit(&sink, ProviderEvent::TurnStarted { turn_id: turn_id.clone() }).await;
        let exe = opts.executable.clone().unwrap_or_else(|| "copilot".into());
        let mut cmd = Command::new(&exe);
        cmd.arg("-p").arg(&prompt).arg("--silent").arg("--no-color")
            .arg("--no-ask-user").arg("--session-id").arg(&session_id);
        if let Some(model) = &opts.model { cmd.arg("--model").arg(model); }
        for root in &opts.writable_roots { cmd.arg("--add-dir").arg(root); }
        for path in prompt.lines().filter_map(|line| line.strip_prefix("Attached file: ")) {
            let Ok(file) = std::path::Path::new(path).canonicalize() else { continue };
            let native = file.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "pdf" | "heic" | "heif"));
            if native && opts.writable_roots.iter().any(|root| root.join("attachments").canonicalize().is_ok_and(|dir| file.starts_with(dir))) && file.is_file() {
                cmd.arg("--attachment").arg(file);
            }
        }
        match mode {
            PermissionMode::Ask => { cmd.arg("--allow-tool=read"); }
            PermissionMode::AcceptEdits => { cmd.arg("--allow-tool=read,write"); }
            PermissionMode::Auto | PermissionMode::AllowAll => { cmd.arg("--allow-all-tools"); }
        }
        if mode == PermissionMode::AllowAll { cmd.arg("--allow-all-paths").arg("--allow-all-urls"); }
        cmd.current_dir(&opts.cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut child = cmd.spawn().with_context(|| format!("failed to start {}", exe.display()))?;
        let stdout = child.stdout.take().context("copilot has no stdout")?;
        let mut lines = BufReader::new(stdout).lines();
        let stderr = child.stderr.take().context("copilot has no stderr")?;
        let errors = tokio::spawn(async move {
            let mut output = String::new();
            let _ = BufReader::new(stderr).take(64 * 1024).read_to_string(&mut output).await;
            output
        });
        let mut answer = String::new();
        loop {
            tokio::select! {
                line = lines.next_line() => match line? {
                    Some(line) => {
                        if !answer.is_empty() { answer.push('\n'); }
                        answer.push_str(&line);
                        emit(&sink, ProviderEvent::TextDelta { delta: format!("{line}\n"), segment: None }).await;
                    }
                    None => break,
                },
                next = input.recv() => match next {
                    Some(RunInput::Text(text)) => queued.push(text),
                    Some(RunInput::PermissionMode(next)) => mode = next,
                    Some(RunInput::Stop) | None => {
                        child.kill().await.ok();
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        let status = child.wait().await?;
        let error = errors.await.unwrap_or_default();
        if !status.success() {
            let detail = error.trim();
            anyhow::bail!("copilot exited with {status}: {}", if detail.is_empty() { "see Copilot CLI authentication and permissions" } else { detail });
        }
        emit(&sink, ProviderEvent::Message { id: format!("copilot-{turn}"), role: "assistant".into(), content: answer }).await;
        emit(&sink, ProviderEvent::TurnCompleted { turn_id, status: "completed".into(), error: None }).await;
    }
}
