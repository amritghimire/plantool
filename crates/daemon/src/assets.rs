use std::collections::HashMap;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use axum::body::{Body, Bytes};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use flate2::write::GzEncoder;
use flate2::Compression;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
pub struct Assets;

pub const TOKEN_PLACEHOLDER: &str = "\"__PLANTOOL_TOKEN__\"";

pub fn serve(uri: &Uri, headers: &HeaderMap, token: &str) -> Response {
    let gzip = accepts_gzip(headers);
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Assets::get(candidate) {
        if candidate.ends_with(".html") {
            return html_response(&file.data, token, gzip);
        }
        let mime = mime_for(candidate);
        let compressible = is_compressible(candidate);
        let body = if gzip && compressible {
            Some(cached_gzip(file.metadata.sha256_hash(), &file.data))
        } else {
            None
        };
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable");
        if compressible {
            response = response.header(header::VARY, "accept-encoding");
        }
        let response = match body {
            Some(gz) => response
                .header(header::CONTENT_ENCODING, "gzip")
                .body(Body::from(gz)),
            None => response.body(Body::from(file.data.into_owned())),
        };
        return response.unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    match Assets::get("index.html") {
        Some(index) => html_response(&index.data, token, gzip),
        None => (
            StatusCode::NOT_FOUND,
            "web assets are not built; run `npm run build` in web/",
        )
            .into_response(),
    }
}

fn html_response(raw: &[u8], token: &str, gzip: bool) -> Response {
    let html = String::from_utf8_lossy(raw).replace(TOKEN_PLACEHOLDER, &format!("{token:?}"));
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::VARY, "accept-encoding");
    let response = if gzip {
        response
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(compress(html.as_bytes())))
    } else {
        response.body(Body::from(html))
    };
    response.unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|coding| {
            let mut parts = coding.split(';');
            let name = parts.next().unwrap_or("").trim();
            let refused = parts.any(|p| {
                p.trim()
                    .strip_prefix("q=")
                    .and_then(|q| q.trim().parse::<f32>().ok())
                    == Some(0.0)
            });
            name.eq_ignore_ascii_case("gzip") && !refused
        })
}

fn is_compressible(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or(""),
        "html" | "js" | "mjs" | "css" | "json" | "svg" | "map" | "txt"
    )
}

fn cached_gzip(hash: [u8; 32], data: &[u8]) -> Bytes {
    static CACHE: OnceLock<Mutex<HashMap<[u8; 32], Bytes>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&hash).cloned()) {
        return hit;
    }
    let gz = Bytes::from(compress(data));
    if let Ok(mut c) = cache.lock() {
        c.insert(hash, gz.clone());
    }
    gz
}

fn compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "map" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use flate2::read::GzDecoder;
    use std::io::Read;

    async fn body_of(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec()
    }

    fn asset_with(ext: &str) -> String {
        Assets::iter()
            .find(|p| p.ends_with(ext))
            .map(|p| p.into_owned())
            .unwrap_or_else(|| panic!("web/dist has no {ext} asset; run `npm run build` in web/"))
    }

    fn gzip_headers(value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT_ENCODING, HeaderValue::from_static(value));
        h
    }

    #[tokio::test]
    async fn js_is_gzipped_only_when_the_client_accepts_it() {
        let path = asset_with(".js");
        let uri: Uri = format!("/{path}").parse().expect("uri");
        let raw = Assets::get(&path).expect("asset").data.into_owned();

        let gz = serve(&uri, &gzip_headers("gzip, deflate, br"), "t");
        assert_eq!(gz.headers()[header::CONTENT_ENCODING], "gzip");
        assert_eq!(gz.headers()[header::VARY], "accept-encoding");
        let packed = body_of(gz).await;
        assert!(packed.len() < raw.len());
        let mut unpacked = Vec::new();
        GzDecoder::new(&packed[..])
            .read_to_end(&mut unpacked)
            .expect("valid gzip");
        assert_eq!(unpacked, raw);

        let plain = serve(&uri, &HeaderMap::new(), "t");
        assert!(plain.headers().get(header::CONTENT_ENCODING).is_none());
        assert_eq!(plain.headers()[header::VARY], "accept-encoding");
        assert_eq!(body_of(plain).await, raw);

        let refused = serve(&uri, &gzip_headers("gzip;q=0, br"), "t");
        assert!(refused.headers().get(header::CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn html_is_gzipped_after_token_injection() {
        let response = serve(&"/".parse().expect("uri"), &gzip_headers("gzip"), "secret");
        assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
        let packed = body_of(response).await;
        let mut html = String::new();
        GzDecoder::new(&packed[..])
            .read_to_string(&mut html)
            .expect("valid gzip");
        assert!(html.contains("\"secret\""));
        assert!(!html.contains(TOKEN_PLACEHOLDER));
    }

    #[tokio::test]
    async fn fonts_are_never_gzipped() {
        let path = asset_with(".woff2");
        let response = serve(
            &format!("/{path}").parse().expect("uri"),
            &gzip_headers("gzip"),
            "t",
        );
        assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
        assert!(response.headers().get(header::VARY).is_none());
    }
}
