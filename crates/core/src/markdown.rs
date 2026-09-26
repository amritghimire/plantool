use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkbox {
    pub line: u32,
    pub checked: bool,
    pub text: String,
    pub phase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhaseProgress {
    pub name: String,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heading {
    pub line: u32,
    pub level: u8,
    pub text: String,
}

pub fn headings(content: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (i, raw) in content.lines().enumerate() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if hashes == 0 || hashes > 6 {
            continue;
        }
        let rest = &line[hashes..];
        if !rest.starts_with(' ') {
            continue;
        }
        out.push(Heading { line: i as u32 + 1, level: hashes as u8, text: rest.trim().to_string() });
    }
    out
}

pub fn checkboxes(content: &str) -> Vec<Checkbox> {
    let mut out = Vec::new();
    let mut phase: Option<String> = None;
    let mut in_fence = false;
    for (i, raw) in content.lines().enumerate() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("### ") {
            phase = Some(h.trim().to_string());
            continue;
        }
        if trimmed.starts_with("## ") || trimmed.starts_with("# ") {
            phase = None;
            continue;
        }
        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "));
        let Some(item) = item else { continue };
        let (checked, text) = if let Some(t) = item.strip_prefix("[ ] ") {
            (false, t)
        } else if let Some(t) = item.strip_prefix("[x] ").or_else(|| item.strip_prefix("[X] ")) {
            (true, t)
        } else if item == "[ ]" {
            (false, "")
        } else if item == "[x]" || item == "[X]" {
            (true, "")
        } else {
            continue;
        };
        out.push(Checkbox { line: i as u32 + 1, checked, text: text.trim().to_string(), phase: phase.clone() });
    }
    out
}

pub fn progress(content: &str) -> Vec<PhaseProgress> {
    let mut phases: Vec<PhaseProgress> = Vec::new();
    for cb in checkboxes(content) {
        let name = cb.phase.clone().unwrap_or_else(|| "Tasks".to_string());
        let entry = match phases.iter_mut().find(|p| p.name == name) {
            Some(p) => p,
            None => {
                phases.push(PhaseProgress { name, done: 0, total: 0 });
                phases.last_mut().expect("just pushed")
            }
        };
        entry.total += 1;
        if cb.checked {
            entry.done += 1;
        }
    }
    phases
}

pub fn title(content: &str) -> Option<String> {
    headings(content).into_iter().find(|h| h.level == 1).map(|h| h.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Plan: x\n\n## Todo List\n\n### Phase 1: A\n- [ ] one\n- [x] two\n\n### Phase 2: B\n- [ ] three\n\n```\n- [ ] not a task\n```\n";

    #[test]
    fn indexes_checkboxes_by_phase() {
        let cbs = checkboxes(DOC);
        assert_eq!(cbs.len(), 3);
        assert_eq!(cbs[0], Checkbox { line: 6, checked: false, text: "one".into(), phase: Some("Phase 1: A".into()) });
        assert!(cbs[1].checked);
        assert_eq!(cbs[2].phase.as_deref(), Some("Phase 2: B"));
    }

    #[test]
    fn progress_per_phase() {
        let p = progress(DOC);
        assert_eq!(p, vec![
            PhaseProgress { name: "Phase 1: A".into(), done: 1, total: 2 },
            PhaseProgress { name: "Phase 2: B".into(), done: 0, total: 1 },
        ]);
    }

    #[test]
    fn title_is_first_h1() {
        assert_eq!(title(DOC).as_deref(), Some("Plan: x"));
        assert_eq!(headings(DOC).len(), 4);
    }
}
