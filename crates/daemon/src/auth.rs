//! Browser identity. WebAuthn ceremony state and one-use action grants never leave the daemon.
use crate::{routes::ApiError, AppState};
use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use webauthn_rs::prelude::*;

const SESSION_IDLE: Duration = Duration::from_secs(12 * 60 * 60);
const CEREMONY_TTL: Duration = Duration::from_secs(120);
const COOKIE: &str = "plantool_session";
fn denied() -> ApiError {
    ApiError(
        StatusCode::FORBIDDEN,
        "Sign in with a passkey and confirm this action in the browser".into(),
    )
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn nonce() -> String {
    crate::config::random_token()
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn agent(headers: &HeaderMap) -> bool {
    headers.contains_key("authorization")
        || headers.contains_key("x-plantool-run-token")
        || headers.contains_key("x-plantool-cli")
}
fn cookie(headers: &HeaderMap) -> Option<String> {
    if agent(headers) {
        return None;
    }
    headers
        .get("cookie")?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|s| s.trim().strip_prefix(&format!("{COOKIE}=")).map(digest_str))
}
fn digest_str(s: &str) -> String {
    digest(s.as_bytes())
}
fn connection(home: &Path) -> anyhow::Result<Connection> {
    crate::database::initialize(home)?;
    crate::database::open(home)
}
/// Daemon-only bootstrap write, requested by the owner-terminal socket endpoint.
pub fn setup_code(home: &Path) -> anyhow::Result<String> {
    let mut db = connection(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    anyhow::ensure!(
        tx.query_row("SELECT count(*) FROM passkeys", [], |r| r.get::<_, i64>(0))? == 0,
        "A passkey already exists. Sign in to add another in the browser."
    );
    let code = nonce();
    tx.execute(
        "INSERT OR REPLACE INTO passkey_setup VALUES(1, ?1, ?2)",
        params![digest_str(&code), now() + 600],
    )?;
    tx.commit()?;
    Ok(code)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Binding {
    action: String,
    target_id: String,
    target_digest: String,
    execution: Option<crate::execution::Status>,
    policy_digest: String,
    workspace: Option<crate::git::WorkspaceSnapshot>,
    request_body: String,
}
struct Pending<T> {
    state: T,
    expires: Instant,
    owner: Option<String>,
    binding: Option<Binding>,
}
struct Registration {
    state: PasskeyRegistration,
    setup_hash: Option<String>,
}
#[derive(Default)]
struct Memory {
    sessions: HashMap<String, Instant>,
    registrations: HashMap<String, Pending<Registration>>,
    assertions: HashMap<String, Pending<PasskeyAuthentication>>,
    grants: HashMap<String, Pending<()>>,
}
impl Memory {
    fn prune(&mut self) {
        let now = Instant::now();
        self.sessions
            .retain(|_, t| now.duration_since(*t) < SESSION_IDLE);
        self.registrations.retain(|_, p| p.expires > now);
        self.assertions.retain(|_, p| p.expires > now);
        self.grants.retain(|_, p| p.expires > now);
    }
}
pub struct Auth {
    home: PathBuf,
    webauthn: Webauthn,
    memory: Mutex<Memory>,
}
impl Auth {
    pub fn new(home: &Path, port: u16) -> anyhow::Result<Self> {
        connection(home)?;
        let origin = Url::parse(&format!("http://localhost:{port}"))?;
        let webauthn = WebauthnBuilder::new("localhost", &origin)?
            .rp_name("PlanTool")
            .build()?;
        Ok(Self {
            home: home.into(),
            webauthn,
            memory: Mutex::new(Memory::default()),
        })
    }
    fn keys(&self) -> anyhow::Result<Vec<Passkey>> {
        let db = connection(&self.home)?;
        let mut q = db.prepare("SELECT credential_json FROM passkeys ORDER BY id")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn authenticated(&self, headers: &HeaderMap) -> bool {
        self.session(headers).is_some()
    }
    fn session(&self, headers: &HeaderMap) -> Option<String> {
        let id = cookie(headers)?;
        let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        memory.prune();
        *memory.sessions.get_mut(&id)? = Instant::now();
        Some(id)
    }
    fn grant(&self, headers: &HeaderMap, binding: &Binding) -> Result<(), ApiError> {
        let owner = self.session(headers).ok_or_else(denied)?;
        let id = headers
            .get("x-plantool-assertion")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(denied)?;
        let mut m = self.memory.lock().unwrap_or_else(|e| e.into_inner());
        m.prune();
        let p = m.grants.remove(&digest_str(id)).ok_or_else(denied)?;
        if p.owner.as_deref() != Some(&owner) || p.binding.as_ref() != Some(binding) {
            return Err(denied());
        }
        Ok(())
    }
}

/// Only existing browser authority is guarded here; future policy endpoints must opt in too.
pub fn requires_assertion(method: &str, path: &str, body: &Value, human: bool) -> bool {
    if matches!(method, "GET" | "HEAD" | "OPTIONS") {
        return false;
    }
    let path = path.split('?').next().unwrap_or(path);
    if path == "/api/execution/resume" || path.ends_with("/pin") {
        return true;
    }
    if path == "/api/engineering-os/repair" {
        return true;
    }
    if path.starts_with("/api/settings/") {
        return true;
    }
    if path == "/api/import" {
        return human;
    }
    if !path.starts_with("/api/sessions/") {
        return false;
    }
    if path.ends_with("/stage") {
        return human || matches!(body["to"].as_str(), Some("approved" | "done"));
    }
    if path.ends_with("/input")
        && (!body["permission"].is_null() || !body["permission_mode"].is_null())
    {
        return true;
    }
    if path.ends_with("/runs") {
        return !body["sandbox"].is_null()
            || body["permission_mode"].as_str().is_some_and(|m| m != "ask")
            || !body["pause_rule"].is_null()
            || !body["milestone"].is_null()
            || body["implementation_mode"] == "step-by-step"
            || matches!(body["provider"].as_str(), Some("copilot" | "ollama"));
    }
    if path.ends_with("/where") {
        return human || body["confirm"] == true;
    }
    path.ends_with("/milestone/approve")
        || path.ends_with("/proposals")
        || path.ends_with("/workspace/adopt")
        || path.ends_with("/commit/cancel")
        || path.ends_with("/comments/resolve")
        || path.ends_with("/comments/remove")
        || (method == "DELETE" && path.ends_with("/comments"))
        || (method == "POST" && path.ends_with("/remove"))
        || path.ends_with("/promote")
        || path.ends_with("/pr")
        || path.ends_with("/pr/commit")
        || (method == "DELETE" && path.split('/').count() == 5)
}
#[derive(Clone)]
pub struct ApprovedAction {
    binding: Binding,
}
impl ApprovedAction {
    pub fn execution(&self) -> Result<&crate::execution::Status, ApiError> {
        self.binding.execution.as_ref().ok_or_else(denied)
    }

    pub fn workspace(&self) -> Result<&crate::git::WorkspaceSnapshot, ApiError> {
        self.binding.workspace.as_ref().ok_or_else(denied)
    }
    pub fn recheck(&self, state: &AppState) -> Result<(), ApiError> {
        if binding(
            state,
            &self.binding.action,
            &self.binding.target_id,
            &self.body()?,
        )? != self.binding
        {
            return Err(stale());
        }
        Ok(())
    }
    pub fn recheck_policy(&self, state: &AppState) -> Result<(), ApiError> {
        let current = make_binding(
            state,
            &self.binding.action,
            &self.binding.target_id,
            &self.body()?,
            false,
        )?;
        if current.policy_digest != self.binding.policy_digest {
            return Err(stale());
        }
        Ok(())
    }
    fn body(&self) -> Result<Value, ApiError> {
        Ok(serde_json::from_str(&self.binding.request_body).map_err(anyhow::Error::from)?)
    }
}
fn stale() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "The approved target changed; review it and confirm again".into(),
    )
}
fn binding(state: &AppState, method: &str, uri: &str, body: &Value) -> Result<Binding, ApiError> {
    make_binding(state, method, uri, body, true)
}
fn make_binding(
    state: &AppState,
    method: &str,
    uri: &str,
    body: &Value,
    capture_workspace: bool,
) -> Result<Binding, ApiError> {
    if !uri.starts_with("/api/") || uri.contains('#') {
        return Err(denied());
    }
    let path = uri.split('?').next().unwrap_or(uri);
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| denied())?;
    let parts: Vec<_> = decoded.split('/').collect();
    let mut data = json!({"body": body});
    let execution = if path.starts_with("/api/execution/") || path == "/api/settings/never-sent" {
        Some(state.runs.execution.status())
    } else {
        None
    };
    if path == "/api/engineering-os/repair" {
        data["worker_version"] = json!(state.config.version);
        data["contract"] = json!(plantool_contract::Health::default());
        data["stop_all"] = json!(state.runs.execution.status());
    }
    if let Some(execution) = &execution {
        data["execution"] = json!(execution);
    }
    if path.starts_with("/api/settings/github") {
        data["github"] =
            crate::routes::github::snapshot(&state.config.home).map_err(|_| denied())?;
    }
    if path.starts_with("/api/settings/models") {
        data["chains"] =
            json!(crate::models::router::config(&state.config.home).map_err(|_| denied())?);
        data["execution"] = json!(state.runs.execution.status());
        if let Some(project) = body["project"].as_str() {
            data["allowed_providers"] = json!(crate::models::router::allowed(
                &crate::database::open(&state.config.home).map_err(|_| denied())?,
                project
            )
            .map_err(|_| denied())?);
        }
    }
    if path == "/api/settings/retention" || path.ends_with("/pin") {
        data["retention"] = serde_json::to_value(
            crate::retention::policy(&state.config.home).map_err(|_| denied())?,
        )
        .map_err(|_| denied())?;
    }
    if path.ends_with("/pin") {
        if let Some(id) = parts.get(6) {
            data["pinned"] =
                json!(crate::retention::pinned(&state.config.home, id).map_err(|_| denied())?);
        }
    }
    let mut workspace = None;
    if parts.get(2) == Some(&"sessions") && parts.len() >= 5 {
        if let Some(s) = state
            .registry
            .get_key(&format!("{}/{}", parts[3], parts[4]))
        {
            let session = s.session();
            data["context"] = json!({"repo":session.repo,"worktree":session.worktree,"base":session.base,"pause_rule":session.pause_rule,"difftool":session.difftool});
            let saved = s.state();
            let approval = path.ends_with("/stage")
                || path.ends_with("/milestone/approve")
                || path.ends_with("/pr")
                || path.ends_with("/pr/commit");
            if approval {
                data["approval"] = json!({"stage":saved.stage,"approved":saved.approved,"pending":saved.plan_revision_pending,"milestones":saved.milestones,"selected":saved.selected_milestone,"comments":saved.comments});
                data["plan"] = json!(std::fs::read(s.doc_path(plantool_core::DocKind::Plan))
                    .ok()
                    .map(|b| digest(&b)));
            } else if path.contains("/comments") || path.ends_with("/promote") {
                data["comments"] = json!(saved.comments);
            } else if path.ends_with("/proposals") {
                data["proposal"] = json!(saved
                    .proposals
                    .iter()
                    .find(|p| Some(p.id.as_str()) == body["id"].as_str()));
            }
            if path.contains("/runs/") {
                if let Some(id) = parts.get(6) {
                    if let Some(run) = s.run(id) {
                        data["run"] = json!({"id":run.id,"cwd":run.cwd,"provider":run.provider,"permission_mode":run.permission_mode,"milestone_pending":run.milestone_pending,"milestones_approved":run.milestones_approved,"plan_sha":run.plan_sha,"milestone_key":run.milestone_key,"milestone_base":run.milestone_base,"milestone_commit":run.milestone_commit,"milestone_review":run.milestone_review});
                    }
                }
            }
            if path.ends_with("/runs") {
                data["resume"] = json!(body["resume_run"].as_str().and_then(|id|s.run(id)).map(|r|json!({"id":r.id,"provider":r.provider,"permission_mode":r.permission_mode,"implementation_mode":r.implementation_mode})));
            }
            if capture_workspace
                && (path.ends_with("/milestone/approve")
                    || path.ends_with("/pr")
                    || path.ends_with("/pr/commit"))
            {
                let cwd = if path.ends_with("/milestone/approve") {
                    parts
                        .get(6)
                        .and_then(|id| s.run(id))
                        .map(|r| r.cwd)
                        .unwrap_or_else(|| session.cwd().clone())
                } else {
                    session.cwd().clone()
                };
                workspace = Some(
                    crate::git::workspace_snapshot(&cwd)
                        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?,
                );
            }
        }
    }
    let policy_digest = digest_str(&data.to_string());
    data["workspace"] = json!(workspace);
    Ok(Binding {
        action: method.into(),
        target_id: uri.into(),
        target_digest: digest_str(&data.to_string()),
        execution,
        policy_digest,
        workspace,
        request_body: body.to_string(),
    })
}

pub async fn guard(State(state): State<AppState>, mut req: Request<Body>, next: Next) -> Response {
    if req.uri().path() == "/v1/chat/completions" {
        if let Some(value) = req.headers_mut().remove("authorization") {
            if req
                .headers()
                .contains_key(crate::identities::RUN_TOKEN_HEADER)
            {
                return denied().into_response();
            }
            let Some(token) = value
                .to_str()
                .ok()
                .and_then(|s| s.strip_prefix("Bearer "))
                .and_then(|s| s.parse::<axum::http::HeaderValue>().ok())
            else {
                return denied().into_response();
            };
            req.headers_mut()
                .insert(crate::identities::RUN_TOKEN_HEADER, token);
        }
    }
    match crate::identities::authorize(&state, &req) {
        Ok(Some(authorized)) => {
            req.extensions_mut().insert(authorized);
        }
        Ok(None) => {}
        Err(error) => return error.into_response(),
    }
    let expected_origin = format!("http://localhost:{}", state.config.port);
    let browser =
        req.headers().contains_key("cookie") || req.uri().path().starts_with("/api/auth/");
    if browser
        && !matches!(req.method().as_str(), "GET" | "HEAD" | "OPTIONS")
        && req.headers().get("origin").and_then(|v| v.to_str().ok())
            != Some(expected_origin.as_str())
    {
        return denied().into_response();
    }
    let is_auth = req.uri().path().starts_with("/api/auth/");
    let refresh = if state.auth.authenticated(req.headers()) {
        req.headers()
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(';').find_map(|s| {
                    s.trim()
                        .strip_prefix("plantool_session=")
                        .map(str::to_owned)
                })
            })
    } else {
        None
    };
    let mut response = guard_inner(state, req, next).await;
    if is_auth || refresh.is_some() {
        response
            .headers_mut()
            .insert("cache-control", "no-store".parse().unwrap());
    }
    if !response.headers().contains_key("set-cookie") {
        if let Some(token) = refresh {
            response.headers_mut().insert(
                "set-cookie",
                format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200")
                    .parse()
                    .unwrap(),
            );
        }
    }
    response
}
async fn guard_inner(state: AppState, req: Request<Body>, next: Next) -> Response {
    let method = req.method().as_str().to_string();
    let path = req
        .uri()
        .path_and_query()
        .map(|s| s.as_str())
        .unwrap_or("/")
        .to_string();
    if agent(req.headers()) && path.starts_with("/api/auth/") {
        return denied().into_response();
    }
    if matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS") || path.starts_with("/api/auth/") {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let bytes = match to_bytes(body, crate::security::MAX_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!({"binary_sha256":digest(&bytes)}))
    };
    let human = state.auth.authenticated(&parts.headers);
    // Explicit confirm cannot import approvals anonymously.
    if path.starts_with("/api/import?")
        && !human
        && Url::parse(&format!("http://localhost{path}"))
            .is_ok_and(|u| u.query_pairs().any(|(k, v)| k == "confirm" && v == "true"))
    {
        return denied().into_response();
    }
    if requires_assertion(&method, &path, &value, human) {
        if human && !parts.headers.contains_key("x-plantool-assertion") {
            return (
                StatusCode::PRECONDITION_REQUIRED,
                Json(json!({"error":"Confirm this action with your passkey"})),
            )
                .into_response();
        }
        let approved = match binding(&state, &method, &path, &value) {
            Ok(b) => b,
            Err(e) => return e.into_response(),
        };
        if let Err(e) = state.auth.grant(&parts.headers, &approved) {
            return e.into_response();
        }
        parts
            .extensions
            .insert(ApprovedAction { binding: approved });
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        json!({"registered":!state.auth.keys()?.is_empty(),"authenticated":state.auth.authenticated(&headers)}),
    ))
}
#[derive(Deserialize)]
struct RegisterStart {
    code: Option<String>,
}
async fn register_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterStart>,
) -> Result<Json<Value>, ApiError> {
    let keys = state.auth.keys()?;
    let owner = state.auth.session(&headers);
    let setup_hash = if keys.is_empty() {
        let hash = digest_str(body.code.as_deref().unwrap_or(""));
        let db = connection(&state.config.home)?;
        let valid = db
            .query_row(
                "SELECT count(*) FROM passkey_setup WHERE code_hash=?1 AND expires_at>?2",
                params![hash, now()],
                |r| r.get::<_, i64>(0),
            )
            .map_err(anyhow::Error::from)?;
        if valid != 1 {
            return Err(denied());
        }
        Some(hash)
    } else {
        state.auth.grant(
            &headers,
            &binding(
                &state,
                "POST",
                "/api/auth/register/start",
                &json!({"code":body.code}),
            )?,
        )?;
        None
    };
    let (mut options, registration) = state
        .auth
        .webauthn
        .start_passkey_registration(
            Uuid::from_u128(1),
            "owner",
            "PlanTool owner",
            Some(keys.iter().map(|k| k.cred_id().clone()).collect()),
        )
        .map_err(|_| denied())?;
    if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
        selection.authenticator_attachment = Some(AuthenticatorAttachment::Platform);
    }
    // Keep the selection constraint in server-side state too.
    let mut rs = serde_json::to_value(registration).map_err(anyhow::Error::from)?;
    rs["rs"]["authenticator_attachment"] = json!("platform");
    let registration = serde_json::from_value(rs).map_err(anyhow::Error::from)?;
    let id = nonce();
    let mut m = state.auth.memory.lock().unwrap_or_else(|e| e.into_inner());
    m.prune();
    if m.registrations.len() >= 64 {
        return Err(denied());
    }
    m.registrations.insert(
        id.clone(),
        Pending {
            state: Registration {
                state: registration,
                setup_hash,
            },
            expires: Instant::now() + CEREMONY_TTL,
            owner,
            binding: None,
        },
    );
    Ok(Json(json!({"id":id,"options":options})))
}
#[derive(Deserialize)]
struct RegisterFinish {
    id: String,
    credential: RegisterPublicKeyCredential,
}
async fn register_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterFinish>,
) -> Result<(HeaderMap, Json<Value>), ApiError> {
    let owner = state.auth.session(&headers);
    let p = {
        let mut m = state.auth.memory.lock().unwrap_or_else(|e| e.into_inner());
        m.prune();
        m.registrations.remove(&body.id).ok_or_else(denied)?
    };
    if p.owner != owner {
        return Err(denied());
    }
    let key = state
        .auth
        .webauthn
        .finish_passkey_registration(&body.credential, &p.state.state)
        .map_err(|_| denied())?;
    let mut db = connection(&state.config.home)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    let count = tx
        .query_row("SELECT count(*) FROM passkeys", [], |r| r.get::<_, i64>(0))
        .map_err(anyhow::Error::from)?;
    if let Some(hash) = p.state.setup_hash {
        let valid = tx
            .execute(
                "DELETE FROM passkey_setup WHERE code_hash=?1 AND expires_at>?2",
                params![hash, now()],
            )
            .map_err(anyhow::Error::from)?;
        if count != 0 || valid != 1 {
            return Err(denied());
        }
    } else if count == 0 || owner.is_none() {
        return Err(denied());
    }
    tx.execute(
        "INSERT INTO passkeys VALUES(?1,?2,?3,NULL)",
        params![
            URL_SAFE_NO_PAD.encode(key.cred_id()),
            serde_json::to_string(&key).map_err(anyhow::Error::from)?,
            now()
        ],
    )
    .map_err(anyhow::Error::from)?;
    tx.commit().map_err(anyhow::Error::from)?;
    Ok((
        login_cookie(&state.auth, &headers),
        Json(json!({"ok":true})),
    ))
}
fn login_cookie(auth: &Auth, headers: &HeaderMap) -> HeaderMap {
    let token = nonce();
    let mut m = auth.memory.lock().unwrap_or_else(|e| e.into_inner());
    m.prune();
    if let Some(old) = cookie(headers) {
        m.sessions.remove(&old);
    }
    m.sessions.insert(digest_str(&token), Instant::now());
    let mut h = HeaderMap::new();
    h.insert(
        "set-cookie",
        format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200")
            .parse()
            .unwrap(),
    );
    h
}
#[derive(Deserialize)]
struct Action {
    method: String,
    path: String,
    #[serde(default)]
    body: Value,
}
#[derive(Deserialize)]
struct AssertionStart {
    action: Option<Action>,
}
async fn assertion_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AssertionStart>,
) -> Result<Json<Value>, ApiError> {
    let owner = state.auth.session(&headers);
    let bound = body
        .action
        .map(|a| {
            if owner.is_none() {
                return Err(denied());
            }
            binding(&state, &a.method, &a.path, &a.body)
        })
        .transpose()?;
    let (mut options, authentication) = state
        .auth
        .webauthn
        .start_passkey_authentication(&state.auth.keys()?)
        .map_err(|_| denied())?;
    let authentication = if let Some(b) = &bound {
        let mut h = Sha256::new();
        h.update(b.action.as_bytes());
        h.update(b.target_id.as_bytes());
        h.update(b.target_digest.as_bytes());
        h.update(options.public_key.challenge.as_slice());
        let challenge = h.finalize().to_vec();
        options.public_key.challenge = challenge.clone().into();
        // This feature is used solely to replace a server-generated challenge. Never deserialize client ceremony state.
        let mut a = serde_json::to_value(authentication).map_err(anyhow::Error::from)?;
        a["ast"]["challenge"] = json!(URL_SAFE_NO_PAD.encode(challenge));
        serde_json::from_value(a).map_err(anyhow::Error::from)?
    } else {
        authentication
    };
    let id = nonce();
    let mut m = state.auth.memory.lock().unwrap_or_else(|e| e.into_inner());
    m.prune();
    if m.assertions.len() >= 128 {
        return Err(denied());
    }
    m.assertions.insert(
        id.clone(),
        Pending {
            state: authentication,
            expires: Instant::now() + CEREMONY_TTL,
            owner,
            binding: bound,
        },
    );
    Ok(Json(json!({"id":id,"options":options})))
}
#[derive(Deserialize)]
struct AssertionFinish {
    id: String,
    credential: PublicKeyCredential,
}
async fn assertion_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AssertionFinish>,
) -> Result<(HeaderMap, Json<Value>), ApiError> {
    let owner = state.auth.session(&headers);
    let p = {
        let mut m = state.auth.memory.lock().unwrap_or_else(|e| e.into_inner());
        m.prune();
        m.assertions.remove(&body.id).ok_or_else(denied)?
    };
    if p.owner != owner {
        return Err(denied());
    }
    let result = state
        .auth
        .webauthn
        .finish_passkey_authentication(&body.credential, &p.state)
        .map_err(|_| denied())?;
    if !result.user_verified() {
        return Err(denied());
    }
    let mut db = connection(&state.config.home)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(anyhow::Error::from)?;
    for mut key in state.auth.keys()? {
        if key.update_credential(&result).is_some() {
            tx.execute(
                "UPDATE passkeys SET credential_json=?1,last_used_at=?2 WHERE id=?3",
                params![
                    serde_json::to_string(&key).map_err(anyhow::Error::from)?,
                    now(),
                    URL_SAFE_NO_PAD.encode(key.cred_id())
                ],
            )
            .map_err(anyhow::Error::from)?;
        }
    }
    tx.commit().map_err(anyhow::Error::from)?;
    if let Some(bound) = p.binding {
        let proof = nonce();
        let mut m = state.auth.memory.lock().unwrap_or_else(|e| e.into_inner());
        m.grants.insert(
            digest_str(&proof),
            Pending {
                state: (),
                expires: Instant::now() + CEREMONY_TTL,
                owner,
                binding: Some(bound),
            },
        );
        Ok((HeaderMap::new(), Json(json!({"proof":proof}))))
    } else {
        Ok((
            login_cookie(&state.auth, &headers),
            Json(json!({"ok":true})),
        ))
    }
}
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> (HeaderMap, Json<Value>) {
    if let Some(id) = cookie(&headers) {
        state
            .auth
            .memory
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .remove(&id);
    }
    let mut h = HeaderMap::new();
    h.insert(
        "set-cookie",
        format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
            .parse()
            .unwrap(),
    );
    (h, Json(json!({"ok":true})))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/status", get(status))
        .route("/auth/register/start", post(register_start))
        .route("/auth/register/finish", post(register_finish))
        .route("/auth/assertion/start", post(assertion_start))
        .route("/auth/assertion/finish", post(assertion_finish))
        .route("/auth/logout", post(logout))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_sessions_and_expired_or_cross_session_grants_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let auth = Auth::new(tmp.path(), 41300).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "plantool_session=test".parse().unwrap());
        auth.memory
            .lock()
            .unwrap()
            .sessions
            .insert(digest_str("test"), Instant::now() - SESSION_IDLE);
        assert!(!auth.authenticated(&headers));
        auth.memory
            .lock()
            .unwrap()
            .sessions
            .insert(digest_str("test"), Instant::now());
        assert!(auth.authenticated(&headers));
        let b = Binding {
            action: "POST".into(),
            target_id: "/api/settings/difftool".into(),
            target_digest: "digest".into(),
            execution: None,
            policy_digest: "digest".into(),
            workspace: None,
            request_body: "null".into(),
        };
        headers.insert("x-plantool-assertion", "proof".parse().unwrap());
        auth.memory.lock().unwrap().grants.insert(
            digest_str("proof"),
            Pending {
                state: (),
                expires: Instant::now() - Duration::from_secs(1),
                owner: Some(digest_str("test")),
                binding: Some(b.clone()),
            },
        );
        assert!(auth.grant(&headers, &b).is_err());
        auth.memory.lock().unwrap().grants.insert(
            digest_str("proof"),
            Pending {
                state: (),
                expires: Instant::now() + CEREMONY_TTL,
                owner: Some("other-session".into()),
                binding: Some(b.clone()),
            },
        );
        assert!(auth.grant(&headers, &b).is_err());
    }
    #[test]
    fn approvals_and_existing_security_changes_require_assertions() {
        for path in [
            "/api/settings/difftool",
            "/api/settings/worktree-dir",
            "/api/sessions/repo/item/milestone/approve",
            "/api/sessions/repo/item/runs/id/milestone/approve",
            "/api/sessions/repo/item/pr",
            "/api/sessions/repo/item/workspace/adopt",
        ] {
            assert!(
                requires_assertion("POST", path, &Value::Null, false),
                "{path}"
            );
        }
        assert!(requires_assertion(
            "POST",
            "/api/sessions/repo/item/stage",
            &json!({"to":"approved"}),
            false
        ));
        assert!(requires_assertion(
            "POST",
            "/api/sessions/repo/item/runs/id/input",
            &json!({"permission":{"decision":"allow"}}),
            false
        ));
        assert!(!requires_assertion(
            "POST",
            "/api/sessions/repo/item/docs/plan/touch",
            &json!({}),
            false
        ));
    }
}
