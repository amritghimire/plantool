pub mod claude;
pub mod codex;
pub mod copilot;
pub mod opencode;

use plantool_core::{PermissionMode, Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputQuestion {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderEvent {
    Message { id: String, role: String, content: String },
    TextDelta { delta: String, #[serde(skip_serializing_if = "Option::is_none")] segment: Option<String> },
    ActivityStart { id: String, kind: String, title: String, #[serde(skip_serializing_if = "Option::is_none")] detail: Option<String> },
    ActivityOutput { id: String, delta: String },
    ActivityComplete { id: String, status: String, #[serde(skip_serializing_if = "Option::is_none")] output: Option<String> },
    Permission { request_id: String, kind: String, title: String, #[serde(skip_serializing_if = "Option::is_none")] detail: Option<String>, options: Vec<PermissionOption> },
    InputRequest { request_id: String, title: String, questions: Vec<InputQuestion> },
    RequestResolved { request_id: String },
    TurnStarted { turn_id: String },
    TurnCompleted { turn_id: String, status: String, #[serde(skip_serializing_if = "Option::is_none")] error: Option<String> },
    ProviderSession { session_id: String },
    Status { label: String, #[serde(skip_serializing_if = "Option::is_none")] detail: Option<String> },
    Raw { line: String },
}

#[derive(Debug, Clone)]
pub enum RunInput {
    Text(String),
    Permission { request_id: String, decision: String },
    Input { request_id: String, answers: BTreeMap<String, String> },
    PermissionMode(PermissionMode),
    Stop,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub cwd: PathBuf,
    pub prompt: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub resume: Option<String>,
    pub executable: Option<PathBuf>,
    pub writable_roots: Vec<PathBuf>,
    pub permission_mode: PermissionMode,
}

pub type EventSink = mpsc::Sender<ProviderEvent>;

pub async fn emit(sink: &EventSink, ev: ProviderEvent) {
    let _ = sink.send(ev).await;
}

pub async fn run_provider(provider: Provider, opts: RunOptions, input: mpsc::Receiver<RunInput>, sink: EventSink) -> anyhow::Result<()> {
    match provider {
        Provider::Claude => claude::run(opts, input, sink).await,
        Provider::Codex => codex::run(opts, input, sink).await,
        Provider::Copilot => copilot::run(opts, input, sink).await,
        Provider::Ollama => opencode::run(opts, input, sink).await,
        Provider::Opencode => anyhow::bail!("the opencode provider is not available in this build yet; run opencode in your terminal with `plantool skill`"),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub models: Vec<ModelOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

pub fn executable_for(provider: Provider) -> Option<PathBuf> {
    let var = match provider {
        Provider::Claude => "PLANTOOL_CLAUDE_PATH",
        Provider::Codex => "PLANTOOL_CODEX_PATH",
        Provider::Copilot => "PLANTOOL_COPILOT_PATH",
        Provider::Ollama => "PLANTOOL_OPENCODE_PATH",
        Provider::Opencode => "PLANTOOL_OPENCODE_PATH",
    };
    if let Some(p) = std::env::var_os(var) {
        return Some(PathBuf::from(p));
    }
    which::which(if provider == Provider::Ollama { "opencode" } else { provider.as_str() }).ok()
}

fn probe_version(exe: &std::path::Path) -> Result<String, String> {
    let out = std::process::Command::new(exe).arg("--version").output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.lines().next().unwrap_or("").trim().to_string())
}

pub fn discover() -> Vec<ProviderInfo> {
    let mut out = Vec::new();
    for (p, id) in [(Provider::Claude, "claude"), (Provider::Codex, "codex"), (Provider::Copilot, "copilot"), (Provider::Ollama, "ollama"), (Provider::Opencode, "opencode")] {
        let models = match p {
            Provider::Claude => vec![
                ModelOption { id: "claude-fable-5-1".into(), label: "Fable 5.1".into() },
                ModelOption { id: "claude-opus-5-5".into(), label: "Opus 5.5".into() },
                ModelOption { id: "claude-sonnet-5".into(), label: "Sonnet 5".into() },
                ModelOption { id: "claude-haiku-4-5-20251001".into(), label: "Haiku 4.5".into() },
            ],
            Provider::Ollama => Vec::new(),
            _ => Vec::new(),
        };
        if p == Provider::Ollama {
            let endpoint = ollama_base_url();
            let server = ollama_models();
            let opencode = executable_for(p).and_then(|exe| probe_version(&exe).ok());
            out.push(match (server, opencode) {
                (Ok(models), Some(version)) if !models.is_empty() => ProviderInfo { id, available: true, version: Some(version), error: None, models },
                (Ok(_), Some(version)) => ProviderInfo { id, available: false, version: Some(version), error: Some("No Ollama models are installed. Pull a model with `ollama pull <model>`.".into()), models: Vec::new() },
                (Err(error), _) => ProviderInfo { id, available: false, version: None, error: Some(format!("Ollama is unavailable at {endpoint}: {error}")), models: Vec::new() },
                (_, None) => ProviderInfo { id, available: false, version: None, error: Some("OpenCode CLI is required for Ollama hosted runs".into()), models: Vec::new() },
            });
            continue;
        }
        let info = match executable_for(p) {
            None => ProviderInfo { id, available: false, version: None, error: Some(format!("{id} is not on PATH")), models },
            Some(exe) => match probe_version(&exe) {
                Ok(v) => ProviderInfo { id, available: p != Provider::Opencode, version: Some(v), error: if p == Provider::Opencode { Some("opencode hosting is not available yet; use it from your terminal with `plantool skill`".into()) } else { None }, models },
                Err(e) => ProviderInfo { id, available: false, version: None, error: Some(e), models },
            },
        };
        out.push(info);
    }
    out
}

pub fn ollama_base_url() -> String {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".into());
    let host = host.trim_end_matches('/');
    if host.starts_with("http://") || host.starts_with("https://") { host.to_string() } else { format!("http://{host}") }
}

pub fn ollama_models() -> Result<Vec<ModelOption>, String> {
    let response = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(2)).build().map_err(|e| e.to_string())?
        .get(format!("{}/api/tags", ollama_base_url())).send().map_err(|e| e.to_string())?
        .error_for_status().map_err(|e| e.to_string())?;
    let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    let models = value.get("models").and_then(|v| v.as_array()).ok_or("Ollama returned no model list")?;
    Ok(models.iter().filter_map(|v| v.get("name").and_then(|n| n.as_str())).map(|name| ModelOption { id: name.into(), label: name.into() }).collect())
}

pub fn summarize_input(tool: &str, input: &serde_json::Value) -> String {
    let pick = |k: &str| input.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    match tool {
        "Bash" => pick("description").or_else(|| pick("command")).unwrap_or_default(),
        "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => pick("file_path").or_else(|| pick("notebook_path")).unwrap_or_default(),
        "Glob" | "Grep" => pick("pattern").unwrap_or_default(),
        "WebFetch" => pick("url").unwrap_or_default(),
        "WebSearch" => pick("query").unwrap_or_default(),
        "Task" | "Agent" => pick("description").unwrap_or_default(),
        _ => {
            let s = input.to_string();
            if s.len() > 120 { format!("{}…", &s[..120]) } else { s }
        }
    }
}

pub fn shorten(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}
