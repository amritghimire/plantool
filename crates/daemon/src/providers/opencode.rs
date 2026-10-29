use super::{emit, ollama_base_url, EventSink, ProviderEvent, RunInput, RunOptions};
use anyhow::Context;
use plantool_core::PermissionMode;
use serde_json::{json, Map, Value};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

fn config(opts: &RunOptions, model: &str, mode: PermissionMode) -> Value {
    let mut external = Map::new();
    external.insert(
        "*".into(),
        json!(if mode == PermissionMode::AllowAll {
            "allow"
        } else {
            "deny"
        }),
    );
    for root in &opts.writable_roots {
        external.insert(root.display().to_string(), json!("allow"));
        external.insert(format!("{}/**", root.display()), json!("allow"));
    }
    let permission = match mode {
        PermissionMode::Ask => {
            json!({ "*": "allow", "bash": "deny", "edit": "deny", "task": "deny", "external_directory": external })
        }
        PermissionMode::AcceptEdits => {
            json!({ "*": "allow", "bash": "deny", "external_directory": external })
        }
        PermissionMode::Auto | PermissionMode::AllowAll => {
            json!({ "*": "allow", "external_directory": external })
        }
    };
    let mut models = Map::new();
    models.insert(model.into(), json!({ "name": model }));
    json!({
        "share": "disabled",
        "provider": { "ollama": {
            "npm": "@ai-sdk/openai-compatible",
            "name": "Ollama (local)",
            "options": { "baseURL": format!("{}/v1", ollama_base_url()) },
            "models": models
        } },
        "permission": permission
    })
}

