//! Outbox delivery never repeats an uncertain external launch.
use super::*;
use crate::{
    models::{lease, router},
    providers::{ProviderEvent, RunInput, RunOptions},
    AppState,
};
use std::sync::Arc;
use tokio::sync::mpsc;
/// Persisted messages are retried under their original idempotency key.
pub async fn deliver_worker(state: &AppState) -> anyhow::Result<()> {
    if !state.runs.worker.availability().available {
        return Ok(());
    }
    super::deliver_pending(
        &state.config.home,
        &[
            "worker.start",
            "worker.resume",
            "worker.cancel",
            "worker.recover",
        ],
        |kind, body| async move {
            let result = state
                .runs
                .worker
                .call(kind.strip_prefix("worker.").unwrap(), Some(&body))
                .await;
            match result {
                Ok(value) if value["state"] == "recovery_needed" => {
                    if let Some(item) = body["thread_id"].as_str() {
                        crate::recovery::freeze_uncertain_start(&state.config.home, item)?;
                    }
                    Ok(value)
                }
                Err(error)
                    if error
                        .downcast_ref::<crate::worker::CallError>()
                        .is_some_and(|e| {
                            e.status == 409
                                && e.detail.contains("Stale interrupt or cancelled graph")
                        }) =>
                {
                    Ok(json!({"ignored":"stale or cancelled interrupt"}))
                }
                other => other,
            }
        },
    )
    .await?;

    Ok(())
}
pub async fn tick(state: &AppState) -> anyhow::Result<()> {
    crate::recovery::announce_existing(&state.config.home)?;
    crate::recovery::announce_worker(
        &state.config.home,
        state.runs.worker.availability().available,
    )?;
    if let Err(error) = deliver_worker(state).await {
        tracing::warn!("Worker outbox delivery deferred: {error}");
    }
    if state.runs.execution.status().stopped {
        return Ok(());
    }
    let rows = {
        let db = database::open(&state.config.home)?;
        let mut q=db.prepare("SELECT a.id,d.request_json,d.execution_generation FROM attempts a JOIN dispatches d ON d.attempt_id=a.id WHERE a.state='queued' ORDER BY a.started_at,a.id LIMIT 32")?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, request, generation) in rows {
        let request: Dispatch = serde_json::from_str(&request)?;
        let claimed = {
            let mut db = database::open(&state.config.home)?;
            let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let changed = tx.execute(
                "UPDATE attempts SET state='starting' WHERE id=?1 AND state='queued'",
                [&id],
            )?;
            tx.execute("UPDATE outbox SET delivered_at=?2 WHERE aggregate_id=?1 AND kind='attempt.dispatch' AND delivered_at IS NULL",params![id,plantool_core::now()])?;
            tx.commit()?;
            changed == 1
        };
        if !claimed {
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = run(&state, &id, &request, generation).await {
                let result = RoleResult {
                    item: request.item.clone(),
                    role: request.role.clone(),
                    attempt_id: id,
                    generation: request.generation,
                    outcome: Outcome::Failed,
                    summary: format!("Dispatch stopped: {error}"),
                    tasks_done: vec![],
                    artifacts: vec![],
                    checks: vec![],
                    findings: vec![],
                    next: None,
                    review_guide: None,
                    details: None,
                };
                if let Err(error) = report(&state.config.home, &result) {
                    let _ = crate::workflow::recover(
                        &state.config.home,
                        &result.attempt_id,
                        &error.to_string(),
                    );
                    tracing::warn!("Failed dispatch requires recovery: {error}");
                }
            }
        });
    }
    Ok(())
}
pub fn handoff_base(home: &Path, request: &Dispatch, default: &str) -> anyhow::Result<String> {
    let db = database::open(home)?;
    let rows = {
        let mut q=db.prepare("SELECT a.outcome_json FROM attempts a JOIN dispatches d ON d.attempt_id=a.id WHERE a.item_id=?1 AND a.state='completed' AND a.generation<?2 AND json_extract(d.request_json,'$.plan_sha')=?3 ORDER BY a.generation DESC")?;
        let rows = q
            .query_map(
                params![request.item, request.generation, request.plan_sha],
                |r| r.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let (_, root) = item(&db, &request.item)?;
    for row in rows {
        let result: RoleResult = serde_json::from_str(&row)?;
        if let Some(artifact) = result.artifacts.iter().rev().find(|a| a.kind == "commit") {
            crate::git::git(
                &root,
                &[
                    "cat-file",
                    "-e",
                    &format!("{}^{{commit}}", artifact.reference),
                ],
            )?;
            return Ok(artifact.reference.clone());
        }
    }
    Ok(default.into())
}
fn worktree(home: &Path, id: &str, request: &Dispatch) -> anyhow::Result<PathBuf> {
    let db = database::open(home)?;
    let (_, root) = item(&db, &request.item)?;
    let dir = home.join("worktrees").join(id);
    std::fs::create_dir_all(dir.parent().unwrap())?;
    // A prior attempt can hand off its verified WIP commit. Graph-supplied arbitrary
    // refs are not shell-evaluated and are never accepted as an approval.
    let default = crate::git::git(&root, &["rev-parse", "HEAD"])?;
    let base = if crate::workflow::state(&db, &request.item)?.is_some() {
        db.query_row(
            "SELECT head_sha FROM workflow_items WHERE item_id=?1",
            [&request.item],
            |r| r.get::<_, String>(0),
        )?
    } else {
        handoff_base(home, request, &default)?
    };
    if crate::workflow::state(&db, &request.item)?.is_some() {
        crate::git::git(
            &root,
            &[
                "clone",
                "--no-hardlinks",
                "--no-checkout",
                &root.to_string_lossy(),
                &dir.to_string_lossy(),
            ],
        )?;
        crate::git::git(&dir, &["checkout", "--detach", &base])?;
        crate::git::git(&dir, &["config", "user.name", "PlanTool"])?;
        crate::git::git(&dir, &["config", "user.email", "plantool@localhost"])?;
        let hook=format!("#!/bin/sh\nexec git interpret-trailers --in-place --trailer {} --trailer {} --trailer {} \"$1\"\n",shlex::try_quote(&format!("PlanTool-Item: {}",request.item))?,shlex::try_quote(&format!("PlanTool-Plan: sha256:{}",request.plan_sha))?,shlex::try_quote(&format!("PlanTool-Attempt: {id}"))?);
        std::fs::write(dir.join(".git/hooks/commit-msg"), hook)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.join(".git/hooks/commit-msg"),
                std::fs::Permissions::from_mode(0o700),
            )?;
        }
        let sha = plantool_core::sha256_hex(&std::fs::read_to_string(dir.join(".git/config"))?);
        db.execute(
            "UPDATE dispatches SET git_config_sha=?2,base_sha=?3 WHERE attempt_id=?1",
            params![id, sha, base],
        )?;
        if let Some(target) = crate::workflow::state(&db, &request.item)?
            .and_then(|s| s["rebase_base"].as_str().map(str::to_owned))
        {
            let _ = crate::git::git(&dir, &["-c", "core.hooksPath=/dev/null", "rebase", &target]);
            db.execute(
                "UPDATE dispatches SET base_sha=?2 WHERE attempt_id=?1",
                params![id, target],
            )?;
        }
    } else {
        crate::git::git(
            &root,
            &["worktree", "add", "--detach", &dir.to_string_lossy(), &base],
        )?;
    }

    db.execute(
        "UPDATE attempts SET worktree=?2 WHERE id=?1",
        params![id, dir.to_string_lossy()],
    )?;
    Ok(dir)
}
fn checkpoint(root: &Path, policy: &crate::sandbox::NeverSent) -> anyhow::Result<Option<String>> {
    let tracked = crate::git::git(root, &["diff", "--name-only", "-z", "HEAD"])?;
    let untracked = crate::git::git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let paths = tracked
        .split('\0')
        .chain(untracked.split('\0'))
        .filter(|s| !s.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if paths.is_empty() {
        return Ok(None);
    }
    for path in &paths {
        policy.check(Path::new(""), Path::new(path))?;
    }
    let mut args = vec!["add", "--"];
    args.extend(paths);
    crate::git::git(root, &args)?;
    if crate::git::git(root, &["diff", "--cached", "--name-only"])?.is_empty() {
        return Ok(None);
    }
    crate::git::git(
        root,
        &[
            "-c",
            "user.name=PlanTool",
            "-c",
            "user.email=plantool@localhost",
            "commit",
            "-m",
            "WIP: checkpoint before changing model route",
        ],
    )?;
    Ok(Some(crate::git::git(root, &["rev-parse", "HEAD"])?))
}
async fn run(
    state: &AppState,
    id: &str,
    request: &Dispatch,
    generation: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        crate::sandbox::supported(),
        "Engineering OS coding runs require macOS sandbox-exec"
    );
    let home = &state.config.home;
    let db = database::open(home)?;
    let (project, root) = item(&db, &request.item)?;
    anyhow::ensure!(
        state.runs.execution.status().generation == generation,
        "Stop All invalidated the dispatch"
    );
    let selected = router::select(
        home,
        &project,
        &request.item,
        &request.role,
        database::unix_now(),
    )?;
    let read_call = !crate::workflow::coding(&request.role)
        || request.inputs["step"] == "discussion"
        || request.inputs["step"] == "sprint_plan"
        || crate::loop_engine::read_step(request.inputs["step"].as_str().unwrap_or(""));
    if read_call && selected.route.adapter != "codex" {
        let prompt = crate::instructions::assemble(home, request, id)?;
        database::open(home)?.execute(
            "UPDATE attempts SET state='running',model_route=?2 WHERE id=?1",
            params![id, selected.name],
        )?;
        database::open(home)?.execute(
            "UPDATE dispatches SET route_json=?2,same_family_warning=?3 WHERE attempt_id=?1",
            params![
                id,
                serde_json::to_string(&selected)?,
                selected.same_family_warning
            ],
        )?;
        database::open(home)?.execute(
            "INSERT INTO runs_summary(attempt_id,provider,model) VALUES(?1,?2,?3)",
            params![id, selected.route.provider, selected.route.model],
        )?;
        let call = plantool_contract::RoleCall {
            request: request.clone(),
            attempt_id: id.into(),
            prompt,
        };
        let started = std::time::Instant::now();
        let result = state
            .runs
            .worker
            .call("role", Some(&serde_json::to_value(call)?))
            .await;
        database::open(home)?.execute(
            "UPDATE runs_summary SET duration_ms=?2 WHERE attempt_id=?1",
            params![id, started.elapsed().as_millis() as u64],
        )?;
        let result = match result {
            Ok(value) => value,
            Err(error) => {
                // Role calls cannot write a worktree or publish. End this attempt,
                // cool down the failed route, then let a new generation select another.
                if error
                    .downcast_ref::<crate::worker::CallError>()
                    .is_some_and(|e| e.status >= 500)
                    && !state.runs.execution.status().stopped
                    && state.runs.execution.status().generation == generation
                {
                    router::exhaust(
                        home,
                        &selected,
                        Some(database::unix_now() + 3600),
                        database::unix_now(),
                    )?;
                    if router::select(
                        home,
                        &project,
                        &request.item,
                        &request.role,
                        database::unix_now(),
                    )
                    .is_ok()
                    {
                        database::open(home)?.execute(
                            "UPDATE dispatches SET quota_pending=1 WHERE attempt_id=?1",
                            [id],
                        )?;
                    }
                }
                return Err(error);
            }
        };
        let result: RoleResult = serde_json::from_value(result)?;
        report(home, &result)?;
        return Ok(());
    }
    let _lease = lease::acquire(
        home,
        &format!("attempt:{id}"),
        Some(id),
        lease::Model {
            route: selected.name.clone(),
            provider: selected.route.provider.clone(),
            model: selected.route.model.clone(),
        },
        db.query_row(
            "SELECT priority='urgent' FROM work_items WHERE id=?1",
            [&request.item],
            |r| r.get::<_, Option<bool>>(0),
        )?
        .unwrap_or(false),
        db.query_row(
            "SELECT queue_order FROM work_items WHERE id=?1",
            [&request.item],
            |r| r.get(0),
        )?,
        state.runs.execution.clone(),
        &lease::Ollama,
    )
    .await?;
    anyhow::ensure!(
        state.runs.execution.status().generation == generation,
        "Execution changed while queued"
    );
    let current = router::select(
        home,
        &project,
        &request.item,
        &request.role,
        database::unix_now(),
    )?;
    anyhow::ensure!(
        current.name == selected.name,
        "Route budget or availability changed while queued; retry the attempt"
    );
    let dir = worktree(home, id, request)?;
    let policy =
        crate::sandbox::NeverSent::new(&state.runs.execution.rules(&crate::git::main_root(&root)))?;
    let sandbox = crate::sandbox::Sandbox::required(
        crate::sandbox::Paths {
            owner_home: std::env::var_os("HOME")
                .ok_or_else(|| anyhow::anyhow!("Owner home unavailable"))?
                .into(),
            daemon_home: home.clone(),
            worktree: dir.clone(),
            docs: dir.join("docs"),
        },
        if read_call || request.role == "reviewer" {
            crate::sandbox::Role::EosReviewer
        } else if request.role == "qa" && crate::workflow::state(&db, &request.item)?.is_some() {
            crate::sandbox::Role::EosQa
        } else if request.role == "qa" {
            crate::sandbox::Role::EosReviewer
        } else {
            crate::sandbox::Role::EosAgent
        },
        policy.clone(),
    )?;
    let mut launch = crate::sandbox::launch::Launch::new(
        state.runs.execution.clone(),
        generation,
        Some(sandbox),
        policy.clone(),
    )?;
    Arc::get_mut(&mut launch).unwrap().attempt = Some((home.clone(), id.into()));
    {
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE attempts SET state='running',model_route=?2 WHERE id=?1",
            params![id, selected.name],
        )?;
        tx.execute("UPDATE dispatches SET route_json=?2,deadline=?3,same_family_warning=?4 WHERE attempt_id=?1",params![id,serde_json::to_string(&selected)?,database::unix_now()+2700,selected.same_family_warning])?;
        tx.execute(
            "INSERT INTO runs_summary(attempt_id,provider,model) VALUES(?1,?2,?3)",
            params![id, selected.route.provider, selected.route.model],
        )?;
        tx.execute("INSERT INTO role_families VALUES(?1,?2,?3) ON CONFLICT(item_id,role) DO UPDATE SET family=excluded.family",params![request.item,request.role,router::family(&selected.route.model,&selected.route.provider)])?;
        tx.commit()?;
    }

    if selected.route.adapter == "codex" {
        router::count(home, &selected, 1, 0, database::unix_now())?;
    }
    let credential = state.runs.tokens.issue(crate::identities::RunScope {
        session: request.item.clone(),
        attempt: id.into(),
    });
    let environment = credential
        .environment(home.clone(), state.config.port)
        .with_proxy(format!("http://localhost:{}/v1", state.config.port));
    let plan = if request.plan_sha.is_empty() {
        String::new()
    } else {
        String::from_utf8(crate::artifacts::read(home, &request.plan_sha)?)?
    };
    let mut prompt = if crate::workflow::state(&db, &request.item)?.is_some() {
        crate::instructions::assemble(home, request, id)?
    } else {
        format!("You are the {} role. Immutable approved plan:\n{plan}\nReport the typed RoleResult for item={}, attempt_id={}, generation={}. Never approve or publish.",request.role,request.item,id,request.generation)
    };
    if read_call {
        prompt.push_str("\nReturn a single RoleResult JSON object as your final answer. Do not write files or invoke approval/publishing commands.\n");
        prompt.push_str(&plantool_contract::result_schema());
    }
    let provider = if selected.route.adapter == "codex" {
        plantool_core::Provider::Codex
    } else {
        plantool_core::Provider::Ollama
    };
    let opts = RunOptions {
        environment: Some(environment),
        launch: Some(launch),
        cwd: dir.clone(),
        prompt,
        model: Some(selected.route.model.clone()),
        effort: None,
        resume: None,
        executable: crate::providers::executable_for(provider),
        writable_roots: if read_call { vec![] } else { vec![dir.clone()] },
        permission_mode: plantool_core::PermissionMode::Auto,
    };
    let (input, rx) = mpsc::channel::<RunInput>(16);
    let (events, mut ev) = mpsc::channel::<ProviderEvent>(1024);
    let mut provider = tokio::spawn(crate::providers::run_provider(provider, opts, rx, events));
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(2700));
    tokio::pin!(deadline);
    let mut seq = 0;
    let mut quota = false;
    let started = std::time::Instant::now();
    let mut error = None;
    let mut joined = false;
    let mut role_reply: Option<String> = None;
    loop {
        tokio::select! {
              _=&mut deadline=>{error=Some("45 minute checkpoint limit reached".to_string());break;}
              result=&mut provider=>{joined=true;match result{Ok(Err(e))=>error=Some(e.to_string()),Err(e)=>error=Some(e.to_string()),_=>{}}break;}
              _=tokio::time::sleep(std::time::Duration::from_millis(100))=>{let pending:bool=database::open(home)?.query_row("SELECT quota_pending FROM dispatches WHERE attempt_id=?1",[id],|r|r.get(0))?;if pending{quota=true;error=Some("Quota reached; checkpoint required before changing routes".into());break}let completed:bool=database::open(home)?.query_row("SELECT state NOT IN('starting','running') FROM attempts WHERE id=?1",[id],|r|r.get(0))?;if completed{break}}
        event=ev.recv()=>if let Some(event)=event{
                if read_call {
                    if let ProviderEvent::Message{role,content,..}=&event {if role=="assistant" { role_reply=Some(content.clone()); }}
                    if let ProviderEvent::TurnCompleted{status,error:turn_error,..}=&event {
                        if turn_error.is_none() && status=="completed" {
                            let parsed=role_reply.as_deref().ok_or_else(||anyhow::anyhow!("Codex produced no final JSON")).and_then(|text|read_result(text,request,id));
                            match parsed.and_then(|result|report(home,&result).map(|_|())) {Ok(())=>{},Err(e)=>{error=Some(format!("Invalid Codex role result: {e}"));break;}}
                        }
                    }
                }
                seq+=1;let value=serde_json::to_value(&event)?;quota|=match &event {ProviderEvent::TurnCompleted{error:Some(error),..}=>router::quota(200,error),ProviderEvent::Raw{line}=>serde_json::from_str::<Value>(line).ok().is_some_and(|v|!v["error"].is_null()&&router::quota(200,line)),_=>false};
                let mut db=database::open(home)?;let tx=db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let bytes=format!("{value}\n");let sha=artifacts::put(home,bytes.as_bytes())?;artifacts::register(&tx,&sha,bytes.len())?;
                tx.execute("INSERT INTO run_events(attempt_id,seq,content_sha256) VALUES(?1,?2,?3)",params![id,seq,sha])?;
                tx.execute("INSERT INTO artifact_refs VALUES('transcript',?1,?2,?3,?4)",params![id,seq.to_string(),sha,database::unix_now()])?;tx.commit()?;
                if quota {error=Some("Provider quota exhausted; checkpoint before the next attempt".into());break;}
                let completed:bool=database::open(home)?.query_row("SELECT state NOT IN('starting','running') FROM attempts WHERE id=?1",[id],|r|r.get(0))?;if completed{break}
              }else{break;}
             }
    }
    let _ = input.send(RunInput::Stop).await;
    // Dropping/aborting the provider future drops ManagedChild and kills its group.
    if !provider.is_finished() {
        provider.abort();
    }
    if !joined {
        let _ = provider.await;
    }
    drop(credential);
    database::open(home)?.execute(
        "UPDATE runs_summary SET duration_ms=?2 WHERE attempt_id=?1",
        params![id, started.elapsed().as_millis() as u64],
    )?;
    let active: bool = database::open(home)?.query_row(
        "SELECT state IN('starting','running') FROM attempts WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    if active {
        if read_call
            && !state.runs.execution.status().stopped
            && state.runs.execution.status().generation == generation
        {
            router::exhaust(
                home,
                &selected,
                Some(database::unix_now() + 3600),
                database::unix_now(),
            )?;
            if router::select(
                home,
                &project,
                &request.item,
                &request.role,
                database::unix_now(),
            )
            .is_ok()
            {
                database::open(home)?.execute(
                    "UPDATE dispatches SET quota_pending=1 WHERE attempt_id=?1",
                    [id],
                )?;
            }
        }
        if error.is_some() && !quota {
            router::provider_failed(home, &selected, database::unix_now())?;
        }
        quota |= error.as_ref().is_some_and(|e| router::quota(200, e));
        if quota {
            let recorded: bool = database::open(home)?.query_row(
                "SELECT EXISTS(SELECT 1 FROM budgets WHERE route=?1 AND exhausted_until>?2)",
                params![selected.name, database::unix_now()],
                |r| r.get(0),
            )?;
            if !recorded {
                router::exhaust(home, &selected, None, database::unix_now())?;
            }
        }
        let mut result = RoleResult {
            item: request.item.clone(),
            role: request.role.clone(),
            attempt_id: id.into(),
            generation: request.generation,
            outcome: Outcome::NeedsFix,
            summary: error.unwrap_or_else(|| {
                "Provider exited without a typed result; review its transcript before retrying"
                    .into()
            }),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: Some(request.role.clone()),
            review_guide: None,
            details: None,
        };
        match checkpoint(&dir, &policy) {
            Ok(Some(commit)) => result.artifacts.push(plantool_contract::Artifact {
                kind: "commit".into(),
                reference: commit,
            }),
            Ok(None) => {}
            Err(error) => {
                result.outcome = Outcome::Failed;
                result.summary = format!("{}; checkpoint failed: {error}", result.summary);
            }
        }
        if let Err(error) = report(home, &result) {
            crate::workflow::recover(home, id, &error.to_string())?;
            return Err(error);
        }
    }
    Ok(())
}

