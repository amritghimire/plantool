//! Explicit, synthetic live checks. Never return provider bodies, prompts or credentials.
use super::{gateway::Gateway, lease, router};
use crate::{database, AppState};
use serde_json::{json, Value};
use std::time::Duration;
fn stage(name: &str, ok: bool, message: &str, next: &str) -> Value {
    json!({"stage":name,"ok":ok,"message":message,"next_step":next})
}
/// Synthetic diagnostics carry no project prompt. Still remove credential-shaped
/// fields and opaque token strings before retaining native/transport error text.
pub(crate) fn sanitized_error(error: &str) -> String {
    let fields = regex::Regex::new(r#"(?i)(authorization|bearer|api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|password|secret)[\s"':=]+[^\s,;}]+"#).unwrap();
    let urls = regex::Regex::new(r"https?://[^/\s@]+@").unwrap();
    let opaque = regex::Regex::new(r"[A-Za-z0-9_-]{24,}").unwrap();
    let clean = fields.replace_all(error, "$1=[REDACTED]");
    let clean = urls.replace_all(&clean, "https://[REDACTED]@");
    opaque
        .replace_all(&clean, "[REDACTED]")
        .chars()
        .take(800)
        .collect()
}
fn failure(name: &str, status: Option<u16>, error: &str) -> Value {
    let (message,next)=match status {
        Some(401|403)=>("Authentication or provider access denied","Check the provider login, Keychain service and account access; then retest."),
        Some(404)=>("Model or upstream backend not found","Verify the exact model ID and provider backend. Catalog presence does not guarantee execution."),
        Some(429)=>("Provider rate limit or quota reached","Wait for the provider reset or use another permitted route. Retesting consumes quota."),
        Some(400|422)=>("Provider rejected the request format","This route may not support tools or structured output. Inspect the named stage and use a compatible model."),
        Some(500..=599)=>("Upstream provider failed","Retry later or use another route; this does not mean the worker needs Repair."),
        _ if error.to_lowercase().contains("quota") || error.to_lowercase().contains("usage limit") => ("Provider quota or usage limit reached", "Wait for account limits to reset or use another configured route."),
        _ if error.to_lowercase().contains("login") || error.to_lowercase().contains("unauthorized") => ("Provider login is unavailable", "Sign in on the daemon host using the provider's normal login flow, then retest."),
        _ if error.contains("workspace routing discovery") => ("Codex workspace routing discovery failed", "The signed-in account could not discover its inference route. Check the native Codex CLI, account workspace access and network; catalog presence is not proof of execution."),
        _ if error.contains("requires permission") => ("Codex requested permission", "The synthetic read should need no elevated access. Inspect Codex sandbox configuration; this check did not grant permission."),
        _ if error.contains("interactive input") => ("Codex requested interactive input", "Check Codex sign-in and setup in your terminal, then retry the synthetic test."),
        _ if error.contains("thread/start failed") => ("Codex could not start a thread", "Check Codex CLI compatibility and account/model access. The app-server initialized but rejected thread creation."),
        _ if error.contains("initialize failed") => ("Codex app-server initialization failed", "Check the installed Codex CLI version and normal signed-in app-server startup."),
        _ if error.contains("failed to start") => ("Codex process could not start", "Check the Codex executable and macOS sandbox permissions on the daemon host."),
        _ if error.contains("Codex response check failed") || error.contains("expected value") => ("Codex returned invalid diagnostic JSON", "The synthetic file read and exact JSON reply were not verified. Retest with a model supporting these capabilities."),
        _ if error.contains("Keychain")=>("Provider credential is missing or inaccessible","Check the matching plantool/<provider> generic-password service in the daemon user's Keychain."),
        _ if error.contains("timed out") || error.contains("timeout")=>("Provider request timed out","Check model loading and server reachability; retry after the model is ready."),
        _ =>("Connection, catalog or response check failed","Follow the provider guide below. Check the exact model, local service and login, then retest."),
    };
    let mut value = stage(name, false, message, next);
    value["http_status"] = json!(status);
    if !error.is_empty() {
        value["technical_detail"] = json!(sanitized_error(error));
    }
    value
}
async fn chat(
    home: &std::path::Path,
    selected: &router::Selected,
    gateway: &dyn Gateway,
    mut body: Value,
) -> Result<Value, Value> {
    body["model"] = json!(selected.route.model);
    router::diagnostic_capacity(home, selected).map_err(|_| {
        stage(
            "budget",
            false,
            "Diagnostic allowance exhausted",
            "Wait for the budget reset; implementation/review reserves are not used",
        )
    })?;
    router::count(home, selected, 1, 0, database::unix_now())
        .map_err(|_| failure("accounting", None, ""))?;
    let reply = gateway
        .chat(&selected.route.provider, body)
        .await
        .map_err(|e| failure("connection", None, &e.to_string()))?;
    if !(200..300).contains(&reply.status) {
        if reply.status == 429 {
            let _ = router::exhaust(home, selected, reply.reset, database::unix_now());
        }
        let body: Value = serde_json::from_str(&reply.body).unwrap_or(Value::Null);
        let message = body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut error = failure("response", Some(reply.status), message);
        error["retry_at"] = json!(reply.reset);
        return Err(error);
    }
    let value: Value =
        serde_json::from_str(&reply.body).map_err(|_| failure("response", None, ""))?;
    let tokens = value["usage"]["prompt_tokens"].as_u64().unwrap_or(0)
        + value["usage"]["completion_tokens"].as_u64().unwrap_or(0);
    let _ = router::count(home, selected, 0, tokens, database::unix_now());
    Ok(value)
}
pub async fn http_checks(
    home: &std::path::Path,
    selected: &router::Selected,
    gateway: &dyn Gateway,
) -> Vec<Value> {
    let mut stages = vec![];
    let tests = [
        (
            "basic chat",
            json!({"messages":[{"role":"user","content":"Reply with OK only."}],"stream":false,"max_tokens":512}),
        ),
        (
            "tools",
            json!({"messages":[{"role":"user","content":"Call diagnostic_add with a=2 and b=3. Do not answer in text."}],"stream":false,"max_tokens":1024,"tool_choice":{"type":"function","function":{"name":"diagnostic_add"}},"tools":[{"type":"function","function":{"name":"diagnostic_add","description":"Add two synthetic numbers","parameters":{"type":"object","properties":{"a":{"type":"integer"},"b":{"type":"integer"}},"required":["a","b"],"additionalProperties":false}}}]}),
        ),
        (
            "structured JSON",
            json!({"messages":[{"role":"user","content":"Return exactly one JSON object matching this schema: {\"type\":\"object\",\"properties\":{\"ok\":{\"type\":\"boolean\"}},\"required\":[\"ok\"],\"additionalProperties\":false}. Set ok=true. No prose."}],"stream":false,"max_tokens":512,"response_format":{"type":"json_schema","json_schema":{"name":"diagnostic","schema":{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false}}}}),
        ),
    ];
    for (name, body) in tests {
        match chat(home, selected, gateway, body).await {
            Err(mut error) => {
                error["stage"] = json!(name);
                stages.push(error);
                break;
            }
            Ok(value) => {
                let message = &value["choices"][0]["message"];
                let ok = match name {
                    "basic chat" => message["content"]
                        .as_str()
                        .is_some_and(|s| s.trim() == "OK"),
                    "tools" => message["tool_calls"].as_array().is_some_and(|calls| {
                        calls.len() == 1
                            && calls[0]["function"]["name"] == "diagnostic_add"
                            && calls[0]["function"]["arguments"]
                                .as_str()
                                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                                .is_some_and(|v| v == json!({"a":2,"b":3}))
                    }),
                    _ => message["content"]
                        .as_str()
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .is_some_and(|v| v == json!({"ok":true})),
                };
                stages.push(stage(name,ok,if ok {"Synthetic check passed"}else{"Provider response did not satisfy this capability check"},if ok {""}else{"Use a model supporting this capability. A catalog check alone does not prove tools/JSON compatibility."}));
                if !ok {
                    break;
                }
            }
        }
    }
    stages
}
pub async fn run(state: &AppState, name: &str) -> anyhow::Result<Value> {
    anyhow::ensure!(
        !state.runs.execution.status().stopped,
        "Resume execution before a live provider check"
    );
    let cfg = router::config(&state.config.home)?;
    let mut route = cfg
        .routes
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown route"))?
        .clone();
    let gateway = super::gateway::HttpGateway;
    let catalog = gateway.models(&route.provider).await;
    let mut stages = vec![];
    match catalog {
        Ok(mut models) => {
            models.sort();
            if route.model == "auto" {
                route.model = models.first().cloned().unwrap_or_default();
            }
            if !models.contains(&route.model) {
                stages.push(stage("catalog",false,"Configured model is absent","Pull the exact Ollama model or choose an available catalog ID; save and verify routes."));
            } else {
                stages.push(stage(
                    "catalog",
                    true,
                    "Exact model found; live execution not yet proven",
                    "",
                ));
            }
        }
        Err(error) => stages.push(failure(
            "catalog / authentication",
            None,
            &error.to_string(),
        )),
    }
    if stages.last().is_some_and(|s| s["ok"] == true) {
        let selected = router::Selected {
            name: name.into(),
            route: route.clone(),
            same_family_warning: false,
        };
        let owner = format!("diagnostic:{}", uuid::Uuid::new_v4());
        let _lease = lease::acquire(
            &state.config.home,
            &owner,
            None,
            lease::Model {
                route: name.into(),
                provider: route.provider.clone(),
                model: route.model.clone(),
            },
            false,
            i64::MAX,
            state.runs.execution.clone(),
            &lease::Ollama,
        )
        .await?;
        let checks = async {
            if route.adapter == "codex" {
                codex_check(state, &selected).await
            } else {
                http_checks(&state.config.home, &selected, &gateway).await
            }
        };
        tokio::pin!(checks);
        let generation = state.runs.execution.status().generation;
        let result=tokio::time::timeout(Duration::from_secs(600),async {
            loop {tokio::select! {result=&mut checks=>break Ok(result),_=tokio::time::sleep(Duration::from_millis(100))=>{
                let current=state.runs.execution.status();if current.stopped||current.generation!=generation {break Err("Execution stopped; diagnostic cancelled");}
            }}}
        }).await;
        match result {
            Ok(Ok(result)) => stages.extend(result),
            Ok(Err(_)) => stages.push(stage(
                "cancelled",
                false,
                "Execution stopped; diagnostic cancelled",
                "Resume execution before retesting",
            )),
            Err(_) => stages.push(failure("timeout", None, "timeout")),
        }
    }
    let result = json!({"route":name,"provider":route.provider,"model":route.model,"checked_at":database::unix_now(),"ok":stages.iter().all(|s|s["ok"]==true),"stages":stages,"redacted":true});
    database::set(
        &database::open(&state.config.home)?,
        &format!("provider.diagnostic:{name}"),
        &result,
    )?;
    database::audit(
        &database::open(&state.config.home)?,
        "human",
        "provider.diagnostic",
        name,
        &result,
    )?;
    Ok(result)
}
#[cfg(not(target_os = "macos"))]
async fn codex_check(_state: &AppState, _selected: &router::Selected) -> Vec<Value> {
    vec![stage(
        "Codex CLI execution",
        false,
        "Native sandboxed Codex probe requires macOS",
        "Run this check on the daemon's macOS host; no OpenAI API key is used",
    )]
}
#[cfg(target_os = "macos")]
async fn codex_check(state: &AppState, selected: &router::Selected) -> Vec<Value> {
    use crate::{
        providers::{ProviderEvent, RunInput, RunOptions},
        sandbox,
    };
    let result=async {
        let root=state.config.home.join("diagnostics");std::fs::create_dir_all(&root)?;
        let dir=tempfile::tempdir_in(root)?;
        let expected=uuid::Uuid::new_v4().to_string();std::fs::write(dir.path().join("probe.txt"),&expected)?;
        let policy=sandbox::NeverSent::new(&[])?;
        let profile=sandbox::Sandbox::required(sandbox::Paths{owner_home:std::env::var_os("HOME").ok_or_else(||anyhow::anyhow!("Owner home missing"))?.into(),daemon_home:state.config.home.clone(),worktree:dir.path().into(),docs:dir.path().join("docs")},sandbox::Role::EosReviewer,policy.clone())?;
        let launch=sandbox::launch::Launch::new(state.runs.execution.clone(),state.runs.execution.status().generation,Some(profile),policy)?;
        let opts=RunOptions{environment:None,launch:Some(launch),cwd:dir.path().into(),prompt:"Read only probe.txt in the current directory, using a read-only file tool or the command cat probe.txt. Make no changes or network requests. Return only JSON {\"ok\":true,\"value\":\"<exact file text>\"}.".into(),model:Some(selected.route.model.clone()),effort:None,resume:None,executable:crate::providers::executable_for(plantool_core::Provider::Codex),writable_roots:vec![],permission_mode:plantool_core::PermissionMode::Ask};
        router::diagnostic_capacity(&state.config.home,selected)?;
        router::count(&state.config.home,selected,1,0,database::unix_now())?;
        let (input,rx)=tokio::sync::mpsc::channel(8);let (sink,mut events)=tokio::sync::mpsc::channel(128);
        // Future is owned here: cancellation drops ManagedChild and kills the process group.
        let provider=crate::providers::run_provider(plantool_core::Provider::Codex,opts,rx,sink);tokio::pin!(provider);
        let mut final_text=String::new();
        loop {tokio::select! {
            result=&mut provider=>{result?;anyhow::bail!("Codex ended without validated completion")},
            event=events.recv()=>match event {
                Some(ProviderEvent::Message{role,content,..}) if role=="assistant"=>final_text=content,
                Some(ProviderEvent::TurnCompleted{status,error,..})=>{if let Some(error)=error {anyhow::bail!("{error}");}let value:Value=serde_json::from_str(&final_text)?;anyhow::ensure!(status=="completed"&&value==json!({"ok":true,"value":expected}),"Codex response check failed");let _=input.try_send(RunInput::Stop);break;},
                Some(ProviderEvent::Permission{kind,title,detail,..})=>anyhow::bail!("Diagnostic requires permission ({kind}): {title}. {} No automatic approval was granted",detail.unwrap_or_default()),
                Some(ProviderEvent::InputRequest{..})=>anyhow::bail!("Diagnostic requested interactive input; no answer was fabricated"),
                Some(_)=>{},None=>anyhow::bail!("Codex event stream ended"),
            }
        }}
        Ok::<_,anyhow::Error>(())
    }.await;
    match result {
        Ok(()) => vec![stage(
            "Codex CLI + read tool + JSON",
            true,
            "Signed-in Codex app-server execution passed",
            "",
        )],
        Err(error) => {
            let mut value = failure("Codex CLI + read tool + JSON", None, &format!("{error:#}"));
            if value["message"] == "Connection, catalog or response check failed" {
                value["next_step"]=json!("Check codex login status and CLI installation on the daemon host. This route never uses an OpenAI API key.");
            }
            vec![value]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::gateway::Reply;
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Fake {
        calls: Arc<Mutex<Vec<Value>>>,
        status: u16,
    }
    impl Gateway for Fake {
        fn models<'a>(
            &'a self,
            _: &'a str,
        ) -> futures::future::BoxFuture<'a, anyhow::Result<Vec<String>>> {
            Box::pin(async { Ok(vec!["fake".into()]) })
        }
        fn chat<'a>(
            &'a self,
            _: &'a str,
            body: Value,
        ) -> futures::future::BoxFuture<'a, anyhow::Result<Reply>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(body.clone());
                assert_eq!(
                    body["messages"].as_array().unwrap().last().unwrap()["role"],
                    "user",
                    "Some native backends reject a system-only diagnostic request"
                );
                let message = if !body["tools"].is_null() {
                    json!({"tool_calls":[{"function":{"name":"diagnostic_add","arguments":"{\"a\":2,\"b\":3}"}}]})
                } else if !body["response_format"].is_null() {
                    json!({"content":"{\"ok\":true}"})
                } else {
                    json!({"content":"OK"})
                };
                Ok(Reply {
                    status: self.status,
                    body: if self.status == 200 {
                        json!({"choices":[{"message":message}],"usage":{"prompt_tokens":2,"completion_tokens":3}}).to_string()
                    } else {
                        "SECRET-provider-error-key".into()
                    },
                    content_type: "application/json".into(),
                    reset: Some(123),
                })
            })
        }
    }
    fn selected() -> router::Selected {
        router::Selected {
            name: "fake".into(),
            route: router::Route {
                adapter: "opencode".into(),
                provider: "openrouter".into(),
                model: "fake:free".into(),
                budget: None,
            },
            same_family_warning: false,
        }
    }
    #[test]
    fn native_diagnostic_errors_keep_context_but_redact_credentials() {
        let value = failure("Codex", None, "codex thread/start failed: invalid params; Bearer secret-token-value api_key=hidden access_token=hidden2 https://name:password@example.com abcdefghijklmnopqrstuvwxyz012345");
        let text = value.to_string();
        assert!(text.contains("invalid params"));
        for secret in [
            "secret-token-value",
            "hidden",
            "hidden2",
            "name:password",
            "abcdefghijklmnopqrstuvwxyz012345",
        ] {
            assert!(!text.contains(secret));
        }
        assert_eq!(value["message"], "Codex could not start a thread");
    }
    #[tokio::test]
    async fn synthetic_capabilities_pass_and_count_every_request() {
        let t = tempfile::tempdir().unwrap();
        let _ = database::open(t.path()).unwrap();
        let fake = Fake {
            calls: Default::default(),
            status: 200,
        };
        let result = http_checks(t.path(), &selected(), &fake).await;
        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|s| s["ok"] == true));
        assert_eq!(fake.calls.lock().unwrap().len(), 3);
        let used: u64 = database::open(t.path())
            .unwrap()
            .query_row(
                "SELECT sum(used_requests) FROM budgets WHERE route='openrouter'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(used, 3);
    }
    #[tokio::test]
    async fn rate_limit_is_actionable_without_copying_provider_body() {
        let t = tempfile::tempdir().unwrap();
        let _ = database::open(t.path()).unwrap();
        let fake = Fake {
            calls: Default::default(),
            status: 429,
        };
        let result = http_checks(t.path(), &selected(), &fake).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["http_status"], 429);
        assert_eq!(result[0]["retry_at"], 123);
        assert!(!json!(result).to_string().contains("SECRET"));
    }
}
