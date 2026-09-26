use plantool_core::{DocKind, Session};
use std::path::Path;

pub const STAGES: [&str; 3] = ["research", "plan", "implement"];

pub const RESEARCH: &str = r#"Research for plantool session `{key}`: {title}

Run `plantool skill` and then `plantool skill research`, and follow both. Write the research document to
`{research_path}` (nothing goes into REVIEWS/). The repository checkout is `{checkout}`.
{brief}
When the document is written, stop and wait for review.
{extra}"#;

pub const PLAN: &str = r#"Plan for plantool session `{key}`: {title}

Run `plantool skill` and then `plantool skill plan`, and follow both. The research, if any, is at `{research_path}`.
Write the plan to `{plan_path}` (nothing goes into REVIEWS/). The repository checkout is `{checkout}`.
{brief}
After writing the plan, loop on the human's comments (`plantool session comment list --session {key} --kind human --unresolved --context`,
revise, reply, resolve, then `plantool session watch --session {key} --since <seq> --timeout 900`) until the stage is `approved`.
{extra}"#;

pub const IMPLEMENT: &str = r#"Implement plantool session `{key}`: {title}

Run `plantool skill` and then `plantool skill implement`, and follow both. The human approved building this.
The plan lives at `{plan_path}`; if that file does not exist, planning was skipped: write the ticket list there first, as the skill says.
Set the stage with `plantool session stage --session {key} --set implementing`, work in `{checkout}`,
and tick the plan's checkboxes as you finish each ticket. When every ticket is done, run the project's checks,
set the stage to `implementation-review`, and summarise what changed and what you skipped.
{brief}
{extra}"#;

pub fn template(home: &Path, stage: &str) -> String {
    let override_path = home.join("prompts").join(format!("{stage}.md"));
    if let Ok(s) = std::fs::read_to_string(&override_path) {
        if !s.trim().is_empty() {
            return s;
        }
    }
    match stage {
        "research" => RESEARCH.to_string(),
        "plan" => PLAN.to_string(),
        "implement" => IMPLEMENT.to_string(),
        _ => String::new(),
    }
}

/// The stage whose prompt a human would paste next, given the session's current stage.
pub fn next_stage(stage: plantool_core::Stage) -> Option<&'static str> {
    use plantool_core::Stage::*;
    match stage {
        New | Researching => Some("research"),
        ResearchReview | Planning | PlanReview => Some("plan"),
        Approved | Implementing => Some("implement"),
        ImplementationReview | Done => None,
    }
}

pub fn render(home: &Path, stage: &str, session: &Session, session_dir: &Path, extra: Option<&str>) -> String {
    let t = template(home, stage);
    let brief = session
        .brief
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(|b| format!("\nWhat the user wants:\n{b}\n"))
        .unwrap_or_default();
    let extra = extra.map(str::trim).filter(|e| !e.is_empty()).map(|e| format!("\nAdditional instructions from the user:\n{e}\n")).unwrap_or_default();
    collapse_blank_lines(
        &t.replace("{title}", &session.title)
            .replace("{slug}", &session.slug)
            .replace("{key}", &session.key())
            .replace("{repo_slug}", &session.repo_slug)
            .replace("{checkout}", &session.cwd().display().to_string())
            .replace("{session_dir}", &session_dir.display().to_string())
            .replace("{research_path}", &session_dir.join(DocKind::Research.file_name()).display().to_string())
            .replace("{plan_path}", &session_dir.join(DocKind::Plan.file_name()).display().to_string())
            .replace("{brief}", &brief)
            .replace("{extra}", &extra),
    )
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = false;
    for line in s.lines() {
        if line.trim().is_empty() {
            if !blank {
                out.push('\n');
            }
            blank = true;
        } else {
            out.push_str(line);
            out.push('\n');
            blank = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plantool_core::{Checkout, Stage};

    fn session(brief: Option<&str>) -> Session {
        Session {
            repo_slug: "repo".into(),
            slug: "fix-it".into(),
            title: "fix it".into(),
            repo: Checkout { root: "/r".into(), common_dir: "/r/.git".into(), branch: "main".into() },
            worktree: None,
            base: "main".into(),
            mirror: false,
            brief: brief.map(|b| b.to_string()),
            created_in: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn renders_brief_and_extra() {
        let dir = Path::new("/home/.plantool/sessions/repo/fix-it");
        let p = render(Path::new("/nonexistent"), "research", &session(Some("Token scopes are confusing")), dir, Some("check issue 1"));
        assert!(p.contains("plantool skill research"));
        assert!(p.contains("What the user wants:\nToken scopes are confusing"));
        assert!(p.contains("Additional instructions from the user:\ncheck issue 1"));
        assert!(p.contains("/home/.plantool/sessions/repo/fix-it/research.md"));
        assert!(!p.contains("\n\n\n"));
    }

    #[test]
    fn omits_empty_sections() {
        let dir = Path::new("/d");
        let p = render(Path::new("/nonexistent"), "plan", &session(None), dir, Some("  "));
        assert!(!p.contains("What the user wants"));
        assert!(!p.contains("Additional instructions"));
        assert!(!p.ends_with('\n'));
    }

    #[test]
    fn next_stage_follows_the_flow() {
        assert_eq!(next_stage(Stage::New), Some("research"));
        assert_eq!(next_stage(Stage::ResearchReview), Some("plan"));
        assert_eq!(next_stage(Stage::Approved), Some("implement"));
        assert_eq!(next_stage(Stage::Done), None);
    }
}
