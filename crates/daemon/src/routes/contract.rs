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
    let value = database::inbox_once(
        &state.config.home,
        "worker.gate",
        &request.idempotency_key,
        |tx| {
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
                json!({"gate_id":id,"publication":"GitHub App publication is unavailable until Phase 3"}),
            )
        },
    )?;
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
            let id = uuid::Uuid::new_v4().to_string();
            database::audit(
                tx,
                "worker",
                "finding.proposed",
                &request.item,
                &json!({"id":id,"title":request.title,"body":request.body}),
            )?;
            Ok(json!({"proposal_id":id,"publication":"pending Phase 3 GitHub App"}))
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
    let db = database::open(&state.config.home)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND mode='eos')",
            [&request.project],
            |r| r.get(0),
        )
        .map_err(anyhow::Error::from)?;
    ensure!(exists, "Unknown Engineering OS project");
    Ok(Json(database::inbox_once(
        &state.config.home,
        "worker.event",
        &request.idempotency_key,
        |tx| {
            database::audit(
                tx,
                "worker",
                "project.event_ack",
                &request.project,
                &serde_json::to_value(&request)?,
            )?;
            Ok(json!({"accepted":true}))
        },
    )?))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/internal/v1/request_dispatch", post(dispatch))
        .route("/internal/v1/report_result", post(report))
        .route("/internal/v1/request_gate", post(gate))
        .route("/internal/v1/read_artifact", post(read_artifact))
        .route("/internal/v1/read_task", post(read_task))
        .route("/internal/v1/read_repo_file", post(read_repo_file))
        .route("/internal/v1/post_message", post(message))
        .route("/internal/v1/file_proposal", post(proposal))
        .route("/internal/v1/project_event_ack", post(ack))
}
