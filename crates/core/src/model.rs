use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    New,
    Researching,
    ResearchReview,
    Planning,
    PlanReview,
    Approved,
    Implementing,
    ImplementationReview,
    Done,
}

impl Stage {
    pub const ALL: [Stage; 9] = [
        Stage::New,
        Stage::Researching,
        Stage::ResearchReview,
        Stage::Planning,
        Stage::PlanReview,
        Stage::Approved,
        Stage::Implementing,
        Stage::ImplementationReview,
        Stage::Done,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::New => "new",
            Stage::Researching => "researching",
            Stage::ResearchReview => "research-review",
            Stage::Planning => "planning",
            Stage::PlanReview => "plan-review",
            Stage::Approved => "approved",
            Stage::Implementing => "implementing",
            Stage::ImplementationReview => "implementation-review",
            Stage::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Stage> {
        Stage::ALL.into_iter().find(|st| st.as_str() == s)
    }

    pub fn index(self) -> usize {
        Stage::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocKind {
    Research,
    Plan,
    Investigation,
    QuickFix,
    Design,
}

impl DocKind {
    pub const ALL: [DocKind; 5] = [
        DocKind::Research,
        DocKind::Plan,
        DocKind::Investigation,
        DocKind::QuickFix,
        DocKind::Design,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Research => "research",
            DocKind::Plan => "plan",
            DocKind::Investigation => "investigation",
            DocKind::QuickFix => "quick-fix",
            DocKind::Design => "design",
        }
    }

    pub fn parse(s: &str) -> Option<DocKind> {
        DocKind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn file_name(self) -> String {
        format!("{}.md", self.as_str())
    }

    pub fn from_file_name(name: &str) -> Option<DocKind> {
        let stem = name.strip_suffix(".md")?;
        DocKind::parse(stem)
    }

    pub fn stage_for_write(self) -> Option<Stage> {
        match self {
            DocKind::Research | DocKind::Investigation => Some(Stage::ResearchReview),
            DocKind::Plan | DocKind::QuickFix => Some(Stage::PlanReview),
            DocKind::Design => None,
        }
    }
}

impl std::fmt::Display for DocKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkout {
    pub root: PathBuf,
    pub common_dir: PathBuf,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub repo_slug: String,
    pub slug: String,
    pub title: String,
    pub repo: Checkout,
    #[serde(default)]
    pub worktree: Option<PathBuf>,
    pub base: String,
    #[serde(default)]
    pub mirror: bool,
    pub created_at: String,
}

impl Session {
    pub fn key(&self) -> String {
        format!("{}/{}", self.repo_slug, self.slug)
    }

    pub fn cwd(&self) -> &PathBuf {
        self.worktree.as_ref().unwrap_or(&self.repo.root)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommentKind {
    Human,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Anchor {
    pub line: u32,
    pub text: String,
    #[serde(default)]
    pub outdated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub doc: DocKind,
    pub anchor: Anchor,
    pub body: String,
    pub kind: CommentKind,
    pub author: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub resolved: bool,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocState {
    pub sha: String,
    pub captured_at: String,
    pub lines: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "kebab-case")]
pub enum ChangeReview {
    Difftool { url: String, review: Option<String>, opened_at: String },
    GitDifftool { opened_at: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub stage: Stage,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub docs: BTreeMap<DocKind, DocState>,
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub review: Option<ChangeReview>,
    #[serde(default)]
    pub updated_at: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            stage: Stage::New,
            comments: Vec::new(),
            docs: BTreeMap::new(),
            seq: 0,
            review: None,
            updated_at: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocRevision {
    pub kind: DocKind,
    pub sha: String,
    pub content: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Claude,
    Codex,
    Opencode,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Opencode => "opencode",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "claude" => Some(Provider::Claude),
            "codex" => Some(Provider::Codex),
            "opencode" => Some(Provider::Opencode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Starting,
    Running,
    Waiting,
    Idle,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub provider: Provider,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    pub stage: Stage,
    pub cwd: PathBuf,
    pub status: RunStatus,
    #[serde(default)]
    pub model: Option<String>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub seq: u64,
}
