use super::{
    gateway::{Gateway, Reply},
    lease, router,
};
use crate::{artifacts, database};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
/// Inspect the payload at the enforcement point, including copied protected
/// file contents; sandbox file denials are an independent earlier boundary.
pub fn check_payload(
    root: &Path,
    policy: &crate::sandbox::NeverSent,
    body: &Value,
) -> anyhow::Result<()> {
    fn strings(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(s) => out.push(s.clone()),
            Value::Array(a) => {
                for v in a {
                    strings(v, out)
                }
            }
            Value::Object(m) => {
                for v in m.values() {
                    strings(v, out)
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    strings(body, &mut text);
    fn scan(path: &Path, text: &[String]) -> anyhow::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_symlink() {
            anyhow::bail!("Protected path is a symlink; refusing proxy payload")
        }
        if meta.is_dir() {
            for file in std::fs::read_dir(path)? {
                scan(&file?.path(), text)?;
            }
        } else if meta.is_file() {
            anyhow::ensure!(
                meta.len() <= 16 * 1024 * 1024,
                "Protected file too large to screen safely"
            );
            let bytes = std::fs::read(path)?;
            if !bytes.is_empty() {
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let plain = String::from_utf8_lossy(&bytes);
                anyhow::ensure!(
                    !text
                        .iter()
                        .any(|s| s.contains(plain.as_ref()) || s.contains(&encoded)),
                    "Proxy payload contains never-sent file content"
                );
            }
        }
        Ok(())
    }
    for path in policy.protected_targets(root)? {
        anyhow::ensure!(
            path.starts_with(root.canonicalize()?),
            "Protected alias leaves the worktree"
        );
        scan(&path, &text)?;
    }
    Ok(())
}
fn usage(body: &str) -> (u64, u64) {
    let values = std::iter::once(body).chain(body.lines().filter_map(|s| s.strip_prefix("data: ")));
    let mut input = 0;
    let mut output = 0;
    for text in values {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            input = input.max(value["usage"]["prompt_tokens"].as_u64().unwrap_or(0));
            output = output.max(value["usage"]["completion_tokens"].as_u64().unwrap_or(0));
        }
    }
    (input, output)
}
async fn fenced_chat(
    gateway: &dyn Gateway,
    provider: &str,
    body: Value,
    execution: &crate::execution::Execution,
    generation: u64,
) -> anyhow::Result<Reply> {
    let future = gateway.chat(provider, body);
    tokio::pin!(future);
    loop {
        tokio::select! {reply=&mut future=>return reply,_=tokio::time::sleep(std::time::Duration::from_millis(50))=>{let status=execution.status();anyhow::ensure!(!status.stopped&&status.generation==generation,"Stop All cancelled the model request");}}
    }
}
pub async fn complete(
    home: &Path,
    owner: &str,
    context: &plantool_contract::LlmContext,
    mut body: Value,
    execution: Arc<crate::execution::Execution>,
    gateway: &dyn Gateway,
    lifecycle: &dyn lease::Lifecycle,
) -> anyhow::Result<Reply> {
    anyhow::ensure!(!execution.status().stopped, "Stop All is active");
    let db = database::open(home)?;
    let (project, root): (String, std::path::PathBuf) = if context.attempt_id.is_some() {
        db.query_row("SELECT i.project_id,p.local_checkout FROM work_items i JOIN projects p ON p.id=i.project_id WHERE i.id=?1 AND i.deleted_at IS NULL",[&context.item],|r|Ok((r.get(0)?,std::path::PathBuf::from(r.get::<_,String>(1)?))))?
    } else {
        crate::dispatcher::item(&db, &context.item)?
    };
    let policy = crate::sandbox::NeverSent::new(&execution.rules(&crate::git::main_root(&root)))?;
    check_payload(&root, &policy, &body)?;
    anyhow::ensure!(
        body["messages"].is_array(),
        "OpenAI messages must be an array"
    );
    let (selected, lease, attempt) = if let Some(attempt) = &context.attempt_id {
        let (item,role,generation,route):(String,String,u64,Option<String>)=db.query_row("SELECT a.item_id,a.role,a.generation,d.route_json FROM attempts a LEFT JOIN dispatches d ON d.attempt_id=a.id WHERE a.id=?1 AND a.state IN('starting','running')",[attempt],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        anyhow::ensure!(
            item == context.item && role == context.role && generation == context.generation,
            "Proxy attempt scope mismatch"
        );
        anyhow::ensure!(
            lease::owns(home, owner)?,
            "Caller does not hold the coding-run lease"
        );
        let selected: router::Selected = if let Some(route) = route {
            serde_json::from_str(&route)?
        } else {
            let model: String = db.query_row(
                "SELECT model FROM runs_summary WHERE attempt_id=?1 AND provider='ollama'",
                [attempt],
                |r| r.get(0),
            )?;
            router::Selected {
                name: "personal:ollama".into(),
                route: router::Route {
                    adapter: "opencode".into(),
                    provider: "ollama".into(),
                    model,
                    budget: None,
                },
                same_family_warning: false,
            }
        };
        anyhow::ensure!(
            router::allowed(&db, &project)?.contains(&selected.route.provider),
            "Provider is disallowed"
        );
        (selected, None, Some(attempt.clone()))
    } else {
        let current: u64 = db.query_row(
            "SELECT coalesce((SELECT generation FROM item_generations WHERE item_id=?1),0)",
            [&context.item],
            |r| r.get(0),
        )?;
        anyhow::ensure!(current == context.generation, "Stale LLM generation");
        let selected = router::select(
            home,
            &project,
            &context.item,
            &context.role,
            database::unix_now(),
        )?;
        anyhow::ensure!(
            selected.route.adapter != "codex",
            "Codex subscription routes require the app-server adapter"
        );
        let lease = lease::acquire(
            home,
            owner,
            None,
            lease::Model {
                route: selected.name.clone(),
                provider: selected.route.provider.clone(),
                model: selected.route.model.clone(),
            },
            false,
            i64::MAX,
            execution.clone(),
            lifecycle,
        )
        .await?;
        (selected, Some(lease), None)
    };
    anyhow::ensure!(
        selected.route.adapter != "codex",
        "Codex cannot be called through HTTP"
    );
    let id = format!("proxy:{}:{}", context.item, context.idempotency_key);
    let digest = plantool_core::sha256_hex(&body.to_string());
    let previous: Option<(String, String, Option<String>)> = db
        .query_row(
            "SELECT request_digest,state,response_sha FROM proxy_calls WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((original, status, sha)) = previous {
        anyhow::ensure!(
            original == digest,
            "LLM idempotency key reused for changed input"
        );
        anyhow::ensure!(
            status == "completed",
            "Previous model outcome is uncertain; refusing replay"
        );
        let value: Value = serde_json::from_slice(&artifacts::read(home, &sha.unwrap())?)?;
        return Ok(Reply {
            status: value["status"].as_u64().unwrap() as u16,
            body: value["body"].as_str().unwrap().into(),
            content_type: value["content_type"].as_str().unwrap().into(),
            reset: None,
        });
    }
    db.execute(
        "INSERT INTO proxy_calls VALUES(?1,?2,?3,'pending',NULL,?4)",
        params![id, attempt, digest, database::unix_now()],
    )?;
    body.as_object_mut().unwrap().remove("plantool");
    body["model"] = json!(selected.route.model);
    router::count(home, &selected, 1, 0, database::unix_now())?;
    let execution_generation = execution.status().generation;
    let mut reply = fenced_chat(
        gateway,
        &selected.route.provider,
        body.clone(),
        &execution,
        execution_generation,
    )
    .await;
    if reply.as_ref().is_err() || reply.as_ref().is_ok_and(|r| r.status >= 500) {
        if !execution.status().stopped {
            router::count(home, &selected, 1, 0, database::unix_now())?;
            reply = fenced_chat(
                gateway,
                &selected.route.provider,
                body,
                &execution,
                execution_generation,
            )
            .await;
        }
    }
    let reply = match reply {
        Ok(reply) => reply,
        Err(error) => {
            router::exhaust(home, &selected, None, database::unix_now())?;
            return Err(error);
        }
    };
    if router::quota(reply.status, &reply.body) || reply.status >= 500 {
        router::exhaust(home, &selected, reply.reset, database::unix_now())?;
    }
    let (input, output) = usage(&reply.body);
    router::count(home, &selected, 0, input + output, database::unix_now())?;
    if let Some(attempt) = attempt {
        db.execute("UPDATE runs_summary SET requests=requests+1,tokens_in=tokens_in+?2,tokens_out=tokens_out+?3 WHERE attempt_id=?1",params![attempt,input,output])?;
    }
    let cached = serde_json::to_vec(
        &json!({"status":reply.status,"body":reply.body,"content_type":reply.content_type}),
    )?;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let sha = artifacts::put(home, &cached)?;
    artifacts::register(&tx, &sha, cached.len())?;
    tx.execute(
        "INSERT INTO artifact_refs VALUES('trace',?1,?2,?3,?4)",
        params![
            context.attempt_id.as_deref().unwrap_or(&context.item),
            id,
            sha,
            database::unix_now()
        ],
    )?;
    tx.execute(
        "UPDATE proxy_calls SET state='completed',response_sha=?2 WHERE id=?1",
        params![id, sha],
    )?;
    database::audit(
        &tx,
        "daemon",
        "llm.completed",
        &id,
        &json!({"route":selected,"tokens_in":input,"tokens_out":output}),
    )?;
    tx.commit()?;
    drop(lease);
    Ok(reply)
}
