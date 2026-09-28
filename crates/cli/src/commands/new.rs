use crate::client::{absolute, cwd, open_browser, print_json, Client};
use anyhow::Context;
use clap::Args as ClapArgs;
use serde_json::json;
use std::io::Read;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Session slug, e.g. fix-login-timeout.
    pub slug: String,
    /// Repository path (default: the git checkout containing the current directory).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Human title (default: derived from the slug).
    #[arg(long)]
    pub title: Option<String>,
    /// What to research or build: the assignment, context, links. Rendered into every stage prompt.
    #[arg(long, short = 'b')]
    pub brief: Option<String>,
    /// Read the brief from a file, or from stdin with `-`.
    #[arg(long, conflicts_with = "brief")]
    pub brief_file: Option<PathBuf>,
    /// Create a git worktree named after the slug for the work.
    #[arg(long)]
    pub worktree: bool,
    /// Where to create the worktree (default: git config plantool.worktreeDir, else <repo>/.worktree/<slug>).
    #[arg(long)]
    pub worktree_dir: Option<PathBuf>,
    /// Base branch for the worktree and the change review (default: origin/HEAD or main).
    #[arg(long)]
    pub base: Option<String>,
    /// Also copy every document revision into <repo>/REVIEWS/.
    #[arg(long)]
    pub mirror: bool,
    /// Override the derived repo slug.
    #[arg(long)]
    pub repo_slug: Option<String>,
    /// Print the URL instead of opening a browser.
    #[arg(long)]
    pub no_open: bool,
    #[arg(long)]
    pub json: bool,
}

pub fn read_brief(brief: Option<String>, file: Option<PathBuf>) -> anyhow::Result<Option<String>> {
    match (brief, file) {
        (Some(b), _) => Ok(Some(b)),
        (None, Some(f)) if f.to_string_lossy() == "-" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(Some(s))
        }
        (None, Some(f)) => Ok(Some(std::fs::read_to_string(absolute(&f)).with_context(|| format!("reading {}", f.display()))?)),
        (None, None) => Ok(None),
    }
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::connect()?;
    let brief = read_brief(a.brief, a.brief_file)?;
    let body = json!({
        "slug": a.slug,
        "cwd": cwd(),
        "repo": a.repo.as_deref().map(absolute),
        "title": a.title,
        "brief": brief,
        "worktree": a.worktree,
        "worktree_dir": a.worktree_dir.as_deref().map(absolute),
        "base": a.base,
        "mirror": a.mirror,
        "repo_slug": a.repo_slug,
    });
    let v: serde_json::Value = c.post("/api/sessions", &body)?;
    let url = v.get("url").and_then(|u| u.as_str()).unwrap_or_default().to_string();
    if a.json {
        return print_json(&v);
    }
    let created = v.get("created").and_then(|b| b.as_bool()).unwrap_or(false);
    let key = v.pointer("/session/key").and_then(|k| k.as_str()).unwrap_or_default();
    let dir = v.pointer("/session/dir").and_then(|k| k.as_str()).unwrap_or_default();
    println!("{} {key}", if created { "created" } else { "reopened" });
    println!("docs: {dir}");
    println!("{url}");
    if !created && brief.is_some() {
        let _: serde_json::Value = c.post(&format!("/api/sessions/{key}/brief"), &json!({ "brief": brief }))?;
        println!("brief updated");
    }
    if let Ok(p) = c.get::<serde_json::Value>(&format!("/api/sessions/{key}/prompt/next")) {
        if let (Some(stage), Some(text)) = (p["stage"].as_str(), p["prompt"].as_str()) {
            println!();
            println!("To start the {stage} stage, paste this into your agent (or run `plantool {stage} {}`):", v.pointer("/session/slug").and_then(|s| s.as_str()).unwrap_or(key));
            println!();
            for l in text.lines() {
                println!("    {l}");
            }
        }
    }
    if !a.no_open {
        open_browser(&url);
    }
    Ok(())
}
