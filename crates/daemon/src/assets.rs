use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
pub struct Assets;

pub const TOKEN_PLACEHOLDER: &str = "\"__PLANTOOL_TOKEN__\"";

pub fn serve(uri: &Uri, token: &str) -> Response {
    let path = uri.path().trim_start_matches('/');
    let candidate = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Assets::get(candidate) {
        let mime = mime_for(candidate);
        if candidate.ends_with(".html") {
            let html = String::from_utf8_lossy(&file.data)
                .replace(TOKEN_PLACEHOLDER, &format!("{token:?}"));
            return html_response(html);
        }
        return (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            file.data.into_owned(),
        )
            .into_response();
    }
    match Assets::get("index.html") {
        Some(index) => html_response(
            String::from_utf8_lossy(&index.data).replace(TOKEN_PLACEHOLDER, &format!("{token:?}")),
        ),
        None => (
            StatusCode::NOT_FOUND,
            "web assets are not built; run `npm run build` in web/",
        )
            .into_response(),
    }
}

fn html_response(html: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(html))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
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
