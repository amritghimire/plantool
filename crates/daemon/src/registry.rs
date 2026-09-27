use crate::events::{LiveEvent, LiveMessage, NavTarget};
use crate::git;
use crate::store::{self, SessionStore};
use plantool_core::anchor;
use plantool_core::markdown;
use plantool_core::{
    transition, Actor, ChangeReview, Comment, CommentKind, DocKind, DocRevision, DocState, Run, Session, Stage, State,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("no session named {0}")]
    NotFound(String),
    #[error("{reference} matches more than one session: {}", keys.join(", "))]
    Ambiguous { reference: String, keys: Vec<String> },
    #[error("session {key} already belongs to {existing}; pass --repo-slug to keep them apart")]
    RepoMismatch { key: String, existing: String },
    #[error("invalid slug {0:?}: use letters, digits, dots, underscores and dashes")]
    BadSlug(String),
    #[error("{0}")]
    Stage(#[from] plantool_core::StageError),
    #[error("{0}")]
    Anchor(#[from] anchor::AnchorError),
    #[error("no {0} document yet; write it to {1}")]
    NoDoc(DocKind, PathBuf),
    #[error("unknown comment id {0}")]
    UnknownComment(String),
    #[error("{0}")]
    Git(#[from] git::GitError),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateSession {
    pub slug: String,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub repo: Option<PathBuf>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub worktree_dir: Option<PathBuf>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub mirror: bool,
    #[serde(default)]
    pub repo_slug: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewComment {
    pub doc: DocKind,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default, rename = "match")]
    pub match_text: Option<String>,
    pub body: String,
    #[serde(default)]
    pub kind: Option<CommentKind>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommentFilter {
    #[serde(default)]
    pub doc: Option<DocKind>,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct DocSummary {
    pub kind: DocKind,
    pub path: PathBuf,
    pub exists: bool,
    pub sha: Option<String>,
    pub lines: u32,
    pub title: Option<String>,
    pub captured_at: Option<String>,
    pub progress: Vec<markdown::PhaseProgress>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoInfo {
    pub root: PathBuf,
    pub repo_slug: String,
    pub branch: String,
    pub base: String,
    pub sessions: usize,
    pub last_used: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub key: String,
    pub url_path: String,
    pub dir: PathBuf,
    pub session: Session,
    pub state: State,
    pub docs: Vec<DocSummary>,
    pub runs: Vec<Run>,
    pub open_comments: usize,
}

pub struct LiveSession {
    pub key: String,
    pub store: SessionStore,
    inner: Mutex<Inner>,
    pub tx: broadcast::Sender<LiveMessage>,
}

struct Inner {
    session: Session,
    state: State,
    docs: BTreeMap<DocKind, DocRevision>,
    runs: BTreeMap<String, Run>,
}

fn short_id() -> String {
    let u = uuid::Uuid::new_v4().simple().to_string();
    u[..10].to_string()
}

pub fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !s.starts_with('.')
}

impl LiveSession {
    fn open(store: SessionStore) -> anyhow::Result<Option<Arc<LiveSession>>> {
        let Some(session) = store.load_meta()? else { return Ok(None) };
        let state = store.load_state()?;
        let mut docs = BTreeMap::new();
        for (kind, ds) in &state.docs {
            if let Some(content) = store.load_revision(*kind, &ds.sha)? {
                docs.insert(*kind, DocRevision { kind: *kind, sha: ds.sha.clone(), content, captured_at: ds.captured_at.clone() });
            }
        }
        let runs = store.load_runs()?.into_iter().map(|r| (r.id.clone(), r)).collect();
        let (tx, _) = broadcast::channel(256);
        let key = session.key();
        Ok(Some(Arc::new(LiveSession { key, store, inner: Mutex::new(Inner { session, state, docs, runs }), tx })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn session(&self) -> Session {
        self.lock().session.clone()
    }

    pub fn state(&self) -> State {
        self.lock().state.clone()
    }

    pub fn stage(&self) -> Stage {
        self.lock().state.stage
    }

    pub fn doc_path(&self, kind: DocKind) -> PathBuf {
        self.store.doc_path(kind)
    }

    pub fn doc(&self, kind: DocKind) -> Option<DocRevision> {
        self.lock().docs.get(&kind).cloned()
    }

    pub fn doc_revision(&self, kind: DocKind, sha: &str) -> anyhow::Result<Option<DocRevision>> {
        if let Some(d) = self.lock().docs.get(&kind) {
            if d.sha == sha {
                return Ok(Some(d.clone()));
            }
        }
        Ok(self.store.load_revision(kind, sha)?.map(|content| DocRevision { kind, sha: sha.to_string(), content, captured_at: String::new() }))
    }

    pub fn view(&self) -> SessionView {
        let g = self.lock();
        let docs = DocKind::ALL
            .iter()
            .filter(|k| g.docs.contains_key(k) || matches!(k, DocKind::Research | DocKind::Plan))
            .map(|k| {
                let rev = g.docs.get(k);
                let ds = g.state.docs.get(k);
                DocSummary {
                    kind: *k,
                    path: self.store.doc_path(*k),
                    exists: rev.is_some(),
                    sha: rev.map(|r| r.sha.clone()),
                    lines: ds.map(|d| d.lines).unwrap_or(0),
                    title: rev.and_then(|r| markdown::title(&r.content)),
                    captured_at: ds.map(|d| d.captured_at.clone()),
                    progress: rev.map(|r| markdown::progress(&r.content)).unwrap_or_default(),
                }
            })
            .collect();
        let open_comments = g.state.comments.iter().filter(|c| !c.resolved && c.parent.is_none()).count();
        SessionView {
            key: self.key.clone(),
            url_path: format!("/s/{}", self.key),
            dir: self.store.dir.clone(),
            session: g.session.clone(),
            state: g.state.clone(),
            docs,
            runs: g.runs.values().cloned().collect(),
            open_comments,
        }
    }

    fn broadcast(&self, g: &mut Inner, event: LiveEvent) -> u64 {
        g.state.seq += 1;
        g.state.updated_at = plantool_core::now();
        let seq = g.state.seq;
        let _ = self.tx.send(LiveMessage { seq, at: g.state.updated_at.clone(), event });
        seq
    }

    fn persist_state(&self, g: &Inner) -> anyhow::Result<()> {
        self.store.save_state(&g.state)
    }

    pub fn capture_doc(&self, kind: DocKind) -> Result<Option<DocRevision>, RegistryError> {
        let content = match self.store.read_doc(kind)? {
            Some(c) => c,
            None => return Ok(None),
        };
        let sha = plantool_core::sha256_hex(&content);
        let mut g = self.lock();
        if g.docs.get(&kind).map(|d| d.sha == sha).unwrap_or(false) {
            return Ok(None);
        }
        let rev = DocRevision { kind, sha: sha.clone(), content: content.clone(), captured_at: plantool_core::now() };
        self.store.save_revision(&rev)?;
        let old = g.docs.insert(kind, rev.clone()).map(|d| d.content).unwrap_or_default();
        let mut affected: Vec<&mut Comment> = g.state.comments.iter_mut().filter(|c| c.doc == kind).collect();
        let mut changed = 0;
        let mut outdated = 0;
        for c in affected.iter_mut() {
            let next = anchor::reanchor_one(&old, &content, &c.anchor);
            if next != c.anchor {
                c.anchor = next;
                changed += 1;
            }
            if c.anchor.outdated {
                outdated += 1;
            }
        }
        let lines = anchor::line_count(&content);
        g.state.docs.insert(kind, DocState { sha: sha.clone(), captured_at: rev.captured_at.clone(), lines });
        let auto = match (kind, g.state.stage) {
            (DocKind::Research | DocKind::Investigation, Stage::New | Stage::Researching) => Some(Stage::ResearchReview),
            (DocKind::Plan | DocKind::QuickFix, Stage::New | Stage::Researching | Stage::ResearchReview | Stage::Planning) => Some(Stage::PlanReview),
            _ => None,
        };
        self.broadcast(&mut g, LiveEvent::DocRefreshed { kind, sha, lines, reanchored: changed, outdated });
        if let Some(to) = auto {
            let from = g.state.stage;
            g.state.stage = to;
            self.broadcast(&mut g, LiveEvent::StageChanged { from, to, actor: Actor::Agent });
        }
        self.persist_state(&g)?;
        if g.session.mirror {
            let target = g.session.repo.root.join("REVIEWS").join(format!("{}_{}.md", kind.as_str(), g.session.slug));
            let _ = store::write_atomic(&target, content.as_bytes());
        }
        Ok(Some(rev))
    }

    pub fn set_stage(&self, to: Stage, actor: Actor) -> Result<Stage, RegistryError> {
        let mut g = self.lock();
        let from = g.state.stage;
        transition(from, to, actor)?;
        if from != to {
            g.state.stage = to;
            self.broadcast(&mut g, LiveEvent::StageChanged { from, to, actor });
            self.persist_state(&g)?;
        }
        Ok(to)
    }

    pub fn set_brief(&self, brief: Option<String>) -> Result<Session, RegistryError> {
        let mut g = self.lock();
        g.session.brief = brief.map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
        self.store.save_meta(&g.session)?;
        let session = g.session.clone();
        self.broadcast(&mut g, LiveEvent::SessionUpdated { session: session.clone() });
        self.persist_state(&g)?;
        Ok(session)
    }

    /// Create the session's git worktree if it has none, and record it on the session.
    pub fn ensure_worktree(&self, dir: Option<PathBuf>) -> Result<Session, RegistryError> {
        let session = self.session();
        if session.worktree.is_some() {
            return Ok(session);
        }
        let dir = dir.unwrap_or_else(|| git::main_root(&session.repo.common_dir).join(".claude").join("worktrees").join(&session.slug));
        git::worktree_add(&session.repo, &dir, &session.slug, &session.base)?;
        let mut g = self.lock();
        g.session.worktree = Some(dir);
        self.store.save_meta(&g.session)?;
        let session = g.session.clone();
        self.broadcast(&mut g, LiveEvent::SessionUpdated { session: session.clone() });
        self.persist_state(&g)?;
        Ok(session)
    }

    /// Tell open tabs a review was opened or refreshed without touching the session-level review.
    pub fn announce_review(&self, review: ChangeReview) {
        let mut g = self.lock();
        self.broadcast(&mut g, LiveEvent::ChangesOpened { review });
    }

    pub fn set_review(&self, review: ChangeReview) -> Result<(), RegistryError> {
        let mut g = self.lock();
        g.state.review = Some(review.clone());
        self.broadcast(&mut g, LiveEvent::ChangesOpened { review });
        self.persist_state(&g)?;
        Ok(())
    }

    fn resolve_anchor(&self, g: &Inner, nc: &NewComment) -> Result<plantool_core::Anchor, RegistryError> {
        if let Some(parent) = &nc.parent {
            let p = g.state.comments.iter().find(|c| &c.id == parent).ok_or_else(|| RegistryError::UnknownComment(parent.clone()))?;
            return Ok(p.anchor.clone());
        }
        let content = g.docs.get(&nc.doc).map(|d| d.content.as_str()).ok_or_else(|| RegistryError::NoDoc(nc.doc, self.store.doc_path(nc.doc)))?;
        if let Some(m) = &nc.match_text {
            return Ok(anchor::resolve_match(content, m)?);
        }
        if let Some(line) = nc.line {
            return Ok(anchor::anchor_at_line(content, line)?);
        }
        Err(anchor::AnchorError::NoMatch(String::new()).into())
    }

    pub fn dry_run(&self, items: &[NewComment]) -> Result<Vec<plantool_core::Anchor>, RegistryError> {
        let g = self.lock();
        items.iter().map(|nc| self.resolve_anchor(&g, nc)).collect()
    }

    pub fn add_comments(&self, items: Vec<NewComment>, default_kind: CommentKind, default_author: &str) -> Result<Vec<Comment>, RegistryError> {
        let mut g = self.lock();
        let mut resolved = Vec::with_capacity(items.len());
        for nc in &items {
            resolved.push(self.resolve_anchor(&g, nc)?);
        }
        let now = plantool_core::now();
        let mut created = Vec::with_capacity(items.len());
        for (nc, anchor) in items.into_iter().zip(resolved) {
            g.state.seq += 1;
            let doc = match &nc.parent {
                Some(p) => g.state.comments.iter().find(|c| &c.id == p).map(|c| c.doc).unwrap_or(nc.doc),
                None => nc.doc,
            };
            let c = Comment {
                id: short_id(),
                doc,
                anchor,
                body: nc.body,
                kind: nc.kind.unwrap_or(default_kind),
                author: nc.author.unwrap_or_else(|| default_author.to_string()),
                parent: nc.parent,
                resolved: false,
                created_at: now.clone(),
                updated_at: None,
                seq: g.state.seq,
            };
            g.state.comments.push(c.clone());
            created.push(c);
        }
        let event = if created.len() == 1 {
            LiveEvent::CommentAdded { comment: created[0].clone() }
        } else {
            LiveEvent::CommentsAdded { comments: created.clone() }
        };
        self.broadcast(&mut g, event);
        self.persist_state(&g)?;
        Ok(created)
    }

    pub fn edit_comment(&self, id: &str, body: String) -> Result<Comment, RegistryError> {
        let mut g = self.lock();
        let seq = g.state.seq + 1;
        let c = g.state.comments.iter_mut().find(|c| c.id == id).ok_or_else(|| RegistryError::UnknownComment(id.to_string()))?;
        c.body = body;
        c.updated_at = Some(plantool_core::now());
        c.seq = seq;
        let out = c.clone();
        self.broadcast(&mut g, LiveEvent::CommentUpdated { comment: out.clone() });
        self.persist_state(&g)?;
        Ok(out)
    }

    pub fn resolve_comments(&self, ids: &[String], resolved: bool) -> Result<Vec<Comment>, RegistryError> {
        let mut g = self.lock();
        for id in ids {
            if !g.state.comments.iter().any(|c| &c.id == id) {
                return Err(RegistryError::UnknownComment(id.clone()));
            }
        }
        let seq = g.state.seq + 1;
        let now = plantool_core::now();
        let mut out = Vec::new();
        for c in g.state.comments.iter_mut() {
            let in_thread = ids.contains(&c.id) || c.parent.as_ref().map(|p| ids.contains(p)).unwrap_or(false);
            if in_thread {
                c.resolved = resolved;
                c.updated_at = Some(now.clone());
                c.seq = seq;
                out.push(c.clone());
            }
        }
        self.broadcast(&mut g, LiveEvent::ThreadResolution { ids: ids.to_vec(), resolved });
        self.persist_state(&g)?;
        Ok(out)
    }

    pub fn remove_comments(&self, ids: &[String]) -> Result<usize, RegistryError> {
        let mut g = self.lock();
        for id in ids {
            if !g.state.comments.iter().any(|c| &c.id == id) {
                return Err(RegistryError::UnknownComment(id.clone()));
            }
        }
        let before = g.state.comments.len();
        let mut removed_ids: Vec<String> = Vec::new();
        g.state.comments.retain(|c| {
            let gone = ids.contains(&c.id) || c.parent.as_ref().map(|p| ids.contains(p)).unwrap_or(false);
            if gone {
                removed_ids.push(c.id.clone());
            }
            !gone
        });
        self.broadcast(&mut g, LiveEvent::CommentRemoved { ids: removed_ids });
        self.persist_state(&g)?;
        Ok(before - g.state.comments.len())
    }

    pub fn comments(&self, f: &CommentFilter) -> Vec<Comment> {
        let g = self.lock();
        g.state
            .comments
            .iter()
            .filter(|c| f.doc.map(|d| c.doc == d).unwrap_or(true))
            .filter(|c| f.kind.map(|k| c.kind == k).unwrap_or(true))
            .filter(|c| f.author.as_ref().map(|a| &c.author == a).unwrap_or(true))
            .filter(|c| match f.status.as_deref() {
                Some("resolved") => c.resolved,
                Some("unresolved") => !c.resolved,
                _ => true,
            })
            .filter(|c| f.since.map(|s| c.seq > s).unwrap_or(true))
            .filter(|c| f.outdated.map(|o| c.anchor.outdated == o).unwrap_or(true))
            .cloned()
            .collect()
    }

    pub fn comment(&self, id: &str) -> Option<Comment> {
        self.lock().state.comments.iter().find(|c| c.id == id).cloned()
    }

    pub fn context(&self, comment: &Comment, radius: u32) -> Vec<(u32, String)> {
        let g = self.lock();
        let Some(doc) = g.docs.get(&comment.doc) else { return Vec::new() };
        let lines: Vec<&str> = doc.content.lines().collect();
        let center = comment.anchor.line.max(1) as usize;
        let start = center.saturating_sub(radius as usize).max(1);
        let end = (center + radius as usize).min(lines.len());
        (start..=end).filter_map(|n| lines.get(n - 1).map(|l| (n as u32, l.to_string()))).collect()
    }

    pub fn navigate(&self, target: NavTarget) -> usize {
        let mut g = self.lock();
        let viewers = self.tx.receiver_count();
        self.broadcast(&mut g, LiveEvent::Navigate { target, viewers });
        viewers
    }

    pub fn runs(&self) -> Vec<Run> {
        self.lock().runs.values().cloned().collect()
    }

    pub fn run(&self, id: &str) -> Option<Run> {
        self.lock().runs.get(id).cloned()
    }

    pub fn upsert_run(&self, run: Run, event: fn(Run) -> LiveEvent) -> anyhow::Result<()> {
        let mut g = self.lock();
        self.store.save_run(&run)?;
        g.runs.insert(run.id.clone(), run.clone());
        self.broadcast(&mut g, event(run));
        Ok(())
    }

    pub fn remove_run(&self, id: &str) -> anyhow::Result<bool> {
        let mut g = self.lock();
        if g.runs.remove(id).is_none() {
            return Ok(false);
        }
        self.store.remove_run(id)?;
        self.broadcast(&mut g, LiveEvent::RunRemoved { id: id.to_string() });
        self.persist_state(&g)?;
        Ok(true)
    }

    pub fn append_run_event(&self, run_id: &str, seq: u64, event: serde_json::Value) -> anyhow::Result<()> {
        let line = serde_json::json!({ "seq": seq, "at": plantool_core::now(), "event": event });
        self.store.append_run_event(run_id, &line.to_string())?;
        let mut g = self.lock();
        self.broadcast(&mut g, LiveEvent::RunEvent { run_id: run_id.to_string(), seq, event });
        Ok(())
    }

    pub fn seq(&self) -> u64 {
        self.lock().state.seq
    }
}

pub struct Registry {
    pub home: PathBuf,
    sessions: RwLock<BTreeMap<String, Arc<LiveSession>>>,
}

impl Registry {
    pub fn load(home: PathBuf) -> anyhow::Result<Registry> {
        let mut map = BTreeMap::new();
        for (_, _, dir) in store::list_session_dirs(&home) {
            match LiveSession::open(SessionStore::new(dir.clone())) {
                Ok(Some(s)) => {
                    map.insert(s.key.clone(), s);
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("skipping {}: {e}", dir.display()),
            }
        }
        let reg = Registry { home, sessions: RwLock::new(map) };
        for s in reg.all() {
            for kind in DocKind::ALL {
                if let Err(e) = s.capture_doc(kind) {
                    tracing::warn!("{}: {kind}: {e}", s.key);
                }
            }
        }
        Ok(reg)
    }

    pub fn all(&self) -> Vec<Arc<LiveSession>> {
        self.sessions.read().unwrap_or_else(|e| e.into_inner()).values().cloned().collect()
    }

    pub fn get_key(&self, key: &str) -> Option<Arc<LiveSession>> {
        self.sessions.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    pub fn resolve(&self, reference: &str, cwd: Option<&Path>) -> Result<Arc<LiveSession>, RegistryError> {
        let reference = reference.trim().trim_matches('/');
        if let Some(s) = self.get_key(reference) {
            return Ok(s);
        }
        let all = self.all();
        let mut matches: Vec<Arc<LiveSession>> = all.iter().filter(|s| s.session().slug == reference).cloned().collect();
        if matches.len() > 1 {
            if let Some(cwd) = cwd {
                if let Ok(c) = git::detect_checkout(cwd) {
                    let narrowed: Vec<Arc<LiveSession>> = matches.iter().filter(|s| same_repo(&s.session(), &c)).cloned().collect();
                    if narrowed.len() == 1 {
                        matches = narrowed;
                    }
                }
            }
        }
        match matches.len() {
            0 => Err(RegistryError::NotFound(reference.to_string())),
            1 => Ok(matches.remove(0)),
            _ => Err(RegistryError::Ambiguous { reference: reference.to_string(), keys: matches.iter().map(|s| s.key.clone()).collect() }),
        }
    }

    pub fn create(&self, intent: CreateSession) -> Result<(Arc<LiveSession>, bool), RegistryError> {
        if !valid_slug(&intent.slug) {
            return Err(RegistryError::BadSlug(intent.slug));
        }
        let start = intent.repo.clone().or(intent.cwd.clone()).unwrap_or_else(|| PathBuf::from("."));
        let checkout = git::detect_checkout(&start)?;
        let repo_slug = match &intent.repo_slug {
            Some(r) => git::sanitize_slug(r),
            None => git::repo_slug(&checkout),
        };
        let key = format!("{repo_slug}/{}", intent.slug);
        if let Some(existing) = self.get_key(&key) {
            let s = existing.session();
            if same_repo(&s, &checkout) {
                return Ok((existing, false));
            }
            return Err(RegistryError::RepoMismatch { key, existing: s.repo.root.display().to_string() });
        }
        let base = intent.base.clone().unwrap_or_else(|| git::default_base(&checkout));
        let worktree = if intent.worktree {
            let dir = intent
                .worktree_dir
                .clone()
                .unwrap_or_else(|| git::main_root(&checkout.common_dir).join(".claude").join("worktrees").join(&intent.slug));
            git::worktree_add(&checkout, &dir, &intent.slug, &base)?;
            Some(dir)
        } else {
            None
        };
        let session = Session {
            repo_slug: repo_slug.clone(),
            slug: intent.slug.clone(),
            title: intent.title.clone().unwrap_or_else(|| intent.slug.replace(['-', '_'], " ")),
            repo: checkout,
            worktree,
            base,
            mirror: intent.mirror,
            brief: intent.brief.as_deref().map(str::trim).filter(|b| !b.is_empty()).map(|b| b.to_string()),
            created_in: intent.repo.clone().or(intent.cwd.clone()),
            created_at: plantool_core::now(),
        };
        let dir = store::session_dir(&self.home, &repo_slug, &intent.slug);
        let st = SessionStore::new(dir);
        std::fs::create_dir_all(&st.dir).map_err(|e| anyhow::anyhow!(e))?;
        st.save_meta(&session)?;
        st.save_state(&State::default())?;
        let live = LiveSession::open(st)?.ok_or_else(|| anyhow::anyhow!("failed to open new session"))?;
        for kind in DocKind::ALL {
            let _ = live.capture_doc(kind);
        }
        self.sessions.write().unwrap_or_else(|e| e.into_inner()).insert(key, live.clone());
        Ok((live, true))
    }

    /// Drop a session: its folder under the plantool home and, on request, its git worktree.
    pub fn remove(&self, key: &str, remove_worktree: bool) -> Result<Session, RegistryError> {
        let live = self.get_key(key).ok_or_else(|| RegistryError::NotFound(key.to_string()))?;
        let session = live.session();
        if remove_worktree {
            if let Some(wt) = &session.worktree {
                git::worktree_remove(&session.repo, wt)?;
            }
        }
        {
            let mut g = live.lock();
            live.broadcast(&mut g, LiveEvent::SessionRemoved { key: key.to_string() });
        }
        self.sessions.write().unwrap_or_else(|e| e.into_inner()).remove(key);
        std::fs::remove_dir_all(&live.store.dir).map_err(|e| anyhow::anyhow!("removing {}: {e}", live.store.dir.display()))?;
        Ok(session)
    }

    /// Repositories seen across sessions, most recently used first.
    pub fn repos(&self) -> Vec<RepoInfo> {
        let mut by_root: BTreeMap<PathBuf, RepoInfo> = BTreeMap::new();
        for s in self.all() {
            let sess = s.session();
            let st = s.state();
            let last = if st.updated_at.is_empty() { sess.created_at.clone() } else { st.updated_at.clone() };
            let e = by_root.entry(sess.repo.root.clone()).or_insert_with(|| RepoInfo { root: sess.repo.root.clone(), repo_slug: sess.repo_slug.clone(), branch: sess.repo.branch.clone(), base: sess.base.clone(), sessions: 0, last_used: String::new() });
            e.sessions += 1;
            if last > e.last_used {
                e.last_used = last;
                e.branch = sess.repo.branch.clone();
                e.base = sess.base.clone();
            }
        }
        let mut out: Vec<RepoInfo> = by_root.into_values().collect();
        out.sort_by(|a, b| b.last_used.cmp(&a.last_used));
        out
    }

    pub fn list(&self, repo: Option<&str>, stage: Option<Stage>) -> Vec<SessionView> {
        let checkout = repo.and_then(|r| git::detect_checkout(Path::new(r)).ok());
        self.all()
            .into_iter()
            .filter(|s| {
                let sess = s.session();
                match (repo, &checkout) {
                    (Some(_), Some(c)) => same_repo(&sess, c),
                    (Some(r), None) => sess.repo_slug == r,
                    (None, _) => true,
                }
            })
            .filter(|s| stage.map(|st| s.stage() == st).unwrap_or(true))
            .map(|s| s.view())
            .collect()
    }

    pub fn capture_path(&self, path: &Path) -> Option<(Arc<LiveSession>, DocKind)> {
        let file = path.file_name()?.to_str()?;
        let kind = DocKind::from_file_name(file)?;
        let dir = path.parent()?;
        let slug = dir.file_name()?.to_str()?;
        let repo_slug = dir.parent()?.file_name()?.to_str()?;
        let key = format!("{repo_slug}/{slug}");
        let s = self.get_key(&key)?;
        Some((s, kind))
    }
}

pub fn same_repo(s: &Session, c: &plantool_core::Checkout) -> bool {
    let a = std::fs::canonicalize(&s.repo.common_dir).unwrap_or_else(|_| s.repo.common_dir.clone());
    let b = std::fs::canonicalize(&c.common_dir).unwrap_or_else(|_| c.common_dir.clone());
    a == b
}
