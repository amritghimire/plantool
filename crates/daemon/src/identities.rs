//! Non-human credentials are ephemeral capabilities, never browser identity.
use crate::{routes::ApiError, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub const RUN_TOKEN_HEADER: &str = "x-plantool-run-token";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunScope {
    /// The current Personal repo/session pair is the project/item identity.
    pub session: String,
    /// Every hosted dispatch, including a resume, gets a new run ID.
    pub attempt: String,
}

#[derive(Default)]
pub struct RunTokens(Mutex<HashMap<[u8; 32], Entry>>);
#[derive(Clone)]
struct Entry {
    scope: Option<RunScope>,
    revoked: tokio::sync::watch::Sender<bool>,
}
#[derive(Clone)]
pub struct AuthorizedRun {
    pub scope: RunScope,
    pub revoked: tokio::sync::watch::Receiver<bool>,
}

/// Own this lease for the provider's lifetime. Dropping it revokes the credential,
/// including when spawning fails or the supervisor task is cancelled.
pub struct RunCredential {
    secret: String,
    tokens: Arc<RunTokens>,
}
impl std::fmt::Debug for RunCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunCredential([REDACTED])")
    }
}
impl RunTokens {
    pub fn issue(self: &Arc<Self>, scope: RunScope) -> RunCredential {
        let secret = crate::config::random_token();
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(
            hash(&secret),
            Entry {
                scope: Some(scope),
                revoked: tokio::sync::watch::channel(false).0,
            },
        );
        RunCredential {
            secret,
            tokens: self.clone(),
        }
    }
    fn lookup(&self, secret: &str) -> Option<Entry> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&hash(secret))
            .cloned()
    }
    pub(crate) fn is_worker(&self, headers: &axum::http::HeaderMap) -> bool {
        headers
            .get(RUN_TOKEN_HEADER)
            .and_then(|h| h.to_str().ok())
            .and_then(|v| self.lookup(v))
            .is_some_and(|e| e.scope.is_none() && !*e.revoked.borrow())
    }
    pub fn revoke_all(&self) {
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for entry in entries.values() {
            entry.revoked.send_replace(true);
        }
        entries.clear();
    }
}
fn hash(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}
impl Drop for RunCredential {
    fn drop(&mut self) {
        if let Some(entry) = self
            .tokens
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&hash(&self.secret))
        {
            entry.revoked.send_replace(true);
        }
    }
}

/// Applied to each provider process, including providers that spawn once per turn.
#[derive(Clone)]
pub struct RunEnvironment {
    token: String,
    home: PathBuf,
    port: u16,
}
impl std::fmt::Debug for RunEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunEnvironment")
            .field("token", &"[REDACTED]")
            .finish()
    }
}
impl RunCredential {
    pub fn environment(&self, home: PathBuf, port: u16) -> RunEnvironment {
        RunEnvironment {
            token: self.secret.clone(),
            home,
            port,
        }
    }
    #[cfg(test)]
    fn secret(&self) -> &str {
        &self.secret
    }
}
impl RunEnvironment {
    pub fn apply(&self, command: &mut tokio::process::Command) {
        command
            .env("PLANTOOL_RUN_TOKEN", &self.token)
            .env("PLANTOOL_HOME", &self.home)
            .env("PLANTOOL_PORT", self.port.to_string());
    }
}

fn denied() -> ApiError {
    ApiError(
        StatusCode::FORBIDDEN,
        "Run credential is invalid, expired, or outside its session/attempt scope".into(),
    )
}

