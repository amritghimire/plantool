//! Credentials stay in the daemon; tests inject a fake gateway, never environment keys.
use futures::future::BoxFuture;
use serde_json::Value;
use std::time::Duration;
pub struct Reply {
    pub status: u16,
    pub body: String,
    pub content_type: String,
    pub reset: Option<i64>,
}
pub trait Gateway: Send + Sync {
    fn models<'a>(&'a self, provider: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<String>>>;
    fn chat<'a>(&'a self, provider: &'a str, body: Value) -> BoxFuture<'a, anyhow::Result<Reply>>;
}
pub struct HttpGateway;
pub(crate) async fn ollama_provider(model: &str) -> anyhow::Result<&'static str> {
    super::ollama::Connection::configured()?
        .provider(model)
        .await
}

fn base(provider: &str) -> anyhow::Result<String> {
    Ok(match provider {
        "openrouter" => "https://openrouter.ai/api/v1",
        "opencode-zen" => "https://opencode.ai/zen/v1",
        _ => anyhow::bail!("Provider is CLI-only or unknown"),
    }
    .to_string())
}
#[cfg(target_os = "macos")]
fn key(provider: &str) -> anyhow::Result<Option<String>> {
    if matches!(provider, "ollama" | "ollama-cloud") {
        return Ok(None);
    }
    let bytes = crate::keychain::read(&format!("plantool/{provider}"), None, false)?;
    Ok(Some(String::from_utf8(bytes)?.trim().to_string()))
}
#[cfg(not(target_os = "macos"))]
fn key(provider: &str) -> anyhow::Result<Option<String>> {
    if matches!(provider, "ollama" | "ollama-cloud") {
        Ok(None)
    } else {
        anyhow::bail!("Provider keys require macOS Keychain")
    }
}
async fn codex_models() -> anyhow::Result<Vec<String>> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let exe = crate::providers::executable_for(plantool_core::Provider::Codex)
        .unwrap_or_else(|| "codex".into());
    let mut command = tokio::process::Command::new(exe);
    crate::sandbox::environment::apply(&mut command, Some(plantool_core::Provider::Codex));
    command
        .arg("app-server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut input = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    input.write_all(b"{\"id\":1,\"method\":\"initialize\",\"params\":{\"clientInfo\":{\"name\":\"plantool\",\"version\":\"0.1\"},\"capabilities\":{}}}\n").await?;
    let result=tokio::time::timeout(Duration::from_secs(10),async {
  while let Some(line)=lines.next_line().await? {let Ok(value)=serde_json::from_str::<Value>(&line) else{continue};
   if value["id"]==1 {anyhow::ensure!(value["error"].is_null(),"Codex initialization failed");input.write_all(b"{\"method\":\"initialized\"}\n{\"id\":2,\"method\":\"model/list\",\"params\":{\"limit\":100}}\n").await?;}
   if value["id"]==2 {anyhow::ensure!(value["error"].is_null(),"Codex model list failed");let list=value["result"]["data"].as_array().ok_or_else(||anyhow::anyhow!("Invalid Codex model catalog"))?;return Ok(list.iter().filter_map(|m|m["model"].as_str().or(m["id"].as_str()).map(str::to_owned)).collect())}
  }anyhow::bail!("Codex closed its model catalog")
 }).await?;
    let _ = child.kill().await;
    result
}
impl Gateway for HttpGateway {
    fn models<'a>(&'a self, provider: &'a str) -> BoxFuture<'a, anyhow::Result<Vec<String>>> {
        Box::pin(async move {
            if matches!(provider, "ollama" | "ollama-cloud") {
                return super::ollama::Connection::configured()?
                    .models(provider)
                    .await;
            }
            if provider == "openai" {
                return codex_models().await;
            }
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(20))
                .build()?;
            let mut request = client.get(format!("{}/models", base(provider)?));
            if let Some(key) = key(provider)? {
                request = request.bearer_auth(key)
            }
            let value: Value = request.send().await?.error_for_status()?.json().await?;
            Ok(value["data"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("Invalid model catalog"))?
                .iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .collect())
        })
    }
    fn chat<'a>(&'a self, provider: &'a str, body: Value) -> BoxFuture<'a, anyhow::Result<Reply>> {
        Box::pin(async move {
            if matches!(provider, "ollama" | "ollama-cloud") {
                return super::ollama::Connection::configured()?
                    .chat(provider, body)
                    .await;
            }
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(180))
                .build()?;
            let mut request = client
                .post(format!("{}/chat/completions", base(provider)?))
                .json(&body);
            if let Some(key) = key(provider)? {
                request = request.bearer_auth(key)
            }
            let response = request.send().await?;
            self::response(response).await
        })
    }
}
// Shared bounded response handling preserves quota headers for every transport.
pub(super) async fn response(response: reqwest::Response) -> anyhow::Result<Reply> {
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let reset = reset_at(response.headers(), crate::database::unix_now());
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= 16 * 1024 * 1024),
        "Provider response too large"
    );
    let bytes = response.bytes().await?;
    anyhow::ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "Provider response too large"
    );
    Ok(Reply {
        status,
        body: String::from_utf8(bytes.to_vec())?,
        content_type,
        reset,
    })
}

