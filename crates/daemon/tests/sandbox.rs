#![cfg(target_os = "macos")]
//! Section 19 Sandbox acceptance tests execute real Seatbelt policies on macOS.
use plantool_daemon::sandbox::{NeverSent, Paths, Role, Sandbox};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
struct Fixture {
    _temp: tempfile::TempDir,
    paths: Paths,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir_in("/tmp").unwrap();
        let paths = Paths {
            owner_home: temp.path().join("owner"),
            daemon_home: temp.path().join("daemon"),
            worktree: temp.path().join("worktree"),
            docs: temp.path().join("docs"),
        };
        for dir in [
            &paths.owner_home,
            &paths.daemon_home,
            &paths.worktree,
            &paths.docs,
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        Self { _temp: temp, paths }
    }
    fn write(&self, path: PathBuf) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "fixture secret\n").unwrap();
        path
    }
    fn sandbox(&self, role: Role) -> Arc<Sandbox> {
        Sandbox::required(
            Paths {
                owner_home: self.paths.owner_home.clone(),
                daemon_home: self.paths.daemon_home.clone(),
                worktree: self.paths.worktree.clone(),
                docs: self.paths.docs.clone(),
            },
            role,
            NeverSent::new(&["private/**".into()]).unwrap(),
        )
        .unwrap()
    }
    async fn command(&self, sandbox: &Sandbox, exe: &str, args: &[&str]) -> std::process::Output {
        let mut cmd = sandbox.command(Path::new(exe)).unwrap();
        plantool_daemon::sandbox::environment::apply(&mut cmd, None);
        sandbox.apply_environment(&mut cmd);
        cmd.current_dir(&self.paths.worktree)
            .args(args)
            .output()
            .await
            .unwrap()
    }
}
#[tokio::test]
async fn sandbox_denies_secret_files_never_sent_files_and_outside_writes() {
    let f = Fixture::new();
    let mut denied = Vec::new();
    for name in [
        ".ssh/id_fixture",
        ".ollama/id_ed25519",
        ".config/gh/hosts.yml",
        ".gitconfig",
        ".git-credentials",
        "Library/Keychains/fixture",
        "Library/Application Support/Google/Chrome/Cookies",
        "Library/Application Support/Firefox/fixture",
        "Library/Application Support/Arc/fixture",
        "Library/Application Support/BraveSoftware/fixture",
        "Library/Safari/fixture",
        "Library/Cookies/fixture",
    ] {
        denied.push(f.write(f.paths.owner_home.join(name)));
    }
    for name in [
        "db/plantool.db",
        "secrets/fixture",
        "auth/fixture",
        "artifacts/fixture",
        "daemon.log",
        "security/lifecycle-token",
        "backups/current/plantool.db",
    ] {
        denied.push(f.write(f.paths.daemon_home.join(name)));
    }
    for name in [
        ".env",
        "nested/.env.local",
        "cert.pem",
        "private.key",
        "nested/secrets/token",
        "private/extra.secret",
    ] {
        denied.push(f.write(f.paths.worktree.join(name)));
    }
    denied.push(
        f.write(
            f.paths
                .daemon_home
                .with_file_name("daemon.v0-backup-123")
                .join("auth/old-secret"),
        ),
    );
    let ordinary = f.write(f.paths.worktree.join("ordinary.txt"));
    let sandbox = f.sandbox(Role::Agent);
    let good = f
        .command(&sandbox, "/bin/cat", &[ordinary.to_str().unwrap()])
        .await;
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    for path in denied {
        let out = f
            .command(&sandbox, "/bin/cat", &[path.to_str().unwrap()])
            .await;
        assert!(!out.status.success(), "sandbox read {}", path.display());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("fixture secret"));
    }
    assert!(f
        .command(&sandbox, "/bin/sh", &["-c", "printf ok > result.txt"])
        .await
        .status
        .success());
    let outside = f.paths.owner_home.join("forbidden-write");
    assert!(!f
        .command(
            &sandbox,
            "/bin/sh",
            &["-c", "printf no > \"$1\"", "sh", outside.to_str().unwrap()]
        )
        .await
        .status
        .success());
    assert!(!outside.exists());
    let plan = f.paths.docs.join("plan.md");
    assert!(f
        .command(
            &sandbox,
            "/bin/sh",
            &["-c", "printf ok > \"$1\"", "sh", plan.to_str().unwrap()]
        )
        .await
        .status
        .success());
    let state = f.paths.docs.join("state.json");
    assert!(!f
        .command(
            &sandbox,
            "/bin/sh",
            &["-c", "printf no > \"$1\"", "sh", state.to_str().unwrap()]
        )
        .await
        .status
        .success());
    let new_env = f.paths.worktree.join("new/.env.created");
    std::fs::create_dir(new_env.parent().unwrap()).unwrap();
    std::fs::write(&new_env, "new secret").unwrap();
    assert!(!f
        .command(&sandbox, "/bin/cat", &[new_env.to_str().unwrap()])
        .await
        .status
        .success());
}
#[tokio::test]
async fn reviewer_cannot_write_worktree_but_can_write_live_docs_and_own_temp() {
    let f = Fixture::new();
    let sandbox = f.sandbox(Role::Reviewer);
    assert!(!f
        .command(&sandbox, "/bin/sh", &["-c", "printf no > reviewer.txt"])
        .await
        .status
        .success());
    assert!(!f.paths.worktree.join("reviewer.txt").exists());
    let output = sandbox.temp_dir().join("own-temp");
    assert!(f
        .command(
            &sandbox,
            "/bin/sh",
            &["-c", "printf ok > \"$1\"", "sh", output.to_str().unwrap()]
        )
        .await
        .status
        .success());
    let temp = sandbox.temp_dir().to_path_buf();
    drop(sandbox);
    assert!(!temp.exists());
}
#[tokio::test]
async fn keychain_mach_services_are_denied_and_model_auth_reads_remain_available() {
    let f = Fixture::new();
    let helper = f.paths.worktree.join("mach-test");
    let source = f.paths.worktree.join("mach-test.c");
    std::fs::write(
        &source,
        r#"#include <mach/mach.h>
#include <servers/bootstrap.h>
int main(int argc, char **argv) {
 mach_port_t service = MACH_PORT_NULL;
 kern_return_t status = bootstrap_look_up(bootstrap_port, argv[1], &service);
 return status == BOOTSTRAP_NOT_PRIVILEGED ? 0 : 1;
}
"#,
    )
    .unwrap();
    let compile = std::process::Command::new("/usr/bin/clang")
        .arg(&source)
        .arg("-o")
        .arg(&helper)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let sandbox = f.sandbox(Role::Agent);
    for service in ["com.apple.SecurityServer", "com.apple.securityd"] {
        let result = f
            .command(&sandbox, helper.to_str().unwrap(), &[service])
            .await;
        assert!(
            result.status.success(),
            "Mach lookup was not explicitly denied: {service}"
        );
    }
    for auth in [".codex/auth.json", ".local/share/opencode/auth.json"] {
        let path = f.write(f.paths.owner_home.join(auth));
        assert!(f
            .command(&sandbox, "/bin/cat", &[path.to_str().unwrap()])
            .await
            .status
            .success());
    }
}
#[tokio::test]
async fn symlink_and_hardlink_aliases_cannot_bypass_never_sent_rules() {
    let f = Fixture::new();
    let secret = f.write(f.paths.worktree.join(".env"));
    let alias = f.paths.worktree.join("public.txt");
    std::fs::hard_link(&secret, &alias).unwrap();
    let link = f.paths.worktree.join("link.txt");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let sandbox = f.sandbox(Role::Agent);
    assert!(!f
        .command(&sandbox, "/bin/cat", &[link.to_str().unwrap()])
        .await
        .status
        .success());
    assert!(!f
        .command(&sandbox, "/bin/cat", &[alias.to_str().unwrap()])
        .await
        .status
        .success());
    let late = f.paths.worktree.join("late.txt");
    assert!(!f
        .command(
            &sandbox,
            "/bin/ln",
            &[secret.to_str().unwrap(), late.to_str().unwrap()]
        )
        .await
        .status
        .success());
}

