use super::sessions::resolve;
use super::ApiError;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use std::path::{Component, Path as FsPath, PathBuf};

const HIDDEN: [&str; 4] = ["meta.json", "state.json", "revisions", "runs"];
const MAX_FILES: usize = 500;

fn hidden(rel: &FsPath) -> bool {
    rel.components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .is_some_and(|first| HIDDEN.contains(&first) || first.starts_with('.'))
}

fn kind_of(path: &FsPath) -> &'static str {
    match extension(path).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "ico" => "image",
        "md" | "markdown" => "markdown",
        "txt" | "json" | "jsonl" | "tsv" | "csv" | "log" | "yaml" | "yml" | "toml" | "diff"
        | "patch" | "html" | "xml" | "rs" | "ts" | "tsx" | "js" | "py" | "sh" => "text",
        _ => "other",
    }
}

fn extension(path: &FsPath) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn mime(path: &FsPath) -> &'static str {
    match extension(path).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "json" | "jsonl" => "application/json; charset=utf-8",
        _ if kind_of(path) != "other" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn list_files(root: &FsPath) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            if hidden(rel) || rel == FsPath::new("research.md") || rel == FsPath::new("plan.md") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && out.len() < MAX_FILES {
                out.push(json!({
                    "path": rel.to_string_lossy().replace('\\', "/"),
                    "size": meta.len(),
                    "kind": kind_of(rel),
                }));
            }
        }
    }
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    out
}

// Only plain relative paths that stay inside the session directory, symlinks included.
pub fn safe_path(root: &FsPath, rel: &str) -> Option<PathBuf> {
    let rel = FsPath::new(rel);
    if rel.as_os_str().is_empty()
        || hidden(rel)
        || !rel.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let path = root.join(rel).canonicalize().ok()?;
    (path.starts_with(&root) && path.is_file()).then_some(path)
}

async fn list(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let dir = s.store.dir.clone();
    let files = tokio::task::spawn_blocking(move || list_files(&dir))
        .await
        .map_err(anyhow::Error::from)?;
    Ok(Json(json!({ "dir": s.store.dir, "files": files })))
}

async fn read(
    State(state): State<AppState>,
    Path((repo, slug, rel)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    let path = safe_path(&s.store.dir, &rel)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no session file {rel}")))?;
    let bytes = tokio::fs::read(&path).await.map_err(anyhow::Error::from)?;
    Ok((
        [
            (header::CONTENT_TYPE, mime(&path)),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (
                header::CONTENT_SECURITY_POLICY,
                "sandbox; default-src 'none'; img-src 'self'; style-src 'unsafe-inline'",
            ),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
        .into_response())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sessions/{repo}/{slug}/files", get(list))
        .route("/sessions/{repo}/{slug}/files/{*path}", get(read))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_extra_files_and_serves_only_safe_paths() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("session");
        std::fs::create_dir_all(dir.join("screens")).unwrap();
        std::fs::create_dir_all(dir.join("revisions")).unwrap();
        for file in [
            "plan.md",
            "meta.json",
            "notes.md",
            "screens/login.png",
            "revisions/a.md",
        ] {
            std::fs::write(dir.join(file), "x").unwrap();
        }
        std::fs::write(root.path().join("secret.txt"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("secret.txt"), dir.join("link.txt")).unwrap();

        let paths: Vec<String> = list_files(&dir)
            .iter()
            .map(|f| f["path"].as_str().unwrap().to_string())
            .collect();
        assert!(paths.contains(&"notes.md".to_string()), "{paths:?}");
        assert!(
            paths.contains(&"screens/login.png".to_string()),
            "{paths:?}"
        );
        assert!(
            !paths
                .iter()
                .any(|p| p == "plan.md" || p == "meta.json" || p.starts_with("revisions")),
            "{paths:?}"
        );

        assert!(safe_path(&dir, "screens/login.png").is_some());
        assert!(safe_path(&dir, "../secret.txt").is_none());
        assert!(safe_path(&dir, "/etc/passwd").is_none());
        assert!(safe_path(&dir, "meta.json").is_none());
        assert!(safe_path(&dir, "link.txt").is_none());
        assert_eq!(mime(FsPath::new("page.html")), "text/plain; charset=utf-8");
    }
}
