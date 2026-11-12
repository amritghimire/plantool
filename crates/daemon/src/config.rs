use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub home: PathBuf,
    pub port: u16,
    pub token: String,
    pub version: String,
    /// Extra address to listen on besides loopback, e.g. a Meshnet IP.
    pub bind: Option<IpAddr>,
    /// Host names besides loopback that the browser may use to reach the daemon.
    pub allowed_hosts: Vec<String>,
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

fn bind_from_env() -> Option<IpAddr> {
    let raw = std::env::var("PLANTOOL_BIND").ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.eq_ignore_ascii_case("auto") {
        let ip = mesh_ip();
        if ip.is_none() {
            tracing::warn!("PLANTOOL_BIND=auto found no Meshnet or Tailscale address; listening on loopback only");
        }
        return ip.map(IpAddr::V4);
    }
    match raw.parse::<IpAddr>() {
        Ok(ip) if !ip.is_loopback() => Some(ip),
        Ok(_) => None,
        Err(_) => {
            tracing::warn!("ignoring PLANTOOL_BIND={raw}: not an IP address");
            None
        }
    }
}

fn is_mesh(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// Asks the routing table which local address reaches the 100.64.0.0/10 range used by
/// Nord Meshnet and Tailscale. Connecting a UDP socket sends no packets.
fn mesh_ip() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(100, 64, 0, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if is_mesh(ip) => Some(ip),
        _ => None,
    }
}

fn allowed_hosts_from_env(bind: Option<IpAddr>) -> Vec<String> {
    let mut hosts: Vec<String> = std::env::var("PLANTOOL_ALLOWED_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    if let Some(ip) = bind.filter(|ip| !ip.is_unspecified()) {
        hosts.push(ip.to_string());
        if matches!(ip, IpAddr::V4(v4) if is_mesh(v4)) {
            hosts.extend([".nord".to_string(), ".ts.net".to_string()]);
        }
    }
    hosts
}

pub fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Config {
    pub fn from_env(version: &str) -> Config {
        let bind = bind_from_env();
        Config {
            bind,
            allowed_hosts: allowed_hosts_from_env(bind),
            home: default_home(),
            port: default_port(),
            token: random_token(),
            version: version.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_range() {
        assert!(is_mesh(Ipv4Addr::new(100, 87, 15, 58)));
        assert!(is_mesh(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(is_mesh(Ipv4Addr::new(100, 127, 255, 255)));
        assert!(!is_mesh(Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_mesh(Ipv4Addr::new(192, 168, 1, 10)));
    }
}