/// A presented invalid credential must never fall back to anonymous Personal access.
/// Keep an explicit agent-operation allowlist so new endpoints fail closed.
pub fn authorize(state: &AppState, req: &Request<Body>) -> Result<Option<AuthorizedRun>, ApiError> {
    let headers = req.headers();
    if headers.contains_key("authorization") {
        return Err(denied());
    }
    let Some(value) = headers.get(RUN_TOKEN_HEADER) else {
        return Ok(None);
    };
    if headers.get_all(RUN_TOKEN_HEADER).iter().count() != 1 {
        return Err(denied());
    }
    let entry = value
        .to_str()
        .ok()
        .and_then(|v| state.runs.tokens.lookup(v))
        .ok_or_else(denied)?;
    if *entry.revoked.borrow() {
        return Err(denied());
    }
    let path = req.uri().path();
    let Some(scope) = entry.scope else {
        // The inherited-fd worker identity has no Personal/browser authority.
        // Contract routes arrive in Phase 2; until then only socket health exists.
        return if req
            .extensions()
            .get::<crate::socket::SocketTransport>()
            .is_some()
            && ((req.method() == "GET" && matches!(path, "/health" | "/api/health"))
                || path.starts_with("/internal/v1/")
                || path == "/v1/chat/completions")
        {
            Ok(None)
        } else {
            Err(denied())
        };
    };
    let authorized = AuthorizedRun {
        scope: scope.clone(),
        revoked: entry.revoked.subscribe(),
    };
    if req.method() == "GET" && matches!(path, "/health" | "/api/health") {
        return Ok(Some(authorized));
    }
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| denied())?;
    let Some(rest) = decoded.strip_prefix("/api/sessions/") else {
        return Err(denied());
    };
    let parts: Vec<_> = rest.split('/').collect();
    // The CLI resolves short references before issuing a command. Resolve them with
    // the same cwd query as the handler, rather than comparing an ambiguous slug.
    if parts.len() == 1 && req.method() == "GET" {
        let url = webauthn_rs::prelude::Url::parse(&format!("http://localhost{}", req.uri()))
            .map_err(|_| denied())?;
        let cwd = url
            .query_pairs()
            .find(|(k, _)| k == "cwd")
            .map(|(_, v)| PathBuf::from(v.as_ref()));
        let session = state
            .registry
            .resolve(parts[0], cwd.as_deref())
            .map_err(|_| denied())?;
        return if session.key == scope.session {
            Ok(Some(authorized))
        } else {
            Err(denied())
        };
    }
    if parts.len() < 2 || format!("{}/{}", parts[0], parts[1]) != scope.session {
        return Err(denied());
    }
    let tail = &parts[2..];
    if matches!(tail, ["docs", "research" | "plan"]) {
        let url = webauthn_rs::prelude::Url::parse(&format!("http://localhost{}", req.uri()))
            .map_err(|_| denied())?;
        for (key, value) in url.query_pairs() {
            // Revision selectors become filenames in the existing store. A run
            // capability must not use that indirection to read another item.
            if key == "sha" && (value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(denied());
            }
        }
    }

    if tail == ["changes", "file"] {
        let session = state.registry.get_key(&scope.session).ok_or_else(denied)?;
        let root = crate::git::main_root(&session.session().repo.root);
        let policy = crate::sandbox::NeverSent::new(&state.runs.execution.rules(&root))
            .map_err(|_| denied())?;
        let url = webauthn_rs::prelude::Url::parse(&format!("http://localhost{}", req.uri()))
            .map_err(|_| denied())?;
        if url.query_pairs().any(|(key, value)| {
            key == "path" && policy.matches(std::path::Path::new(value.as_ref()))
        }) {
            return Err(denied());
        }
    }
    let allowed = match req.method().as_str() {
        "GET" => match tail {
            []
            | ["where"]
            | ["comments"]
            | ["changes"]
            | ["changes", "file"]
            | ["export"]
            | ["live"] => true,
            ["comments", _, "context"]
            | ["docs", "research" | "plan"]
            | ["docs", "research" | "plan", "path"]
            | ["docs", "plan", "revisions"]
            | ["prompt", _] => true,
            ["runs", id, "events"] => *id == scope.attempt,
            _ => false,
        },
        "POST" => matches!(
            tail,
            ["tasks", _, "done"]
                | ["comments"]
                | ["comments", "batch" | "edit"]
                | ["where"]
                | ["plan", "propose-revision"]
                | ["stage"]
                | ["navigate"]
                | ["brief"]
                | ["pause"]
                | ["worktree"]
                | ["docs", "research" | "plan", "touch"]
        ),
        _ => false,
    };
    if allowed {
        Ok(Some(authorized))
    } else {
        Err(denied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn worker_credential_uses_an_inherited_fd_and_expires_with_its_lease() {
        let tokens = Arc::new(RunTokens::default());
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", "IFS= read -r token <&0; printf '%s' \"$token\""]);
        let lease = tokens.worker_command(&mut command).unwrap();
        assert!(command
            .as_std()
            .get_envs()
            .all(|(_, value)| value
                .is_none_or(|v| !v.to_string_lossy().contains(lease._lease.secret()))));
        let output = command.output().await.unwrap();
        assert!(output.status.success());
        assert!(output.stdout == lease._lease.secret().as_bytes());
        let old = lease._lease.secret().to_owned();
        assert!(tokens.lookup(&old).unwrap().scope.is_none());
        drop(lease);
        assert!(tokens.lookup(&old).is_none());
    }

    #[test]
    fn credentials_are_unique_ephemeral_and_redacted() {
        let tokens = Arc::new(RunTokens::default());
        let scope = RunScope {
            session: "project/item".into(),
            attempt: "attempt".into(),
        };
        let one = tokens.issue(scope.clone());
        let two = tokens.issue(scope.clone());
        assert_ne!(one.secret(), two.secret());
        assert_eq!(tokens.lookup(one.secret()).unwrap().scope, Some(scope));
        assert!(!format!("{one:?}").contains(one.secret()));
        let old = one.secret().to_string();
        drop(one);
        assert!(tokens.lookup(&old).is_none());
        tokens.revoke_all();
        assert!(tokens.lookup(two.secret()).is_none());
    }
}

#[cfg(unix)]
pub struct WorkerCredential {
    _lease: RunCredential,
}

#[cfg(unix)]
impl RunTokens {
    /// Identity groundwork only: attach a worker credential through inherited fd 0.
    /// No worker runtime or contract endpoint is implemented in Phase 0.
    pub fn worker_command(
        self: &Arc<Self>,
        command: &mut tokio::process::Command,
    ) -> std::io::Result<WorkerCredential> {
        use std::{io::Write, os::fd::OwnedFd};
        let (reader, mut writer) = std::os::unix::net::UnixStream::pair()?;
        let secret = crate::config::random_token();
        writer.write_all(format!("{secret}\n").as_bytes())?;
        writer.shutdown(std::net::Shutdown::Write)?;
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(
            hash(&secret),
            Entry {
                scope: None,
                revoked: tokio::sync::watch::channel(false).0,
            },
        );
        let fd: OwnedFd = reader.into();
        command
            .stdin(std::process::Stdio::from(fd))
            .env("PLANTOOL_WORKER_TOKEN_FD", "0")
            .env_remove("PLANTOOL_RUN_TOKEN");
        Ok(WorkerCredential {
            _lease: RunCredential {
                secret,
                tokens: self.clone(),
            },
        })
    }
}

#[cfg(unix)]
impl WorkerCredential {
    pub(crate) fn token(&self) -> &str {
        &self._lease.secret
    }
}
