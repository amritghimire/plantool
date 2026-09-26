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

pub fn stat(session: &Session, review: Option<ChangeReview>) -> Changes {
    let tool = detect_change_tool();
    let (stat, message) = match git::diff_numstat(session.cwd(), &session.base) {
        Ok(s) => (s, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    Changes { tool: tool.name(), base: session.base.clone(), stat, review, message }
}

pub fn open_review(session: &Session, plan_path: Option<PathBuf>) -> anyhow::Result<(ChangeReview, Option<String>)> {
    let cwd = session.cwd().clone();
    match detect_change_tool() {
        ChangeTool::Difftool { path } => {
            let mut cmd = Command::new(&path);
            cmd.arg("diff").arg("-C").arg(&cwd).arg(&session.base).arg("--no-open");
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
                    Ok((ChangeReview::Difftool { url, review, opened_at: plantool_core::now() }, None))
                }
                None => anyhow::bail!("difftool printed no review URL: {}", stdout.trim()),
            }
        }
        ChangeTool::GitDifftool => {
            let configured = git::git(&cwd, &["config", "--get", "diff.tool"]).ok().filter(|s| !s.is_empty());
            if configured.is_none() {
                anyhow::bail!("difftool is not installed and git has no diff.tool configured; set one with `git config --global diff.tool <name>` or install difftool");
            }
            let base = session.base.clone();
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
