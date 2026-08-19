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

    /// Subscribe to realtime server events.
    ///
    /// Phase 1 implementations may return [`CommunicationError::WebSocketUnavailable`];
    /// the seam exists so a WebSocket client can slot in without refactor.
    async fn subscribe_events(&self) -> Result<ServerEventStream, CommunicationError>;
}
