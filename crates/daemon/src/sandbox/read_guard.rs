use super::{NeverSent, Paths};
use std::path::{Path, PathBuf};
pub fn protected(paths: &Paths) -> Vec<PathBuf> {
    let home = &paths.owner_home;
    let mut out = vec![
        home.join(".ssh"),
        home.join(".config/gh"),
        home.join(".gitconfig"),
        home.join(".git-credentials"),
        home.join(".config/git/credentials"),
        home.join("Library/Keychains"),
        home.join("Library/Safari"),
        home.join("Library/Cookies"),
    ];
    for browser in ["Google/Chrome", "Firefox", "Arc", "BraveSoftware"] {
        out.push(home.join("Library/Application Support").join(browser));
    }
    for daemon in [home.join(".plantool"), paths.daemon_home.clone()] {
        for name in [
            "db",
            "secrets",
            "auth",
            "artifacts",
            "daemon.log",
            "security",
        ] {
            out.push(daemon.join(name));
        }
    }
    // A clean-break backup contains the same daemon secrets as the active home.
    for daemon in [home.join(".plantool"), paths.daemon_home.clone()] {
        if let (Some(parent), Some(name)) = (daemon.parent(), daemon.file_name()) {
            let prefix = format!("{}.v0-backup-", name.to_string_lossy());
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name
                        .to_string_lossy()
                        .strip_prefix(&prefix)
                        .is_some_and(|suffix| {
                            !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                        })
                    {
                        out.push(entry.path());
                    }
                }
            }
        }
    }
    // Query config only: never invoke a credential helper or evaluate a shell.
    // Includes are resolved by Git, including credential.<url>.helper sections.
    for config in [home.join(".gitconfig"), home.join(".config/git/config")] {
        if let Ok(result) = std::process::Command::new("git")
            .args(["config", "--null", "--file"])
            .arg(config)
            .args([
                "--includes",
                "--get-regexp",
                "^credential(\\..*)?\\.helper$",
            ])
            .env("HOME", home)
            .output()
        {
            for record in result.stdout.split(|b| *b == 0) {
                if let Some((_, value)) = std::str::from_utf8(record)
                    .ok()
                    .and_then(|s| s.split_once('\n'))
                {
                    out.extend(helper_stores(home, &paths.worktree, value));
                }
            }
        }
    }
    out
}
fn helper_stores(home: &Path, worktree: &Path, value: &str) -> Vec<PathBuf> {
    let Some(words) = shlex::split(value) else {
        return Vec::new();
    };
    if !words
        .first()
        .is_some_and(|w| w == "store" || w.ends_with("/git-credential-store"))
    {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for (i, word) in words.iter().enumerate().skip(1) {
        let file = word.strip_prefix("--file=").or_else(|| {
            (word == "--file")
                .then(|| words.get(i + 1).map(String::as_str))
                .flatten()
        });
        if let Some(file) = file {
            paths.push(if let Some(relative) = file.strip_prefix("~/") {
                home.join(relative)
            } else {
                let p = PathBuf::from(file);
                if p.is_absolute() {
                    p
                } else {
                    worktree.join(p)
                }
            });
        }
    }
    paths
}
pub fn check(paths: &Paths, policy: &NeverSent, path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.is_absolute()
            && !path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir)),
        "path must stay inside its worktree"
    );
    policy.check(Path::new(""), path)?;
    let root = paths.worktree.canonicalize()?;
    let target = root.join(path).canonicalize()?;
    anyhow::ensure!(target.starts_with(&root), "file is outside its worktree");
    policy.check(&root, &target)?;
    anyhow::ensure!(
        !policy
            .protected_targets(&root)?
            .iter()
            .any(|p| target.starts_with(p)),
        "File is excluded by the project's never-sent rules"
    );
    for secret in protected(paths) {
        if target.starts_with(&secret) || secret.canonicalize().is_ok_and(|p| target.starts_with(p))
        {
            anyhow::bail!("File is protected by sandbox policy");
        }
    }
    Ok(())
}
pub fn session_check(
    state: &crate::AppState,
    session: &crate::registry::LiveSession,
    path: &Path,
) -> anyhow::Result<()> {
    let s = session.session();
    let root = crate::git::main_root(&s.repo.root);
    let policy = NeverSent::new(&state.runs.execution.rules(&root))?;
    check(
        &Paths {
            owner_home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("owner home unavailable"))?,
            daemon_home: state.config.home.clone(),
            worktree: s.cwd().clone(),
            docs: session.store.dir.join("docs"),
        },
        &policy,
        path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_credential_store_paths_are_protected_without_running_helpers() {
        let t = tempfile::tempdir().unwrap();
        let owner = t.path().join("owner");
        std::fs::create_dir_all(&owner).unwrap();
        let external = t.path().join("external credentials");
        std::fs::write(
            owner.join(".gitconfig"),
            format!(
                "[credential]\n helper = store --file '{}'\n",
                external.display()
            ),
        )
        .unwrap();
        let paths = Paths {
            owner_home: owner.clone(),
            daemon_home: t.path().join("daemon"),
            worktree: t.path().join("repo"),
            docs: t.path().join("docs"),
        };
        assert!(protected(&paths).contains(&external));
        assert_eq!(
            helper_stores(&owner, &paths.worktree, "store --file=~/my-credentials"),
            vec![owner.join("my-credentials")]
        );
        assert!(helper_stores(&owner, &paths.worktree, "!touch /tmp/never-execute").is_empty());
        assert_eq!(
            helper_stores(&owner, &paths.worktree, "store --file=credentials.txt"),
            vec![paths.worktree.join("credentials.txt")]
        );
    }
    #[test]
    fn daemon_file_reads_enforce_defaults_custom_rules_and_aliases() {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths {
            owner_home: t.path().join("owner"),
            daemon_home: t.path().join("daemon"),
            worktree: t.path().join("repo"),
            docs: t.path().join("docs"),
        };
        std::fs::create_dir_all(&paths.worktree).unwrap();
        for name in [".env", "ordinary.txt", "private.txt"] {
            std::fs::write(paths.worktree.join(name), "fixture").unwrap();
        }
        let policy = NeverSent::new(&["private.txt".into()]).unwrap();
        assert!(check(&paths, &policy, Path::new("ordinary.txt")).is_ok());
        for name in [".env", "private.txt", "../secret"] {
            assert!(check(&paths, &policy, Path::new(name)).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(paths.worktree.join(".env"), paths.worktree.join("link"))
                .unwrap();
            std::fs::hard_link(paths.worktree.join(".env"), paths.worktree.join("alias")).unwrap();
            assert!(check(&paths, &policy, Path::new("link")).is_err());
            assert!(check(&paths, &policy, Path::new("alias")).is_err());
        }
    }
}
