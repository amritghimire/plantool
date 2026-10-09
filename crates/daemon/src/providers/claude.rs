use super::{emit, EventSink, InputQuestion, PermissionOption, ProviderEvent, RunInput, RunOptions};
use anyhow::Context;
use plantool_core::PermissionMode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

const READ_ONLY_TOOLS: &[&str] = &["Read", "Glob", "Grep", "LS", "WebFetch", "WebSearch", "TodoWrite", "TodoRead", "NotebookRead", "Task", "Agent", "ToolSearch", "Skill", "BashOutput", "KillShell", "LSP"];

struct Pending {
    tool: String,
    input: Value,
}

fn claude_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Ask => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Auto => "auto",
        PermissionMode::AllowAll => "bypassPermissions",
    }
}

pub async fn run(opts: RunOptions, mut input: mpsc::Receiver<RunInput>, sink: EventSink) -> anyhow::Result<()> {
    let exe = opts.executable.clone().unwrap_or_else(|| "claude".into());
    let mut mode = opts.permission_mode;
    let mut cmd = Command::new(&exe);
    cmd.arg("-p")
        .arg("--input-format").arg("stream-json")
        .arg("--output-format").arg("stream-json")
        .arg("--verbose")
        .arg("--include-partial-messages")
        .arg("--permission-mode").arg(claude_mode(mode))
        .arg("--allow-dangerously-skip-permissions")
        .arg("--permission-prompt-tool").arg("stdio");
    if let Some(m) = &opts.model {
        cmd.arg("--model").arg(m);
    }
    if let Some(effort) = &opts.effort {
        cmd.arg("--effort").arg(effort);
    }
    if let Some(r) = &opts.resume {
        cmd.arg("--resume").arg(r);
    }
    cmd.current_dir(&opts.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().with_context(|| format!("failed to start {}", exe.display()))?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();
    let mut stderr_tail: Vec<String> = Vec::new();

    let init = json!({ "type": "control_request", "request_id": "plantool-init", "request": { "subtype": "initialize", "hooks": {} } });
    stdin.write_all(format!("{init}\n").as_bytes()).await?;
    send_user(&mut stdin, &opts.prompt).await?;
    emit(&sink, ProviderEvent::Message { id: "prompt".into(), role: "user".into(), content: opts.prompt.clone() }).await;

    let mut pending: HashMap<String, Pending> = HashMap::new();
    let mut tool_names: HashMap<String, String> = HashMap::new();
    let mut turn_counter = 0u32;
    let mut turn_open = false;
    let mut stopped = false;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let v: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => { emit(&sink, ProviderEvent::Raw { line: super::shorten(&line, 500) }).await; continue; } };
                match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "system" => {
                        if v.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                            if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                                emit(&sink, ProviderEvent::ProviderSession { session_id: sid.to_string() }).await;
                            }
                            let model = v.get("model").and_then(|m| m.as_str()).unwrap_or("");
                            emit(&sink, ProviderEvent::Status { label: format!("claude session ready ({model})"), detail: None }).await;
                            if !turn_open {
                                turn_counter += 1;
                                turn_open = true;
                                emit(&sink, ProviderEvent::TurnStarted { turn_id: format!("t{turn_counter}") }).await;
                            }
                        }
                    }
                    "stream_event" => {
                        let ev = v.get("event").cloned().unwrap_or(Value::Null);
                        if ev.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                            if let Some(text) = ev.pointer("/delta/text").and_then(|t| t.as_str()) {
                                emit(&sink, ProviderEvent::TextDelta { delta: text.to_string(), segment: None }).await;
                            }
                        }
                    }
                    "assistant" => {
                        let msg_id = v.pointer("/message/id").and_then(|s| s.as_str()).unwrap_or("msg").to_string();
                        if let Some(blocks) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                            for (i, b) in blocks.iter().enumerate() {
                                match b.get("type").and_then(|t| t.as_str()) {
                                    Some("text") => {
                                        let text = b.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                                        if !text.is_empty() {
                                            emit(&sink, ProviderEvent::Message { id: format!("{msg_id}:{i}"), role: "assistant".into(), content: text }).await;
                                        }
                                    }
                                    Some("tool_use") => {
                                        let id = b.get("id").and_then(|s| s.as_str()).unwrap_or("tool").to_string();
                                        let name = b.get("name").and_then(|s| s.as_str()).unwrap_or("tool").to_string();
                                        let input = b.get("input").cloned().unwrap_or(Value::Null);
                                        tool_names.insert(id.clone(), name.clone());
                                        let title = format!("{name} {}", super::shorten(&super::summarize_input(&name, &input), 100));
                                        let detail = serde_json::to_string_pretty(&input).ok().map(|d| super::shorten(&d, 4000));
                                        emit(&sink, ProviderEvent::ActivityStart { id, kind: "tool".into(), title, detail }).await;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    "user" => {
                        if let Some(blocks) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                            for b in blocks {
                                if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                                    let id = b.get("tool_use_id").and_then(|s| s.as_str()).unwrap_or("tool").to_string();
                                    let is_error = b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
                                    let output = match b.get("content") {
                                        Some(Value::String(s)) => s.clone(),
                                        Some(Value::Array(a)) => a.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"),
                                        _ => String::new(),
                                    };
                                    emit(&sink, ProviderEvent::ActivityComplete { id, status: if is_error { "failed".into() } else { "completed".into() }, output: Some(super::shorten(&output, 6000)) }).await;
                                }
                            }
                        }
                    }
                    "result" => {
                        let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
                        let status = if subtype == "success" { "completed" } else { "failed" };
                        let error = if subtype == "success" { None } else { Some(v.get("result").and_then(|r| r.as_str()).map(|s| s.to_string()).unwrap_or_else(|| subtype.to_string())) };
                        turn_open = false;
                        emit(&sink, ProviderEvent::TurnCompleted { turn_id: format!("t{turn_counter}"), status: status.into(), error }).await;
                        if let Some(cost) = v.get("total_cost_usd").and_then(|c| c.as_f64()) {
                            emit(&sink, ProviderEvent::Status { label: format!("turn done · ${cost:.2} at API rates (an estimate; a subscription is not charged per token)"), detail: None }).await;
                        }
                    }
                    "control_request" => {
                        let request_id = v.get("request_id").and_then(|s| s.as_str()).unwrap_or("").to_string();
                        let req = v.get("request").cloned().unwrap_or(Value::Null);
                        if req.get("subtype").and_then(|s| s.as_str()) != Some("can_use_tool") {
                            continue;
                        }
                        let tool = req.get("tool_name").and_then(|s| s.as_str()).unwrap_or("").to_string();
                        let input_v = req.get("input").cloned().unwrap_or(json!({}));
                        if READ_ONLY_TOOLS.contains(&tool.as_str()) || (mode == PermissionMode::AllowAll && tool != "AskUserQuestion") {
                            respond(&mut stdin, &request_id, json!({ "behavior": "allow", "updatedInput": input_v })).await?;
                            continue;
                        }
                        if tool == "AskUserQuestion" {
                            let questions = input_v.get("questions").and_then(|q| q.as_array()).cloned().unwrap_or_default();
                            let qs: Vec<InputQuestion> = questions.iter().enumerate().map(|(i, q)| InputQuestion {
                                id: q.get("question").and_then(|s| s.as_str()).unwrap_or(&format!("q{i}")).to_string(),
                                text: q.get("question").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                                options: q.get("options").and_then(|o| o.as_array()).map(|a| a.iter().filter_map(|o| o.get("label").and_then(|l| l.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default(),
                            }).collect();
                            let title = qs.first().map(|q| q.text.clone()).unwrap_or_else(|| "The agent has a question".into());
                            pending.insert(request_id.clone(), Pending { tool, input: input_v });
                            emit(&sink, ProviderEvent::InputRequest { request_id, title, questions: qs }).await;
                            continue;
                        }
                        let kind = match tool.as_str() { "Bash" => "command", "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => "file-change", _ => "tool" };
                        let title = format!("Allow {tool}: {}?", super::shorten(&super::summarize_input(&tool, &input_v), 120));
                        let detail = match tool.as_str() {
                            "Bash" => input_v.get("command").and_then(|c| c.as_str()).map(|s| s.to_string()),
                            "Write" => Some(format!("{}\n\n{}", input_v.get("file_path").and_then(|c| c.as_str()).unwrap_or(""), super::shorten(input_v.get("content").and_then(|c| c.as_str()).unwrap_or(""), 3000))),
                            "Edit" => Some(format!("{}\n\n--- old\n{}\n\n+++ new\n{}", input_v.get("file_path").and_then(|c| c.as_str()).unwrap_or(""), super::shorten(input_v.get("old_string").and_then(|c| c.as_str()).unwrap_or(""), 1500), super::shorten(input_v.get("new_string").and_then(|c| c.as_str()).unwrap_or(""), 1500))),
                            _ => serde_json::to_string_pretty(&input_v).ok().map(|d| super::shorten(&d, 3000)),
                        };
                        pending.insert(request_id.clone(), Pending { tool, input: input_v });
                        emit(&sink, ProviderEvent::Permission { request_id, kind: kind.into(), title, detail, options: vec![
                            PermissionOption { id: "allow".into(), label: "Allow".into() },
                            PermissionOption { id: "deny".into(), label: "Deny".into() },
                        ] }).await;
                    }
                    "control_cancel_request" => {
                        if let Some(id) = v.get("request_id").and_then(|s| s.as_str()) {
                            pending.remove(id);
                            emit(&sink, ProviderEvent::RequestResolved { request_id: id.to_string() }).await;
                        }
                    }
                    _ => {}
                }
            }
            err = err_lines.next_line() => {
                if let Ok(Some(l)) = err {
                    if stderr_tail.len() >= 40 { stderr_tail.remove(0); }
                    stderr_tail.push(l);
                }
            }
            msg = input.recv() => {
                match msg {
                    Some(RunInput::Text(text)) => {
                        if !turn_open {
                            turn_counter += 1;
                            turn_open = true;
                            emit(&sink, ProviderEvent::TurnStarted { turn_id: format!("t{turn_counter}") }).await;
                        }
                        emit(&sink, ProviderEvent::Message { id: format!("u{turn_counter}-{}", plantool_core::now()), role: "user".into(), content: text.clone() }).await;
                        send_user(&mut stdin, &text).await?;
                    }
                    Some(RunInput::Permission { request_id, decision }) => {
                        if let Some(p) = pending.remove(&request_id) {
                            let body = if decision.starts_with("allow") {
                                json!({ "behavior": "allow", "updatedInput": p.input })
                            } else {
                                json!({ "behavior": "deny", "message": "The user declined this action in plantool." })
                            };
                            respond(&mut stdin, &request_id, body).await?;
                            emit(&sink, ProviderEvent::RequestResolved { request_id }).await;
                        }
                    }
                    Some(RunInput::Input { request_id, answers }) => {
                        if let Some(p) = pending.remove(&request_id) {
                            let mut updated = p.input.clone();
                            let mut map = serde_json::Map::new();
                            for (k, v) in answers { map.insert(k, Value::String(v)); }
                            updated["answers"] = Value::Object(map);
                            let _ = p.tool;
                            respond(&mut stdin, &request_id, json!({ "behavior": "allow", "updatedInput": updated })).await?;
                            emit(&sink, ProviderEvent::RequestResolved { request_id }).await;
                        }
                    }
                    Some(RunInput::PermissionMode(m)) => {
                        mode = m;
                        let req = json!({ "type": "control_request", "request_id": format!("plantool-mode-{}", plantool_core::now()), "request": { "subtype": "set_permission_mode", "mode": claude_mode(m) } });
                        stdin.write_all(format!("{req}\n").as_bytes()).await?;
                        emit(&sink, ProviderEvent::Status { label: format!("permissions: {m}"), detail: None }).await;
                        if m == PermissionMode::AllowAll {
                            let ids: Vec<String> = pending.iter().filter(|(_, p)| p.tool != "AskUserQuestion").map(|(id, _)| id.clone()).collect();
                            for id in ids {
                                if let Some(p) = pending.remove(&id) {
                                    respond(&mut stdin, &id, json!({ "behavior": "allow", "updatedInput": p.input })).await?;
                                    emit(&sink, ProviderEvent::RequestResolved { request_id: id }).await;
                                }
                            }
                        }
                    }
                    Some(RunInput::Stop) | None => { stopped = true; break; }
                }
            }
        }
    }
    if stopped {
        let _ = child.start_kill();
        let _ = child.wait().await;
        emit(&sink, ProviderEvent::Status { label: "stopped".into(), detail: None }).await;
        return Ok(());
    }
    let status = child.wait().await?;
    if !status.success() {
        let tail = stderr_tail.join("\n");
        anyhow::bail!("claude exited with {status}{}", if tail.is_empty() { String::new() } else { format!(": {}", super::shorten(&tail, 800)) });
    }
    Ok(())
}

async fn send_user(stdin: &mut tokio::process::ChildStdin, text: &str) -> anyhow::Result<()> {
    let msg = json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } });
    stdin.write_all(format!("{msg}\n").as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

async fn respond(stdin: &mut tokio::process::ChildStdin, request_id: &str, response: Value) -> anyhow::Result<()> {
    let msg = json!({ "type": "control_response", "response": { "subtype": "success", "request_id": request_id, "response": response } });
    stdin.write_all(format!("{msg}\n").as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}
