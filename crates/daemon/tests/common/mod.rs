#![allow(dead_code)]
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::Url;

pub async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    cookie: Option<&str>,
    proof: Option<&str>,
) -> Response {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:41300")
        .header("origin", "http://localhost:41300")
        .header("content-type", "application/json");
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    if let Some(p) = proof {
        b = b.header("x-plantool-assertion", p);
    }
    app.clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
pub async fn value(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
pub struct Browser {
    pub auth: WebauthnAuthenticator<SoftPasskey>,
    pub cookie: String,
}
impl Browser {
    pub async fn register(app: &Router, home: &std::path::Path) -> Self {
        let code = plantool_daemon::auth::setup_code(home).unwrap();
        let mut b = Self {
            auth: WebauthnAuthenticator::new(SoftPasskey::new(true)),
            cookie: String::new(),
        };
        let response = raw(
            app,
            "POST",
            "/api/auth/register/start",
            json!({"code":code}),
            None,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let start = value(response).await;
        assert_eq!(start["options"]["publicKey"]["rp"]["id"], "localhost");
        assert_eq!(
            start["options"]["publicKey"]["authenticatorSelection"]["authenticatorAttachment"],
            "platform"
        );
        assert_eq!(
            start["options"]["publicKey"]["authenticatorSelection"]["userVerification"],
            "required"
        );
        let cred = b
            .auth
            .do_registration(
                Url::parse("http://localhost:41300").unwrap(),
                registration_options(start["options"].clone()),
            )
            .unwrap();
        let response = raw(
            app,
            "POST",
            "/api/auth/register/finish",
            json!({"id":start["id"],"credential":cred}),
            None,
            None,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            value_if_error(&response)
        );
        let cookie = response.headers()["set-cookie"].to_str().unwrap();
        assert!(
            cookie.contains("HttpOnly")
                && cookie.contains("SameSite=Strict")
                && cookie.contains("Path=/")
        );
        b.cookie = cookie.split(';').next().unwrap().to_string();
        b
    }
    pub async fn proof(&mut self, app: &Router, method: &str, path: &str, body: Value) -> String {
        let start = value(
            raw(
                app,
                "POST",
                "/api/auth/assertion/start",
                json!({"action":{"method":method,"path":path,"body":body}}),
                Some(&self.cookie),
                None,
            )
            .await,
        )
        .await;
        let credential = self
            .auth
            .do_authentication(
                Url::parse("http://localhost:41300").unwrap(),
                serde_json::from_value(start["options"].clone()).unwrap(),
            )
            .unwrap();
        let response = raw(
            app,
            "POST",
            "/api/auth/assertion/finish",
            json!({"id":start["id"],"credential":credential}),
            Some(&self.cookie),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        value(response).await["proof"].as_str().unwrap().to_string()
    }
}
fn value_if_error(r: &Response) -> String {
    format!("headers {:?}", r.headers())
}

pub async fn with_human(app: &Router, req: Request<Body>) -> Request<Body> {
    use std::{collections::HashMap, sync::LazyLock};
    static BROWSERS: LazyLock<tokio::sync::Mutex<HashMap<String, Browser>>> =
        LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));
    let health = value(raw(app, "GET", "/health", Value::Null, None, None).await).await;
    let home = health["home"].as_str().unwrap();
    let mut browsers = BROWSERS.lock().await;
    if !browsers.contains_key(home) {
        browsers.insert(
            home.to_string(),
            Browser::register(app, std::path::Path::new(home)).await,
        );
    }
    let browser = browsers.get_mut(home).unwrap();
    let (mut parts, body) = req.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            use sha2::{Digest, Sha256};
            json!({"binary_sha256":format!("{:x}",Sha256::digest(&bytes))})
        })
    };
    let path = parts.uri.path_and_query().unwrap().as_str();
    if plantool_daemon::auth::requires_assertion(parts.method.as_str(), path, &val, true) {
        let proof = browser.proof(app, parts.method.as_str(), path, val).await;
        parts
            .headers
            .insert("x-plantool-assertion", proof.parse().unwrap());
    }
    parts
        .headers
        .insert("cookie", browser.cookie.parse().unwrap());
    parts
        .headers
        .insert("origin", "http://localhost:41300".parse().unwrap());
    Request::from_parts(parts, Body::from(bytes))
}

// The software authenticator is not attached hardware. Assert the emitted platform
// requirement above, then adapt only the emulator input; the server keeps its UV policy.
pub fn registration_options(mut options: Value) -> webauthn_rs::prelude::CreationChallengeResponse {
    options["publicKey"]["authenticatorSelection"]["authenticatorAttachment"] = Value::Null;
    serde_json::from_value(options).unwrap()
}