pub(crate) fn unix_seconds(value: i64) -> i64 {
    if value > 100_000_000_000 {
        value / 1000
    } else {
        value
    }
}
fn reset_at(headers: &reqwest::header::HeaderMap, now: i64) -> Option<i64> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.parse::<i64>()
                .ok()
                .filter(|n| *n >= 0)
                .map(|n| now.saturating_add(n))
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(s)
                        .ok()
                        .map(|d| d.timestamp())
                })
        })
        .or_else(|| {
            headers
                .get("x-ratelimit-reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<i64>().ok())
                .map(unix_seconds)
        })
}
#[cfg(test)]
mod reset_tests {
    use super::*;
    #[test]
    fn reset_headers_accept_seconds_milliseconds_and_http_dates() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-reset", "1793059200000".parse().unwrap());
        assert_eq!(reset_at(&headers, 1_793_000_000), Some(1_793_059_200));
        headers.insert("retry-after", "120".parse().unwrap());
        assert_eq!(reset_at(&headers, 1_793_000_000), Some(1_793_000_120));
        headers.insert(
            "retry-after",
            "Tue, 27 Oct 2026 00:00:00 GMT".parse().unwrap(),
        );
        assert_eq!(reset_at(&headers, 1_793_000_000), Some(1_793_059_200));
    }
}

pub async fn verify(
    home: &std::path::Path,
    gateway: &dyn Gateway,
) -> anyhow::Result<serde_json::Value> {
    use rusqlite::params;
    let cfg = super::router::config(home)?;
    cfg.validate()?;
    let mut catalogs = std::collections::BTreeMap::new();
    for route in cfg.routes.values() {
        if !catalogs.contains_key(&route.provider) {
            catalogs.insert(
                route.provider.clone(),
                gateway
                    .models(&route.provider)
                    .await
                    .map_err(|e| e.to_string()),
            );
        }
    }
    let mut db = crate::database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut results = serde_json::Map::new();
    for (name, route) in cfg.routes {
        let (model, error) = match &catalogs[&route.provider] {
            Ok(models) => {
                let mut models = models.clone();
                models.sort();
                let chosen = if route.model == "auto" {
                    models.first().cloned()
                } else {
                    models.iter().find(|m| *m == &route.model).cloned()
                };
                if chosen.is_some() {
                    (chosen, None)
                } else {
                    (
                        None,
                        Some("Configured model is absent from the live catalog".to_string()),
                    )
                }
            }
            Err(error) => (None, Some(error.clone())),
        };
        tx.execute("INSERT INTO route_health(route,available,model,verified_at,error) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(route) DO UPDATE SET available=excluded.available,model=excluded.model,verified_at=excluded.verified_at,error=excluded.error",params![name,model.is_some(),model,crate::database::unix_now(),error])?;
        results.insert(name, serde_json::json!({"model":model,"error":error}));
    }
    crate::database::audit(
        &tx,
        "human",
        "routes.verified",
        "providers",
        &Value::Object(results.clone()),
    )?;
    tx.commit()?;
    Ok(Value::Object(results))
}

#[cfg(test)]
mod credential_tests {
    #[test]
    fn ollama_uses_service_login_without_daemon_api_keys() {
        assert!(super::key("ollama").unwrap().is_none());
        assert!(super::key("ollama-cloud").unwrap().is_none());
    }
}