fn read_result(text: &str, request: &Dispatch, id: &str) -> anyhow::Result<RoleResult> {
    let mut value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|_| anyhow::anyhow!("Final answer is not JSON"))?;
    anyhow::ensure!(value.is_object(), "Final answer is not an object");
    value["item"] = json!(request.item);
    value["role"] = json!(request.role);
    value["attempt_id"] = json!(id);
    value["generation"] = json!(request.generation);
    serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("Final JSON does not satisfy RoleResult"))
}

#[cfg(test)]
mod read_result_tests {
    use super::*;
    #[test]
    fn codex_read_claims_are_scoped_and_invalid_output_is_not_exposed() {
        let request = Dispatch {
            item: "i".into(),
            role: "pm".into(),
            generation: 2,
            inputs: json!({"step":"triage"}),
            plan_sha: "".into(),
            policy_version: 1,
            capability: "read".into(),
            thread_id: "i".into(),
            interrupt_id: "x".into(),
            idempotency_key: "d".into(),
        };
        let text = r#"{"item":"forged","role":"backend","attempt_id":"bad","generation":9,"outcome":"done","summary":"Triage","tasks_done":[],"artifacts":[],"checks":[],"findings":[],"next":null,"review_guide":null,"details":{"kind":"triage","disposition":"fits","reason":"small docs task","duplicate_of":null}}"#;
        let result = read_result(text, &request, "a").unwrap();
        assert_eq!(result.item, "i");
        assert_eq!(result.role, "pm");
        assert_eq!(result.generation, 2);
        let error = read_result("SECRET invalid JSON", &request, "a").unwrap_err();
        assert!(!error.to_string().contains("SECRET"));
    }
}
