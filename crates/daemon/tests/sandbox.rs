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
    let output = f.command(
        &sandbox,
        "/usr/bin/curl",
        &[
            "--silent",
            "--max-time",
            "3",
            "--unix-socket",
            socket.to_str().unwrap(),
            "http://localhost/",
        ],
    );
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
