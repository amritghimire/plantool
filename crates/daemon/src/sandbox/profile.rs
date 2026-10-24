use super::{NeverSent, Paths, Role};
use std::path::{Path, PathBuf};
fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
fn path(path: &Path) -> anyhow::Result<String> {
    let path = if path.exists() {
        path.canonicalize()?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("path has no parent"))?;
        // Resolve the nearest existing ancestor, including /var -> /private/var.
        PathBuf::from(unquoted(parent)?).join(
            path.file_name()
                .ok_or_else(|| anyhow::anyhow!("path has no name"))?,
        )
    };
    Ok(quote(path.to_str().ok_or_else(|| {
        anyhow::anyhow!("sandbox paths must be UTF-8")
    })?))
}
fn unquoted(p: &Path) -> anyhow::Result<String> {
    if p.exists() {
        return Ok(p
            .canonicalize()?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("sandbox paths must be UTF-8"))?
            .into());
    }
    let parent = p
        .parent()
        .ok_or_else(|| anyhow::anyhow!("relative sandbox path"))?;
    Ok(PathBuf::from(unquoted(parent)?)
        .join(
            p.file_name()
                .ok_or_else(|| anyhow::anyhow!("invalid sandbox path"))?,
        )
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("sandbox paths must be UTF-8"))?
        .into())
}
pub fn build(paths: &Paths, temp: &Path, role: Role, policy: &NeverSent) -> anyhow::Result<String> {
    let home = &paths.owner_home;
    let root = paths.worktree.canonicalize()?;
    let mut writable = vec![
        temp.into(),
        home.join(".cache"),
        home.join(".cargo/registry"),
        home.join(".npm"),
        home.join("Library/Caches"),
    ];
    if matches!(role, Role::Agent | Role::EosAgent) {
        writable.push(root.clone());
    }
    if role == Role::Worker {
        writable.push(paths.daemon_home.join("worker-state"));
    }
    let exceptions = writable
        .iter()
        .map(|p| path(p).map(|p| format!("(subpath {p})")))
        .collect::<anyhow::Result<Vec<_>>>()?
        .join(" ");
    // The current flat session directory also contains authoritative state and
    // transcripts. Only live document/draft files are agent-writable.
    let docs = ["research.md", "plan.md", "pr-draft.md", "commit-draft.md"]
        .iter()
        .map(|name| path(&paths.docs.join(name)).map(|p| format!("(literal {p})")))
        .collect::<anyhow::Result<Vec<_>>>()?
        .join(" ");
    let docs = if matches!(role, Role::EosQa | Role::EosReviewer | Role::Worker) {
        String::new()
    } else {
        docs
    };
    let worker_socket = if role == Role::Worker {
        format!(
            "(literal {})",
            path(&paths.daemon_home.join("run/worker.sock"))?
        )
    } else {
        String::new()
    };
    let qa = if role == Role::EosQa {
        let expression = format!(
            r"^{}(/(tests?|[^/]+/tests?)(/.*)?|/(.*/)?(test_[^/]+\.py|[^/]+(_test\.rs|\.test\.[^/]+|\.spec\.[^/]+))|/\.git/.*)$",
            regex::escape(&unquoted(&root)?)
        );
        format!("(regex #\"{}\")", expression.replace('"', "\\\""))
    } else {
        String::new()
    };
    let exceptions = format!("{exceptions} {qa} {worker_socket} {docs} (literal \"/dev/null\")");
    let mut text = format!("(version 1)\n(allow default)\n(deny file-link)\n(deny file-write* (require-not (require-any {exceptions})))\n");
    let mut denied = super::read_guard::protected(paths);
    denied.extend(policy.protected_targets(&root)?);
    if matches!(
        role,
        Role::Worker | Role::EosAgent | Role::EosReviewer | Role::EosQa
    ) {
        denied.push(home.join(".local/share/opencode"));
        denied.push(home.join(".config/opencode"));
        denied.push(home.join(".claude"));
        if role == Role::Worker {
            denied.push(home.join(".codex"));
        }
    }
    for p in denied {
        text.push_str(&format!(
            "(deny file-read* file-write* (subpath {}))\n",
            path(&p)?
        ));
    }
    for expression in policy.absolute_expressions(&root) {
        text.push_str(&format!(
            "(deny file-read* file-write* (regex #{}))\n",
            // Seatbelt's #"..." regex reader preserves backslashes. Applying
            // Scheme string escaping here would turn \. into \\. and match a
            // literal backslash instead of a dot in newly-created filenames.
            format!("\"{}\"", expression.replace('"', "\\\""))
        ));
    }
    if matches!(role, Role::EosAgent | Role::EosQa | Role::EosReviewer) {
        for protected in [".git/config", ".git/hooks", ".git/objects/info"] {
            text.push_str(&format!(
                "(deny file-write* (subpath {}))\n",
                path(&root.join(protected))?
            ));
        }
    }
    if !matches!(role, Role::Agent | Role::EosAgent | Role::EosQa) {
        text.push_str(&format!("(deny file-write* (subpath {}))\n", path(&root)?));
    }
    text.push_str("(deny mach-lookup (global-name \"com.apple.SecurityServer\") (global-name \"com.apple.securityd\"))\n");
    let mut readable = vec![
        PathBuf::from("/nix/store"),
        PathBuf::from("/nix/var/nix/daemon-socket/socket"),
    ];
    if role != Role::Worker {
        readable.push(home.join(".codex/auth.json"));
    }
    if matches!(role, Role::Agent | Role::Reviewer) {
        readable.push(home.join(".local/share/opencode/auth.json"));
    }
    for p in readable {
        text.push_str(&format!("(allow file-read* (subpath {}))\n", path(&p)?));
    }

    Ok(text)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_escapes_paths_and_never_allows_default_writes_for_reviewers() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("repo\"name");
        std::fs::create_dir(&root).unwrap();
        let p = Paths {
            owner_home: t.path().join("owner"),
            daemon_home: t.path().join("daemon"),
            worktree: root.clone(),
            docs: t.path().join("docs"),
        };
        let profile = build(&p, t.path(), Role::Reviewer, &NeverSent::new(&[]).unwrap()).unwrap();
        assert!(profile.contains("repo\\\"name"));
        assert!(profile.contains("com.apple.SecurityServer"));
        assert!(profile.contains("(deny file-write* (subpath"));
        assert!(!profile.contains("(allow file-write*"));
    }
}
