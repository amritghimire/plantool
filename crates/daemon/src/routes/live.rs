use super::sessions::resolve;
use super::ApiError;
use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LiveQuery {
    #[serde(default)]
    pub since: Option<u64>,
}

async fn live(
    State(state): State<AppState>,
    Path((repo, slug)): Path<(String, String)>,
    Query(q): Query<LiveQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let s = resolve(&state, &format!("{repo}/{slug}"), None)?;
    Ok(ws.on_upgrade(move |socket| handle(socket, s, q.since)))
}

async fn handle(
    mut socket: WebSocket,
    s: std::sync::Arc<crate::registry::LiveSession>,
    since: Option<u64>,
) {
    let mut rx = s.tx.subscribe();
    let hello = serde_json::json!({ "type": "hello", "seq": s.seq(), "key": s.key });
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    if let Some(since) = since {
        let newer = s.comments(&crate::registry::CommentFilter {
            since: Some(since),
            ..Default::default()
        });
        if !newer.is_empty() {
            let msg =
                serde_json::json!({ "type": "comments-since", "seq": s.seq(), "comments": newer });
            if socket
                .send(Message::Text(msg.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
    }
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(msg) => {
                        let text = match serde_json::to_string(&msg) { Ok(t) => t, Err(_) => continue };
                        if socket.send(Message::Text(text.into())).await.is_err() { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let resync = serde_json::json!({ "type": "resync", "seq": s.seq() });
                        if socket.send(Message::Text(resync.to_string().into())).await.is_err() { break; }
                    }
                    Err(_) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => { let _ = socket.send(Message::Pong(p)).await; }
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/sessions/{repo}/{slug}/live", get(live))
}
