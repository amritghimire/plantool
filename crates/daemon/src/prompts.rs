use plantool_core::{ChangeReview, DocKind, ImplementationMode, Session};
use std::path::Path;

pub const STAGES: [&str; 8] = [
    "research",
    "plan",
    "implement",
    "review",
    "resume",
    "critique",
    "assist",
    "draft-pr",
];

pub const ASSIST: &str = r#"Help with plantool session `{key}` at stage `{stage}`.

The checkout is `{checkout}` and the session files are in `{session_dir}`. Read `plantool skill` for the session commands. Use the current documents, open comments, and diff when they matter to the request. Answer in plain terms and point to the relevant file or change. Keep the stage unchanged. Before the plan is approved, do not implement it.
{brief}
{extra}"#;

pub const DRAFT_PR: &str = r#"Draft a pull request for plantool session `{key}`.

If the git-workflow skill is available, follow its PR voice. Otherwise, use the repository's recent PRs and commits for style. Review the changes and commits in `{checkout}` against `{base}`. Write a title on the first line as `# Title` and the PR body below it to `{session_dir}/pr-draft.md`. Do not push, create a PR, or change the session stage. The human will edit the draft and confirm the final action in the browser.
If `{checkout}` has uncommitted changes, also write a commit message for them to `{session_dir}/commit-draft.md`: subject line, blank line, body, in the repository's commit style, referencing the session slug `{key}`. Do not commit.
{brief}
{extra}"#;

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
After writing the plan, post blocking questions as comments on their own lines. Read human comments, revise the plan, reply on each thread, and resolve requests you addressed. Leave question threads open for the human. Wait for new comments or approval as the plan skill describes.
{extra}"#;

pub const IMPLEMENT: &str = r#"Implement plantool session `{key}`: {title}

Run `plantool skill` and then `plantool skill implement`, and follow both. The human approved building this.
The plan lives at `{plan_path}`; if that file does not exist, planning was skipped: write the task list there first, as the skill says.
Set the stage with `plantool session stage --session {key} --set implementing`, work in `{checkout}`,
and tick the plan's checkboxes as you finish each task. When every task is done, run the project's checks,
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

Run `plantool skill` if it is no longer in your context. Check the current stage with `plantool session get --session {key} --json`; it may have changed while this run was stopped. Read new human comments with `plantool session comment list --session {key} --kind human --unresolved --context --json`. Answer questions on their threads and leave them open; for requested changes, update the relevant document or code, reply with what changed, and resolve the thread. Continue the unfinished work for the current stage, following its stage skill. Do not edit the plan during implementation unless the plan needs correction, and do not start a new task before milestone approval.
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
        "assist" => ASSIST.to_string(),
        "draft-pr" => DRAFT_PR.to_string(),
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

pub fn render(
    home: &Path,
    stage: &str,
    session: &Session,
    session_dir: &Path,
    extra: Option<&str>,
) -> String {
    render_at(
        home,
        stage,
        session,
        session_dir,
        extra,
        plantool_core::Stage::New,
    )
}

pub fn render_at(
    home: &Path,
    stage: &str,
    session: &Session,
    session_dir: &Path,
    extra: Option<&str>,
    current: plantool_core::Stage,
) -> String {
    let t = template(home, stage);
    let brief = session
        .brief
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(|b| format!("\nWhat the user wants:\n{b}\n"))
        .unwrap_or_default();
    let extra = extra
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| format!("\nAdditional instructions from the user:\n{e}\n"))
        .unwrap_or_default();
    let rendered = collapse_blank_lines(
        &t.replace("{title}", &session.title)
            .replace("{slug}", &session.slug)
            .replace("{key}", &session.key())
            .replace("{repo_slug}", &session.repo_slug)
            .replace("{checkout}", &session.cwd().display().to_string())
            .replace("{base}", &session.base)
            .replace("{session_dir}", &session_dir.display().to_string())
            .replace(
                "{research_path}",
                &session_dir
                    .join(DocKind::Research.file_name())
                    .display()
                    .to_string(),
            )
            .replace(
                "{plan_path}",
                &session_dir
                    .join(DocKind::Plan.file_name())
                    .display()
                    .to_string(),
            )
            .replace(
                "{review_doc_path}",
                &session_dir
                    .join(review_doc(current).file_name())
                    .display()
                    .to_string(),
            )
            .replace("{stage}", current.as_str())
            .replace("{brief}", &brief)
            .replace("{extra}", &extra),
    );
    format!("{rendered}\n\n{}", context_prompt(session))
}

