use plantool_core::{DocKind, Session};
use std::path::Path;

pub const RESEARCH: &str = r#"/research {title} for {slug}

This work is tracked by plantool as session `{key}`. Read `plantool skill` first (run: `plantool skill`).
Write the research document to `{research_path}` instead of REVIEWS/. Nothing goes into REVIEWS/.
The repository checkout is `{checkout}`. When the document is written, stop and wait for review.
{extra}"#;

pub const PLAN: &str = r#"/plan {slug}: {title}

This work is tracked by plantool as session `{key}`. Read `plantool skill` first (run: `plantool skill`).
The research, if any, is at `{research_path}`. Write the plan to `{plan_path}` instead of REVIEWS/.
Nothing goes into REVIEWS/. The repository checkout is `{checkout}`.

After writing the plan, read the human's open comments (`plantool session comment list --session {key} --kind human --unresolved --context`),
then arm `plantool session watch --session {key} --since <seq> --timeout 900` and loop: revise the plan, reply on each thread with what
changed, resolve it, and watch again. Stop looping when the stage becomes `approved` or the human says so.
{extra}"#;

pub const IMPLEMENT: &str = r#"/implement {plan_path}

This work is tracked by plantool as session `{key}`. Read `plantool skill` first (run: `plantool skill`).
The plan is approved. Set the stage with `plantool session stage --session {key} --set implementing`, work in `{checkout}`,
and tick the plan's checkboxes in `{plan_path}` as you finish each task (the browser shows progress live).
When every task is done, run the project's checks, then `plantool session stage --session {key} --set implementation-review`
and summarise what changed and what you skipped.
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

pub fn render(home: &Path, stage: &str, session: &Session, session_dir: &Path, extra: Option<&str>) -> String {
    let t = template(home, stage);
    t.replace("{title}", &session.title)
        .replace("{slug}", &session.slug)
        .replace("{key}", &session.key())
        .replace("{repo_slug}", &session.repo_slug)
        .replace("{checkout}", &session.cwd().display().to_string())
        .replace("{session_dir}", &session_dir.display().to_string())
        .replace("{research_path}", &session_dir.join(DocKind::Research.file_name()).display().to_string())
        .replace("{plan_path}", &session_dir.join(DocKind::Plan.file_name()).display().to_string())
        .replace("{extra}", extra.map(|e| format!("\nAdditional instructions from the user:\n{e}\n")).unwrap_or_default().as_str())
        .trim()
        .to_string()
}
