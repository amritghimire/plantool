use crate::git::{self, FileStat};
use plantool_core::{ChangeReview, Session};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "tool", rename_all = "kebab-case")]
pub enum ChangeTool {
    Difftool { path: PathBuf },
    GitDifftool,
}

impl ChangeTool {
    pub fn name(&self) -> &'static str {
        match self {
            ChangeTool::Difftool { .. } => "difftool",
            ChangeTool::GitDifftool => "git-difftool",
        }
    }
}

pub fn detect_change_tool() -> ChangeTool {
    if let Some(p) = std::env::var_os("PLANTOOL_DIFFTOOL_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return ChangeTool::Difftool { path: p };
        }
    }
    match which::which("difftool") {
        Ok(p) => ChangeTool::Difftool { path: p },
        Err(_) => ChangeTool::GitDifftool,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Changes {
    pub tool: &'static str,
    pub base: String,
    pub stat: Vec<FileStat>,
    pub review: Option<ChangeReview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn stat(session: &Session, base: &str, review: Option<ChangeReview>) -> Changes {
    let tool = detect_change_tool();
    let (stat, message) = match git::diff_numstat(session.cwd(), base) {
        Ok(s) => (s, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    Changes { tool: tool.name(), base: base.to_string(), stat, review, message }
}

pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = { let mut c = Command::new("open"); c.arg(url); c };
    #[cfg(target_os = "windows")]
    let mut cmd = { let mut c = Command::new("cmd"); c.args(["/C", "start", "", url]); c };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = { let mut c = Command::new("xdg-open"); c.arg(url); c };
    let _ = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Open an existing difftool review from the daemon: refresh its diff, then let difftool open the
/// browser itself so the page never navigates cross-origin. Returns false when the review is gone.
fn reopen_difftool(path: &PathBuf, review: &str) -> bool {
    let refreshed = Command::new(path).arg("refresh").arg(review).stdin(Stdio::null()).output().map(|o| o.status.success()).unwrap_or(false);
    if !refreshed {
        return false;
    }
    Command::new(path).arg("open").arg(review).stdin(Stdio::null()).output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn open_review(session: &Session, base: &str, plan_path: Option<PathBuf>, existing: Option<&ChangeReview>) -> anyhow::Result<(ChangeReview, Option<String>)> {
    let cwd = session.cwd().clone();
    match detect_change_tool() {
        ChangeTool::Difftool { path } => {
            if let Some(ChangeReview::Difftool { url, review: Some(review), .. }) = existing {
                if reopen_difftool(&path, review) {
                    return Ok((ChangeReview::Difftool { url: url.clone(), review: Some(review.clone()), opened_at: plantool_core::now() }, Some("refreshed the existing difftool review and opened it".into())));
                }
            }
            let mut cmd = Command::new(&path);
            cmd.arg("diff").arg("-C").arg(&cwd);
            if base == "HEAD" { cmd.arg("--uncommitted"); } else { cmd.arg(base); }
            cmd.arg("--no-open");
            if let Some(p) = plan_path.filter(|p| p.is_file()) {
                cmd.arg("--design").arg(p);
            }
            let out = cmd.stdin(Stdio::null()).output()?;
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if !out.status.success() {
                anyhow::bail!("difftool failed: {}", if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() });
            }
            let url = stdout
                .split_whitespace()
                .chain(stderr.split_whitespace())
                .find(|w| w.starts_with("http://") || w.starts_with("https://"))
                .map(|w| w.trim_end_matches(['.', ',', ')']).to_string());
            match url {
                Some(url) => {
                    let review = url.split("/r/").nth(1).map(|s| s.trim_end_matches('/').to_string());
                    let opened = match &review {
                        Some(r) => Command::new(&path).arg("open").arg(r).stdin(Stdio::null()).output().map(|o| o.status.success()).unwrap_or(false),
                        None => false,
                    };
                    if !opened {
                        open_url(&url);
                    }
                    Ok((ChangeReview::Difftool { url, review, opened_at: plantool_core::now() }, Some("difftool opened the review in your browser".into())))
                }
                None => anyhow::bail!("difftool printed no review URL: {}", stdout.trim()),
            }
        }
        ChangeTool::GitDifftool => {
            let configured = git::git(&cwd, &["config", "--get", "diff.tool"]).ok().filter(|s| !s.is_empty());
            if configured.is_none() {
                anyhow::bail!("difftool is not installed and git has no diff.tool configured; set one with `git config --global diff.tool <name>` or install difftool");
            }
            let base = base.to_string();
            let attempt = |dir_diff: bool| -> std::io::Result<std::process::Child> {
                let mut cmd = Command::new("git");
                cmd.arg("-C").arg(&cwd).arg("difftool").arg("--no-prompt");
                if dir_diff {
                    cmd.arg("--dir-diff");
                }
                cmd.arg(&base).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()
            };
            let mut child = attempt(true)?;
            std::thread::sleep(std::time::Duration::from_millis(400));
            if let Ok(Some(status)) = child.try_wait() {
                if !status.success() {
                    let _ = attempt(false)?;
                }
            }
            Ok((ChangeReview::GitDifftool { opened_at: plantool_core::now() }, Some(format!("launched git difftool ({})", configured.unwrap_or_default()))))
        }
    }
}

pub fn unresolved_human_comments(review: &ChangeReview) -> anyhow::Result<usize> {
    let Some(reference) = (match review { ChangeReview::Difftool { review, .. } => review.as_deref(), _ => None }) else { return Ok(0) };
    let ChangeTool::Difftool { path } = detect_change_tool() else { anyhow::bail!("difftool is no longer available") };
    let out = Command::new(path).args(["review", "comment", "list", "--review", reference, "--kind", "human", "--unresolved", "--source", "local", "--json"]).output()?;
    if !out.status.success() { anyhow::bail!("could not read difftool comments: {}", String::from_utf8_lossy(&out.stderr).trim()) }
    let value: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(value.get("comments").and_then(|v| v.as_array()).map_or(0, Vec::len))
}