pub fn render_run(
    home: &Path,
    stage: &str,
    session: &Session,
    session_dir: &Path,
    extra: Option<&str>,
    current: plantool_core::Stage,
    mode: ImplementationMode,
) -> String {
    let mut prompt = render_at(home, stage, session, session_dir, extra, current);
    if stage == "implement" {
        prompt.push_str("\nBefore changing plan task scope, pause at a safe boundary and run `plantool session plan propose-revision --session <ref> --reason ...`. Then edit plan.md and wait for owner acceptance before continuing. Use comment replies with --proposes-resolve to request owner review.\n");
    }
    if mode == ImplementationMode::StepByStep && matches!(stage, "implement" | "resume") {
        prompt.push_str("\n\nImplementation mode: step by step. Work on one plan phase in this turn. Complete its unchecked tasks. If the plan has no phases, complete one unchecked task. Tick only finished work, run the relevant checks, and leave the stage at implementing, even when it was the final task. Plantool opens or refreshes the change review and forwards human feedback. Finish each turn after addressing comments. Do not start a blocking difftool watch. Wait for explicit milestone approval before another phase. Plantool starts a new scoped run for the next phase after approval. The owner can choose whether to commit the milestone; do not commit it yourself. Run full checks after the final phase.\n");
    }
    prompt
}

pub fn render_milestone_review(
    session: &Session,
    session_dir: &Path,
    review: Option<&ChangeReview>,
    extra: Option<&str>,
) -> String {
    let reference = match review {
        Some(ChangeReview::Difftool {
            review: Some(id), ..
        }) => format!("The difftool review is `{id}`."),
        _ => "Open the current changes with `plantool changes` if the review is missing.".into(),
    };
    let mut prompt = format!("Continue reviewing the current implementation milestone for plantool session `{}`.\n\nRun `plantool skill` and `plantool skill implement`. The plan is at `{}` and the checkout is `{}`. {reference} Read the open human comments with `difftool review comment list --review <ref> --kind human --unresolved --context --json` when difftool is available. Fix the current milestone, refresh the review, and reply to comments. Remain at stage `implementing`. Plantool forwards new human comments to this run, so finish your turn without starting a blocking difftool watch. Wait for the human to approve this milestone in plantool. Do not start another plan task yet.", session.key(), session_dir.join(DocKind::Plan.file_name()).display(), session.cwd().display());
    if let Some(extra) = extra.map(str::trim).filter(|e| !e.is_empty()) {
        prompt.push_str(&format!(
            "\n\nAdditional instructions from the user:\n{extra}"
        ));
    }
    prompt.push_str(&format!("\n\n{}", context_prompt(session)));
    prompt
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
            pause_rule: Default::default(),
            difftool: None,
            repo_slug: "repo".into(),
            slug: "fix-it".into(),
            title: "fix it".into(),
            repo: Checkout {
                root: "/r".into(),
                common_dir: "/r/.git".into(),
                branch: "main".into(),
            },
            worktree: None,
            pull_request: None,
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
        let p = render(
            Path::new("/nonexistent"),
            "research",
            &session(Some("Token scopes are confusing")),
            dir,
            Some("check issue 1"),
        );
        assert!(p.contains("plantool skill research"));
        assert!(p.contains("What the user wants:\nToken scopes are confusing"));
        assert!(p.contains("Additional instructions from the user:\ncheck issue 1"));
        assert!(p.contains("/home/.plantool/sessions/repo/fix-it/research.md"));
        assert!(!p.contains("\n\n\n"));
    }

    #[test]
    fn omits_empty_sections() {
        let dir = Path::new("/d");
        let p = render(
            Path::new("/nonexistent"),
            "plan",
            &session(None),
            dir,
            Some("  "),
        );
        assert!(!p.contains("What the user wants"));
        assert!(!p.contains("Additional instructions"));
        assert!(!p.ends_with('\n'));
    }

    #[test]
    fn review_prompt_points_at_the_document_under_review() {
        let dir = Path::new("/d");
        let p = render_at(
            Path::new("/nonexistent"),
            "review",
            &session(None),
            dir,
            None,
            Stage::ResearchReview,
        );
        assert!(p.contains("/d/research.md"), "{p}");
        assert!(
            p.contains("comment list --session repo/fix-it --kind human --unresolved"),
            "{p}"
        );
        let p = render_at(
            Path::new("/nonexistent"),
            "review",
            &session(None),
            dir,
            None,
            Stage::PlanReview,
        );
        assert!(p.contains("/d/plan.md"), "{p}");
    }

    #[test]
    fn resumed_run_checks_the_current_stage_before_changing_files() {
        let p = render_at(
            Path::new("/nonexistent"),
            "resume",
            &session(None),
            Path::new("/d"),
            None,
            Stage::ImplementationReview,
        );
        assert!(p.contains("session get --session repo/fix-it --json"));
        assert!(p.contains("do not start a new task before milestone approval"));
        assert!(!p.contains("revise `/d/plan.md`"));
    }

    #[test]
    fn critique_prompt_reviews_without_editing() {
        let p = render_at(
            Path::new("/nonexistent"),
            "critique",
            &session(None),
            Path::new("/d"),
            None,
            Stage::PlanReview,
        );
        assert!(p.contains("/d/plan.md"), "{p}");
        assert!(p.contains("comment apply --session repo/fix-it"), "{p}");
        assert!(p.contains("do not edit the document"), "{p}");
    }

    #[test]
    fn step_by_step_prompt_scopes_a_phase_and_keeps_review_boundary() {
        let s = session(None);
        let dir = Path::new("/d");
        let step = render_run(
            Path::new("/nonexistent"),
            "implement",
            &s,
            dir,
            None,
            Stage::Approved,
            ImplementationMode::StepByStep,
        );
        assert!(step.contains("Work on one plan phase"));
        assert!(step.contains("leave the stage at implementing, even when it was the final task"));
        let bulk = render_run(
            Path::new("/nonexistent"),
            "implement",
            &s,
            dir,
            None,
            Stage::Approved,
            ImplementationMode::AllAtOnce,
        );
        assert!(!bulk.contains("Work on one plan phase"));
    }

    #[test]
    fn next_stage_follows_the_flow() {
        assert_eq!(next_stage(Stage::New), Some("research"));
        assert_eq!(next_stage(Stage::ResearchReview), Some("plan"));
        assert_eq!(next_stage(Stage::Approved), Some("implement"));
        assert_eq!(next_stage(Stage::Done), None);
    }
}