pub async fn run(
    opts: RunOptions,
    mut input: mpsc::Receiver<RunInput>,
    sink: EventSink,
) -> anyhow::Result<()> {
    let model = opts
        .model
        .clone()
        .context("choose an installed Ollama model")?;
    let mut session_id = opts.resume.clone();
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
        emit(
            &sink,
            ProviderEvent::Message {
                id: format!("opencode-user-{turn}"),
                role: "user".into(),
                content: prompt.clone(),
            },
        )
        .await;
        emit(
            &sink,
            ProviderEvent::TurnStarted {
                turn_id: turn_id.clone(),
            },
        )
        .await;
        let exe = opts.executable.clone().unwrap_or_else(|| "opencode".into());
        let mut cmd = Command::new(&exe);
        cmd.arg("run")
            .arg("--format")
            .arg("json")
            .arg("--auto")
            .arg("--model")
            .arg(format!("ollama/{model}"));
        if let Some(id) = &session_id {
            cmd.arg("--session").arg(id);
        }
        if let Some(variant) = &opts.effort {
            cmd.arg("--variant").arg(variant);
        }
        for file in super::attachments::file_args(
            &prompt,
            &opts.writable_roots,
            plantool_core::Provider::Ollama,
        )? {
            cmd.arg("--file").arg(file);
        }
        cmd.arg(&prompt)
            .current_dir(&opts.cwd)
            .env(
                "OPENCODE_CONFIG_CONTENT",
                config(&opts, &model, mode).to_string(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to start {}", exe.display()))?;
        let stdout = child.stdout.take().context("OpenCode has no stdout")?;
        let stderr = child.stderr.take().context("OpenCode has no stderr")?;
        let errors = tokio::spawn(async move {
            let mut output = String::new();
            let _ = BufReader::new(stderr)
                .take(64 * 1024)
                .read_to_string(&mut output)
                .await;
            output
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut answer = String::new();
        let mut error: Option<String> = None;
        loop {
            tokio::select! {
                line = lines.next_line() => match line? {
                    Some(line) => {
                        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
                        if session_id.is_none() {
                            if let Some(id) = event.get("sessionID").and_then(Value::as_str) {
                                session_id = Some(id.to_string());
                                emit(&sink, ProviderEvent::ProviderSession { session_id: id.to_string() }).await;
                            }
                        }
                        match event.get("type").and_then(Value::as_str).unwrap_or("") {
                            "text" => {
                                if let Some(content) = event.pointer("/part/text").and_then(Value::as_str) {
                                    if !answer.is_empty() { answer.push('\n'); }
                                    answer.push_str(content);
                                    emit(&sink, ProviderEvent::TextDelta { delta: format!("{content}\n"), segment: None }).await;
                                }
                            }
                            "tool_use" => {
                                let part = &event["part"];
                                let id = part["id"].as_str().unwrap_or("tool").to_string();
                                let tool = part["tool"].as_str().unwrap_or("tool").to_string();
                                let detail = part.pointer("/state/input").map(|v| super::shorten(&v.to_string(), 200));
                                emit(&sink, ProviderEvent::ActivityStart { id: id.clone(), kind: "tool".into(), title: tool, detail }).await;
                                let failed = part.pointer("/state/status").and_then(Value::as_str) == Some("error");
                                let output = part.pointer(if failed { "/state/error" } else { "/state/output" }).and_then(Value::as_str).map(|s| super::shorten(s, 4000));
                                emit(&sink, ProviderEvent::ActivityComplete { id, status: if failed { "failed" } else { "completed" }.into(), output }).await;
                            }
                            "error" => {
                                error = event.pointer("/error/data/message").and_then(Value::as_str).or_else(|| event.pointer("/error/message").and_then(Value::as_str)).map(str::to_string).or_else(|| Some(event["error"].to_string()));
                            }
                            _ => {}
                        }
                    }
                    None => break,
                },
                next = input.recv() => match next {
                    Some(RunInput::Text(text)) => queued.push(text),
                    Some(RunInput::PermissionMode(next)) => mode = next,
                    Some(RunInput::Stop) | None => { child.kill().await.ok(); return Ok(()); }
                    _ => {}
                }
            }
        }
        let status = child.wait().await?;
        let stderr = errors.await.unwrap_or_default();
        if !status.success() || error.is_some() {
            anyhow::bail!(
                "OpenCode run failed: {}",
                error.unwrap_or_else(|| if stderr.trim().is_empty() {
                    status.to_string()
                } else {
                    stderr.trim().to_string()
                })
            );
        }
        emit(
            &sink,
            ProviderEvent::Message {
                id: format!("opencode-{turn}"),
                role: "assistant".into(),
                content: answer,
            },
        )
        .await;
        emit(
            &sink,
            ProviderEvent::TurnCompleted {
                turn_id,
                status: "completed".into(),
                error: None,
            },
        )
        .await;
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn reads_opencode_events_and_keeps_the_session_for_followups() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-opencode");
        std::fs::write(&script, "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"step_start\",\"sessionID\":\"ses_test\"}' '{\"type\":\"text\",\"sessionID\":\"ses_test\",\"part\":{\"text\":\"Done\"}}'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opts = RunOptions {
            cwd: dir.path().into(),
            prompt: "hello".into(),
            model: Some("qwen3:8b".into()),
            effort: None,
            resume: None,
            executable: Some(script),
            writable_roots: vec![dir.path().into()],
            permission_mode: PermissionMode::Ask,
        };
        let (tx, rx) = mpsc::channel(8);
        let (events_tx, mut events_rx) = mpsc::channel(32);
        let task = tokio::spawn(run(opts, rx, events_tx));
        let mut session = false;
        let mut answer = false;
        while let Some(event) =
            tokio::time::timeout(std::time::Duration::from_secs(5), events_rx.recv())
                .await
                .unwrap()
        {
            match event {
                ProviderEvent::ProviderSession { session_id } => {
                    assert_eq!(session_id, "ses_test");
                    session = true;
                }
                ProviderEvent::Message { role, content, .. } if role == "assistant" => {
                    assert_eq!(content, "Done");
                    answer = true;
                }
                ProviderEvent::TurnCompleted { .. } => break,
                _ => {}
            }
        }
        tx.send(RunInput::Stop).await.unwrap();
        task.await.unwrap().unwrap();
        assert!(session && answer);
    }
}
