#![cfg(unix)]
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::Url;
struct Daemon(Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Test {
    home: PathBuf,
    repo: PathBuf,
    path: PathBuf,
    owner: PathBuf,
}
impl Test {
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_plantool"));
        c.args(args)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.owner)
            .env("PLANTOOL_HOME", &self.home)
            .env("PLANTOOL_PORT", "41300")
            .current_dir(&self.repo);
        c
    }
    fn run(&self, args: &[&str]) -> String {
        let out = self.command(args).output().unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.run(args)).unwrap()
    }
}
fn request(
    client: &reqwest::blocking::Client,
    path: &str,
    body: &Value,
    cookie: Option<&str>,
    proof: Option<&str>,
) -> reqwest::blocking::Response {
    let mut r = client
        .post(format!("http://localhost:41300{path}"))
        .header("origin", "http://localhost:41300")
        .json(body);
    if let Some(c) = cookie {
        r = r.header("cookie", c)
    }
    if let Some(p) = proof {
        r = r.header("x-plantool-assertion", p)
    }
    r.send().unwrap()
}
fn approve(
    client: &reqwest::blocking::Client,
    auth: &mut WebauthnAuthenticator<SoftPasskey>,
    cookie: &str,
    path: &str,
    body: Value,
) {
    let start: Value = request(
        client,
        "/api/auth/assertion/start",
        &json!({"action":{"method":"POST","path":path,"body":body}}),
        Some(cookie),
        None,
    )
    .error_for_status()
    .unwrap()
    .json()
    .unwrap();
    let credential = auth
        .do_authentication(
            Url::parse("http://localhost:41300").unwrap(),
            serde_json::from_value(start["options"].clone()).unwrap(),
        )
        .unwrap();
    let grant: Value = request(
        client,
        "/api/auth/assertion/finish",
        &json!({"id":start["id"],"credential":credential}),
        Some(cookie),
        None,
    )
    .error_for_status()
    .unwrap()
    .json()
    .unwrap();
    let response = request(client, path, &body, Some(cookie), grant["proof"].as_str());
    assert!(
        response.status().is_success(),
        "{}",
        response.text().unwrap()
    );
}
#[test]
fn full_personal_flow_has_no_python_uv_worker_or_github_app_dependency() {
    let t = tempfile::tempdir().unwrap();
    let test = Test {
        home: t.path().join("home"),
        repo: t.path().join("repo"),
        path: t.path().join("bin"),
        owner: t.path().join("owner"),
    };
    for dir in [&test.repo, &test.path, &test.owner] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // The daemon and CLI get a restricted PATH containing only Git and a shell.
    let git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let git = PathBuf::from(String::from_utf8(git.stdout).unwrap().trim());
    std::os::unix::fs::symlink(git, test.path.join("git")).unwrap();
    std::os::unix::fs::symlink("/bin/sh", test.path.join("sh")).unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
        vec!["commit", "--allow-empty", "-qm", "initial"],
    ] {
        assert!(Command::new(test.path.join("git"))
            .args(args)
            .current_dir(&test.repo)
            .env("HOME", &test.owner)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success());
    }
    assert!(!test.path.join("python3").exists());
    assert!(!test.path.join("uv").exists());
    assert!(!test.path.join("gh").exists());
    let log = std::fs::File::create(t.path().join("daemon.log")).unwrap();
    let claim = std::net::TcpListener::bind(("127.0.0.1", 41300))
        .expect("Port 41300 must be free for the isolated Personal test");
    drop(claim);
    let child = test
        .command(&["serve"])
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let mut daemon = Daemon(child);
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(response) = client.get("http://localhost:41300/health").send() {
            let health: Value = response.json().unwrap();
            assert_eq!(
                health["pid"].as_u64(),
                Some(u64::from(daemon.0.id())),
                "Another daemon occupied the test port"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "daemon failed: {:?}",
            daemon.0.try_wait()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    test.run(&[
        "new",
        "personal",
        "--no-open",
        "--brief",
        "No runtime dependencies",
    ]);
    let research = test.run(&[
        "session",
        "doc",
        "path",
        "--session",
        "personal",
        "--kind",
        "research",
    ]);
    std::fs::write(Path::new(research.trim()), "# Research\nReady.\n").unwrap();
    test.run(&[
        "session",
        "doc",
        "touch",
        "--session",
        "personal",
        "--kind",
        "research",
    ]);
    let plan = test.run(&[
        "session",
        "doc",
        "path",
        "--session",
        "personal",
        "--kind",
        "plan",
    ]);
    let original = "# Plan\n### Phase 1\n- [ ] T1: Implement the result\n";
    std::fs::write(Path::new(plan.trim()), original).unwrap();
    test.run(&[
        "session",
        "doc",
        "touch",
        "--session",
        "personal",
        "--kind",
        "plan",
    ]);
    let view = test.json(&["session", "get", "--session", "personal", "--json"]);
    let key = view["key"].as_str().unwrap();
    let stage = format!("/api/sessions/{key}/stage");
    assert_eq!(view["state"]["stage"], "plan-review");
    let code = plantool_daemon::auth::setup_code(&test.home).unwrap();
    let start: Value = request(
        &client,
        "/api/auth/register/start",
        &json!({"code":code}),
        None,
        None,
    )
    .error_for_status()
    .unwrap()
    .json()
    .unwrap();
    let mut auth = WebauthnAuthenticator::new(SoftPasskey::new(true));
    assert_eq!(
        start["options"]["publicKey"]["authenticatorSelection"]["authenticatorAttachment"],
        "platform"
    );
    let mut options = start["options"].clone();
    options["publicKey"]["authenticatorSelection"]["authenticatorAttachment"] = Value::Null;
    let options = serde_json::from_value(options).unwrap();
    let credential = auth
        .do_registration(Url::parse("http://localhost:41300").unwrap(), options)
        .unwrap();
    let finish = request(
        &client,
        "/api/auth/register/finish",
        &json!({"id":start["id"],"credential":credential}),
        None,
        None,
    )
    .error_for_status()
    .unwrap();
    let cookie = finish.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let sha = view["docs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "plan")
        .unwrap()["sha"]
        .as_str()
        .unwrap();
    approve(
        &client,
        &mut auth,
        &cookie,
        &stage,
        json!({"to":"approved","plan_sha":sha}),
    );
    assert!(!test
        .command(&[
            "session",
            "stage",
            "--session",
            "personal",
            "--set",
            "approved"
        ])
        .output()
        .unwrap()
        .status
        .success());
    test.run(&[
        "session",
        "stage",
        "--session",
        "personal",
        "--set",
        "implementing",
    ]);
    test.run(&["task", "done", "personal", "T1"]);
    assert_eq!(
        std::fs::read_to_string(Path::new(plan.trim())).unwrap(),
        original
    );
    test.run(&[
        "session",
        "stage",
        "--session",
        "personal",
        "--set",
        "implementation-review",
    ]);
    approve(&client, &mut auth, &cookie, &stage, json!({"to":"done"}));
    assert_eq!(
        test.json(&["session", "get", "--session", "personal", "--json"])["state"]["stage"],
        "done"
    );
    let archive = t.path().join("export.zip");
    test.run(&["export", "personal", "-o", archive.to_str().unwrap()]);
    assert!(archive.is_file());
    test.run(&["daemon", "stop"]);
}
