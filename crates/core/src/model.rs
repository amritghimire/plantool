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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequest>,
    pub base: String,
    #[serde(default)]
    pub pause_rule: PauseRule,
    #[serde(default)]
    pub difftool: Option<String>,
    #[serde(default)]
    pub mirror: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<String>,
    /// The directory `plantool new` was run from (or the browser's chosen repo path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_in: Option<PathBuf>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub state: String,
    pub draft: bool,
    pub updated_at: String,
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CommentType {
    #[default]
    Note,
    Blocker,
    Question,
    Suggestion,
    ChangeApproach,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorScope {
    #[default]
    Line,
    Section,
    Document,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", content = "rule", rename_all = "kebab-case")]
pub enum PauseRule {
    EveryMilestone,
    #[default]
    NoPauses,
    PlainLanguage(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub sha: String,
    pub at: String,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub open_blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalKind {
    Base,
    Branch,
    Workspace,
    Difftool,
    PlanRevision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub kind: ProposalKind,
    pub old: String,
    pub new: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub seq: u64,
    pub at: String,
    #[serde(rename = "type")]
    pub activity_type: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodeAnchor {
    pub path: String,
    pub side: String,
    pub line: u32,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Anchor {
    #[serde(default)]
    pub code: Option<CodeAnchor>,
    #[serde(default)]
    pub scope: AnchorScope,
    pub line: u32,
    pub text: String,
    #[serde(default)]
    pub outdated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    #[serde(default, rename = "type")]
    pub comment_type: CommentType,
    #[serde(default)]
    pub proposes_resolve: bool,
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
    Difftool {
        url: String,
        review: Option<String>,
        opened_at: String,
    },
    GitDifftool {
        opened_at: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub key: String,
    pub status: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub completed_tasks: Vec<String>,
    #[serde(default)]
    pub pause_rule: PauseRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub selected_milestone: Option<String>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub pause_reason: Option<String>,
    #[serde(default)]
    pub plan_revision_pending: bool,
    #[serde(default)]
    pub plan_change: Option<crate::markdown::PlanChange>,
    #[serde(default)]
    pub approved: Option<Approval>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
    #[serde(default)]
    pub viewed: BTreeMap<DocKind, String>,
    #[serde(default)]
    pub activity: Vec<Activity>,
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
            selected_milestone: None,
            milestones: Vec::new(),
            pause_reason: None,
            plan_revision_pending: false,
            plan_change: None,
            approved: None,
            proposals: Vec::new(),
            viewed: BTreeMap::new(),
            activity: Vec::new(),
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
    Copilot,
    Ollama,
    Opencode,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Copilot => "copilot",
            Provider::Ollama => "ollama",
            Provider::Opencode => "opencode",
        }
    }

    pub fn parse(s: &str) -> Option<Provider> {
        match s {
            "claude" => Some(Provider::Claude),
            "codex" => Some(Provider::Codex),
            "copilot" => Some(Provider::Copilot),
            "ollama" => Some(Provider::Ollama),
            "opencode" => Some(Provider::Opencode),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    #[default]
    Ask,
    AcceptEdits,
    Auto,
    AllowAll,
}

impl PermissionMode {
    pub const ALL: [PermissionMode; 4] = [
        PermissionMode::Ask,
        PermissionMode::AcceptEdits,
        PermissionMode::Auto,
        PermissionMode::AllowAll,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Ask => "ask",
            PermissionMode::AcceptEdits => "accept-edits",
            PermissionMode::Auto => "auto",
            PermissionMode::AllowAll => "allow-all",
        }
    }

    pub fn parse(s: &str) -> Option<PermissionMode> {
        PermissionMode::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImplementationMode {
    #[default]
    AllAtOnce,
    StepByStep,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    #[serde(default)]
    pub milestone_key: Option<String>,
    #[serde(default)]
    pub plan_sha: Option<String>,
    #[serde(default)]
    pub idle_stopped: bool,
    pub id: String,
    pub provider: Provider,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    pub stage: Stage,
    #[serde(default)]
    pub implementation_mode: ImplementationMode,
    #[serde(default)]
    pub milestone_pending: bool,
    #[serde(default)]
    pub milestone_review: Option<ChangeReview>,
    /// Commit the current milestone's diff is measured from (step-by-step only).
    #[serde(default)]
    pub milestone_base: Option<String>,
    #[serde(default)]
    pub milestones_approved: u32,
    /// A milestone commit that landed but was held back because hooks rewrote files; the next approval amends it.
    #[serde(default)]
    pub milestone_commit: Option<String>,
    /// Commit the step-by-step implementation started from; the whole-implementation diff is measured from here.
    #[serde(default)]
    pub implementation_base: Option<String>,
    /// What the run was started to do: research, plan, implement, critique or resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub cwd: PathBuf,
    pub status: RunStatus,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub permission_mode: PermissionMode,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CommitScope {
    Milestone { run_id: String },
    Pr,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CommitPhase {
    Staging,
    Hooks,
    Done,
    Failed,
    Cancelled,
}

impl CommitPhase {
    pub fn is_final(self) -> bool {
        matches!(
            self,
            CommitPhase::Done | CommitPhase::Failed | CommitPhase::Cancelled
        )
    }
}

/// A commit started from the browser, while it runs. Never persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitJob {
    pub id: String,
    pub scope: CommitScope,
    pub phase: CommitPhase,
    pub started_at: String,
    /// The most recent output lines from git and its hooks.
    pub lines: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn loads_v0_0_14_session_and_state() {
        let session: Session =
            serde_json::from_str(include_str!("../tests/fixtures/v0.0.14/meta.json")).unwrap();
        let state: State =
            serde_json::from_str(include_str!("../tests/fixtures/v0.0.14/state.json")).unwrap();
        let run: Run =
            serde_json::from_str(include_str!("../tests/fixtures/v0.0.14/run.json")).unwrap();
        assert_eq!(run.provider_session_id.as_deref(), Some("resume-me"));
        assert!(run.milestone_key.is_none());
        assert!(run.plan_sha.is_none());
        assert!(!run.idle_stopped);
        assert_eq!(session.pause_rule, PauseRule::NoPauses);
        assert!(session.difftool.is_none());
        assert_eq!(state.stage, Stage::Implementing);
        assert_eq!(state.comments.len(), 2);
        assert_eq!(state.comments[0].anchor.scope, AnchorScope::Line);
        assert_eq!(state.comments[0].comment_type, CommentType::Note);
        assert!(!state.comments[1].proposes_resolve);
        assert!(state.approved.is_none());
        assert!(state.proposals.is_empty());
        assert!(state.viewed.is_empty());
        assert!(state.activity.is_empty());
        let reloaded: State =
            serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert_eq!(reloaded.comments[1].parent.as_deref(), Some("human-note"));
        assert_eq!(reloaded.docs[&DocKind::Plan].sha, "old-plan-sha");
    }
}
