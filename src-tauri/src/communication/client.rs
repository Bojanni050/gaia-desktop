//! The contract between Gaia Desktop and Gaia Server.
//!
//! The desktop depends only on this trait. Payloads crossing it are opaque:
//! the desktop makes no assumptions about models, prompts, reasoning steps
//! or memory layout — those belong to Gaia Server.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use super::events::ServerEvent;
use super::CommunicationError;

/// A generic server request envelope.
///
/// REST-shaped today; the shape leaves room for other request styles later
/// without leaking transport details into callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerRequest {
    pub method: ServerMethod,
    /// Path relative to the configured server root, e.g. `conversation/turn`.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl ServerMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerMethod::Get => "GET",
            ServerMethod::Post => "POST",
            ServerMethod::Put => "PUT",
            ServerMethod::Delete => "DELETE",
        }
    }
}

/// A generic server response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerResponse {
    pub status: u16,
    pub body: Value,
}

/// Result of a reachability probe. Content-free by design.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Receiver for realtime events pushed by Gaia Server.
///
/// Delivered as a plain channel so the transport (WebSocket today or
/// tomorrow, SSE the day after) stays swappable.
pub type ServerEventStream = mpsc::UnboundedReceiver<ServerEvent>;

/// One incremental piece of a streamed turn — assistant content or
/// reasoning content, exactly as Gaia Server's SSE frames distinguish them
/// (gaia-api's turn.js `writeSseDelta`). Opaque otherwise: relayed text,
/// never interpreted here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// Receiver for a streamed turn's incremental deltas. Closes when the
/// server sends `[DONE]` or the connection ends; a delivered `Err` means
/// the stream failed mid-flight (the channel still closes right after).
pub type TurnDeltaStream = mpsc::UnboundedReceiver<Result<TurnDelta, CommunicationError>>;

/// The single seam to Gaia Server.
///
/// Implementations must be transport-only: they move bytes, they never
/// interpret them. Adding a WebSocket layer means adding an implementation
/// (or extending the existing one), not changing this contract.
#[async_trait]
pub trait GaiaServerClient: Send + Sync {
    /// Probe whether the server is reachable.
    async fn health(&self) -> Result<HealthReport, CommunicationError>;

    /// Perform a generic request and return the raw server response.
    async fn request(&self, request: ServerRequest) -> Result<ServerResponse, CommunicationError>;

    /// Subscribe to realtime server events. `HttpGaiaClient` backs this with
    /// gaia-api's `conversations/events` SSE endpoint; a future WebSocket
    /// transport can slot in behind the same trait method without a caller
    /// changing.
    async fn subscribe_events(&self) -> Result<ServerEventStream, CommunicationError>;

    /// Perform a streaming turn against `conversation/turn` (Gaia Server's
    /// SSE path — gaia-api's turn.js `performStreamingTurn`). `body` is the
    /// same turn payload `request` would send, minus the `stream` flag,
    /// which implementations set themselves. Returns once the connection is
    /// established; deltas arrive on the returned channel as the server
    /// sends them.
    async fn stream_turn(&self, body: Value) -> Result<TurnDeltaStream, CommunicationError>;
}
