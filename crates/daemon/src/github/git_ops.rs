//! Daemon-only Git transport. App tokens travel through an inherited descriptor,
//! never a URL, command-line argument, environment value or credential file.
use super::api::Api;
use anyhow::ensure;
use futures::future::BoxFuture;
use std::path::Path;
pub trait GitOps: Send + Sync {
    fn fetch<'a>(
        &'a self,
        api: &'a dyn Api,
        root: &'a Path,
        repo: &'a str,
        branch: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<String>>;
    fn push<'a>(
        &'a self,
        api: &'a dyn Api,
        root: &'a Path,
        repo: &'a str,
        branch: &'a str,
        head: &'a str,
        old: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<()>>;
}
pub struct AppGit;
#[cfg(unix)]
async fn transport(
    api: &dyn Api,
    root: &Path,
    repo: &str,
    args: &[String],
) -> anyhow::Result<String> {
    use std::{
        io::Write,
        os::{fd::AsRawFd, unix::fs::PermissionsExt},
    };
    super::api::repo_path(repo)?;
    validate_transport(root)?;
    let temp = tempfile::tempdir()?;
    let helper = temp.path().join("credential");
    std::fs::write(&helper,format!("#!/bin/sh\n[ \"$1\" = get ] || exit 0\nprotocol= host= path=\nwhile IFS='=' read -r name value; do case \"$name\" in protocol) protocol=$value;; host) host=$value;; path) path=$value;; esac; done\n[ \"$protocol\" = https ] && [ \"$host\" = github.com ] && [ \"$path\" = '{repo}.git' ] || exit 1\nIFS= read -r token <&3 || exit 1\nprintf 'username=x-access-token\\npassword=%s\\n' \"$token\"\n"))?;
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
    let token = api.git_token().await?;
    ensure!(!token.contains(['\r', '\n']), "Invalid installation token");
    let (read, mut write) = std::os::unix::net::UnixStream::pair()?;
    let fd = read.as_raw_fd();
    let mut cmd = tokio::process::Command::new("git");
    crate::sandbox::environment::apply(&mut cmd, None);
    cmd.current_dir(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "credential.helper=",
            "-c",
            &format!(
                "credential.helper=!{}",
                shlex::try_quote(helper.to_str().unwrap())?
            ),
            "-c",
            "credential.useHttpPath=true",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "/bin/false")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    unsafe {
        cmd.pre_exec(move || {
            if fd != 3 && libc::dup2(fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    drop(read);
    write.write_all(token.as_bytes())?;
    write.write_all(b"\n")?;
    drop(write);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await??;
    // Never include transport stderr: third-party helpers must not echo a token.
    ensure!(
        output.status.success(),
        "App-authenticated Git operation failed ({})",
        output.status
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}
#[cfg(not(unix))]
async fn transport(_: &dyn Api, _: &Path, _: &str, _: &[String]) -> anyhow::Result<String> {
    anyhow::bail!("App Git transport requires Unix descriptor isolation")
}
impl GitOps for AppGit {
    fn fetch<'a>(
        &'a self,
        api: &'a dyn Api,
        root: &'a Path,
        repo: &'a str,
        branch: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move {
            ensure!(
                !branch.starts_with('-') && !branch.contains(['\n', '\r', ':']),
                "Unsafe default branch"
            );
            transport(
                api,
                root,
                repo,
                &[
                    "fetch".into(),
                    "--no-tags".into(),
                    format!("https://github.com/{repo}.git"),
                    format!("refs/heads/{branch}:refs/remotes/origin/{branch}"),
                ],
            )
            .await?;
            crate::git::git(
                root,
                &[
                    "symbolic-ref",
                    "refs/remotes/origin/HEAD",
                    &format!("refs/remotes/origin/{branch}"),
                ],
            )?;
            Ok(crate::git::git(
                root,
                &["rev-parse", &format!("refs/remotes/origin/{branch}")],
            )?)
        })
    }
    fn push<'a>(
        &'a self,
        api: &'a dyn Api,
        root: &'a Path,
        repo: &'a str,
        branch: &'a str,
        head: &'a str,
        old: Option<&'a str>,
    ) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            ensure!(
                branch.starts_with("plantool/")
                    && !branch.contains(['\n', '\r', ':'])
                    && head.len() == 40
                    && head.bytes().all(|b| b.is_ascii_hexdigit()),
                "Unsafe push target"
            );
            let mut args = vec!["push".into()];
            if let Some(old) = old {
                ensure!(
                    old.len() == 40 && old.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Unsafe push lease"
                );
                args.push(format!("--force-with-lease=refs/heads/{branch}:{old}"));
            }
            args.extend([
                format!("https://github.com/{repo}.git"),
                format!("{head}:refs/heads/{branch}"),
            ]);
            transport(api, root, repo, &args).await?;
            Ok(())
        })
    }
}

/// Match the actual isolated transport environment; ignore ordinary global/system helpers.
#[cfg(unix)]
fn validate_transport(root: &Path) -> anyhow::Result<()> {
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "config",
            "--get-regexp",
            r"^(url\.|http\.|credential\.|core\.ssh)",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .output()?;
    ensure!(
        matches!(out.status.code(), Some(0 | 1)),
        "Cannot validate repository transport configuration"
    );
    ensure!(
        out.stdout.is_empty(),
        "Unsafe repository transport override; inspect repository configuration"
    );
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn repo_transport_overrides_and_bad_config_fail_closed() {
        let t = tempfile::tempdir().unwrap();
        crate::git::git(t.path(), &["init"]).unwrap();
        validate_transport(t.path()).unwrap();
        crate::git::git(t.path(), &["config", "credential.helper", "unsafe-helper"]).unwrap();
        assert!(validate_transport(t.path()).is_err());
        crate::git::git(t.path(), &["config", "--unset", "credential.helper"]).unwrap();
        std::fs::write(t.path().join(".git/config"), "broken[").unwrap();
        assert!(validate_transport(t.path()).is_err());
    }
}
