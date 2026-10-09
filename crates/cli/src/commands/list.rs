use crate::client::{absolute, print_json, urlencode, Client};
use clap::Args as ClapArgs;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Only sessions for this repository (a path, or a repo slug).
    #[arg(long)]
    pub repo: Option<String>,
    /// Only sessions in this stage.
    #[arg(long)]
    pub stage: Option<String>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::connect()?;
    let mut q = Vec::new();
    if let Some(r) = &a.repo {
        let p = PathBuf::from(r);
        let value = if p.exists() {
            absolute(&p).to_string_lossy().to_string()
        } else {
            r.clone()
        };
        q.push(format!("repo={}", urlencode(&value)));
    }
    if let Some(s) = &a.stage {
        q.push(format!("stage={}", urlencode(s)));
    }
    let path = if q.is_empty() {
        "/api/sessions".to_string()
    } else {
        format!("/api/sessions?{}", q.join("&"))
    };
    let v: Vec<serde_json::Value> = c.get(&path)?;
    if a.json {
        return print_json(&v);
    }
    if v.is_empty() {
        println!("no sessions");
        return Ok(());
    }
    for s in &v {
        let key = s.get("key").and_then(|k| k.as_str()).unwrap_or("?");
        let stage = s
            .pointer("/state/stage")
            .and_then(|k| k.as_str())
            .unwrap_or("?");
        let title = s
            .pointer("/session/title")
            .and_then(|k| k.as_str())
            .unwrap_or("");
        let open = s.get("open_comments").and_then(|k| k.as_u64()).unwrap_or(0);
        println!("{key:40} {stage:22} {open:>3} open  {title}");
    }
    Ok(())
}
