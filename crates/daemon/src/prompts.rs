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

pub const CRITIQUE_CODE: &str = r#"Critically review the code changes for plantool session `{key}`: {title}

Run `plantool skill` and follow it. You are the reviewer here, not the author: do not edit code, the plan, or the session stage.
Run `plantool changes {key} --scope all --no-open --json` to open or refresh the change review and note its difftool review id.
Read the plan at `{plan_path}`, then the diff in `{checkout}` against `{base}`. Read the code around each change, not only the diff:
callers, tests, and the files the change touches.

If the ponytail-review, interrogate, or code-review skill is installed, use it for this review. Otherwise look for: logic that is
wrong on real inputs or edge cases; lost data, leaked secrets, or missing checks at trust boundaries; risky code with no test;
slow paths under real load; plan tasks ticked but not done, or done differently; work the plan did not ask for; and code that
could be deleted or reuse what the codebase already has. Run the project's checks if they are cheap.

Post each finding as an inline comment on the line it is about, all in one batch
(`difftool review comment add --review <id> --input -` with `{"comments":[{"filePath":"<path>","match":"<line text>","type":"issue|suggestion","body":"…"}]}`).
If difftool is not available, post them as plan comments with `plantool session comment apply --session {key} --input -`.
Each body: the problem, a concrete input or state that triggers it, and the fix; one finding per comment; skip praise and style nits.
End with a short summary: the verdict (ready to merge, needs fixes, or needs rework) and the two or three findings that matter most.
{brief}
{extra}"#;

pub const RESUME: &str = r#"Continue plantool session `{key}` ({title}) where you left off; the run was restarted and you keep your context.

Run `plantool skill` if it is no longer in your context. Check the current stage with `plantool session get --session {key} --json`; it may have changed while this run was stopped. Read new human comments with `plantool session comment list --session {key} --kind human --unresolved --context --json`. Answer questions on their threads and leave them open; for requested changes, update the relevant document or code, reply with what changed, and resolve the thread. Continue the unfinished work for the current stage, following its stage skill. Do not edit the plan during implementation unless the plan needs correction, and do not start a new task before milestone approval.
{brief}
{extra}"#;

/// Steps that read an optional `~/.plantool/prompts/<step>.extra.md`, appended to the built-in prompt.
pub const EXTRA_STAGES: [&str; 6] = [
    "research",
    "plan",
    "implement",
    "critique",
    "review",
    "draft-pr",
];

const EXTRAS_REPOSITORY: &str = match option_env!("PLANTOOL_RELEASE_REPOSITORY") {
    Some(r) => r,
    None => "amritghimire/plantool",
};

fn extra_path(home: &Path, stage: &str) -> std::path::PathBuf {
    home.join("prompts").join(format!("{stage}.extra.md"))
}

fn with_extra(home: &Path, stage: &str, prompt: String) -> String {
    let path = extra_path(home, stage);
    match std::fs::read_to_string(&path) {
        Ok(extra) if !extra.trim().is_empty() => {
            let user_home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            let skills = user_home
                .map(|h| skill_files(&h, &home.join("skills"), &extra))
                .unwrap_or_default();
            let mut out = format!(
                "{prompt}\n\nStanding instructions from `{}`:\n{}",
                path.display(),
                extra.trim()
            );
            if !skills.is_empty() {
                out.push_str("\n\nSkill files on this machine. When you cannot load one of these skills directly, read its file and follow it:");
                for (name, file) in skills {
                    out.push_str(&format!("\n- {name}: {}", file.display()));
                }
            }
            out
        }
        _ => prompt,
    }
}

