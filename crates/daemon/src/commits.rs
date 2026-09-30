use crate::git::{self, CommitError, CommitOutcome, CommitStep};
use crate::registry::LiveSession;
use plantool_core::{CommitPhase, CommitScope};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum RunCommitError {
    #[error("a commit is already running in this session")]
    Busy,
    #[error(transparent)]
    Commit(#[from] CommitError),
}

/// Commit everything in `cwd` while the browser watches: the session records the commit, streams
/// its output as `commit-progress` events, and lets the human cancel it. The work runs in its own
/// task so a dropped request cannot leave the record behind.
pub async fn run_commit(session: Arc<LiveSession>, scope: CommitScope, cwd: PathBuf, message: String, amend: bool) -> Result<CommitOutcome, RunCommitError> {
    let (id, cancel) = session.begin_commit(scope).ok_or(RunCommitError::Busy)?;
    let task = tokio::spawn(async move {
        let dirty = Arc::new(AtomicBool::new(false));
        let flusher = tokio::spawn({
            let (session, id, dirty) = (session.clone(), id.clone(), dirty.clone());
            async move {
                let mut tick = tokio::time::interval(Duration::from_millis(200));
                loop {
                    tick.tick().await;
                    if dirty.swap(false, Ordering::Relaxed) {
                        session.update_commit(&id, true, |_| {});
                    }
                }
            }
        });
        let outcome = git::commit_streaming(&cwd, &message, amend, cancel, |step| match step {
            CommitStep::Committing => session.update_commit(&id, true, |job| job.phase = CommitPhase::Hooks),
            CommitStep::Line(line) => {
                session.update_commit(&id, false, |job| job.lines.push(line));
                dirty.store(true, Ordering::Relaxed);
            }
        })
        .await;
        flusher.abort();
        let (phase, error) = match &outcome {
            Ok(CommitOutcome::Committed(_)) | Ok(CommitOutcome::HookRewrote { sha: Some(_), .. }) => (CommitPhase::Done, None),
            Ok(CommitOutcome::HookRewrote { files, .. }) => (CommitPhase::Failed, Some(format!("git hooks rewrote {}", files.join(", ")))),
            Err(CommitError::Cancelled) => (CommitPhase::Cancelled, None),
            Err(e) => (CommitPhase::Failed, Some(e.to_string())),
        };
        session.finish_commit(&id, phase, error);
        outcome
    });
    let outcome = task.await.map_err(|e| CommitError::Git(git::GitError::Io(std::io::Error::other(e))))?;
    Ok(outcome?)
}

impl RunCommitError {
    /// The message for a failed commit request, with `context` in front of git errors.
    pub fn message(&self, context: &str) -> String {
        match self {
            RunCommitError::Commit(CommitError::Git(e)) => format!("{context}: {e}"),
            other => other.to_string(),
        }
    }
}