#[tokio::test]
async fn sandbox_can_connect_to_the_daemon_unix_socket() {
    let f = Fixture::new();
    let run = f.paths.daemon_home.join("run");
    std::fs::create_dir_all(&run).unwrap();
    let socket = run.join("daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let sandbox = f.sandbox(Role::Agent);
    let args = [
        "--silent",
        "--max-time",
        "3",
        "--unix-socket",
        socket.to_str().unwrap(),
        "http://localhost/",
    ];
    let output = f.command(&sandbox, "/usr/bin/curl", &args);
    let server = async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        connection.read(&mut buffer).await.unwrap();
        connection
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    };
    let (output, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(output, server)
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ok");
}
#[tokio::test]
async fn eos_proxy_roles_cannot_read_legacy_provider_keys_and_worker_has_no_model_auth() {
    let f = Fixture::new();
    let opencode = f.write(f.paths.owner_home.join(".local/share/opencode/auth.json"));
    let codex = f.write(f.paths.owner_home.join(".codex/auth.json"));
    let agent = f.sandbox(Role::EosAgent);
    assert!(!f
        .command(&agent, "/bin/cat", &[opencode.to_str().unwrap()])
        .await
        .status
        .success());
    assert!(!agent.temp_dir().join("data/opencode/auth.json").exists());
    assert!(f
        .command(&agent, "/bin/cat", &[codex.to_str().unwrap()])
        .await
        .status
        .success());
    let worker = f.sandbox(Role::Worker);
    assert!(!f
        .command(&worker, "/bin/cat", &[codex.to_str().unwrap()])
        .await
        .status
        .success());
    assert!(!worker.temp_dir().join("codex/auth.json").exists());
}

#[tokio::test]
async fn qa_writes_tests_and_git_objects_but_not_production_or_git_configuration() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.paths.worktree.join("tests")).unwrap();
    std::fs::create_dir_all(f.paths.worktree.join("src")).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "test"],
        vec!["config", "user.email", "test@localhost"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&f.paths.worktree)
            .status()
            .unwrap()
            .success());
    }
    let qa = f.sandbox(Role::EosQa);
    assert!(f
        .command(&qa, "/bin/sh", &["-c", "printf test > tests/qa.rs"])
        .await
        .status
        .success());
    assert!(!f
        .command(&qa, "/bin/sh", &["-c", "printf bad > src/contested.rs"])
        .await
        .status
        .success());
    assert!(!f
        .command(
            &qa,
            "/usr/bin/git",
            &["config", "credential.helper", "malicious"]
        )
        .await
        .status
        .success());
    assert!(f
        .command(&qa, "/usr/bin/git", &["add", "tests/qa.rs"])
        .await
        .status
        .success());
    assert!(f
        .command(&qa, "/usr/bin/git", &["commit", "-qm", "test-only change"])
        .await
        .status
        .success());
    let reviewer = f.sandbox(Role::EosReviewer);
    assert!(!f
        .command(&reviewer, "/bin/sh", &["-c", "printf bad > tests/qa.rs"])
        .await
        .status
        .success());
}

