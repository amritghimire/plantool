//! Worker capabilities have no browser, Personal or GitHub authority.
use super::{engineering, ApiError};
use crate::{database, dispatcher, AppState};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Extension, Json, Router,
};
use plantool_contract::*;
use rusqlite::params;
use serde_json::{json, Value};
macro_rules! ensure {($condition:expr,$($message:tt)*)=>{if !$condition{return Err(anyhow::anyhow!($($message)*).into())}}}
fn worker(
    state: &AppState,
    headers: &HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
) -> Result<(), ApiError> {
    engineering::worker(state, headers, socket.as_deref())
}
async fn task_done(
    State(state): State<AppState>,
    authorized: Option<Extension<crate::identities::AuthorizedRun>>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let auth = authorized.ok_or_else(|| {
        ApiError(
            StatusCode::FORBIDDEN,
            "Scoped attempt token required".into(),
        )
    })?;
    ensure!(body["item"] == auth.scope.session, "Task item mismatch");
    ensure!(!state.runs.execution.status().stopped, "Stop All is active");
    if let Some(s) =
        crate::workflow::state(&database::open(&state.config.home)?, &auth.scope.session)?
    {
        ensure!(
            crate::oversight::active_key(&s, body["key"].as_str().unwrap_or("")),
            "Task is outside the current phase"
        );
    }
    let key = body["key"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing task key"))?;
    let mut db = database::open(&state.config.home)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts a JOIN work_items i ON i.id=a.item_id JOIN plan_tasks t ON t.revision_id=i.approved_plan_revision_id WHERE a.id=?1 AND a.item_id=?2 AND a.state='running' AND t.task_key=?3 AND t.area=a.role AND coalesce(json_extract(i.state_json,'$.plan_revision_pending'),0)=0)",params![auth.scope.attempt,auth.scope.session,key],|r|r.get(0)).map_err(anyhow::Error::from)?;
    ensure!(valid, "Task is not approved for this role");
    tx.execute("INSERT INTO task_progress VALUES(?1,?2,'done',?3,?4) ON CONFLICT(item_id,task_key) DO UPDATE SET status='done',updated_by_attempt=excluded.updated_by_attempt,updated_at=excluded.updated_at",params![auth.scope.session,key,auth.scope.attempt,plantool_core::now()]).map_err(anyhow::Error::from)?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok(Json(json!({"ok":true})))
}
async fn workflow_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<WorkflowContext>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    Ok(Json(crate::workflow::context(
        &state.config.home,
        &request.item,
    )?))
}
async fn workflow_pr(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<WorkflowPr>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    Ok(Json(crate::workflow::pr_request(
        &state.config.home,
        &request,
    )?))
}
async fn store_artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    authorized: Option<Extension<crate::identities::AuthorizedRun>>,
    Json(request): Json<StoreArtifact>,
) -> Result<Json<Value>, ApiError> {
    if let Some(auth) = authorized {
        ensure!(
            auth.scope.attempt == request.attempt_id && auth.scope.session == request.item,
            "Artifact scope mismatch"
        );
    } else {
        worker(&state, &headers, socket)?;
    }
    let db = database::open(&state.config.home)?;
    let valid:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND item_id=?2 AND state IN('starting','running'))",params![request.attempt_id,request.item],|r|r.get(0)).map_err(anyhow::Error::from)?;
    ensure!(valid, "Inactive artifact attempt");
    let (_, root) = dispatcher::item(&db, &request.item)?;
    crate::models::proxy::check_payload(
        &root,
        &crate::sandbox::NeverSent::new(&state.runs.execution.rules(&root))?,
        &json!({"content":request.content}),
    )?;
    let sha = crate::artifacts::put(&state.config.home, request.content.as_bytes())?;
    crate::artifacts::register(&db, &sha, request.content.len())?;
    db.execute(
        "INSERT OR IGNORE INTO artifact_refs VALUES('attempt',?1,?2,?3,?4)",
        params![
            request.attempt_id,
            format!("evidence:{sha}"),
            sha,
            crate::database::unix_now()
        ],
    )
    .map_err(anyhow::Error::from)?;
    Ok(Json(json!({"reference":format!("artifact://{sha}")})))
}
async fn dispatch(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<Dispatch>,
) -> Result<Json<DispatchReply>, ApiError> {
    worker(&state, &headers, socket)?;
    let admission = state.runs.execution.admit()?;
    Ok(Json(
        dispatcher::admit(&state.config.home, &request, admission.generation())
            .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?,
    ))
}
async fn report(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    authorized: Option<Extension<crate::identities::AuthorizedRun>>,
    Json(result): Json<RoleResult>,
) -> Result<Json<Value>, ApiError> {
    if let Some(authorized) = authorized {
        if authorized.scope.attempt != result.attempt_id {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "Result belongs to another attempt".into(),
            ));
        }
    } else {
        worker(&state, &headers, socket)?;
    }
    let value = dispatcher::report(&state.config.home, &result)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(value))
}
async fn read_artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<ReadArtifact>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    let db = database::open(&state.config.home)?;
    dispatcher::item(&db, &request.item)?;
    let bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM revisions WHERE item_id=?1 AND content_sha256=?2) OR EXISTS(SELECT 1 FROM artifact_refs r LEFT JOIN attempts a ON a.id=r.owner_id WHERE r.sha256=?2 AND (r.owner_id=?1 OR a.item_id=?1))",params![request.item,request.sha256],|r|r.get(0)).map_err(anyhow::Error::from)?;
    if !bound {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Artifact is outside this item".into(),
        ));
    }
    let bytes = crate::artifacts::read(&state.config.home, &request.sha256)?;
    Ok(Json(
        json!({"sha256":request.sha256,"content":String::from_utf8(bytes).map_err(anyhow::Error::from)?}),
    ))
}
async fn read_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<ReadTask>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    let db = database::open(&state.config.home)?;
    dispatcher::item(&db, &request.item)?;
    let value=db.query_row("SELECT t.title,t.area,t.phase,t.review_tag,coalesce(p.status,'pending') FROM work_items i JOIN plan_tasks t ON t.revision_id=i.approved_plan_revision_id LEFT JOIN task_progress p ON p.item_id=i.id AND p.task_key=t.task_key WHERE i.id=?1 AND t.task_key=?2",params![request.item,request.key],|r|Ok(json!({"key":request.key,"title":r.get::<_,String>(0)?,"area":r.get::<_,Option<String>>(1)?,"phase":r.get::<_,Option<String>>(2)?,"review_tag":r.get::<_,bool>(3)?,"status":r.get::<_,String>(4)?}))).map_err(anyhow::Error::from)?;
    Ok(Json(value))
}
async fn read_repo_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<ReadRepoFile>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    let db = database::open(&state.config.home)?;
    let (_, root) = dispatcher::item(&db, &request.item)?;
    let relative = std::path::Path::new(&request.path);
    ensure!(
        !relative.is_absolute()
            && !relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            && !request.path.contains(['\0', '\n', '\r', ':']),
        "Invalid repository path"
    );
    let policy =
        crate::sandbox::NeverSent::new(&state.runs.execution.rules(&crate::git::main_root(&root)))?;
    policy.check(std::path::Path::new(""), relative)?;
    if root.join(relative).exists() {
        crate::sandbox::read_guard::check(
            &crate::sandbox::Paths {
                owner_home: std::env::var_os("HOME").unwrap_or_default().into(),
                daemon_home: state.config.home.clone(),
                worktree: root.clone(),
                docs: root.clone(),
            },
            &policy,
            relative,
        )?;
    }
    let default = crate::git::git(&root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        .or_else(|_| crate::git::git(&root, &["rev-parse", "--verify", "refs/heads/main"]))
        .or_else(|_| crate::git::git(&root, &["rev-parse", "--verify", "refs/heads/master"]))
        .map_err(anyhow::Error::from)?;
    let tree = crate::git::git(&root, &["ls-tree", &default, "--", &request.path])
        .map_err(anyhow::Error::from)?;
    ensure!(
        tree.starts_with("100644 ") || tree.starts_with("100755 "),
        "Only regular default-branch files can be read"
    );
    let content = crate::git::git(&root, &["show", &format!("{default}:{}", request.path)])
        .map_err(anyhow::Error::from)?;
    crate::models::proxy::check_payload(&root, &policy, &json!({"content":content}))?;
    Ok(Json(
        json!({"path":request.path,"ref":default,"content":content}),
    ))
}
async fn gate(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<Gate>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    let db = database::open(&state.config.home)?;
    dispatcher::item(&db, &request.item)?;
    ensure!(
        [
            "research",
            "plan",
            "milestone",
            "pr",
            "sprint",
            "production"
        ]
        .contains(&request.kind.as_str()),
        "Invalid gate kind"
    );
    let digest =
        plantool_core::sha256_hex(&serde_json::to_string(&request).map_err(anyhow::Error::from)?);
    let value = database::inbox_once(
        &state.config.home,
        "worker.gate",
        &request.idempotency_key,
        |tx| {
            if request.kind == "milestone" {
                let id = crate::github::milestones::request(tx, &request)?;
                return Ok(json!({"gate_id":id,"request_digest":digest}));
            }
            if request.kind == "plan" || request.kind == "research" {
                let id = crate::github::gates::request_revision(
                    tx,
                    &request.item,
                    &request.target_digest,
                    request.thread_id.as_deref().unwrap_or(""),
                    request.interrupt_id.as_deref().unwrap_or(""),
                    &request.kind,
                )?;
                if let Some(thread) = &request.discussion_thread_id {
                    let belongs:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM threads t JOIN work_items i ON i.id=t.item_id WHERE t.id=?1 AND t.item_id=?2 AND t.project_id=i.project_id)",params![thread,request.item],|r|r.get(0))?;
                    ensure!(belongs, "Discussion thread is outside this item");
                }
                tx.execute("UPDATE github_gate_context SET summary=coalesce(?2,summary),discussion_thread_id=?3 WHERE gate_id=?1",params![id,request.summary,request.discussion_thread_id])?;
                return Ok(json!({"gate_id":id,"publication":"pending","request_digest":digest}));
            }
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO gates(id,item_id,kind,target_digest,target_sha,state,created_at) VALUES(?1,?2,?3,?4,?5,'pending',?6)",params![id,request.item,request.kind,request.target_digest,request.target_sha,plantool_core::now()])?;
            database::audit(
                tx,
                "worker",
                "gate.requested",
                &id,
                &serde_json::to_value(&request)?,
            )?;
            Ok(
                json!({"gate_id":id,"publication":"This gate kind is not connected in the GitHub half-phase","request_digest":digest}),
            )
        },
    )?;
    ensure!(
        value["request_digest"].as_str() == Some(digest.as_str()),
        "Gate idempotency key reused with different input"
    );
    Ok(Json(value))
}
async fn message(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<PostMessage>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    let db = database::open(&state.config.home)?;
    let (project, _) = dispatcher::item(&db, &request.item)?;
    let value = database::inbox_once(
        &state.config.home,
        "worker.message",
        &request.idempotency_key,
        |tx| {
            let valid: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1 AND project_id=?2 AND item_id=?3)",
                params![request.thread_id, project, request.item],
                |r| r.get(0),
            )?;
            ensure!(valid, "Thread is outside the item's project");
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO messages(id,thread_id,sender_role,body,dedupe_key,created_at) VALUES(?1,?2,'worker',?3,?4,?5)",params![id,request.thread_id,request.body,request.idempotency_key,plantool_core::now()])?;
            Ok(json!({"message_id":id}))
        },
    )?;
    Ok(Json(value))
}
async fn proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<FileProposal>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    dispatcher::item(&database::open(&state.config.home)?, &request.item)?;
    let value = database::inbox_once(
        &state.config.home,
        "worker.proposal",
        &request.idempotency_key,
        |tx| {
            let id = crate::workflow::proposal(tx, &request.item, &request.title, &request.body)?;
            database::audit(
                tx,
                "worker",
                "finding.proposed",
                &request.item,
                &json!({"id":id,"title":request.title,"body":request.body}),
            )?;
            Ok(json!({"proposal_id":id,"publication":"queued"}))
        },
    )?;
    Ok(Json(value))
}
async fn ack(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<EventAck>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    Ok(Json(crate::scheduler::ack(&state.config.home, &request)?))
}
async fn project_wait(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<ProjectWait>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    Ok(Json(crate::scheduler::wait(&state.config.home, &request)?))
}
async fn project_publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<WorkflowPr>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    Ok(Json(crate::scheduler::publish(
        &state.config.home,
        &request,
    )?))
}
async fn project_start_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    socket: Option<Extension<crate::socket::SocketTransport>>,
    Json(request): Json<WorkflowPr>,
) -> Result<Json<Value>, ApiError> {
    worker(&state, &headers, socket)?;
    Ok(Json(crate::scheduler::start_items(
        &state.config.home,
        &request,
    )?))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/internal/v1/request_dispatch", post(dispatch))
        .route("/internal/v1/workflow_context", post(workflow_context))
        .route("/internal/v1/workflow_pr", post(workflow_pr))
        .route("/internal/v1/store_artifact", post(store_artifact))
        .route("/internal/v1/task_done", post(task_done))
        .route("/internal/v1/report_result", post(report))
        .route("/internal/v1/request_gate", post(gate))
        .route("/internal/v1/read_artifact", post(read_artifact))
        .route("/internal/v1/read_task", post(read_task))
        .route("/internal/v1/read_repo_file", post(read_repo_file))
        .route("/internal/v1/post_message", post(message))
        .route("/internal/v1/file_proposal", post(proposal))
        .route("/internal/v1/project_event_ack", post(ack))
        .route("/internal/v1/project_wait", post(project_wait))
        .route("/internal/v1/project_publish", post(project_publish))
        .route(
            "/internal/v1/project_start_items",
            post(project_start_items),
        )
}
