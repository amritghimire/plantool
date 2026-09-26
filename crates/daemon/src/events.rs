use plantool_core::{ChangeReview, Comment, DocKind, Run, Stage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LiveEvent {
    CommentAdded { comment: Comment },
    CommentsAdded { comments: Vec<Comment> },
    CommentUpdated { comment: Comment },
    CommentRemoved { ids: Vec<String> },
    ThreadResolution { ids: Vec<String>, resolved: bool },
    DocRefreshed { kind: DocKind, sha: String, lines: u32, reanchored: usize, outdated: usize },
    StageChanged { from: Stage, to: Stage, actor: plantool_core::Actor },
    RunStarted { run: Run },
    RunUpdated { run: Run },
    RunEvent { run_id: String, seq: u64, event: serde_json::Value },
    RunEnded { run: Run },
    ChangesOpened { review: ChangeReview },
    Navigate { target: NavTarget, viewers: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavTarget {
    pub doc: Option<DocKind>,
    pub line: Option<u32>,
    pub comment: Option<String>,
    #[serde(default)]
    pub tab: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMessage {
    pub seq: u64,
    pub at: String,
    #[serde(flatten)]
    pub event: LiveEvent,
}
