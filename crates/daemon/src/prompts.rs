use plantool_core::{DocKind, Session};
use std::path::Path;

pub const STAGES: [&str; 6] = ["research", "plan", "implement", "review", "resume", "critique"];

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

pub const REVIEW: &str = r#"Act on the review comments for plantool session `{key}`: {title}

Run `plantool skill` and follow it. The human left comments on `{review_doc_path}`.
Read them with `plantool session comment list --session {key} --kind human --unresolved --context --json` (note the `seq`).
A comment that asks a question gets an answer on its thread (`comment add --parent <id>`); leave it open for the human to resolve.
A comment that asks for a change gets the change in the document, a reply saying what changed, and a resolve.
Then `plantool session watch --session {key} --since <seq> --timeout 900` and repeat until the stage changes or the human says stop.
{brief}
{extra}"#;

pub const CRITIQUE: &str = r#"Critically review the document for plantool session `{key}`: {title}

Run `plantool skill` and follow it. Read `{review_doc_path}` (and `{research_path}` if it exists) against the code in `{checkout}`;
open the files the document names and check its claims. You are the reviewer here, not the author: do not edit the document.

Look for: assumptions that do not match the code; missing edge cases, error handling, tests, migrations, security or backwards
compatibility; steps too big for one session; acceptance criteria that cannot be verified; a simpler approach that follows the
codebase's existing patterns; anything the brief asked for that the document does not cover, and anything it covers that was not asked.

Post each finding as a comment anchored on the line it is about, all in one batch
(`plantool session comment apply --session {key} --input -` with `{"comments":[{"doc":"<research|plan>","match":"<line text>","body":"…"}]}`).
Each body: the problem, why it matters, and the concrete fix or question; one finding per comment; skip praise.
End with one comment on the title line: the verdict (ready, needs updates, or needs revision) and the two or three findings that matter most.
{brief}
{extra}"#;

pub const RESUME: &str = r#"Continue plantool session `{key}` ({title}) where you left off; the run was restarted and you keep your context.

Run `plantool skill` if it is no longer in your context. The stage is `{stage}`.
Read new comments with `plantool session comment list --session {key} --kind human --unresolved --context --json` and act on them
(revise `{review_doc_path}`, reply on each thread, resolve it), then `plantool session watch --session {key} --since <seq> --timeout 900`
and carry on as the stage skill says.
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
        "review" => REVIEW.to_string(),
        "resume" => RESUME.to_string(),
        "critique" => CRITIQUE.to_string(),
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

/// The document a review prompt should point at for the session's current stage.
pub fn review_doc(stage: plantool_core::Stage) -> DocKind {
    if stage.index() < plantool_core::Stage::Planning.index() {
        DocKind::Research
    } else {
        DocKind::Plan
    }
}

pub fn render(home: &Path, stage: &str, session: &Session, session_dir: &Path, extra: Option<&str>) -> String {
    render_at(home, stage, session, session_dir, extra, plantool_core::Stage::New)
}

pub fn render_at(home: &Path, stage: &str, session: &Session, session_dir: &Path, extra: Option<&str>, current: plantool_core::Stage) -> String {
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
            .replace("{review_doc_path}", &session_dir.join(review_doc(current).file_name()).display().to_string())
            .replace("{stage}", current.as_str())
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
    fn review_prompt_points_at_the_document_under_review() {
        let dir = Path::new("/d");
        let p = render_at(Path::new("/nonexistent"), "review", &session(None), dir, None, Stage::ResearchReview);
        assert!(p.contains("/d/research.md"), "{p}");
        assert!(p.contains("comment list --session repo/fix-it --kind human --unresolved"), "{p}");
        let p = render_at(Path::new("/nonexistent"), "review", &session(None), dir, None, Stage::PlanReview);
        assert!(p.contains("/d/plan.md"), "{p}");
    }

    #[test]
    fn critique_prompt_reviews_without_editing() {
        let p = render_at(Path::new("/nonexistent"), "critique", &session(None), Path::new("/d"), None, Stage::PlanReview);
        assert!(p.contains("/d/plan.md"), "{p}");
        assert!(p.contains("comment apply --session repo/fix-it"), "{p}");
        assert!(p.contains("do not edit the document"), "{p}");
    }

    #[test]
    fn next_stage_follows_the_flow() {
        assert_eq!(next_stage(Stage::New), Some("research"));
        assert_eq!(next_stage(Stage::ResearchReview), Some("plan"));
        assert_eq!(next_stage(Stage::Approved), Some("implement"));
        assert_eq!(next_stage(Stage::Done), None);
    }
}
