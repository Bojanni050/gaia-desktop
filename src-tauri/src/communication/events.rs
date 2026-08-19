//! Realtime events from Gaia Server.
//!
//! The envelope is intentionally opaque: the desktop relays and displays,
//! it does not interpret. Event semantics (conversation deltas, presence
//! changes, capture acknowledgements, ...) are defined by the server and
//! consumed by the frontend.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A structured but content-agnostic server event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEventEnvelope {
    /// Server-defined topic, e.g. `conversation.delta`, `presence.changed`.
    pub topic: String,
    /// Opaque, server-defined payload.
    pub payload: Value,
}

/// An event as delivered to subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServerEvent {
    Envelope(ServerEventEnvelope),
    /// Fallback for anything the desktop does not yet shape.
    Raw(Value),
}

impl From<ServerEventEnvelope> for ServerEvent {
    fn from(envelope: ServerEventEnvelope) -> Self {
        ServerEvent::Envelope(envelope)
    }
}
