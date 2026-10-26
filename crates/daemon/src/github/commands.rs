//! One command on its own line. Natural language is never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ApprovePlan(String),
    ApproveResearch(String),
    RequestChanges(String),
    ApproveSprint,
    ApproveItem(String),
    Continue,
    Urgent,
}
pub fn parse(body: &str) -> Option<Command> {
    let mut lines = body.lines().filter(|line| !line.is_empty());
    let body = lines.next()?.trim_end_matches([' ', '\t']);
    if lines.next().is_some() || body.trim() != body {
        return None;
    }
    if body == "/plantool approve-sprint" {
        return Some(Command::ApproveSprint);
    }
    if body == "/plantool continue" {
        return Some(Command::Continue);
    }
    if body == "/plantool urgent" {
        return Some(Command::Urgent);
    }
    if let Some(hash) = body.strip_prefix("/plantool approve-research sha256:") {
        if (12..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(Command::ApproveResearch(hash.to_ascii_lowercase()));
        }
        return None;
    }
    if let Some(hash) = body.strip_prefix("/plantool approve-plan sha256:") {
        if (12..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(Command::ApprovePlan(hash.to_ascii_lowercase()));
        }
        return None;
    }
    if let Some(reason) = body.strip_prefix("/plantool request-changes ") {
        if !reason.trim().is_empty() {
            return Some(Command::RequestChanges(reason.into()));
        }
    }
    if let Some(item) = body.strip_prefix("/plantool approve ") {
        if !item.is_empty() && !item.chars().any(char::is_whitespace) {
            return Some(Command::ApproveItem(item.into()));
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar_is_exact() {
        assert_eq!(
            parse("/plantool approve-plan sha256:abcdef012345"),
            Some(Command::ApprovePlan("abcdef012345".into()))
        );
        assert_eq!(parse("\n/plantool urgent\r\n"), Some(Command::Urgent));
        assert_eq!(
            parse("/plantool approve-plan sha256:997d4e53e827 "),
            Some(Command::ApprovePlan("997d4e53e827".into()))
        );
        assert_eq!(
            parse("/plantool approve-plan sha256:997d4e53e827\t"),
            Some(Command::ApprovePlan("997d4e53e827".into()))
        );
        assert!(parse("/plantool approve-plan sha256:997d4e53e827 extra").is_none());
        for s in [
            "LGTM",
            "approved",
            "👍",
            "/Plantool urgent",
            " /plantool urgent",
            "/plantool urgent\n/plantool continue",
            "/plantool approve-plan sha256:abc",
            "/plantool approve-plan sha256:zzzzzzzzzzzz",
        ] {
            assert_eq!(parse(s), None, "{s}")
        }
    }
}

/// Verification context for all §13.4 commands. The sprint and milestone
/// executors are intentionally left to their later workflow phases.
pub struct Context<'a> {
    pub place: Place,
    pub opened_at: &'a str,
    pub snapshot_at: Option<&'a str>,
    pub pending_hash: Option<&'a str>,
    pub gate_commit: Option<&'a str>,
    pub current_commit: Option<&'a str>,
    pub snapshot_items: &'a [(&'a str, &'a str, &'a str)], // item, snapshot hash, current hash
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Issue,
    TrackingIssue,
    DraftPr,
}
pub fn verify(
    db: &rusqlite::Connection,
    c: &serde_json::Value,
    command: &Command,
    context: &Context<'_>,
) -> anyhow::Result<()> {
    use anyhow::{ensure, Context as _};
    ensure!(
        super::gates::valid_identity(db, c),
        "Allowlisted human user required"
    );
    let id = c["id"].as_u64().context("Missing comment ID")?;
    ensure!(db.query_row("SELECT NOT EXISTS(SELECT 1 FROM decisions WHERE source='github' AND source_event_id=?1)",[id.to_string()],|r|r.get::<_,bool>(0))?,"Comment already used");
    let created = c["created_at"].as_str().context("Missing comment time")?;
    ensure!(c["created_at"] == c["updated_at"], "Edited comment");
    let time = chrono::DateTime::parse_from_rfc3339(created)?;
    ensure!(
        time > chrono::DateTime::parse_from_rfc3339(context.opened_at)?,
        "Comment predates gate"
    );
    match command {
        Command::ApprovePlan(hash) | Command::ApproveResearch(hash) => {
            ensure!(
                context.place == Place::Issue,
                "Plan approval belongs on the item issue"
            );
            ensure!(
                super::gates::hash_matches(hash, context.pending_hash.context("No pending plan")?),
                "Plan hash is stale"
            );
        }
        Command::RequestChanges(_) => {
            ensure!(
                matches!(context.place, Place::Issue | Place::DraftPr),
                "Changes belong on an item issue or draft PR"
            );
            if context.place == Place::DraftPr {
                ensure!(
                    context.gate_commit.zip(context.current_commit).is_some_and(
                        |(expected, current)| super::gates::hash_matches(expected, current)
                    ),
                    "Milestone commit changed"
                );
            }
        }
        Command::Continue => {
            ensure!(
                context.place == Place::DraftPr,
                "Continue belongs on a draft PR"
            );
            ensure!(
                context.gate_commit.zip(context.current_commit).is_some_and(
                    |(expected, current)| super::gates::hash_matches(expected, current)
                ),
                "Milestone commit changed"
            );
        }
        Command::ApproveSprint | Command::ApproveItem(_) => {
            ensure!(
                context.place == Place::TrackingIssue,
                "Sprint approval belongs on tracking issue"
            );
            ensure!(
                time > chrono::DateTime::parse_from_rfc3339(
                    context.snapshot_at.context("No snapshot")?
                )?,
                "Comment predates snapshot"
            );
            let selected: Vec<_> = context
                .snapshot_items
                .iter()
                .filter(|(item, _, _)| {
                    if let Command::ApproveItem(target) = command {
                        *item == target
                    } else {
                        true
                    }
                })
                .collect();
            ensure!(!selected.is_empty(), "No items in current snapshot");
            for (_, hash, current) in selected {
                ensure!(
                    super::gates::hash_matches(hash.trim_start_matches("sha256:"), current),
                    "Snapshot hash is stale"
                );
            }
        }
        Command::Urgent => ensure!(context.place == Place::Issue, "Urgent belongs on an issue"),
    }
    Ok(())
}
