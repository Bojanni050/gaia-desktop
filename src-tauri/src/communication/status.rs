//! Connection status of the link to Gaia Server.
//!
//! Purely descriptive of reachability — it says nothing about Gaia's
//! internal state, model availability or capabilities. Those are the
//! server's concern.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatus {
    /// No server configured; the desktop runs in local-only mode.
    NotConfigured,
    /// Configured and attempting to reach the server.
    Connecting,
    /// Server reachable and responding.
    Online,
    /// Configured but currently unreachable.
    Offline,
}

impl ConnectionStatus {
    pub fn is_operational(&self) -> bool {
        matches!(self, ConnectionStatus::Online)
    }
}
