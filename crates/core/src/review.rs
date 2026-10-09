use crate::{Comment, CommentKind, CommentType};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreadState {
    Open,
    AgentReplied,
    ProposedResolved,
    Resolved,
    OutdatedUnaddressed,
}

pub fn thread_state(root: &Comment, comments: &[Comment]) -> ThreadState {
    if root.resolved {
        return ThreadState::Resolved;
    }
    let reply = comments
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&root.id) && c.kind == CommentKind::Agent)
        .max_by_key(|c| c.seq);
    match reply {
        Some(c) if c.proposes_resolve => ThreadState::ProposedResolved,
        Some(_) => ThreadState::AgentReplied,
        None if root.anchor.outdated => ThreadState::OutdatedUnaddressed,
        None => ThreadState::Open,
    }
}

pub fn owner_blockers(comments: &[Comment]) -> Vec<String> {
    comments
        .iter()
        .filter(|c| {
            c.parent.is_none()
                && !c.resolved
                && c.kind == CommentKind::Human
                && c.comment_type == CommentType::Blocker
        })
        .map(|c| c.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(id: &str) -> Comment {
        serde_json::from_value(serde_json::json!({"id":id,"doc":"plan","anchor":{"line":1,"text":"Plan"},"body":"Review","kind":"human","author":"you","created_at":"","seq":1})).unwrap()
    }

    #[test]
    fn thread_transitions_and_latest_reply() {
        let mut root = comment("root");
        assert_eq!(thread_state(&root, &[]), ThreadState::Open);
        root.anchor.outdated = true;
        assert_eq!(thread_state(&root, &[]), ThreadState::OutdatedUnaddressed);
        let mut reply = comment("reply");
        reply.parent = Some(root.id.clone());
        reply.kind = CommentKind::Agent;
        assert_eq!(
            thread_state(&root, &[reply.clone()]),
            ThreadState::AgentReplied
        );
        reply.proposes_resolve = true;
        assert_eq!(
            thread_state(&root, &[reply.clone()]),
            ThreadState::ProposedResolved
        );
        let mut newer = reply.clone();
        newer.seq += 1;
        newer.proposes_resolve = false;
        assert_eq!(
            thread_state(&root, &[reply.clone(), newer]),
            ThreadState::AgentReplied
        );
        root.resolved = true;
        assert_eq!(thread_state(&root, &[reply]), ThreadState::Resolved);
    }

    #[test]
    fn only_open_owner_root_blockers_gate() {
        let mut owner = comment("owner");
        owner.comment_type = CommentType::Blocker;
        let mut agent = owner.clone();
        agent.id = "agent".into();
        agent.kind = CommentKind::Agent;
        assert_eq!(owner_blockers(&[owner.clone(), agent]), vec!["owner"]);
        owner.resolved = true;
        assert!(owner_blockers(&[owner]).is_empty());
    }
}
