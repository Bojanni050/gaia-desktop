//! HTTP/REST transport for the [`GaiaServerClient`] contract.
//!
//! Transport only: this client moves opaque JSON, applies auth headers and
//! reports status codes. It never inspects or shapes payloads semantically.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, Method, RequestBuilder};
use serde_json::Value;
use url::Url;

use tokio::sync::mpsc;

use super::client::{
    GaiaServerClient, HealthReport, ServerEventStream, ServerRequest, ServerResponse, TurnDelta,
    TurnDeltaStream,
};
use super::events::{ServerEvent, ServerEventEnvelope};
use super::CommunicationError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// A streaming turn covers the whole reply, not just a round-trip — recall
/// and reasoning on the server side can legitimately take minutes. Applied
/// per-request, overriding REQUEST_TIMEOUT for this call only.
const STREAM_TIMEOUT: Duration = Duration::from_secs(600);
/// The realtime events connection (`conversations/events`) is meant to sit
/// open for the life of the app, not one request/response — long enough
/// that it should never legitimately hit this ceiling; gaia-api's own
/// heartbeat (every 20s, historyRoutes.js) is what actually detects a dead
/// connection well before this would.
const EVENTS_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

pub struct HttpGaiaClient {
    base: Url,
    auth_token: Option<String>,
    http: Client,
}

impl HttpGaiaClient {
    pub fn new(base: Url, auth_token: Option<String>) -> Self {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest client builder is valid");
        Self {
            base,
            auth_token,
            http,
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, CommunicationError> {
        self.base
            .join(path.trim_start_matches('/'))
            .map_err(|e| CommunicationError::InvalidUrl(e.to_string()))
    }

    fn authorize(&self, builder: RequestBuilder) -> RequestBuilder {
        match &self.auth_token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }
}

#[async_trait]
impl GaiaServerClient for HttpGaiaClient {
    async fn health(&self) -> Result<HealthReport, CommunicationError> {
        let started = Instant::now();
        let response = self
            .authorize(self.http.get(self.base.clone()))
            .timeout(Duration::from_secs(5))
            .send()
            .await;
        match response {
            Ok(response) => Ok(HealthReport {
                reachable: true,
                latency_ms: Some(started.elapsed().as_millis() as u64),
                detail: Some(format!("HTTP {}", response.status().as_u16())),
            }),
            Err(e) => Ok(HealthReport {
                reachable: false,
                latency_ms: None,
                detail: Some(e.to_string()),
            }),
        }
    }

    async fn request(&self, request: ServerRequest) -> Result<ServerResponse, CommunicationError> {
        let url = self.endpoint(&request.path)?;
        let method = Method::from_bytes(request.method.as_str().as_bytes())
            .map_err(|e| CommunicationError::Transport(e.to_string()))?;

        let mut builder = self.authorize(self.http.request(method, url));
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }

        let response = builder.send().await.map_err(map_transport)?;
        let status = response.status().as_u16();
        let body: Value = response.json().await.unwrap_or(Value::Null);

        if status >= 400 {
            return Err(CommunicationError::Server { status });
        }
        Ok(ServerResponse { status, body })
    }

    async fn subscribe_events(&self) -> Result<ServerEventStream, CommunicationError> {
        // Backed by gaia-api's `conversations/events` SSE endpoint
        // (historyRoutes.js) rather than a WebSocket — same seam, simpler
        // transport, no server-side change needed to add a real WebSocket
        // later behind this same trait method.
        let url = self.endpoint("conversations/events")?;
        let builder = self.authorize(self.http.get(url)).timeout(EVENTS_TIMEOUT);

        let mut response = builder.send().await.map_err(map_transport)?;
        let status = response.status().as_u16();
        if status >= 400 {
            return Err(CommunicationError::Server { status });
        }

        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                match response.chunk().await {
                    Ok(Some(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some(pos) = buffer.find("\n\n") {
                            let frame: String = buffer.drain(..pos + 2).collect();
                            if let Some(event) = parse_conversation_event(&frame) {
                                if tx.send(event).is_err() {
                                    return; // receiver dropped
                                }
                            }
                        }
                    }
                    Ok(None) => return,  // connection closed — caller reconnects
                    Err(_) => return,    // transport error — caller reconnects
                }
            }
        });

