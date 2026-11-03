use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub home: PathBuf,
    pub port: u16,
    pub token: String,
    pub version: String,
}

pub fn default_home() -> PathBuf {
    if let Some(h) = std::env::var_os("PLANTOOL_HOME") {
        return PathBuf::from(h);
    }
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".plantool")
}

/// The GitHub sign-in to use for the private plantool repository, if any.
pub fn github_token() -> Option<String> {
    ["GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .or_else(|| {
            std::process::Command::new("gh")
                .args(["auth", "token"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|t| !t.is_empty())
        })
}

pub fn default_port() -> u16 {
    std::env::var("PLANTOOL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(plantool_core::DEFAULT_PORT)
}

pub fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Config {
    pub fn from_env(version: &str) -> Config {
        Config {
            home: default_home(),
            port: default_port(),
            token: random_token(),
            version: version.to_string(),
        }
    }
}
