//! Stable plan identities and a presentation-only progress overlay.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub key: String,
    pub title: String,
    pub phase: Option<String>,
    pub area: Option<String>,
    pub review_tag: bool,
    pub line: u32,
}
pub fn valid_key(key: &str) -> bool {
    key.strip_prefix('T').is_some_and(|n| {
        !n.is_empty()
            && !n.starts_with('0')
            && n.bytes().all(|b| b.is_ascii_digit())
            && n.parse::<u32>().is_ok()
    })
}
pub fn parse(content: &str) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    let mut keys = std::collections::HashSet::new();
    for cb in crate::markdown::checkboxes(content) {
        let Some((key, title)) = cb.text.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if !key.starts_with('T') {
            continue;
        }
        if !valid_key(key) {
            return Err(format!("Invalid task key {key}; use T1, T2, …"));
        }
        if !keys.insert(key.to_string()) {
            return Err(format!("Duplicate task key {key}"));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(format!("Task {key} needs a title"));
        }
        let area = title
            .strip_prefix('[')
            .and_then(|s| s.split_once(']'))
            .filter(|(area, _)| *area != "review")
            .map(|(area, _)| area.to_string());
        tasks.push(Task {
            key: key.into(),
            title: title.into(),
            phase: cb.phase,
            area,
            review_tag: title.contains("[review]"),
            line: cb.line,
        });
    }
    Ok(tasks)
}
/// Checkbox state is never part of the approved plan's content identity.
pub fn normalize(content: &str) -> String {
    let lines: std::collections::HashSet<_> = crate::markdown::checkboxes(content)
        .iter()
        .map(|c| c.line)
        .collect();
    content
        .split_inclusive('\n')
        .enumerate()
        .map(|(i, line)| {
            if lines.contains(&(i as u32 + 1)) {
                line.replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1)
            } else {
                line.into()
            }
        })
        .collect()
}
pub fn overlay(content: &str, done: &std::collections::BTreeMap<String, bool>) -> String {
    let lines: std::collections::HashSet<_> = parse(content)
        .unwrap_or_default()
        .iter()
        .filter(|t| done.get(&t.key) == Some(&true))
        .map(|t| t.line)
        .collect();
    content
        .split_inclusive('\n')
        .enumerate()
        .map(|(i, line)| {
            if lines.contains(&(i as u32 + 1)) {
                line.replacen("[ ]", "[x]", 1)
            } else {
                line.into()
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_are_unique_and_checkbox_progress_is_not_identity() {
        let text = "### API\n- [ ] T1: [Backend] Add endpoint [review]\n- [ ] T2: Test it\n";
        let tasks = parse(text).unwrap();
        assert_eq!(tasks[0].phase.as_deref(), Some("API"));
        assert_eq!(tasks[0].area.as_deref(), Some("Backend"));
        assert!(tasks[0].review_tag);
        assert!(parse(&format!("{text}- [ ] T1: Duplicate\n")).is_err());
        let done = std::collections::BTreeMap::from([("T1".into(), true)]);
        let displayed = overlay(text, &done);
        assert!(displayed.contains("[x] T1"));
        assert_eq!(normalize(&displayed), text);
    }
}