        Ok(rx)
    }

    async fn stream_turn(&self, mut body: Value) -> Result<TurnDeltaStream, CommunicationError> {
        if let Value::Object(map) = &mut body {
            map.insert("stream".to_string(), Value::Bool(true));
        }
        let url = self.endpoint("conversation/turn")?;
        // Overrides the client's default REQUEST_TIMEOUT, which is sized for
        // a quick request/response and would otherwise cut off a long-running
        // turn (recall/reasoning can legitimately take minutes) mid-stream.
        let builder = self
            .authorize(self.http.post(url))
            .timeout(STREAM_TIMEOUT)
            .json(&body);

        let mut response = builder.send().await.map_err(map_transport)?;
        let status = response.status().as_u16();
        if status >= 400 {
            return Err(CommunicationError::Server { status });
        }

        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                match response.chunk().await {
                    Ok(Some(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        // SSE frames are separated by a blank line; a chunk
                        // boundary may land mid-frame, so only consume
                        // complete frames and leave the rest buffered.
                        while let Some(pos) = buffer.find("\n\n") {
                            let frame: String = buffer.drain(..pos + 2).collect();
                            match parse_sse_frame(&frame) {
                                Some(Ok(Some(delta))) => {
                                    if tx.send(Ok(delta)).is_err() {
                                        return; // receiver dropped
                                    }
                                }
                                Some(Ok(None)) => return, // [DONE]
                                Some(Err(e)) => {
                                    let _ = tx.send(Err(e));
                                    return;
                                }
                                None => {} // frame carried nothing worth relaying
                            }
                        }
                    }
                    Ok(None) => return, // connection closed cleanly
                    Err(e) => {
                        let _ = tx.send(Err(map_transport(e)));
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }
}

/// Parses one SSE frame (everything up to, but not including, its
/// trailing blank line) into a delta. `None` means the frame carried
/// nothing this client relays (a comment, an empty keep-alive); `Some(Ok(None))`
/// means the server's `[DONE]` sentinel — the stream is over, not an error.
fn parse_sse_frame(frame: &str) -> Option<Result<Option<TurnDelta>, CommunicationError>> {
    let data = frame.lines().find_map(|line| line.strip_prefix("data: "))?;
    if data == "[DONE]" {
        return Some(Ok(None));
    }
    let value: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => return Some(Err(CommunicationError::Transport(e.to_string()))),
    };
    let delta = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("delta"))
        .cloned()
        .unwrap_or(Value::Null);
    let content = delta
        .get("content")
        .and_then(|v| v.as_str())
        .map(String::from);
    let reasoning_content = delta
        .get("reasoning_content")
        .and_then(|v| v.as_str())
        .map(String::from);
    if content.is_none() && reasoning_content.is_none() {
        return None;
    }
    Some(Ok(Some(TurnDelta {
        content,
        reasoning_content,
    })))
}

fn map_transport(error: reqwest::Error) -> CommunicationError {
    CommunicationError::Transport(error.to_string())
}

/// Parses one SSE frame from `conversations/events` into a [`ServerEvent`].
/// gaia-api sends `event: changed` on every save/delete and `:heartbeat`/
/// `:ok` comment-only frames in between (historyRoutes.js) — only the
/// former is worth relaying, so anything else (including a malformed
/// frame) is silently dropped rather than surfaced as an error: a missed
/// heartbeat is not a failure the frontend needs to know about.
fn parse_conversation_event(frame: &str) -> Option<ServerEvent> {
    let is_changed = frame
        .lines()
        .any(|line| line.trim() == "event: changed");
    if !is_changed {
        return None;
    }
    Some(ServerEvent::from(ServerEventEnvelope {
        topic: "conversation.history.changed".to_string(),
        payload: Value::Null,
    }))
}
