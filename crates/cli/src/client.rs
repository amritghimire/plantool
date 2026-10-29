use anyhow::{anyhow, bail, Context};
use plantool_daemon::config::{default_home, default_port};
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct Client {
    pub base: String,
    pub port: u16,
    http: reqwest::blocking::Client,
    author: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ErrorBody {
    error: String,
}

impl Client {
    pub fn connect() -> anyhow::Result<Client> {
        let port = default_port();
        let c = Client::bare(port);
        c.ensure_daemon()?;
        Ok(c)
    }

    pub fn bare(port: u16) -> Client {
        Client {
            base: format!("http://127.0.0.1:{port}"),
            port,
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("http client"),
            author: std::env::var("PLANTOOL_AUTHOR")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn health(&self) -> Option<serde_json::Value> {
        self.http
            .get(self.url("/health"))
            .timeout(Duration::from_millis(800))
            .send()
            .ok()?
            .json()
            .ok()
    }

    pub fn ensure_daemon(&self) -> anyhow::Result<()> {
        if let Some(h) = self.health() {
            let protocol = h.get("protocol").and_then(|p| p.as_u64()).unwrap_or(0);
            if protocol == plantool_core::PROTOCOL_VERSION as u64 {
                return Ok(());
            }
            eprintln!(
                "plantool: replacing a daemon with protocol {protocol} (need {})",
                plantool_core::PROTOCOL_VERSION
            );
            let _ = self.http.post(self.url("/shutdown")).send();
            let start = Instant::now();
            while self.health().is_some() && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        spawn_daemon(self.port)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if self.health().is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(120));
        }
        bail!(
            "the daemon did not come up on port {}; see {}",
            self.port,
            default_home().join("daemon.log").display()
        )
    }

    fn check<T: DeserializeOwned>(&self, resp: reqwest::blocking::Response) -> anyhow::Result<T> {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if !status.is_success() {
            let msg = serde_json::from_str::<ErrorBody>(&text)
                .map(|e| e.error)
                .unwrap_or(text);
            bail!("{msg}");
        }
        serde_json::from_str(&text).with_context(|| format!("unexpected response: {text}"))
    }

    fn with_headers(
        &self,
        r: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        match &self.author {
            Some(a) => r.header("x-plantool-author", a),
            None => r,
        }
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let r = self.with_headers(self.http.get(self.url(path))).send()?;
        self.check(r)
    }

    pub fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let r = self
            .with_headers(self.http.post(self.url(path)).json(body))
            .send()?;
        self.check(r)
    }

    pub fn delete<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let r = self
            .with_headers(self.http.delete(self.url(path)).json(body))
            .send()?;
        self.check(r)
    }

    pub fn get_bytes(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        let response = self.with_headers(self.http.get(self.url(path))).send()?;
        if !response.status().is_success() {
            let _: serde_json::Value = self.check(response)?;
            bail!("export failed");
        }
        Ok(response.bytes()?.to_vec())
    }

    pub fn post_bytes(&self, path: &str, bytes: Vec<u8>) -> anyhow::Result<serde_json::Value> {
        let response = self
            .with_headers(
                self.http
                    .post(self.url(path))
                    .header("content-type", "application/zip")
                    .body(bytes),
            )
            .send()?;
        self.check(response)
    }

    pub fn resolve_key(&self, reference: &str) -> anyhow::Result<(String, serde_json::Value)> {
        let cwd = std::env::current_dir().ok();
        let mut path = format!("/api/sessions/{}", urlencode(reference));
        if reference.contains('/') {
            path = format!("/api/sessions/{reference}");
        } else if let Some(c) = cwd {
            path.push_str(&format!("?cwd={}", urlencode(&c.to_string_lossy())));
        }
        let v: serde_json::Value = self.get(&path)?;
        let key = v
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow!("no key in response"))?
            .to_string();
        Ok((key, v))
    }

    pub fn session_url(&self, key: &str) -> String {
        format!("{}/s/{}", self.base, key)
    }
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn spawn_daemon(port: u16) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("cannot locate the plantool binary")?;
    let home = default_home();
    std::fs::create_dir_all(&home)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("daemon.log"))?;
    let err = log.try_clone()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve")
        .env("PLANTOOL_PORT", port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0020_0000);
    }
    cmd.spawn().context("failed to start the daemon")?;
    Ok(())
}

pub fn open_browser(url: &str) {
    if let Err(e) = open::that(url) {
        eprintln!("could not open a browser ({e}); open {url} yourself");
    }
}

pub fn home() -> PathBuf {
    default_home()
}

pub fn print_json<T: serde::Serialize>(v: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

pub fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd().join(p)
    }
}