/// The installed SKILL.md for every `backticked` skill name in the hints, in order of first mention.
fn skill_files(
    user_home: &Path,
    downloaded: &Path,
    hints: &str,
) -> Vec<(String, std::path::PathBuf)> {
    let mut roots = vec![
        user_home.join(".claude/skills"),
        user_home.join(".agents/skills"),
        user_home.join(".codex/skills"),
    ];
    // Claude Code plugins live at plugins/cache/<marketplace>/<plugin>/<version>/skills.
    let dirs = |p: &Path| -> Vec<std::path::PathBuf> {
        std::fs::read_dir(p)
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default()
    };
    for marketplace in dirs(&user_home.join(".claude/plugins/cache")) {
        for plugin in dirs(&marketplace) {
            let mut versions = dirs(&plugin);
            versions.sort_by_key(|v| std::fs::metadata(v).and_then(|m| m.modified()).ok());
            if let Some(latest) = versions.pop() {
                roots.push(latest.join("skills"));
            }
        }
    }
    // Skills the user installed win over the copies plantool downloaded.
    roots.push(downloaded.to_path_buf());
    let mut found: Vec<(String, std::path::PathBuf)> = Vec::new();
    for name in skill_names(hints) {
        if let Some(file) = roots
            .iter()
            .map(|root| root.join(&name).join("SKILL.md"))
            .find(|f| f.is_file())
        {
            found.push((name, file));
        }
    }
    found
}

