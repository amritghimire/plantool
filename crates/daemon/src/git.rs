use plantool_core::Checkout;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git is not installed or not on PATH")]
    Missing,
    #[error("{0} is not inside a git repository")]
    NotARepo(PathBuf),
    #[error("git {args}: {stderr}")]
    Failed { args: String, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn git(cwd: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = Command::new("git").args(args).current_dir(cwd).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::Missing
        } else {
            GitError::Io(e)
        }
    })?;
    if !out.status.success() {
        return Err(GitError::Failed {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

pub fn detect_checkout(start: &Path) -> Result<Checkout, GitError> {
    let start = if start.is_dir() { start.to_path_buf() } else { start.parent().map(Path::to_path_buf).unwrap_or_else(|| start.to_path_buf()) };
    let out = git(&start, &["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-common-dir"]).map_err(|e| match e {
        GitError::Failed { .. } => GitError::NotARepo(start.clone()),
        other => other,
    })?;
    let mut lines = out.lines();
    let root = PathBuf::from(lines.next().unwrap_or_default());
    let common_dir = PathBuf::from(lines.next().unwrap_or_default());
    let branch = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "HEAD".to_string());
    Ok(Checkout { root, common_dir, branch })
}

pub fn main_root(common_dir: &Path) -> PathBuf {
    if common_dir.file_name().and_then(|n| n.to_str()) == Some(".git") {
        common_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| common_dir.to_path_buf())
    } else {
        common_dir.to_path_buf()
    }
}

pub fn sanitize_slug(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in s.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "repo".to_string()
    } else {
        trimmed
    }
}

pub fn slug_from_remote(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let no_git = url.strip_suffix(".git").unwrap_or(url);
    let no_git = no_git.trim_end_matches('/');
    let path = if let Some((_, rest)) = no_git.split_once("://") {
        rest.split_once('/').map(|(_, p)| p.to_string()).unwrap_or_default()
    } else if let Some((_, rest)) = no_git.split_once(':') {
        rest.to_string()
    } else {
        no_git.to_string()
    };
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let tail = if parts.len() >= 2 { &parts[parts.len() - 2..] } else { &parts[..] };
    Some(sanitize_slug(&tail.join("-")))
}

pub fn repo_slug(c: &Checkout) -> String {
    let root = main_root(&c.common_dir);
    if let Ok(url) = git(&c.root, &["config", "--get", "remote.origin.url"]) {
        if let Some(s) = slug_from_remote(&url) {
            return s;
        }
    }
    root.file_name().map(|n| sanitize_slug(&n.to_string_lossy())).unwrap_or_else(|| "repo".to_string())
}

pub fn default_base(c: &Checkout) -> String {
    if let Ok(r) = git(&c.root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
        if let Some((_, b)) = r.split_once('/') {
            return b.to_string();
        }
    }
    for cand in ["main", "master", "develop"] {
        if git(&c.root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{cand}")]).is_ok() {
            return cand.to_string();
        }
    }
    c.branch.clone()
}

pub fn worktree_add(c: &Checkout, path: &Path, branch: &str, base: &str) -> Result<(), GitError> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let p = path.to_string_lossy().to_string();
    let exists = git(&c.root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok();
    if exists {
        git(&c.root, &["worktree", "add", &p, branch])?;
    } else {
        git(&c.root, &["worktree", "add", "-b", branch, &p, base])?;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileStat {
    pub path: String,
    pub added: u32,
    pub deleted: u32,
}

pub fn diff_numstat(cwd: &Path, base: &str) -> Result<Vec<FileStat>, GitError> {
    let out = git(cwd, &["diff", "--numstat", base])?;
    let mut stats = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(3, '\t');
        let a = parts.next().unwrap_or("0");
        let d = parts.next().unwrap_or("0");
        let p = parts.next().unwrap_or("");
        if p.is_empty() {
            continue;
        }
        stats.push(FileStat { path: p.to_string(), added: a.parse().unwrap_or(0), deleted: d.parse().unwrap_or(0) });
    }
    let untracked = git(cwd, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    for p in untracked.lines().filter(|l| !l.is_empty()) {
        let added = std::fs::read_to_string(cwd.join(p)).map(|s| s.lines().count() as u32).unwrap_or(0);
        stats.push(FileStat { path: p.to_string(), added, deleted: 0 });
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_ssh_https_and_bare() {
        assert_eq!(slug_from_remote("git@github.com:datachain-ai/studio.git").as_deref(), Some("datachain-ai-studio"));
        assert_eq!(slug_from_remote("https://github.com/datachain-ai/studio").as_deref(), Some("datachain-ai-studio"));
        assert_eq!(slug_from_remote("ssh://git@github.com/Owner/Repo.git").as_deref(), Some("owner-repo"));
        assert_eq!(slug_from_remote("/srv/git/tools.git").as_deref(), Some("git-tools"));
        assert_eq!(slug_from_remote(""), None);
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_slug("My Repo!!"), "my-repo");
        assert_eq!(sanitize_slug("a__b.c"), "a__b.c");
        assert_eq!(sanitize_slug("///"), "repo");
    }

    #[test]
    fn detects_checkout_and_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]).unwrap();
        git(&root, &["config", "user.email", "t@t"]).unwrap();
        git(&root, &["config", "user.name", "t"]).unwrap();
        std::fs::write(root.join("a.txt"), "hi\n").unwrap();
        git(&root, &["add", "."]).unwrap();
        git(&root, &["commit", "-q", "-m", "init"]).unwrap();
        let c = detect_checkout(&root).unwrap();
        assert_eq!(c.root.canonicalize().unwrap(), root.canonicalize().unwrap());
        assert_eq!(c.branch, "main");
        assert_eq!(repo_slug(&c), "repo");
        assert_eq!(default_base(&c), "main");

        let wt = tmp.path().join("wt");
        worktree_add(&c, &wt, "feature", "main").unwrap();
        let c2 = detect_checkout(&wt).unwrap();
        assert_eq!(c2.branch, "feature");
        assert_eq!(main_root(&c2.common_dir).canonicalize().unwrap(), root.canonicalize().unwrap());
        assert_eq!(repo_slug(&c2), "repo");

        std::fs::write(wt.join("b.txt"), "x\ny\n").unwrap();
        let stat = diff_numstat(&wt, "main").unwrap();
        assert_eq!(stat.len(), 1);
        assert_eq!(stat[0].path, "b.txt");
        assert_eq!(stat[0].added, 2);
    }

    #[test]
    fn not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(detect_checkout(tmp.path()), Err(GitError::NotARepo(_))));
    }
}
