//! Private keys exist only in Keychain and short-lived signing memory.
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub app_id: u64,
    pub installation_id: u64,
}
pub trait KeyStore: Send + Sync {
    fn read(&self, app_id: u64) -> anyhow::Result<Vec<u8>>;
    fn write(&self, app_id: u64, pem: &[u8]) -> anyhow::Result<()>;
}
pub struct Keychain;
#[cfg(target_os = "macos")]
impl KeyStore for Keychain {
    fn read(&self, id: u64) -> anyhow::Result<Vec<u8>> {
        crate::keychain::read("plantool/github-app", Some(&id.to_string()), false)
    }
    fn write(&self, id: u64, pem: &[u8]) -> anyhow::Result<()> {
        security_framework::passwords::set_generic_password(
            "plantool/github-app",
            &id.to_string(),
            pem,
        )?;
        invalidate(id);
        Ok(())
    }
}
#[cfg(not(target_os = "macos"))]
impl KeyStore for Keychain {
    fn read(&self, _: u64) -> anyhow::Result<Vec<u8>> {
        anyhow::bail!("GitHub App keys require macOS Keychain")
    }
    fn write(&self, _: u64, _: &[u8]) -> anyhow::Result<()> {
        anyhow::bail!("GitHub App keys require macOS Keychain")
    }
}
/// Used only by browser mutations after their action-bound passkey check.
pub struct InteractiveKeychain;
impl KeyStore for InteractiveKeychain {
    fn read(&self, id: u64) -> anyhow::Result<Vec<u8>> {
        crate::keychain::read("plantool/github-app", Some(&id.to_string()), true)
    }
    fn write(&self, id: u64, pem: &[u8]) -> anyhow::Result<()> {
        Keychain.write(id, pem)
    }
}
pub fn jwt(id: u64, pem: &[u8], now: i64) -> anyhow::Result<String> {
    #[derive(Serialize)]
    struct Claims {
        iat: i64,
        exp: i64,
        iss: String,
    }
    ensure!(id > 0, "App ID must be positive");
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(pem).context("Invalid RSA private key")?;
    Ok(jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &Claims {
            iat: now - 60,
            exp: now + 540,
            iss: id.to_string(),
        },
        &key,
    )?)
}
struct Cached {
    value: String,
    until: Instant,
}
fn cache() -> &'static Mutex<HashMap<(u64, u64), Cached>> {
    static CACHE: OnceLock<Mutex<HashMap<(u64, u64), Cached>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
pub fn invalidate(id: u64) {
    cache().lock().unwrap().retain(|(app, _), _| *app != id);
}
pub async fn token(
    http: &reqwest::Client,
    base: &str,
    cfg: &AppConfig,
    keys: &dyn KeyStore,
) -> anyhow::Result<String> {
    if let Some(c) = cache()
        .lock()
        .unwrap()
        .get(&(cfg.app_id, cfg.installation_id))
    {
        if c.until > Instant::now() {
            return Ok(c.value.clone());
        }
    }
    let signed = jwt(
        cfg.app_id,
        &keys.read(cfg.app_id)?,
        crate::database::unix_now(),
    )?;
    let reply = http
        .post(format!(
            "{base}/app/installations/{}/access_tokens",
            cfg.installation_id
        ))
        .bearer_auth(signed)
        .send()
        .await?;
    ensure!(
        reply.status().is_success(),
        "Installation token request failed ({})",
        reply.status()
    );
    let body: serde_json::Value = reply.json().await?;
    let expires = chrono::DateTime::parse_from_rfc3339(
        body["expires_at"]
            .as_str()
            .context("Missing token expiry")?,
    )?
    .timestamp();
    let lifetime = (expires - crate::database::unix_now() - 60).clamp(0, 3540) as u64;
    ensure!(lifetime > 0, "Installation token is already expiring");
    let value = body["token"]
        .as_str()
        .context("Missing installation token")?
        .to_string();
    cache().lock().unwrap().insert(
        (cfg.app_id, cfg.installation_id),
        Cached {
            value: value.clone(),
            until: Instant::now() + Duration::from_secs(lifetime),
        },
    );
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_rsa() {
        assert!(jwt(1, b"not a key", 100).is_err());
        assert!(jwt(0, b"not a key", 100).is_err());
    }
}
