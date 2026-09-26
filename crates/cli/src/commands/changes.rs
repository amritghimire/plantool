use crate::client::{open_browser, print_json, Client};
use clap::Args as ClapArgs;

#[derive(ClapArgs)]
pub struct Args {
    pub reference: String,
    /// Only print the stat and the current review, do not launch a tool.
    #[arg(long)]
    pub stat: bool,
    #[arg(long)]
    pub no_open: bool,
    #[arg(long)]
    pub json: bool,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::connect()?;
    let (key, _) = c.resolve_key(&a.reference)?;
    if a.stat {
        let v: serde_json::Value = c.get(&format!("/api/sessions/{key}/changes"))?;
        return print_changes(&v, a.json);
    }
    let v: serde_json::Value = c.post(&format!("/api/sessions/{key}/changes/open"), &serde_json::json!({}))?;
    if a.json {
        return print_json(&v);
    }
    if let Some(url) = v.pointer("/review/url").and_then(|u| u.as_str()) {
        println!("difftool review: {url}");
        if !a.no_open {
            open_browser(url);
        }
    } else {
        println!("{}", v.get("message").and_then(|m| m.as_str()).unwrap_or("launched git difftool"));
    }
    Ok(())
}

pub fn print_changes(v: &serde_json::Value, json: bool) -> anyhow::Result<()> {
    if json {
        return print_json(v);
    }
    println!("tool: {}", v.get("tool").and_then(|t| t.as_str()).unwrap_or("?"));
    if let Some(url) = v.pointer("/review/url").and_then(|u| u.as_str()) {
        println!("review: {url}");
    }
    if let Some(files) = v.get("stat").and_then(|s| s.as_array()) {
        for f in files {
            println!("{:>5} {:>5}  {}", format!("+{}", f["added"]), format!("-{}", f["deleted"]), f["path"].as_str().unwrap_or(""));
        }
        if files.is_empty() {
            println!("no changes against the base");
        }
    }
    Ok(())
}
