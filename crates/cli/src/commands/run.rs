use crate::client::{open_browser, print_json, Client};
use clap::Args as ClapArgs;

#[derive(ClapArgs)]
pub struct Args {
    pub reference: String,
    /// Agent provider: claude, codex or opencode (default: claude, or PLANTOOL_PROVIDER).
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    /// Extra instructions appended to the stage prompt.
    #[arg(long)]
    pub prompt: Option<String>,
    /// Permission mode: ask (default), accept-edits, auto or allow-all. Also PLANTOOL_PERMISSION.
    #[arg(long)]
    pub permission: Option<String>,
    /// For implement: work in the current checkout instead of a git worktree.
    #[arg(long)]
    pub no_worktree: bool,
    #[arg(long)]
    pub no_open: bool,
    #[arg(long)]
    pub json: bool,
}

pub fn run(a: Args, stage: &str) -> anyhow::Result<()> {
    let c = Client::connect()?;
    let (key, _) = c.resolve_key(&a.reference)?;
    let provider = a.provider.or_else(|| std::env::var("PLANTOOL_PROVIDER").ok()).unwrap_or_else(|| "claude".to_string());
    let permission = a.permission.or_else(|| std::env::var("PLANTOOL_PERMISSION").ok());
    let body = serde_json::json!({ "provider": provider, "stage": stage, "model": a.model, "prompt": a.prompt, "permission_mode": permission, "worktree": !a.no_worktree });
    let v: serde_json::Value = c.post(&format!("/api/sessions/{key}/runs"), &body)?;
    if a.json {
        return print_json(&v);
    }
    let id = v.pointer("/run/id").and_then(|i| i.as_str()).unwrap_or("?");
    let url = format!("{}?run={id}", c.session_url(&key));
    println!("started {provider} {stage} run {id}");
    println!("{url}");
    if !a.no_open {
        open_browser(&url);
    }
    Ok(())
}
