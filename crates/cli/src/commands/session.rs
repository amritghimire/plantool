use crate::client::{absolute, open_browser, print_json, urlencode, Client};
use anyhow::{bail, Context};
use clap::{Args as ClapArgs, Subcommand};
use serde_json::{json, Value};
use std::io::Read;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(ClapArgs)]
pub struct Target {
    /// Session ref: <slug> when unique, else <repo_slug>/<slug>.
    #[arg(long, short = 's')]
    pub session: String,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Stage, documents, open comments and runs.
    Get {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        json: bool,
    },
    /// Document helpers.
    Doc {
        #[command(subcommand)]
        cmd: DocCmd,
    },
    /// Move the stage forward (agent actor; approval is browser-only).
    Stage {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        set: String,
        #[arg(long)]
        json: bool,
    },
    /// Read and write inline comments.
    Comment {
        #[command(subcommand)]
        cmd: CommentCmd,
    },
    /// Block until the next event on the session (NDJSON), then exit.
    Watch {
        #[command(flatten)]
        target: Target,
        /// Return comments newer than this cursor immediately instead of blocking.
        #[arg(long)]
        since: Option<u64>,
        /// Give up after this many seconds (exit 124).
        #[arg(long)]
        timeout: Option<u64>,
        /// Keep printing events instead of exiting after the first.
        #[arg(long)]
        follow: bool,
    },
    /// Scroll the user's open tab to a line, comment or document.
    Goto {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        line: Option<u32>,
        #[arg(long = "match")]
        match_text: Option<String>,
        #[arg(long)]
        comment: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The change review state: tool, stat, difftool review URL.
    Changes {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum DocCmd {
    /// Print the file an agent should write for this document kind.
    Path {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        json: bool,
    },
    /// Force a capture of the document now.
    Touch {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        json: bool,
    },
    /// Print the document with line numbers.
    Get {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        sha: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CommentCmd {
    List {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        doc: Option<String>,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        unresolved: bool,
        #[arg(long)]
        resolved: bool,
        #[arg(long)]
        since: Option<u64>,
        #[arg(long)]
        outdated: bool,
        /// Inline the surrounding lines of each comment (radius, default 4).
        #[arg(long, num_args = 0..=1, default_missing_value = "4")]
        context: Option<u32>,
        #[arg(long)]
        json: bool,
        /// Print bare ids only.
        #[arg(long, short = 'q')]
        quiet: bool,
    },
    Add {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        line: Option<u32>,
        #[arg(long = "match")]
        match_text: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<PathBuf>,
        /// Reply in an existing thread.
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Add many comments from a JSON file or stdin: {"comments":[{doc,match|line,body,parent}]}.
    Apply {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        input: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    Edit {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        id: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Resolve {
        #[command(flatten)]
        target: Target,
        #[arg(long, value_delimiter = ',')]
        id: Vec<String>,
        /// Reopen instead of resolving.
        #[arg(long)]
        reopen: bool,
        #[arg(long)]
        json: bool,
    },
    Rm {
        #[command(flatten)]
        target: Target,
        #[arg(long, value_delimiter = ',')]
        id: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    Context {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        id: String,
        #[arg(long, default_value_t = 6)]
        radius: u32,
        #[arg(long)]
        json: bool,
    },
}

fn read_body(body: Option<String>, file: Option<PathBuf>) -> anyhow::Result<String> {
    match (body, file) {
        (Some(b), _) => Ok(b),
        (None, Some(f)) if f.to_string_lossy() == "-" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            Ok(s)
        }
        (None, Some(f)) => std::fs::read_to_string(absolute(&f)).with_context(|| format!("reading {}", f.display())),
        (None, None) => bail!("give --body or --body-file"),
    }
}

fn print_comment(c: &Value, context: bool) {
    let id = c["id"].as_str().unwrap_or("?");
    let kind = c["kind"].as_str().unwrap_or("?");
    let author = c["author"].as_str().unwrap_or("?");
    let doc = c["doc"].as_str().unwrap_or("?");
    let line = c["anchor"]["line"].as_u64().unwrap_or(0);
    let outdated = c["anchor"]["outdated"].as_bool().unwrap_or(false);
    let resolved = c["resolved"].as_bool().unwrap_or(false);
    let parent = c["parent"].as_str();
    let mut flags = Vec::new();
    if resolved {
        flags.push("resolved");
    }
    if outdated {
        flags.push("outdated");
    }
    let head = match parent {
        Some(p) => format!("{id}  reply to {p}  {kind}/{author}"),
        None => format!("{id}  {doc}.md:{line}  {kind}/{author}"),
    };
    println!("{head}{}{}", if flags.is_empty() { "" } else { "  [" }, if flags.is_empty() { String::new() } else { format!("{}]", flags.join(", ")) });
    if context {
        if let Some(ctx) = c["context"].as_array() {
            for l in ctx {
                let n = l["line"].as_u64().unwrap_or(0);
                let mark = if n == line { ">" } else { " " };
                println!("  {mark}{n:>4} | {}", l["text"].as_str().unwrap_or(""));
            }
        }
    }
    for l in c["body"].as_str().unwrap_or("").lines() {
        println!("    {l}");
    }
    println!();
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::connect()?;
    match a.cmd {
        Cmd::Get { target, json } => {
            let (_, v) = c.resolve_key(&target.session)?;
            if json {
                return print_json(&v);
            }
            println!("{}  [{}]", v["key"].as_str().unwrap_or(""), v["state"]["stage"].as_str().unwrap_or(""));
            println!("title:    {}", v["session"]["title"].as_str().unwrap_or(""));
            println!("checkout: {}", v["session"]["worktree"].as_str().or_else(|| v["session"]["repo"]["root"].as_str()).unwrap_or(""));
            println!("base:     {}", v["session"]["base"].as_str().unwrap_or(""));
            println!("docs:     {}", v["dir"].as_str().unwrap_or(""));
            for d in v["docs"].as_array().cloned().unwrap_or_default() {
                let exists = d["exists"].as_bool().unwrap_or(false);
                let prog: Vec<String> = d["progress"].as_array().cloned().unwrap_or_default().iter().map(|p| format!("{} {}/{}", p["name"].as_str().unwrap_or(""), p["done"], p["total"])).collect();
                println!("  {:<14} {} {}{}", d["kind"].as_str().unwrap_or(""), if exists { "written" } else { "missing" }, d["path"].as_str().unwrap_or(""), if prog.is_empty() { String::new() } else { format!("  ({})", prog.join("; ")) });
            }
            println!("open comments: {}   seq: {}", v["open_comments"], v["state"]["seq"]);
            println!("{}", c.session_url(v["key"].as_str().unwrap_or("")));
            Ok(())
        }
        Cmd::Doc { cmd } => match cmd {
            DocCmd::Path { target, kind, json } => {
                let (key, _) = c.resolve_key(&target.session)?;
                let v: Value = c.get(&format!("/api/sessions/{key}/docs/{kind}/path"))?;
                if json {
                    return print_json(&v);
                }
                println!("{}", v["path"].as_str().unwrap_or(""));
                Ok(())
            }
            DocCmd::Touch { target, kind, json } => {
                let (key, _) = c.resolve_key(&target.session)?;
                let v: Value = c.post(&format!("/api/sessions/{key}/docs/{kind}/touch"), &json!({}))?;
                if json {
                    return print_json(&v);
                }
                println!("{} {}", if v["changed"].as_bool().unwrap_or(false) { "captured" } else { "unchanged" }, v["sha"].as_str().unwrap_or("(missing)"));
                Ok(())
            }
            DocCmd::Get { target, kind, json, sha } => {
                let (key, _) = c.resolve_key(&target.session)?;
                let q = sha.map(|s| format!("?sha={s}")).unwrap_or_default();
                let v: Value = c.get(&format!("/api/sessions/{key}/docs/{kind}{q}"))?;
                if json {
                    return print_json(&v);
                }
                for (i, l) in v["content"].as_str().unwrap_or("").lines().enumerate() {
                    println!("{:>4} | {l}", i + 1);
                }
                Ok(())
            }
        },
        Cmd::Stage { target, set, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let v: Value = c.post(&format!("/api/sessions/{key}/stage"), &json!({ "to": set }))?;
            if json {
                return print_json(&v);
            }
            println!("stage: {}", v["stage"].as_str().unwrap_or(""));
            Ok(())
        }
        Cmd::Comment { cmd } => comment(&c, cmd),
        Cmd::Watch { target, since, timeout, follow } => watch(&c, &target.session, since, timeout, follow),
        Cmd::Goto { target, kind, line, match_text, comment, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let mut body = json!({ "doc": kind, "line": line, "comment": comment });
            if let Some(m) = match_text {
                let kind = body["doc"].as_str().map(|s| s.to_string()).unwrap_or_else(|| "plan".to_string());
                let doc: Value = c.get(&format!("/api/sessions/{key}/docs/{kind}"))?;
                let content = doc["content"].as_str().unwrap_or("");
                let anchor = plantool_core::anchor::resolve_match(content, &m)?;
                body["doc"] = json!(kind);
                body["line"] = json!(anchor.line);
            }
            let v: Value = c.post(&format!("/api/sessions/{key}/navigate"), &body)?;
            if json {
                return print_json(&v);
            }
            println!("navigated; {} viewer(s)", v["viewers"]);
            Ok(())
        }
        Cmd::Changes { target, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let v: Value = c.get(&format!("/api/sessions/{key}/changes"))?;
            super::changes::print_changes(&v, json)
        }
    }
}

fn comment(c: &Client, cmd: CommentCmd) -> anyhow::Result<()> {
    match cmd {
        CommentCmd::List { target, kind, doc, author, unresolved, resolved, since, outdated, context, json, quiet } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let mut q = Vec::new();
            if let Some(k) = kind {
                q.push(format!("kind={}", urlencode(&k)));
            }
            if let Some(d) = doc {
                q.push(format!("doc={}", urlencode(&d)));
            }
            if let Some(a) = author {
                q.push(format!("author={}", urlencode(&a)));
            }
            if unresolved {
                q.push("status=unresolved".into());
            } else if resolved {
                q.push("status=resolved".into());
            }
            if let Some(s) = since {
                q.push(format!("since={s}"));
            }
            if outdated {
                q.push("outdated=true".into());
            }
            if let Some(r) = context {
                q.push(format!("context={r}"));
            }
            let path = format!("/api/sessions/{key}/comments{}{}", if q.is_empty() { "" } else { "?" }, q.join("&"));
            let v: Value = c.get(&path)?;
            if json {
                return print_json(&v);
            }
            let comments = v["comments"].as_array().cloned().unwrap_or_default();
            if quiet {
                for cm in &comments {
                    println!("{}", cm["id"].as_str().unwrap_or(""));
                }
                return Ok(());
            }
            if comments.is_empty() {
                println!("no comments (seq {})", v["seq"]);
                return Ok(());
            }
            for cm in &comments {
                print_comment(cm, context.is_some());
            }
            println!("seq: {}", v["seq"]);
            Ok(())
        }
        CommentCmd::Add { target, kind, line, match_text, body, body_file, parent, dry_run, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let body_text = read_body(body, body_file)?;
            if parent.is_none() && line.is_none() && match_text.is_none() {
                bail!("give --match \"<line text>\", --line <n>, or --parent <id>");
            }
            let doc = kind.unwrap_or_else(|| "plan".to_string());
            let item = json!({ "doc": doc, "line": line, "match": match_text, "body": body_text, "parent": parent });
            let payload = json!({ "comments": [item], "dry_run": dry_run });
            let v: Value = c.post(&format!("/api/sessions/{key}/comments/batch"), &payload)?;
            if json {
                return print_json(&v);
            }
            if dry_run {
                let a = &v["anchors"][0];
                println!("would anchor at {doc}.md:{}  {:?}", a["line"], a["text"].as_str().unwrap_or(""));
                return Ok(());
            }
            let cm = &v["comments"][0];
            println!("{}  {}.md:{}", cm["id"].as_str().unwrap_or(""), cm["doc"].as_str().unwrap_or(""), cm["anchor"]["line"]);
            Ok(())
        }
        CommentCmd::Apply { target, input, dry_run, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let text = match input {
                Some(p) if p.to_string_lossy() != "-" => std::fs::read_to_string(absolute(&p))?,
                _ => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
            };
            let mut payload: Value = serde_json::from_str(&text).context("input must be JSON {\"comments\":[...]}")?;
            if payload.is_array() {
                payload = json!({ "comments": payload });
            }
            payload["dry_run"] = json!(dry_run);
            let v: Value = c.post(&format!("/api/sessions/{key}/comments/batch"), &payload)?;
            if json {
                return print_json(&v);
            }
            if dry_run {
                for a in v["anchors"].as_array().cloned().unwrap_or_default() {
                    println!("line {}  {:?}", a["line"], a["text"].as_str().unwrap_or(""));
                }
                return Ok(());
            }
            for cm in v["comments"].as_array().cloned().unwrap_or_default() {
                println!("{}  {}.md:{}", cm["id"].as_str().unwrap_or(""), cm["doc"].as_str().unwrap_or(""), cm["anchor"]["line"]);
            }
            Ok(())
        }
        CommentCmd::Edit { target, id, body, body_file, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let text = read_body(body, body_file)?;
            let v: Value = c.post(&format!("/api/sessions/{key}/comments/edit"), &json!({ "edits": [{ "id": id, "body": text }] }))?;
            if json {
                return print_json(&v);
            }
            println!("edited {id}");
            Ok(())
        }
        CommentCmd::Resolve { target, id, reopen, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let v: Value = c.post(&format!("/api/sessions/{key}/comments/resolve"), &json!({ "ids": id, "resolved": !reopen }))?;
            if json {
                return print_json(&v);
            }
            println!("{} {} comment(s)", if reopen { "reopened" } else { "resolved" }, v["comments"].as_array().map(|a| a.len()).unwrap_or(0));
            Ok(())
        }
        CommentCmd::Rm { target, id, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let v: Value = c.delete(&format!("/api/sessions/{key}/comments"), &json!({ "ids": id }))?;
            if json {
                return print_json(&v);
            }
            println!("removed {}", v["removed"]);
            Ok(())
        }
        CommentCmd::Context { target, id, radius, json } => {
            let (key, _) = c.resolve_key(&target.session)?;
            let v: Value = c.get(&format!("/api/sessions/{key}/comments/{id}/context?radius={radius}"))?;
            if json {
                return print_json(&v);
            }
            let mut cm = v["comment"].clone();
            cm["context"] = v["context"].clone();
            print_comment(&cm, true);
            Ok(())
        }
    }
}

fn watch(c: &Client, reference: &str, since: Option<u64>, timeout: Option<u64>, follow: bool) -> anyhow::Result<()> {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    let (key, _) = c.resolve_key(reference)?;
    let url = format!("ws://127.0.0.1:{}/api/sessions/{key}/live{}", c.port, since.map(|s| format!("?since={s}")).unwrap_or_default());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let code = rt.block_on(async move {
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.context("connecting to the live socket")?;
        let (_, mut read) = ws.split();
        let deadline = timeout.map(|t| tokio::time::Instant::now() + std::time::Duration::from_secs(t));
        loop {
            let next = async { read.next().await };
            let msg = match deadline {
                Some(d) => match tokio::time::timeout_at(d, next).await {
                    Ok(m) => m,
                    Err(_) => return Ok::<i32, anyhow::Error>(124),
                },
                None => next.await,
            };
            let Some(msg) = msg else { return Ok(0) };
            let text = match msg? {
                Message::Text(t) => t.to_string(),
                Message::Close(_) => return Ok(0),
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let ty = v["type"].as_str().unwrap_or("");
            if ty == "hello" || ty == "resync" {
                continue;
            }
            println!("{}", serde_json::to_string(&v)?);
            if !follow {
                return Ok(0);
            }
        }
    })?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

#[allow(dead_code)]
pub fn open_session(c: &Client, key: &str) {
    open_browser(&c.session_url(key));
}
