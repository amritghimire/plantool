use super::sessions::resolve;
use super::{actor_from, author_from, bad_request, ApiError};
use crate::registry::{CommentFilter, NewComment};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use plantool_core::{Actor, Comment, CommentKind};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub doc: Option<plantool_core::DocKind>,
    #[serde(default)]
    pub kind: Option<CommentKind>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub since: Option<u64>,
    #[serde(default)]
    pub outdated: Option<bool>,
    #[serde(default)]
    pub context: Option<u32>,
}

impl ListQuery {
    fn filter(&self) -> CommentFilter {
        CommentFilter {
            doc: self.doc,
            kind: self.kind,
            author: self.author.clone(),
            status: self.status.clone(),
            since: self.since,
            outdated: self.outdated,
        }
    }
}

fn with_context(
    s: &crate::registry::LiveSession,
    comments: Vec<Comment>,
    radius: Option<u32>,
) -> Vec<serde_json::Value> {
    comments
        .into_iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c).unwrap_or(json!({}));
            v["thread_state"] = json!(plantool_core::review::thread_state(&c, &s.state().comments));
            if let Some(r) = radius {
                let ctx: Vec<serde_json::Value> = s
                    .context(&c, r)
                    .into_iter()
                    .map(|(n, l)| json!({ "line": n, "text": l }))
                    .collect();
                v["context"] = json!(ctx);
            }
            v
        })
        .collect()
}

async fn list(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    if s.session().cwd().is_dir() {
        s.refresh_code_anchors()?;
    }
    let comments = s.comments(&q.filter());
    let seq = s.seq();
    Ok(Json(
        json!({ "seq": seq, "comments": with_context(&s, comments, q.context) }),
    ))
}

fn default_kind(actor: Actor) -> CommentKind {
    match actor {
        Actor::Human => CommentKind::Human,
        Actor::Agent => CommentKind::Agent,
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum AddBody {
    One(NewComment),
    Many {
        comments: Vec<NewComment>,
        #[serde(default)]
        dry_run: bool,
    },
}

async fn add(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<AddBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let actor = actor_from(&headers, &state);
    let author = author_from(&headers, actor);
    let (items, dry_run) = match body {
        AddBody::One(c) => (vec![c], false),
        AddBody::Many { comments, dry_run } => (comments, dry_run),
    };
    if items.is_empty() {
        return Err(bad_request("no comments given"));
    }
    if dry_run {
        let anchors = s.dry_run(&items)?;
        return Ok((
            StatusCode::OK,
            Json(json!({ "dry_run": true, "anchors": anchors })),
        ));
    }
    let created = s.add_comments(items, default_kind(actor), &author)?;
    if actor == Actor::Human {
        let feedback = created
            .iter()
            .map(|c| format!("{} comment {}: {}", c.doc, c.id, c.body))
            .collect::<Vec<_>>()
            .join("\n");
        let message = format!("New owner feedback:\n{feedback}\nRead the full open comment board with `plantool session comment list --session {} --unresolved --context --json`, address it, and reply on its threads.", s.key);
        if let Err(error) = state.runs.forward_comment(s.clone(), message).await {
            tracing::warn!("could not forward comment: {error}");
        }
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "comments": created, "seq": s.seq() })),
    ))
}

#[derive(Deserialize)]
pub struct EditItem {
    pub id: String,
    pub body: String,
}

#[derive(Deserialize)]
pub struct EditBody {
    pub edits: Vec<EditItem>,
}

async fn edit(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<EditBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let mut out = Vec::new();
    for e in body.edits {
        out.push(s.edit_comment(&e.id, e.body)?);
    }
    Ok(Json(json!({ "comments": out })))
}

#[derive(Deserialize)]
pub struct IdsBody {
    pub ids: Vec<String>,
    #[serde(default = "default_true")]
    pub resolved: bool,
}

fn default_true() -> bool {
    true
}

async fn resolve_threads(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<IdsBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "only the owner can resolve threads; reply with proposes_resolve instead".into(),
        ));
    }
    let out = s.resolve_comments(&body.ids, body.resolved)?;
    Ok(Json(json!({ "comments": out })))
}

async fn remove(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Json(body): Json<IdsBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let n = s.remove_comments(&body.ids)?;
    Ok(Json(json!({ "removed": n })))
}

#[derive(Deserialize)]
pub struct ContextQuery {
    #[serde(default = "default_radius")]
    pub radius: u32,
}

fn default_radius() -> u32 {
    6
}

async fn context(
    State(state): State<AppState>,
    Path((repo, slug, id)): Path<(String, String, String)>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let c = s
        .comment(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("unknown comment {id}")))?;
    let ctx: Vec<serde_json::Value> = s
        .context(&c, q.radius)
        .into_iter()
        .map(|(n, l)| json!({ "line": n, "text": l }))
        .collect();
    Ok(Json(json!({ "comment": c, "context": ctx })))
}

async fn promote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((repo, slug, id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if actor_from(&headers, &state) != Actor::Human {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "only the owner can promote a blocker".into(),
        ));
    }
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    Ok(Json(json!({ "comment": s.promote_blocker(&id)? })))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sessions/{repo}/{slug}/comments",
            get(list).post(add).delete(remove),
        )
        .route("/sessions/{repo}/{slug}/comments/batch", post(add))
        .route("/sessions/{repo}/{slug}/comments/edit", post(edit))
        .route(
            "/sessions/{repo}/{slug}/comments/resolve",
            post(resolve_threads),
        )
        .route(
            "/sessions/{repo}/{slug}/comments/{id}/context",
            get(context),
        )
        .route(
            "/sessions/{repo}/{slug}/comments/{id}/promote",
            post(promote),
        )
        .route(
            "/sessions/{repo}/{slug}/comments/remove",
            delete(remove).post(remove),
        )
}