/// Every `backticked` name in the hints that looks like a skill, in order of first mention.
fn skill_names(hints: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in hints.split('`').skip(1).step_by(2) {
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names
}

#[derive(serde::Deserialize)]
struct SkillSource {
    repo: String,
    path: String,
    #[serde(default = "default_ref", rename = "ref")]
    git_ref: String,
}

fn default_ref() -> String {
    "main".into()
}

/// Copy `<top>/<path>/<name>/…` for each wanted skill out of a GitHub tarball into `dest/<name>/`.
fn extract_skills(
    archive: impl std::io::Read,
    path: &str,
    wanted: &[String],
    dest: &Path,
) -> anyhow::Result<Vec<String>> {
    let prefix: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut done: Vec<String> = Vec::new();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry.path()?.into_owned();
        let parts: Vec<&str> = match entry_path
            .components()
            .map(|c| match c {
                std::path::Component::Normal(p) => p.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        {
            Some(parts) => parts,
            None => continue,
        };
        // parts: [top, ..prefix, name, ..file]
        if parts.len() < prefix.len() + 3 || parts[1..=prefix.len()] != prefix[..] {
            continue;
        }
        let name = parts[prefix.len() + 1];
        if !wanted.iter().any(|w| w == name) {
            continue;
        }
        if !done.iter().any(|d| d == name) {
            let _ = std::fs::remove_dir_all(dest.join(name));
            done.push(name.to_string());
        }
        let out = parts[prefix.len() + 1..]
            .iter()
            .fold(dest.to_path_buf(), |p, part| p.join(part));
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&out)?;
    }
    Ok(done)
}

pub fn extras_status(home: &Path) -> serde_json::Value {
    serde_json::json!({
        "dir": home.join("prompts"),
        "source": format!("https://github.com/{EXTRAS_REPOSITORY}/tree/main/prompts/extras"),
        "files": EXTRA_STAGES.iter().map(|stage| serde_json::json!({
            "stage": stage,
            "installed": extra_path(home, stage).is_file(),
        })).collect::<Vec<_>>(),
    })
}

/// Download the recommended extras from the plantool repository, the way `plantool update` fetches releases.
pub fn install_extras(
    home: &Path,
    git_ref: &str,
    force: bool,
) -> anyhow::Result<serde_json::Value> {
    use anyhow::Context;
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("plantool/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    // A private plantool repository only serves raw files to a signed-in GitHub user.
    let token = crate::config::github_token();
    let mut files = Vec::new();
    for stage in EXTRA_STAGES {
        let url = format!("https://raw.githubusercontent.com/{EXTRAS_REPOSITORY}/{git_ref}/prompts/extras/{stage}.extra.md");
        let mut request = client.get(&url);
        if let Some(token) = &token {
            request = request.bearer_auth(token);
        }
        let text = request
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .with_context(|| {
                format!("could not download {url}; for a private repository, sign in with `gh auth login` or set GH_TOKEN")
            })?;
        files.push((stage, text));
    }
    let sources_url = format!("https://raw.githubusercontent.com/{EXTRAS_REPOSITORY}/{git_ref}/prompts/extras/sources.json");
    let mut request = client.get(&sources_url);
    if let Some(token) = &token {
        request = request.bearer_auth(token);
    }
    let sources: Vec<SkillSource> = request
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .with_context(|| format!("could not download {sources_url}"))?;
    let mut result = write_extras(home, &files, force)?;

    // Fetch the skills the hints name, from the repositories that publish them, so agents
    // without those skills installed can still read and follow them.
    let mut wanted = Vec::new();
    for stage in EXTRA_STAGES {
        let text = std::fs::read_to_string(extra_path(home, stage)).unwrap_or_default();
        for name in skill_names(&text) {
            if !wanted.contains(&name) {
                wanted.push(name);
            }
        }
    }
    let dest = home.join("skills");
    let archives = reqwest::blocking::Client::builder()
        .user_agent(format!("plantool/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let mut skills: Vec<String> = Vec::new();
    for source in sources {
        let missing: Vec<String> = wanted
            .iter()
            .filter(|w| !skills.contains(w))
            .cloned()
            .collect();
        if missing.is_empty() {
            break;
        }
        let url = format!(
            "https://codeload.github.com/{}/tar.gz/{}",
            source.repo, source.git_ref
        );
        let response = archives
            .get(&url)
            .send()
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("could not download skills from {url}"))?;
        skills.extend(extract_skills(response, &source.path, &missing, &dest)?);
    }
    result["skills_dir"] = serde_json::json!(dest);
    result["skills"] = serde_json::json!(skills);
    Ok(result)
}

// Download everything before writing anything, so a failed download leaves no half-installed set.
fn write_extras(
    home: &Path,
    files: &[(&str, String)],
    force: bool,
) -> anyhow::Result<serde_json::Value> {
    std::fs::create_dir_all(home.join("prompts"))?;
    let (mut installed, mut kept) = (Vec::new(), Vec::new());
    for (stage, text) in files {
        let path = extra_path(home, stage);
        if path.exists() && !force {
            kept.push(stage.to_string());
        } else {
            std::fs::write(&path, text)?;
            installed.push(stage.to_string());
        }
    }
    Ok(serde_json::json!({ "dir": home.join("prompts"), "installed": installed, "kept": kept }))
}

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
        "critique-code" => CRITIQUE_CODE.to_string(),
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
    let code_review =
        stage == "critique" && current.index() >= plantool_core::Stage::Implementing.index();
    let t = template(home, if code_review { "critique-code" } else { stage });
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
    let extra_stage = match (stage, current) {
        (
            "resume",
            plantool_core::Stage::New
            | plantool_core::Stage::Researching
            | plantool_core::Stage::ResearchReview,
        ) => "research",
        ("resume", plantool_core::Stage::Planning | plantool_core::Stage::PlanReview) => "plan",
        ("resume", plantool_core::Stage::ImplementationReview | plantool_core::Stage::Done) => {
            "review"
        }
        ("resume", _) => "implement",
        (other, _) => other,
    };
    let rendered = with_extra(home, extra_stage, rendered);
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
    home: &Path,
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
    let mut prompt = with_extra(home, "implement", prompt);
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
    fn critique_after_implementation_reviews_the_code() {
        let p = render_at(
            Path::new("/nonexistent"),
            "critique",
            &session(None),
            Path::new("/d"),
            None,
            Stage::ImplementationReview,
        );
        assert!(
            p.contains("plantool changes repo/fix-it --scope all"),
            "{p}"
        );
        assert!(p.contains("difftool review comment add --review"), "{p}");
        assert!(p.contains("do not edit code"), "{p}");
    }

    #[test]
    fn extras_are_appended_and_installs_keep_user_edits() {
        let home = tempfile::tempdir().unwrap();
        let plain = render(home.path(), "plan", &session(None), Path::new("/d"), None);
        assert!(!plain.contains("Standing instructions"));
        let files = [
            ("plan", "Use ponytail.".to_string()),
            ("implement", "Use tdd.".to_string()),
        ];
        write_extras(home.path(), &files, false).unwrap();
        let p = render(home.path(), "plan", &session(None), Path::new("/d"), None);
        assert!(p.contains("plan.extra.md`:\nUse ponytail."), "{p}");
        assert!(
            p.find("Use ponytail.") < p.find("Current session context:"),
            "{p}"
        );

        std::fs::write(home.path().join("prompts/plan.extra.md"), "Mine.").unwrap();
        let result = write_extras(home.path(), &files, false).unwrap();
        assert_eq!(result["kept"], serde_json::json!(["plan", "implement"]));
        assert_eq!(
            std::fs::read_to_string(home.path().join("prompts/plan.extra.md")).unwrap(),
            "Mine."
        );
        let resumed = render_at(
            home.path(),
            "resume",
            &session(None),
            Path::new("/d"),
            None,
            Stage::Implementing,
        );
        assert!(resumed.contains("Use tdd."), "{resumed}");
        write_extras(home.path(), &files, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.path().join("prompts/plan.extra.md")).unwrap(),
            "Use ponytail."
        );
    }

    #[test]
    fn skill_files_point_at_installed_skills_only() {
        let home = tempfile::tempdir().unwrap();
        let mk = |rel: &str| {
            let dir = home.path().join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), "x").unwrap();
        };
        mk(".claude/skills/ponytail");
        mk(".claude/plugins/cache/market/pstack/0.1.0/skills/tdd");
        mk("downloaded/ponytail");
        mk("downloaded/why");
        let hints = "- `tdd` first\n- `ponytail` and `ponytail` again, `why`\n- `missing`, `~/.plantool`, `shortcut:`";
        let found = skill_files(home.path(), &home.path().join("downloaded"), hints);
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["tdd", "ponytail", "why"]);
        assert!(found[0].1.ends_with("pstack/0.1.0/skills/tdd/SKILL.md"));
        assert!(
            found[1].1.ends_with(".claude/skills/ponytail/SKILL.md"),
            "installed copy wins"
        );
        assert!(found[2].1.ends_with("downloaded/why/SKILL.md"));
    }

    #[test]
    fn extracts_only_wanted_skills_from_a_github_tarball() {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, body) in [
            ("plugins-main/pstack/skills/tdd/SKILL.md", "tdd"),
            (
                "plugins-main/pstack/skills/poteto-mode/playbooks/feature.md",
                "feature",
            ),
            ("plugins-main/pstack/skills/other/SKILL.md", "no"),
            ("plugins-main/other/skills/tdd/SKILL.md", "wrong place"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, body.as_bytes())
                .unwrap();
        }
        let archive = builder.into_inner().unwrap().finish().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dest.path().join("tdd")).unwrap();
        std::fs::write(dest.path().join("tdd/stale.md"), "old").unwrap();
        let wanted = vec!["tdd".to_string(), "poteto-mode".to_string()];
        let done = extract_skills(&archive[..], "pstack/skills", &wanted, dest.path()).unwrap();
        assert_eq!(done, ["tdd", "poteto-mode"]);
        assert_eq!(
            std::fs::read_to_string(dest.path().join("tdd/SKILL.md")).unwrap(),
            "tdd"
        );
        assert!(
            !dest.path().join("tdd/stale.md").exists(),
            "a refreshed skill drops old files"
        );
        assert!(dest
            .path()
            .join("poteto-mode/playbooks/feature.md")
            .is_file());
        assert!(!dest.path().join("other").exists());
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
