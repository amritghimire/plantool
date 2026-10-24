//! Local Ollama owns cloud sign-in. Transport location never changes provider policy.
use super::gateway::Reply;
use anyhow::{ensure, Context};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

pub(super) struct Connection {
    base: reqwest::Url,
    client: reqwest::Client,
}
#[derive(Deserialize)]
struct Model {
    name: String,
    #[serde(default)]
    remote_model: String,
    #[serde(default)]
    remote_host: String,
}
impl Model {
    fn cloud(&self) -> bool {
        let name = self.name.to_ascii_lowercase();
        !self.remote_model.is_empty()
            || !self.remote_host.is_empty()
            || name.ends_with(":cloud")
            || name.ends_with("-cloud")
    }
    fn matches(&self, name: &str) -> bool {
        self.name == name || self.name.strip_suffix(":latest") == Some(name)
    }
}
impl Connection {
    pub(super) fn configured() -> anyhow::Result<Self> {
        Self::new(&crate::providers::ollama_base_url())
    }
    fn new(base: &str) -> anyhow::Result<Self> {
        let base = reqwest::Url::parse(base)?;
        let host = base
            .host_str()
            .unwrap_or("")
            .trim_start_matches('[')
            .trim_end_matches(']');
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        ensure!(
            loopback
                && ["http", "https"].contains(&base.scheme())
                && base.username().is_empty()
                && base.password().is_none()
                && base.query().is_none()
                && base.fragment().is_none()
                && base.path() == "/",
            "Engineering OS Ollama requires a loopback OLLAMA_HOST without credentials or a path"
        );
        Ok(Self {
            base,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(180))
                .build()?,
        })
    }
    async fn catalog(&self) -> anyhow::Result<Vec<Model>> {
        let response = self
            .client
            .get(self.base.join("api/tags")?)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .context("Start local Ollama, then verify routes again")?;
        let reply = super::gateway::response(response).await?;
        ensure!(reply.status == 200, "Ollama catalog unavailable (HTTP {}); start Ollama and use ollama signin for cloud access", reply.status);
        #[derive(Deserialize)]
        struct Catalog {
            models: Vec<Model>,
        }
        let models = serde_json::from_str::<Catalog>(&reply.body)
            .context("Invalid Ollama native model catalog")?
            .models;
        ensure!(
            models
                .iter()
                .all(|m| !m.name.is_empty() && m.name.len() <= 200),
            "Invalid Ollama model name"
        );
        Ok(models)
    }
    pub(super) async fn provider(&self, name: &str) -> anyhow::Result<&'static str> {
        let models = self.catalog().await?;
        let model = models
            .iter()
            .find(|m| m.matches(name))
            .context("Model is absent from local Ollama")?;
        Ok(if model.cloud() {
            "ollama-cloud"
        } else {
            "ollama"
        })
    }
    pub(super) async fn models(&self, provider: &str) -> anyhow::Result<Vec<String>> {
        ensure!(
            ["ollama", "ollama-cloud"].contains(&provider),
            "Invalid Ollama provider"
        );
        let mut names = Vec::new();
        for model in self.catalog().await? {
            if model.cloud() == (provider == "ollama-cloud") {
                // An omitted tag denotes :latest in Ollama, not a different model.
                if let Some(alias) = model.name.strip_suffix(":latest") {
                    names.push(alias.to_owned());
                }
                names.push(model.name);
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }
    pub(super) async fn chat(&self, provider: &str, body: Value) -> anyhow::Result<Reply> {
        ensure!(
            ["ollama", "ollama-cloud"].contains(&provider),
            "Invalid Ollama provider"
        );
        let name = body["model"].as_str().context("Ollama model is required")?;
        // Recheck before transmitting prompts: an owner may have repointed an alias
        // since setup or a persisted route-health check.
        let models = self.catalog().await?;
        let model = models.iter().find(|m| m.matches(name)).context(
            "Model is absent from local Ollama; pull its exact reference and verify routes",
        )?;
        ensure!(
            model.cloud() == (provider == "ollama-cloud"),
            "Ollama model does not match its local/cloud provider policy; verify routes"
        );
        let response = self
            .client
            .post(self.base.join("v1/chat/completions")?)
            .json(&body)
            .send()
            .await?;
        let mut reply = super::gateway::response(response).await?;
        if provider == "ollama-cloud"
            && matches!(reply.status, 401 | 403)
            && !super::router::quota(reply.status, &reply.body)
        {
            reply.content_type = "application/json".into();
            reply.body = serde_json::json!({"error":{"message":"Ollama Cloud authentication failed. Run ollama signin on the daemon host, then verify routes.","type":"authentication_error"}}).to_string();
        }
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Fake {
        catalog: Value,
        status: StatusCode,
        calls: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    }
    async fn fixture(status: StatusCode) -> (Connection, Fake, tokio::task::JoinHandle<()>) {
        let fake = Fake {
            catalog: json!({"models":[
                {"name":"local:latest"},
                {"name":"big:cloud","remote_model":"big","remote_host":"https://ollama.com"},
                {"name":"big:120b-cloud"},
                {"name":"innocent:latest","remote_model":"remote-aliased-model","remote_host":"https://ollama.com"}
            ]}),
            status,
            calls: Default::default(),
        };
        let app=Router::new().route("/api/tags",get(|State(f):State<Fake>|async move{Json(f.catalog)}))
            .route("/v1/chat/completions",post(|State(f):State<Fake>,headers:HeaderMap,Json(body):Json<Value>|async move {
                f.calls.lock().unwrap().push((headers,body));
                let body = if f.status == StatusCode::FORBIDDEN {json!({"error":"quota exceeded"})}
                    else {json!({"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":3}})};
                (f.status,[("retry-after","7")],Json(body))
            })).with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let connection =
            Connection::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (connection, fake, handle)
    }
    #[test]
    fn only_loopback_endpoints_without_credentials_or_paths_are_accepted() {
        for host in [
            "http://localhost:11434",
            "http://127.0.0.2:11434",
            "http://[::1]:11434",
        ] {
            assert!(Connection::new(host).is_ok(), "{host}");
        }
        for host in [
            "http://ollama.com",
            "http://192.168.1.5:11434",
            "http://localhost.example.com",
            "http://user:secret@localhost:11434",
            "http://localhost:11434/api",
            "http://localhost/?key=secret",
            "file:///tmp/ollama",
        ] {
            assert!(Connection::new(host).is_err(), "{host}");
        }
    }
    #[tokio::test]
    async fn native_catalog_separates_cloud_references_and_renamed_aliases() {
        let (connection, _, server) = fixture(StatusCode::OK).await;
        assert_eq!(
            connection.provider("innocent").await.unwrap(),
            "ollama-cloud"
        );
        assert_eq!(connection.provider("local").await.unwrap(), "ollama");
        assert_eq!(
            connection.models("ollama").await.unwrap(),
            vec!["local", "local:latest"]
        );
        assert_eq!(
            connection.models("ollama-cloud").await.unwrap(),
            vec!["big:120b-cloud", "big:cloud", "innocent", "innocent:latest"]
        );
        server.abort();
    }
    #[tokio::test]
    async fn cloud_chat_uses_local_service_without_an_authorization_key_and_preserves_quota() {
        let (connection, fake, server) = fixture(StatusCode::TOO_MANY_REQUESTS).await;
        let body = json!({"model":"big:cloud","messages":[{"role":"user","content":"fake prompt"}],"stream":false});
        let reply = connection.chat("ollama-cloud", body.clone()).await.unwrap();
        assert_eq!(reply.status, 429);
        assert!(reply.reset.unwrap() >= crate::database::unix_now() + 5);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, body);
        assert!(!calls[0].0.contains_key("authorization"));
        server.abort();
    }
    #[tokio::test]
    async fn local_only_policy_never_transmits_a_prompt_to_cloud_including_an_alias() {
        let (connection, fake, server) = fixture(StatusCode::OK).await;
        for name in ["big:cloud", "big:120b-cloud", "innocent", "innocent:latest"] {
            assert!(connection
                .chat("ollama", json!({"model":name,"messages":[]}))
                .await
                .is_err());
        }
        assert!(connection
            .chat("ollama-cloud", json!({"model":"local","messages":[]}))
            .await
            .is_err());
        assert!(connection
            .chat(
                "ollama-cloud",
                json!({"model":"missing:cloud","messages":[]})
            )
            .await
            .is_err());
        assert!(fake.calls.lock().unwrap().is_empty());
        connection
            .chat("ollama", json!({"model":"local","messages":[]}))
            .await
            .unwrap();
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        server.abort();
    }
    #[tokio::test]
    async fn quota_errors_on_403_are_preserved_instead_of_rewritten_as_login_errors() {
        let (connection, _, server) = fixture(StatusCode::FORBIDDEN).await;
        let reply = connection
            .chat("ollama-cloud", json!({"model":"big:cloud","messages":[]}))
            .await
            .unwrap();
        assert_eq!(reply.status, 403);
        assert!(super::super::router::quota(reply.status, &reply.body));
        assert!(!reply.body.contains("ollama signin"));
        server.abort();
    }
    #[tokio::test]
    async fn signed_out_cloud_returns_actionable_login_error() {
        let (connection, _, server) = fixture(StatusCode::UNAUTHORIZED).await;
        let reply = connection
            .chat("ollama-cloud", json!({"model":"big:cloud","messages":[]}))
            .await
            .unwrap();
        assert_eq!(reply.status, 401);
        assert!(reply.body.contains("ollama signin"));
        server.abort();
    }
    #[tokio::test]
    async fn redirects_are_not_followed_and_no_prompt_is_sent_without_a_catalog() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/api/tags",
                get(|| async {
                    (
                        StatusCode::TEMPORARY_REDIRECT,
                        [("location", "/redirect-target")],
                    )
                        .into_response()
                }),
            )
            .route(
                "/redirect-target",
                get(
                    |State(c): State<Arc<std::sync::atomic::AtomicUsize>>| async move {
                        c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Json(json!({"models":[]}))
                    },
                ),
            )
            .with_state(calls.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let connection = Connection::new(&format!("http://{address}")).unwrap();
        assert!(connection.models("ollama-cloud").await.is_err());
        assert!(connection
            .chat("ollama", json!({"model":"local","messages":[]}))
            .await
            .is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        server.abort();
    }
}