/// Startup-only live check: uses the signed-in cache, never starts a model turn.
#[tokio::test]
#[ignore = "manual macOS check requiring a signed-in Codex account and network"]
async fn native_codex_account_routing_in_seatbelt() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let owner = PathBuf::from(std::env::var_os("HOME").unwrap());
    let sandbox = Sandbox::required(
        Paths {
            owner_home: owner.clone(),
            daemon_home: owner.join(".plantool"),
            worktree: root.path().into(),
            docs: root.path().join("docs"),
        },
        Role::EosReviewer,
        NeverSent::new(&[]).unwrap(),
    )
    .unwrap();
    let exe = plantool_daemon::providers::executable_for(plantool_core::Provider::Codex)
        .unwrap_or_else(|| "/opt/homebrew/bin/codex".into());
    let mut cmd = sandbox.command(&exe).unwrap();
    plantool_daemon::sandbox::environment::apply(&mut cmd, Some(plantool_core::Provider::Codex));
    sandbox.apply_environment(&mut cmd);
    cmd.args([
        "app-server",
        "--stdio",
        "-c",
        "cli_auth_credentials_store=\"file\"",
        "-c",
        "forced_login_method=\"chatgpt\"",
    ])
    .current_dir(root.path())
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true);
    let mut child = cmd.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    input.write_all(b"{\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"plantool\",\"version\":\"0.1\"},\"capabilities\":{\"experimentalApi\":true}}}\n").await.unwrap();
    let outcome=tokio::time::timeout(std::time::Duration::from_secs(60),async {
        while let Some(line)=lines.next_line().await.unwrap() {
            let value:serde_json::Value=serde_json::from_str(&line).unwrap();
            if value["id"]==1 {input.write_all(b"{\"method\":\"initialized\"}\n{\"id\":2,\"method\":\"account/read\",\"params\":{\"refreshToken\":false}}\n").await.unwrap();}
            if value["id"]==2 {
                assert!(value["error"].is_null(),"Account RPC failed: {}",value["error"]["message"]);
                assert_eq!(value["result"]["account"]["type"],"chatgpt");
                assert!(!value["result"]["workspaceRouting"].is_null(),"Account exists but native routing metadata is missing");
                return;
            }
        }
        panic!("Codex account stream closed without a response");
    }).await;
    let _ = child.kill().await;
    assert!(
        outcome.is_ok(),
        "Sandboxed account routing timed out; no model turn was started"
    );
}