pub fn pause_rule_prompt(rule: &plantool_core::PauseRule, session: &str) -> String {
    match rule {
        plantool_core::PauseRule::EveryMilestone => "Finish this phase, then wait for the owner to review and approve the milestone.".into(),
        plantool_core::PauseRule::NoPauses => "Finish this phase. Plantool will start the next pending phase without owner approval.".into(),
        plantool_core::PauseRule::PlainLanguage(rule) => format!("Judge whether this checkpoint rule applies: {rule}. When it applies, run `plantool session pause --session {session} --reason <reason>` at a safe boundary and finish your turn. Otherwise finish this phase and Plantool will continue."),
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    #[test]
    fn renders_each_pause_rule() {
        assert!(
            pause_rule_prompt(&plantool_core::PauseRule::EveryMilestone, "repo/session")
                .contains("wait for the owner")
        );
        assert!(
            pause_rule_prompt(&plantool_core::PauseRule::NoPauses, "repo/session")
                .contains("without owner approval")
        );
        let adaptive = pause_rule_prompt(
            &plantool_core::PauseRule::PlainLanguage("Pause before API changes".into()),
            "repo/session",
        );
        assert!(adaptive.contains("Pause before API changes"));
        assert!(adaptive.contains("plantool session pause --session repo/session"));
        assert!(adaptive.contains("safe boundary"));
    }
}

pub fn context_prompt(session: &Session) -> String {
    let branch = crate::git::git(session.cwd(), &["branch", "--show-current"])
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "detached or unavailable".into());
    let tool = crate::changes::resolve_tool(session)
        .as_ref()
        .map_or("built-in", crate::changes::ChangeTool::name);
    format!("Current session context:\nWorkspace: {}\nBranch: {branch}\nBase: {}\nDiff tool: {tool}\n{}", session.cwd().display(), session.base, pause_rule_prompt(&session.pause_rule, &session.key()))
}
