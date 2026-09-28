use plantool_core::Checkout;
use std::ffi::OsString;
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
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd);
    if args.first() == Some(&"commit") {
        if let Some(path) = commit_hook_path(cwd) {
            command.env("PATH", path);
        }
    }
    let out = command.output().map_err(|e| {
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

/// Git hooks run under the daemon's environment, which may not include the repo's virtualenv.
fn commit_hook_path(cwd: &Path) -> Option<OsString> {
    let mut paths = Vec::new();
    let checkout_bin = cwd.join(".venv").join(if cfg!(windows) { "Scripts" } else { "bin" });
    if checkout_bin.is_dir() {
        paths.push(checkout_bin);
    }
    if let Ok(common_dir) = git(cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
        let main_bin = main_root(Path::new(&common_dir)).join(".venv").join(if cfg!(windows) { "Scripts" } else { "bin" });
        if main_bin.is_dir() && !paths.contains(&main_bin) {
            paths.push(main_bin);
        }
    }
    if paths.is_empty() {
        return None;
    }
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    std::env::join_paths(paths).ok()
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

pub const DEFAULT_WORKTREE_DIR: &str = ".worktree/{slug}";

/// Where a session's worktree goes when none is given: `git config plantool.worktreeDir`, else
/// `.worktree/{slug}`. `{repo}` is the main checkout's folder name and `{slug}` the session slug
/// (appended when absent); `~/` is the home directory and relative paths start at the main checkout.
pub fn default_worktree_dir(c: &Checkout, slug: &str) -> PathBuf {
    let root = main_root(&c.common_dir);
    resolve_worktree_dir(&root, &worktree_dir_template(&root), slug)
}

pub const WORKTREE_DIR_KEY: &str = "plantool.worktreeDir";

/// The template in effect for a checkout: its own value, else the global one, else the default.
pub fn worktree_dir_template(root: &Path) -> String {
    git(root, &["config", "--get", WORKTREE_DIR_KEY])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_WORKTREE_DIR.to_string())
}

/// `plantool.worktreeDir` as set in the global git config, or in this checkout's own config.
pub fn worktree_dir_setting(root: &Path, global: bool) -> Option<String> {
    let scope = if global { "--global" } else { "--local" };
    git(root, &["config", scope, "--get", WORKTREE_DIR_KEY]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Set `plantool.worktreeDir` in the global or the checkout's config; an empty value unsets it.
pub fn set_worktree_dir_setting(root: &Path, global: bool, value: Option<&str>) -> Result<(), GitError> {
    let scope = if global { "--global" } else { "--local" };
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => git(root, &["config", scope, WORKTREE_DIR_KEY, v]).map(|_| ()),
        None if worktree_dir_setting(root, global).is_none() => Ok(()),
        None => git(root, &["config", scope, "--unset-all", WORKTREE_DIR_KEY]).map(|_| ()),
    }
}

pub fn resolve_worktree_dir(root: &Path, template: &str, slug: &str) -> PathBuf {
    let repo = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".to_string());
    let mut path = template.replace("{repo}", &repo);
    if path.contains("{slug}") {
        path = path.replace("{slug}", slug);
    } else {
        path = format!("{}/{slug}", path.trim_end_matches('/'));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let path = match (path.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    };
    let path = if path.is_absolute() { path } else { root.join(path) };
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

pub fn worktree_remove(c: &Checkout, path: &Path) -> Result<(), GitError> {
    if !path.exists() {
        return Ok(());
    }
    let p = path.to_string_lossy().to_string();
    git(&c.root, &["worktree", "remove", "--force", &p])?;
    Ok(())
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

pub fn head_sha(cwd: &Path) -> Result<String, GitError> {
    git(cwd, &["rev-parse", "HEAD"])
}

pub fn is_dirty(cwd: &Path) -> Result<bool, GitError> {
    Ok(!git(cwd, &["status", "--porcelain", "--untracked-files=all"])?.is_empty())
}

pub fn commit_all(cwd: &Path, message: &str) -> Result<String, GitError> {
    match commit_milestone(cwd, message, false)? {
        CommitOutcome::Committed(sha) | CommitOutcome::HookRewrote { sha: Some(sha), .. } => Ok(sha),
        CommitOutcome::HookRewrote { files, .. } => Err(GitError::Failed { args: "commit".into(), stderr: format!("hooks rewrote {}", files.join(", ")) }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    Committed(String),
    /// Hooks changed files. `sha` is set when the commit still landed; the rewritten files are staged either way.
    HookRewrote { sha: Option<String>, files: Vec<String> },
}

/// Paths with edits that are not in the index. Everything is staged before a milestone commit
/// runs, so anything unstaged afterwards came from a hook.
fn unstaged_paths(cwd: &Path) -> Result<Vec<String>, GitError> {
    let mut files: Vec<String> = git(cwd, &["diff", "--name-only"])?.lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
    files.extend(git(cwd, &["ls-files", "--others", "--exclude-standard"])?.lines().filter(|l| !l.is_empty()).map(str::to_string));
    files.sort();
    files.dedup();
    Ok(files)
}

/// Stage everything and commit it as a milestone. When a hook rewrites files, the rewritten
/// files are staged and reported instead of being silently left behind.
pub fn commit_milestone(cwd: &Path, message: &str, amend: bool) -> Result<CommitOutcome, GitError> {
    git(cwd, &["add", "-A"])?;
    let mut args = vec!["commit", "-q", "-m", message];
    if amend {
        args.push("--amend");
    }
    let result = git(cwd, &args);
    let files = unstaged_paths(cwd)?;
    match result {
        Ok(_) if files.is_empty() => Ok(CommitOutcome::Committed(head_sha(cwd)?)),
        Ok(_) => {
            git(cwd, &["add", "-A"])?;
            Ok(CommitOutcome::HookRewrote { sha: Some(head_sha(cwd)?), files })
        }
        Err(_) if !files.is_empty() => {
            git(cwd, &["add", "-A"])?;
            Ok(CommitOutcome::HookRewrote { sha: None, files })
        }
        Err(e) => Err(e),
    }
}

/// Unified diff of one path between `base` and the working tree. Untracked files diff against
/// nothing, so the whole file shows as added.
pub fn diff_file(cwd: &Path, base: &str, path: &str) -> Result<String, GitError> {
    let tracked = git(cwd, &["ls-files", "--error-unmatch", "--", path]).is_ok();
    if tracked {
        return git(cwd, &["diff", "--no-color", base, "--", path]);
    }
    let out = Command::new("git").args(["diff", "--no-color", "--no-index", "--", "/dev/null", path]).current_dir(cwd).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
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
    fn resolves_worktree_dir_templates() {
        let root = Path::new("/code/app");
        assert_eq!(resolve_worktree_dir(root, DEFAULT_WORKTREE_DIR, "fix-x"), PathBuf::from("/code/app/.worktree/fix-x"));
        assert_eq!(resolve_worktree_dir(root, "../{repo}-worktrees/{slug}", "fix-x"), PathBuf::from("/code/app-worktrees/fix-x"));
        assert_eq!(resolve_worktree_dir(root, "/tmp/wt/", "fix-x"), PathBuf::from("/tmp/wt/fix-x"));
        assert_eq!(resolve_worktree_dir(root, "./trees/{repo}-{slug}", "fix-x"), PathBuf::from("/code/app/trees/app-fix-x"));
    }

    #[test]
    fn reads_worktree_dir_from_git_config() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]).unwrap();
        let c = detect_checkout(&root).unwrap();
        let main = main_root(&c.common_dir);
        assert_eq!(default_worktree_dir(&c, "s"), main.join(".worktree").join("s"));
        set_worktree_dir_setting(&root, false, Some("../{repo}-worktrees")).unwrap();
        assert_eq!(worktree_dir_setting(&root, false).as_deref(), Some("../{repo}-worktrees"));
        assert_eq!(default_worktree_dir(&c, "s"), main.parent().unwrap().join("repo-worktrees").join("s"));
        set_worktree_dir_setting(&root, false, Some("  ")).unwrap();
        assert_eq!(worktree_dir_setting(&root, false), None);
        set_worktree_dir_setting(&root, false, None).unwrap();
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
    fn commits_and_diffs_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]).unwrap();
        git(&root, &["config", "user.email", "t@t"]).unwrap();
        git(&root, &["config", "user.name", "t"]).unwrap();
        std::fs::write(root.join("a.txt"), "hi\n").unwrap();
        git(&root, &["add", "."]).unwrap();
        git(&root, &["commit", "-q", "-m", "init"]).unwrap();
        let first = head_sha(&root).unwrap();
        assert!(!is_dirty(&root).unwrap());

        std::fs::write(root.join("a.txt"), "hi\nthere\n").unwrap();
        std::fs::write(root.join("new.txt"), "x\n").unwrap();
        assert!(is_dirty(&root).unwrap());
        let tracked = diff_file(&root, "HEAD", "a.txt").unwrap();
        assert!(tracked.contains("+there"), "{tracked}");
        let untracked = diff_file(&root, "HEAD", "new.txt").unwrap();
        assert!(untracked.contains("+x"), "{untracked}");

        let second = commit_all(&root, "Milestone 1").unwrap();
        assert_ne!(first, second);
        assert!(!is_dirty(&root).unwrap());
        assert!(diff_numstat(&root, &second).unwrap().is_empty());
        assert_eq!(git(&root, &["log", "-1", "--format=%s"]).unwrap(), "Milestone 1");
    }

    fn hook_repo(hook: &str) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]).unwrap();
        git(&root, &["config", "user.email", "t@t"]).unwrap();
        git(&root, &["config", "user.name", "t"]).unwrap();
        git(&root, &["config", "core.hooksPath", ".git/hooks"]).unwrap();
        std::fs::write(root.join("a.txt"), "hi\n").unwrap();
        git(&root, &["add", "."]).unwrap();
        git(&root, &["commit", "-q", "-m", "init"]).unwrap();
        let path = root.join(".git/hooks/pre-commit");
        std::fs::write(&path, hook).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(root.join("a.txt"), "hi\nchanged\n").unwrap();
        (tmp, root)
    }

    #[cfg(unix)]
    #[test]
    fn hook_that_rewrites_and_fails_stages_its_edits() {
        let (_tmp, root) = hook_repo("#!/bin/sh\nprintf 'formatted\\n' >> a.txt\nexit 1\n");
        let out = commit_milestone(&root, "Milestone 1", false).unwrap();
        assert_eq!(out, CommitOutcome::HookRewrote { sha: None, files: vec!["a.txt".into()] });
        assert_eq!(git(&root, &["log", "--oneline"]).unwrap().lines().count(), 1, "nothing committed");
        assert!(git(&root, &["diff", "--name-only"]).unwrap().is_empty(), "hook edits are staged");
        // the hook appends again on the retry, so disable it to mimic a now-clean formatter run
        std::fs::remove_file(root.join(".git/hooks/pre-commit")).unwrap();
        assert!(matches!(commit_milestone(&root, "Milestone 1", false).unwrap(), CommitOutcome::Committed(_)));
        assert!(std::fs::read_to_string(root.join("a.txt")).unwrap().contains("formatted"));
    }

    #[cfg(unix)]
    #[test]
    fn hook_that_rewrites_but_passes_leaves_a_commit_to_amend() {
        let (_tmp, root) = hook_repo("#!/bin/sh\nprintf 'formatted\\n' >> a.txt\nexit 0\n");
        let out = commit_milestone(&root, "Milestone 1", false).unwrap();
        let sha = match out { CommitOutcome::HookRewrote { sha: Some(sha), files } => { assert_eq!(files, vec!["a.txt".to_string()]); sha } other => panic!("{other:?}") };
        assert_eq!(head_sha(&root).unwrap(), sha);
        assert!(git(&root, &["diff", "--name-only"]).unwrap().is_empty(), "hook edits are staged");
        std::fs::remove_file(root.join(".git/hooks/pre-commit")).unwrap();
        assert!(matches!(commit_milestone(&root, "Milestone 1", true).unwrap(), CommitOutcome::Committed(_)));
        assert_eq!(git(&root, &["log", "--oneline"]).unwrap().lines().count(), 2, "the retry amended instead of adding a commit");
        assert!(git(&root, &["show", "HEAD:a.txt"]).unwrap().contains("formatted"));
    }

    #[cfg(unix)]
    #[test]
    fn milestone_hook_finds_pre_commit_in_main_checkout_venv() {
        use std::os::unix::fs::PermissionsExt;

        let (_tmp, root) = hook_repo("#!/bin/sh\npre-commit\n");
        let hooks = root.join(".git/hooks");
        git(&root, &["config", "core.hooksPath", hooks.to_str().unwrap()]).unwrap();
        let bin = root.join(".venv/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let pre_commit = bin.join("pre-commit");
        std::fs::write(&pre_commit, "#!/bin/sh\nprintf 'ran' > hook-ran\n").unwrap();
        std::fs::set_permissions(&pre_commit, std::fs::Permissions::from_mode(0o755)).unwrap();

        let worktree = root.parent().unwrap().join("worktree");
        git(&root, &["worktree", "add", "-q", "-b", "milestone", worktree.to_str().unwrap()]).unwrap();
        std::fs::write(worktree.join("a.txt"), "changed\n").unwrap();
        assert!(matches!(commit_milestone(&worktree, "Milestone 1", false).unwrap(), CommitOutcome::HookRewrote { .. }));
        assert_eq!(std::fs::read_to_string(worktree.join("hook-ran")).unwrap(), "ran");
    }

    #[test]
    fn not_a_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(detect_checkout(tmp.path()), Err(GitError::NotARepo(_))));
    }
}
