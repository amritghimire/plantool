use crate::model::{Anchor, Comment};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AnchorError {
    #[error("no line matches {0:?}")]
    NoMatch(String),
    #[error("{count} lines match {needle:?}; quote a longer, unique line")]
    Ambiguous { needle: String, count: usize },
    #[error("line {0} is out of range (document has {1} lines)")]
    OutOfRange(u32, u32),
}

pub fn line_count(content: &str) -> u32 {
    content.lines().count() as u32
}

pub fn line_text(content: &str, line: u32) -> Option<&str> {
    if line == 0 {
        return None;
    }
    content.lines().nth(line as usize - 1)
}

pub fn anchor_at_line(content: &str, line: u32) -> Result<Anchor, AnchorError> {
    match line_text(content, line) {
        Some(text) => Ok(Anchor { line, text: text.trim().to_string(), outdated: false }),
        None => Err(AnchorError::OutOfRange(line, line_count(content))),
    }
}

pub fn resolve_match(content: &str, needle: &str) -> Result<Anchor, AnchorError> {
    let needle_trim = needle.trim();
    if needle_trim.is_empty() {
        return Err(AnchorError::NoMatch(needle.to_string()));
    }
    let exact: Vec<(usize, &str)> = content
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim() == needle_trim)
        .collect();
    let hits = if exact.is_empty() {
        content.lines().enumerate().filter(|(_, l)| l.contains(needle_trim)).collect::<Vec<_>>()
    } else {
        exact
    };
    match hits.len() {
        0 => Err(AnchorError::NoMatch(needle.to_string())),
        1 => Ok(Anchor { line: hits[0].0 as u32 + 1, text: hits[0].1.trim().to_string(), outdated: false }),
        n => Err(AnchorError::Ambiguous { needle: needle.to_string(), count: n }),
    }
}

fn map_line(old: &str, new: &str, line: u32) -> Option<u32> {
    let diff = TextDiff::from_lines(old, new);
    let target = line as usize - 1;
    for change in diff.iter_all_changes() {
        if let Some(oi) = change.old_index() {
            if oi == target {
                return match change.tag() {
                    ChangeTag::Equal => change.new_index().map(|n| n as u32 + 1),
                    _ => None,
                };
            }
        }
    }
    None
}

pub fn reanchor_one(old: &str, new: &str, anchor: &Anchor) -> Anchor {
    let text = anchor.text.trim();
    if !text.is_empty() {
        if let Some(t) = line_text(new, anchor.line) {
            if t.trim() == text {
                return Anchor { line: anchor.line, text: anchor.text.clone(), outdated: false };
            }
        }
        let matches: Vec<usize> = new
            .lines()
            .enumerate()
            .filter(|(_, l)| l.trim() == text)
            .map(|(i, _)| i)
            .collect();
        if matches.len() == 1 {
            return Anchor { line: matches[0] as u32 + 1, text: anchor.text.clone(), outdated: false };
        }
        if matches.len() > 1 {
            let nearest = matches
                .iter()
                .min_by_key(|i| (**i as i64 - (anchor.line as i64 - 1)).abs())
                .copied()
                .unwrap_or(matches[0]);
            return Anchor { line: nearest as u32 + 1, text: anchor.text.clone(), outdated: false };
        }
    }
    if let Some(mapped) = map_line(old, new, anchor.line) {
        let new_text = line_text(new, mapped).unwrap_or("").trim().to_string();
        return Anchor { line: mapped, text: new_text, outdated: false };
    }
    Anchor { line: anchor.line, text: anchor.text.clone(), outdated: true }
}

pub fn reanchor(old: &str, new: &str, comments: &mut [Comment]) -> usize {
    let mut changed = 0;
    for c in comments.iter_mut() {
        let next = reanchor_one(old, new, &c.anchor);
        if next != c.anchor {
            c.anchor = next;
            changed += 1;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "# Plan\n\nalpha\nbeta\ngamma\ndelta\n";

    fn a(line: u32, text: &str) -> Anchor {
        Anchor { line, text: text.into(), outdated: false }
    }

    #[test]
    fn match_resolves_unique_line() {
        assert_eq!(resolve_match(OLD, "beta"), Ok(a(4, "beta")));
        assert_eq!(resolve_match(OLD, "  gamma "), Ok(a(5, "gamma")));
    }

    #[test]
    fn match_errors_on_none_or_many() {
        assert_eq!(resolve_match(OLD, "zeta"), Err(AnchorError::NoMatch("zeta".into())));
        assert_eq!(resolve_match("x\nx\n", "x"), Err(AnchorError::Ambiguous { needle: "x".into(), count: 2 }));
    }

    #[test]
    fn substring_match_when_no_exact_line() {
        assert_eq!(resolve_match("one two three\nfour\n", "two"), Ok(a(1, "one two three")));
    }

    #[test]
    fn keeps_anchor_when_line_unchanged() {
        assert_eq!(reanchor_one(OLD, OLD, &a(4, "beta")), a(4, "beta"));
    }

    #[test]
    fn follows_inserted_lines() {
        let new = "# Plan\n\nintro\nalpha\nbeta\ngamma\ndelta\n";
        assert_eq!(reanchor_one(OLD, new, &a(4, "beta")), a(5, "beta"));
    }

    #[test]
    fn follows_deleted_lines() {
        let new = "# Plan\n\nbeta\ngamma\ndelta\n";
        assert_eq!(reanchor_one(OLD, new, &a(5, "gamma")), a(4, "gamma"));
    }

    #[test]
    fn follows_moved_paragraph() {
        let new = "# Plan\n\ngamma\ndelta\nalpha\nbeta\n";
        assert_eq!(reanchor_one(OLD, new, &a(3, "alpha")), a(5, "alpha"));
    }

    #[test]
    fn edited_line_goes_outdated() {
        let new = "# Plan\n\nalpha\nBETA changed\ngamma\ndelta\n";
        let r = reanchor_one(OLD, new, &a(4, "beta"));
        assert!(r.outdated);
        assert_eq!(r.line, 4);
    }

    #[test]
    fn ambiguous_duplicate_picks_nearest() {
        let new = "beta\n# Plan\n\nalpha\nbeta\ngamma\n";
        assert_eq!(reanchor_one(OLD, new, &a(4, "beta")), a(5, "beta"));
    }

    #[test]
    fn reanchor_counts_changes() {
        let new = "# Plan\n\nintro\nalpha\nbeta\ngamma\ndelta\n";
        let mut comments = vec![
            Comment { id: "1".into(), doc: crate::DocKind::Plan, anchor: a(4, "beta"), body: "".into(), kind: crate::CommentKind::Human, author: "me".into(), parent: None, resolved: false, created_at: "".into(), updated_at: None, seq: 1 },
            Comment { id: "2".into(), doc: crate::DocKind::Plan, anchor: a(1, "# Plan"), body: "".into(), kind: crate::CommentKind::Human, author: "me".into(), parent: None, resolved: false, created_at: "".into(), updated_at: None, seq: 2 },
        ];
        assert_eq!(reanchor(OLD, new, &mut comments), 1);
        assert_eq!(comments[0].anchor.line, 5);
    }
}
