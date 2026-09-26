use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

fn host_is_local(host: &str) -> bool {
    let h = host.trim();
    let h = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h)
    };
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

fn origin_is_local(origin: &str) -> bool {
    let rest = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
    match rest {
        Some(r) => host_is_local(r.split('/').next().unwrap_or("")),
        None => origin == "null",
    }
}

pub fn reject_reason(headers: &HeaderMap) -> Option<&'static str> {
    match headers.get("host").and_then(|h| h.to_str().ok()) {
        Some(h) if host_is_local(h) => {}
        _ => return Some("host is not loopback"),
    }
    if let Some(o) = headers.get("origin").and_then(|h| h.to_str().ok()) {
        if !origin_is_local(o) {
            return Some("origin is not loopback");
        }
    }
    None
}

pub async fn guard(req: Request<Body>, next: Next) -> Response {
    if let Some(reason) = reject_reason(req.headers()) {
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
        assert_eq!(reject_reason(&headers(Some("127.0.0.1:41200"), None)), None);
        assert_eq!(reject_reason(&headers(Some("localhost"), Some("http://localhost:5173"))), None);
        assert_eq!(reject_reason(&headers(Some("[::1]:41200"), Some("http://[::1]:41200"))), None);
    }

    #[test]
    fn rejects_foreign() {
        assert!(reject_reason(&headers(Some("evil.example:41200"), None)).is_some());
        assert!(reject_reason(&headers(Some("127.0.0.1"), Some("http://evil.example"))).is_some());
        assert!(reject_reason(&headers(None, None)).is_some());
    }
}
