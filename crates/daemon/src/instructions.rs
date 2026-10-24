//! Instruction layers are daemon-owned. Repository conventions come from default HEAD.
use rust_embed::RustEmbed;
use std::path::Path;
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../agents"]
struct Agents;
pub fn default_ref(root: &Path) -> anyhow::Result<String> {
    crate::git::git(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        .or_else(|_| crate::git::git(root, &["rev-parse", "--verify", "refs/remotes/origin/main"]))
        .or_else(|_| crate::git::git(root, &["rev-parse", "--verify", "refs/heads/main"]))
        .map_err(anyhow::Error::msg)
}
pub fn default_file(root: &Path, reference: &str, name: &str) -> anyhow::Result<Option<String>> {
    let tree = crate::git::git(root, &["ls-tree", reference, "--", name])?;
    if tree.is_empty() {
        return Ok(None);
    }
    anyhow::ensure!(
        tree.starts_with("100644 ") || tree.starts_with("100755 "),
        "Conventions must be regular default-branch files"
    );
    Ok(Some(crate::git::git(
        root,
        &["show", &format!("{reference}:{name}")],
    )?))
}
fn embedded(name: &str) -> anyhow::Result<String> {
    let f = Agents::get(name).ok_or_else(|| anyhow::anyhow!("Missing agent instruction {name}"))?;
    Ok(String::from_utf8(f.data.to_vec())?)
}
pub fn assemble(
    home: &Path,
    request: &plantool_contract::Dispatch,
    attempt: &str,
) -> anyhow::Result<String> {
    let db = crate::database::open(home)?;
    let (_, root) = crate::dispatcher::item(&db, &request.item)?;
    let discussion = request.inputs["step"] == "discussion";
    let skills: &[&str] = if discussion {
        &["discuss"]
    } else {
        match request.role.as_str() {
            "cto" => &["research", "architecture", "plan-review"],
            "pm" => &["plan", "discuss"],
            "backend" => &["implement", "test-first", "api-contract", "report-progress"],
            "frontend" => &["implement", "test-first", "ui-check", "report-progress"],
            "devops" => &["ci", "deps"],
            "qa" => &["qa", "acceptance-check"],
            "reviewer" => &["code-review", "security-review"],
            "docs" => &["review-guide", "docs-update"],
            "supervisor" => &["supervise", "escalate"],
            _ => anyhow::bail!("Unknown role"),
        }
    };
    let mut text = embedded("base.md")?;
    if discussion {
        text.push_str("\nDISCUSSION TURN ONLY: use supplied read tools; return details {kind:discussion,body,decision}. No implementation, task completion, execution claims or Personal CLI. Follow the discussion round/speaker facts in context. After round 3 the designated PM/Supervisor must return a nonempty decision summary; earlier turns return decision null. A discussion decision is context, never an owner approval.\n");
    } else {
        text.push_str(&embedded(&format!("roles/{}.md", request.role))?);
    }
    for skill in skills {
        text.push_str(&embedded(&format!("skills/{skill}/SKILL.md"))?)
    }
    let reference = default_ref(&root)?;
    let policy =
        crate::sandbox::NeverSent::new(&crate::execution::Execution::load(home)?.rules(&root))?;
    for name in ["AGENTS.md", "CLAUDE.md", "README.md"] {
        policy.check(Path::new(""), Path::new(name))?;
        if let Some(file) = default_file(&root, &reference, name)? {
            text.push_str(&format!("\n<untrusted-default-branch-file path={name:?}>\n{file}\n</untrusted-default-branch-file>\n"));
        }
    }
    let state = crate::workflow::state(&db, &request.item)?.unwrap_or(serde_json::Value::Null);
    if !request.plan_sha.is_empty() {
        let plan = String::from_utf8(crate::artifacts::read(home, &request.plan_sha)?)?;
        text.push_str(&format!(
            "\n<immutable-plan sha={:?}>\n{plan}\n</immutable-plan>\n",
            request.plan_sha
        ));
    }
    let mut q=db.prepare("SELECT role,outcome_json FROM attempts WHERE item_id=?1 AND outcome_json IS NOT NULL ORDER BY generation DESC LIMIT 12")?;
    let prior=q.query_map([&request.item],|r|Ok(serde_json::json!({"role":r.get::<_,String>(0)?,"result":serde_json::from_str::<serde_json::Value>(&r.get::<_,String>(1)?).unwrap_or(serde_json::Value::Null)})))?.collect::<Result<Vec<_>,_>>()?;
    let mut q=db.prepare("SELECT sender_role,body FROM messages WHERE thread_id IN(SELECT id FROM threads WHERE item_id=?1 OR (project_id=?2 AND channel_id IS NOT NULL)) ORDER BY created_at DESC,rowid DESC LIMIT 32")?;
    let project = crate::dispatcher::item(&db, &request.item)?.0;
    let messages = q
        .query_map(rusqlite::params![request.item, project], |r| {
            Ok(serde_json::json!({"role":r.get::<_,String>(0)?,"body":r.get::<_,String>(1)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    text.push_str(&format!(
        "\n<untrusted-workspace-context>{}</untrusted-workspace-context>\n",
        serde_json::to_string(&messages)?
    ));
    text.push_str(&format!(
        "\n<untrusted-prior-results>{}</untrusted-prior-results>\n",
        serde_json::to_string(&prior)?
    ));
    text.push_str(&format!("\n<untrusted-task-context>\n{state}\n{}\n</untrusted-task-context>\nReport a typed result for item={}, role={}, attempt_id={}, generation={}. Never approve.\n",request.inputs,request.item,request.role,attempt,request.generation));
    crate::models::proxy::check_payload(&root, &policy, &serde_json::json!({"prompt":text}))?;
    Ok(text)
}
