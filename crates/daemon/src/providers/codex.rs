use super::{emit, EventSink, InputQuestion, PermissionOption, ProviderEvent, RunInput, RunOptions};
use anyhow::Context;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};

const COMMAND_APPROVAL: &str = "item/commandExecution/requestApproval";
const FILE_APPROVAL: &str = "item/fileChange/requestApproval";
const PERMISSIONS_APPROVAL: &str = "item/permissions/requestApproval";
const REQUEST_USER_INPUT: &str = "item/tool/requestUserInput";

struct Rpc {
    stdin: ChildStdin,
    next_id: u64,
    waiting: HashMap<u64, oneshot::Sender<Result<Value, String>>>,
}

impl Rpc {
    async fn write(&mut self, v: Value) -> anyhow::Result<()> {
        self.stdin.write_all(format!("{v}\n").as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn request(&mut self, method: &str, params: Value) -> anyhow::Result<oneshot::Receiver<Result<Value, String>>> {
        self.next_id += 1;
        let id = self.next_id;
        let (tx, rx) = oneshot::channel();
        self.waiting.insert(id, tx);
        self.write(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        Ok(rx)
    }

    async fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write(json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    async fn respond(&mut self, id: Value, result: Value) -> anyhow::Result<()> {
        self.write(json!({ "jsonrpc": "2.0", "id": id, "result": result })).await
    }

    async fn respond_error(&mut self, id: Value, message: &str) -> anyhow::Result<()> {
        self.write(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } })).await
    }

    fn settle(&mut self, id: u64, result: Result<Value, String>) {
        if let Some(tx) = self.waiting.remove(&id) {
            let _ = tx.send(result);
        }
    }
}

struct PendingApproval {
    rpc_id: Value,
    decisions: HashMap<String, Value>,
}

fn decision_options(available: Option<&Vec<Value>>, kind: &str) -> (Vec<PermissionOption>, HashMap<String, Value>) {
    let native: Vec<Value> = match available {
        Some(a) if !a.is_empty() => a.clone(),
        _ if kind == "file-change" => vec![json!("accept"), json!("acceptForSession"), json!("decline"), json!("cancel")],
        _ => vec![json!("accept"), json!("decline")],
    };
    let mut options = Vec::new();
    let mut map = HashMap::new();
    for d in native {
        let (id, label) = match &d {
            Value::String(s) => match s.as_str() {
                "accept" => ("allow".to_string(), "Allow".to_string()),
                "acceptForSession" => ("allow-session".to_string(), "Allow for this run".to_string()),
                "decline" => ("deny".to_string(), "Deny".to_string()),
                "cancel" => ("cancel".to_string(), "Deny and stop".to_string()),
                other => (other.to_string(), other.to_string()),
            },
            other => {
                let s = other.to_string();
                (format!("native:{}", super::shorten(&s, 40)), format!("Allow ({})", super::shorten(&s, 60)))
            }
        };
        map.insert(id.clone(), json!({ "decision": d }));
        options.push(PermissionOption { id, label });
    }
    (options, map)
}

pub async fn run(opts: RunOptions, mut input: mpsc::Receiver<RunInput>, sink: EventSink) -> anyhow::Result<()> {
    let exe = opts.executable.clone().unwrap_or_else(|| "codex".into());
    let mut cmd = Command::new(&exe);
    cmd.arg("app-server").arg("--stdio").current_dir(&opts.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().with_context(|| format!("failed to start {}", exe.display()))?;
    let stdin = child.stdin.take().context("no stdin")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let mut lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();
    let mut stderr_tail: Vec<String> = Vec::new();
    let mut rpc = Rpc { stdin, next_id: 0, waiting: HashMap::new() };

    let init_rx = rpc.request("initialize", json!({ "clientInfo": { "name": "plantool", "title": "plantool", "version": env!("CARGO_PKG_VERSION") }, "capabilities": { "experimentalApi": true } })).await?;
    let mut thread_id: Option<String> = None;
    let mut active_turn: Option<String> = None;
    let mut pending_approvals: HashMap<String, PendingApproval> = HashMap::new();
    let mut pending_inputs: HashMap<String, Value> = HashMap::new();
    let mut queued: Vec<String> = vec![opts.prompt.clone()];
    let mut phase = 0u8;
    let mut thread_rx: Option<oneshot::Receiver<Result<Value, String>>> = None;
    let mut turn_rx: Option<oneshot::Receiver<Result<Value, String>>> = None;
    let mut init_rx = Some(init_rx);
    let mut stopped = false;
    emit(&sink, ProviderEvent::Message { id: "prompt".into(), role: "user".into(), content: opts.prompt.clone() }).await;

    loop {
        if phase == 0 {
            if let Some(rx) = init_rx.as_mut() {
                if let Ok(res) = rx.try_recv() {
                    res.map_err(|e| anyhow::anyhow!("codex initialize failed: {e}"))?;
                    rpc.notify("initialized", json!({})).await?;
                    let mut cfg = json!({ "cwd": opts.cwd, "sandbox": "workspace-write", "approvalPolicy": "on-request", "approvalsReviewer": "user" });
                    if let Some(m) = &opts.model { cfg["model"] = json!(m); }
                    let rx = if let Some(t) = &opts.resume {
                        cfg["threadId"] = json!(t);
                        rpc.request("thread/resume", cfg).await?
                    } else {
                        rpc.request("thread/start", cfg).await?
                    };
                    thread_rx = Some(rx);
                    phase = 1;
                    init_rx = None;
                }
            }
        }
        if phase == 1 {
            if let Some(rx) = thread_rx.as_mut() {
                if let Ok(res) = rx.try_recv() {
                    let v = res.map_err(|e| anyhow::anyhow!("codex thread/start failed: {e}"))?;
                    let id = v.pointer("/thread/id").and_then(|s| s.as_str()).context("codex returned no thread id")?.to_string();
                    emit(&sink, ProviderEvent::ProviderSession { session_id: id.clone() }).await;
                    emit(&sink, ProviderEvent::Status { label: "codex thread ready".into(), detail: None }).await;
                    thread_id = Some(id);
                    phase = 2;
                    thread_rx = None;
                }
            }
        }
        if phase == 2 && active_turn.is_none() && turn_rx.is_none() && !queued.is_empty() {
            let text = queued.remove(0);
            let tid = thread_id.clone().unwrap_or_default();
            let rx = rpc.request("turn/start", json!({ "threadId": tid, "input": [{ "type": "text", "text": text, "text_elements": [] }], "sandboxPolicy": { "type": "workspaceWrite", "writableRoots": opts.writable_roots, "networkAccess": true } })).await?;
            turn_rx = Some(rx);
        }
        if let Some(rx) = turn_rx.as_mut() {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(v) => {
                        if let Some(id) = v.pointer("/turn/id").and_then(|s| s.as_str()) {
                            if active_turn.is_none() {
                                active_turn = Some(id.to_string());
                                emit(&sink, ProviderEvent::TurnStarted { turn_id: id.to_string() }).await;
                            }
                        }
                    }
                    Err(e) => emit(&sink, ProviderEvent::TurnCompleted { turn_id: "?".into(), status: "failed".into(), error: Some(e) }).await,
                }
                turn_rx = None;
            }
        }

        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let v: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => { emit(&sink, ProviderEvent::Raw { line: super::shorten(&line, 500) }).await; continue; } };
                let method = v.get("method").and_then(|m| m.as_str());
                let id = v.get("id").cloned();
                let params = v.get("params").cloned().unwrap_or(json!({}));
                match (method, id) {
                    (None, Some(Value::Number(n))) => {
                        let rid = n.as_u64().unwrap_or(0);
                        let result = match v.get("error") {
                            Some(e) => Err(e.get("message").and_then(|m| m.as_str()).unwrap_or("rpc error").to_string()),
                            None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        rpc.settle(rid, result);
                    }
                    (Some(m), Some(id)) => {
                        let request_id = format!("codex-{}", id);
                        match m {
                            COMMAND_APPROVAL | FILE_APPROVAL => {
                                let kind = if m == COMMAND_APPROVAL { "command" } else { "file-change" };
                                let (options, decisions) = decision_options(params.get("availableDecisions").and_then(|a| a.as_array()), kind);
                                let detail = params.get("command").and_then(|c| c.as_str()).map(|s| s.to_string())
                                    .or_else(|| params.get("reason").and_then(|c| c.as_str()).map(|s| s.to_string()))
                                    .or_else(|| params.get("changes").map(|c| super::shorten(&c.to_string(), 3000)));
                                let title = if kind == "command" { "Codex wants to run a command".to_string() } else { "Codex wants to edit files".to_string() };
                                pending_approvals.insert(request_id.clone(), PendingApproval { rpc_id: id, decisions });
                                emit(&sink, ProviderEvent::Permission { request_id, kind: kind.into(), title, detail, options }).await;
                            }
                            REQUEST_USER_INPUT => {
                                let qs: Vec<InputQuestion> = params.get("questions").and_then(|q| q.as_array()).map(|a| a.iter().filter_map(|q| {
                                    let qid = q.get("id").and_then(|s| s.as_str())?;
                                    let text = q.get("question").and_then(|s| s.as_str())?;
                                    let options = q.get("options").and_then(|o| o.as_array()).map(|o| o.iter().filter_map(|x| x.get("label").and_then(|l| l.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default();
                                    Some(InputQuestion { id: qid.to_string(), text: text.to_string(), options })
                                }).collect()).unwrap_or_default();
                                if qs.is_empty() {
                                    rpc.respond_error(id, "no supported question").await?;
                                } else {
                                    let title = qs[0].text.clone();
                                    pending_inputs.insert(request_id.clone(), id);
                                    emit(&sink, ProviderEvent::InputRequest { request_id, title, questions: qs }).await;
                                }
                            }
                            PERMISSIONS_APPROVAL => {
                                let (options, decisions) = decision_options(params.get("availableDecisions").and_then(|a| a.as_array()), "tool");
                                let detail = Some(super::shorten(&params.to_string(), 2000));
                                pending_approvals.insert(request_id.clone(), PendingApproval { rpc_id: id, decisions });
                                emit(&sink, ProviderEvent::Permission { request_id, kind: "tool".into(), title: "Codex requests additional permissions".into(), detail, options }).await;
                            }
                            other => { rpc.respond_error(id, &format!("unsupported request {other}")).await?; }
                        }
                    }
                    (Some(m), None) => {
                        match m {
                            "item/agentMessage/delta" => {
                                if let Some(d) = params.get("delta").and_then(|d| d.as_str()) {
                                    emit(&sink, ProviderEvent::TextDelta { delta: d.to_string(), segment: params.get("itemId").and_then(|s| s.as_str()).map(|s| s.to_string()) }).await;
                                }
                            }
                            "item/commandExecution/outputDelta" => {
                                if let Some(d) = params.get("delta").and_then(|d| d.as_str()) {
                                    emit(&sink, ProviderEvent::ActivityOutput { id: params.get("itemId").and_then(|s| s.as_str()).unwrap_or("command").to_string(), delta: d.to_string() }).await;
                                }
                            }
                            "item/started" | "item/completed" => {
                                let item = params.get("item").cloned().unwrap_or(json!({}));
                                let completed = m == "item/completed";
                                let iid = item.get("id").and_then(|s| s.as_str()).unwrap_or("item").to_string();
                                match item.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                                    "agentMessage" => if completed { if let Some(t) = item.get("text").and_then(|t| t.as_str()) { emit(&sink, ProviderEvent::Message { id: iid, role: "assistant".into(), content: t.to_string() }).await; } },
                                    "commandExecution" => {
                                        let cmd = item.get("command").and_then(|c| c.as_str()).unwrap_or("command").to_string();
                                        if completed {
                                            let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("completed");
                                            let st = if status == "failed" { "failed" } else if status == "declined" { "denied" } else { "completed" };
                                            emit(&sink, ProviderEvent::ActivityComplete { id: iid, status: st.into(), output: item.get("aggregatedOutput").and_then(|o| o.as_str()).map(|s| super::shorten(s, 6000)) }).await;
                                        } else {
                                            emit(&sink, ProviderEvent::ActivityStart { id: iid, kind: "command".into(), title: super::shorten(&cmd, 120), detail: Some(cmd) }).await;
                                        }
                                    }
                                    "fileChange" => {
                                        let paths: Vec<String> = item.get("changes").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|c| c.get("path").and_then(|p| p.as_str()).map(|s| s.to_string())).collect()).unwrap_or_default();
                                        if completed {
                                            let status = item.get("status").and_then(|s| s.as_str()).unwrap_or("completed");
                                            emit(&sink, ProviderEvent::ActivityComplete { id: iid, status: if status == "failed" { "failed".into() } else { "completed".into() }, output: None }).await;
                                        } else {
                                            emit(&sink, ProviderEvent::ActivityStart { id: iid, kind: "edit".into(), title: super::shorten(&paths.join(", "), 120), detail: None }).await;
                                        }
                                    }
                                    "webSearch" | "mcpToolCall" | "dynamicToolCall" => {
                                        let title = item.get("query").or_else(|| item.get("tool")).and_then(|s| s.as_str()).unwrap_or("tool").to_string();
                                        if completed {
                                            emit(&sink, ProviderEvent::ActivityComplete { id: iid, status: "completed".into(), output: None }).await;
                                        } else {
                                            emit(&sink, ProviderEvent::ActivityStart { id: iid, kind: "tool".into(), title, detail: None }).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "turn/started" => {
                                if let Some(id) = params.pointer("/turn/id").and_then(|s| s.as_str()) {
                                    if active_turn.as_deref() != Some(id) {
                                        active_turn = Some(id.to_string());
                                        emit(&sink, ProviderEvent::TurnStarted { turn_id: id.to_string() }).await;
                                    }
                                }
                            }
                            "turn/completed" => {
                                let turn = params.get("turn").cloned().unwrap_or(json!({}));
                                let id = turn.get("id").and_then(|s| s.as_str()).unwrap_or("?").to_string();
                                let status = turn.get("status").and_then(|s| s.as_str()).unwrap_or("completed").to_string();
                                let error = turn.pointer("/error/message").and_then(|s| s.as_str()).map(|s| s.to_string());
                                if active_turn.as_deref() == Some(id.as_str()) || params.get("threadId").and_then(|t| t.as_str()) == thread_id.as_deref() {
                                    active_turn = None;
                                }
                                emit(&sink, ProviderEvent::TurnCompleted { turn_id: id, status: if status == "completed" { "completed".into() } else if status == "interrupted" { "interrupted".into() } else { "failed".into() }, error }).await;
                            }
                            "thread/status/changed" => {
                                if let Some(s) = params.pointer("/status/type").and_then(|s| s.as_str()) {
                                    emit(&sink, ProviderEvent::Status { label: format!("codex: {s}"), detail: None }).await;
                                }
                            }
                            "model/rerouted" => {
                                emit(&sink, ProviderEvent::Status { label: "model rerouted".into(), detail: Some(super::shorten(&params.to_string(), 300)) }).await;
                            }
                            _ => {}
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
                        emit(&sink, ProviderEvent::Message { id: format!("u-{}", plantool_core::now()), role: "user".into(), content: text.clone() }).await;
                        if let (Some(tid), Some(turn)) = (&thread_id, &active_turn) {
                            let _rx = rpc.request("turn/steer", json!({ "threadId": tid, "expectedTurnId": turn, "input": [{ "type": "text", "text": text, "text_elements": [] }] })).await?;
                        } else {
                            queued.push(text);
                        }
                    }
                    Some(RunInput::Permission { request_id, decision }) => {
                        if let Some(p) = pending_approvals.remove(&request_id) {
                            let result = p.decisions.get(&decision).cloned()
                                .or_else(|| if decision.starts_with("allow") { p.decisions.get("allow").cloned() } else { p.decisions.get("deny").cloned().or_else(|| p.decisions.get("cancel").cloned()) })
                                .unwrap_or(json!({ "decision": "decline" }));
                            rpc.respond(p.rpc_id, result).await?;
                            emit(&sink, ProviderEvent::RequestResolved { request_id }).await;
                        }
                    }
                    Some(RunInput::Input { request_id, answers }) => {
                        if let Some(rpc_id) = pending_inputs.remove(&request_id) {
                            let mut map = serde_json::Map::new();
                            for (k, v) in answers { map.insert(k, json!({ "answers": [v] })); }
                            rpc.respond(rpc_id, json!({ "answers": map })).await?;
                            emit(&sink, ProviderEvent::RequestResolved { request_id }).await;
                        }
                    }
                    Some(RunInput::Stop) | None => { stopped = true; break; }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }
    }
    if stopped {
        if let (Some(tid), Some(turn)) = (&thread_id, &active_turn) {
            let _rx = rpc.request("turn/interrupt", json!({ "threadId": tid, "turnId": turn })).await;
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        let _ = child.start_kill();
        let _ = child.wait().await;
        emit(&sink, ProviderEvent::Status { label: "stopped".into(), detail: None }).await;
        return Ok(());
    }
    let status = child.wait().await?;
    if !status.success() {
        let tail = stderr_tail.join("\n");
        anyhow::bail!("codex exited with {status}{}", if tail.is_empty() { String::new() } else { format!(": {}", super::shorten(&tail, 800)) });
    }
    Ok(())
}
