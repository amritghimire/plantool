use super::auth::{self, AppConfig, KeyStore};
use anyhow::{ensure, Context};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub etag: Option<String>,
    pub next: bool,
}
pub trait Api: Send + Sync {
    fn request<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Value>,
        etag: Option<String>,
    ) -> BoxFuture<'a, anyhow::Result<Reply>>;
}
pub struct Client {
    http: reqwest::Client,
    base: String,
    config: AppConfig,
    keys: Arc<dyn KeyStore>,
}
impl Client {
    pub fn new(config: AppConfig, keys: Arc<dyn KeyStore>) -> anyhow::Result<Self> {
        Self::with_base(config, keys, "https://api.github.com")
    }
    /// Alternate origin is restricted to loopback for mocked integration tests.
    pub fn with_base(
        config: AppConfig,
        keys: Arc<dyn KeyStore>,
        base: &str,
    ) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(base)?;
        ensure!(
            base == "https://api.github.com"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))),
            "Invalid GitHub API origin"
        );
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("accept", "application/vnd.github+json".parse()?);
        headers.insert("x-github-api-version", "2022-11-28".parse()?);
        Ok(Self {
            http: reqwest::Client::builder()
                .default_headers(headers)
                .user_agent("plantool")
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()?,
            base: base.into(),
            config,
            keys,
        })
    }
}
impl Api for Client {
    fn request<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Value>,
        etag: Option<String>,
    ) -> BoxFuture<'a, anyhow::Result<Reply>> {
        Box::pin(async move {
            ensure!(
                path.starts_with('/')
                    && !path.starts_with("//")
                    && !path.contains(['\r', '\n', '#']),
                "Invalid GitHub resource"
            );
            let token =
                auth::token(&self.http, &self.base, &self.config, self.keys.as_ref()).await?;
            let mut request = self
                .http
                .request(method.parse()?, format!("{}{path}", self.base))
                .bearer_auth(token);
            if let Some(body) = body {
                request = request.json(&body)
            }
            if let Some(tag) = etag {
                request = request.header("if-none-match", tag)
            }
            let response = request.send().await?;
            let status = response.status().as_u16();
            if status == 401 {
                auth::invalidate(self.config.app_id);
            }
            let etag = response
                .headers()
                .get("etag")
                .and_then(|x| x.to_str().ok())
                .map(str::to_owned);
            let next = response
                .headers()
                .get("link")
                .and_then(|x| x.to_str().ok())
                .is_some_and(|s| s.contains("rel=\"next\""));
            let bytes = response.bytes().await?;
            ensure!(bytes.len() <= 32 * 1024 * 1024, "GitHub response too large");
            let body = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).context("Invalid GitHub response JSON")?
            };
            Ok(Reply {
                status,
                body,
                etag,
                next,
            })
        })
    }
}
pub async fn get(api: &dyn Api, path: &str) -> anyhow::Result<Value> {
    let r = api.request("GET", path, None, None).await?;
    ensure!(
        (200..300).contains(&r.status),
        "GitHub GET failed ({})",
        r.status
    );
    Ok(r.body)
}
pub fn repo_path(name: &str) -> anyhow::Result<String> {
    let parts: Vec<_> = name.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|p| !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
        "Use owner/repository"
    );
    Ok(format!("/repos/{name}"))
}
pub const LABELS: [&str; 7] = [
    "plantool:accept",
    "plantool:proposed",
    "plantool:declined",
    "plantool:urgent",
    "plantool:urgent-requested",
    "plantool:sprint",
    "plantool:in-progress",
];
pub async fn labels(api: &dyn Api, repo: &str) -> anyhow::Result<()> {
    for label in LABELS {
        let p = format!(
            "{repo}/labels/{}",
            percent_encoding::utf8_percent_encode(label, percent_encoding::NON_ALPHANUMERIC)
        );
        let old = api.request("GET", &p, None, None).await?;
        if old.status == 404 {
            let r = api
                .request(
                    "POST",
                    &format!("{repo}/labels"),
                    Some(json!({"name":label,"color":"5b5bd6"})),
                    None,
                )
                .await?;
            ensure!(r.status == 201, "Label creation failed ({})", r.status)
        } else {
            ensure!(old.status == 200, "Label lookup failed ({})", old.status)
        }
    }
    Ok(())
}
pub async fn preflight(api: &dyn Api, repo: &str, branch: &str) -> anyhow::Result<Value> {
    let r = api
        .request(
            "GET",
            &format!(
                "{repo}/branches/{}/protection",
                percent_encoding::utf8_percent_encode(branch, percent_encoding::NON_ALPHANUMERIC)
            ),
            None,
            None,
        )
        .await?;
    let mut warnings = vec![];
    if r.status != 200 {
        warnings.push(format!("Cannot confirm default-branch protection (HTTP {}). Require PR reviews, checks and dismissal of stale reviews.",r.status));
    } else {
        if r.body["required_status_checks"]["contexts"]
            .as_array()
            .is_none_or(|a| a.is_empty())
            && r.body["required_status_checks"]["checks"]
                .as_array()
                .is_none_or(|a| a.is_empty())
        {
            warnings.push("No required checks configured".into())
        }
        if r.body["required_pull_request_reviews"]["required_approving_review_count"]
            .as_u64()
            .unwrap_or(0)
            < 1
        {
            warnings.push("Require at least one approving review".into())
        }
        if r.body["required_pull_request_reviews"]["dismiss_stale_reviews"] != true {
            warnings.push("Enable dismissal of stale reviews".into())
        }
    }
    Ok(
        json!({"warnings":warnings,"policy_check":"Offer plantool/policy as a required check after the PR workflow is implemented; this phase does not publish that check.","settings_url":"https://github.com/settings/apps"}),
    )
}
