use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;

pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

fn host_is_allowed(host: &str, allowed: &[String]) -> bool {
    let h = host.trim();
    let h = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h)
    };
    let h = h.to_ascii_lowercase();
    matches!(h.as_str(), "127.0.0.1" | "localhost" | "::1")
        || allowed.iter().any(|a| match a.strip_prefix('.') {
            Some(suffix) => h
                .strip_suffix(suffix)
                .is_some_and(|rest| rest.ends_with('.')),
            None => *a == h,
        })
}

fn origin_is_allowed(origin: &str, allowed: &[String]) -> bool {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    match rest {
        Some(r) => host_is_allowed(r.split('/').next().unwrap_or(""), allowed),
        None => origin == "null",
    }
}

pub fn reject_reason(headers: &HeaderMap, allowed: &[String]) -> Option<&'static str> {
    match headers.get("host").and_then(|h| h.to_str().ok()) {
        Some(h) if host_is_allowed(h, allowed) => {}
        _ => return Some("host is not allowed"),
    }
    if let Some(o) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        if !origin_is_allowed(o, allowed) {
            return Some("origin is not allowed");
        }
    }
    None
}

pub async fn guard(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    if let Some(reason) = reject_reason(req.headers(), &state.config.allowed_hosts) {
        return (StatusCode::FORBIDDEN, reason).into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(host: Option<&str>, origin: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = host {
            h.insert("host", HeaderValue::from_str(v).unwrap());
        }
        if let Some(v) = origin {
            h.insert("origin", HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn accepts_loopback() {
        assert_eq!(
            reject_reason(&headers(Some("127.0.0.1:41200"), None), &[]),
            None
        );
        assert_eq!(
            reject_reason(
                &headers(Some("localhost"), Some("http://localhost:5173")),
                &[]
            ),
            None
        );
        assert_eq!(
            reject_reason(
                &headers(Some("[::1]:41200"), Some("http://[::1]:41200")),
                &[]
            ),
            None
        );
    }

    #[test]
    fn rejects_foreign() {
        assert!(reject_reason(&headers(Some("evil.example:41200"), None), &[]).is_some());
        assert!(reject_reason(
            &headers(Some("127.0.0.1"), Some("http://evil.example")),
            &[]
        )
        .is_some());
        assert!(reject_reason(&headers(None, None), &[]).is_some());
    }

    #[test]
    fn accepts_allowed_hosts_only() {
        let allowed = vec!["100.80.10.20".to_string(), "laptop.nord".to_string()];
        assert_eq!(
            reject_reason(
                &headers(
                    Some("100.80.10.20:41200"),
                    Some("http://100.80.10.20:41200")
                ),
                &allowed
            ),
            None
        );
        assert_eq!(
            reject_reason(&headers(Some("Laptop.Nord:41200"), None), &allowed),
            None
        );
        assert!(reject_reason(&headers(Some("100.80.10.21:41200"), None), &allowed).is_some());

        let suffix = vec![".nord".to_string()];
        assert_eq!(
            reject_reason(
                &headers(
                    Some("nordvpn-foraker5954.nord:41200"),
                    Some("http://nordvpn-foraker5954.nord:41200")
                ),
                &suffix
            ),
            None
        );
        assert!(reject_reason(&headers(Some("nord:41200"), None), &suffix).is_some());
        assert!(reject_reason(&headers(Some("evilnord:41200"), None), &suffix).is_some());
        assert!(reject_reason(
            &headers(Some("100.80.10.20:41200"), Some("http://evil.example")),
            &allowed
        )
        .is_some());
    }
}
