//! Configuration for reaching Gaia Server.
//!
//! Kept deliberately transport-agnostic: `base_url` serves REST today and can
//! host a WebSocket tomorrow (`websocket_url`), without touching callers.

use serde::{Deserialize, Serialize};
use url::Url;

use super::http::HttpGaiaClient;
use super::CommunicationError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerConfig {
    /// Root URL of Gaia Server's HTTP API, e.g. `https://gaia.local/api`.
    /// `None` means the desktop runs unconfigured (presence-only).
    pub base_url: Option<String>,

    /// Optional explicit WebSocket endpoint. Derived from `base_url` when absent.
    pub websocket_url: Option<String>,

    /// Bearer token for server authentication.
    /// Stored locally per-device; never shared between users.
    pub auth_token: Option<String>,
}

impl ServerConfig {
    pub fn is_configured(&self) -> bool {
        self.base_url
            .as_deref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false)
    }

    fn parsed_base(&self) -> Result<Url, CommunicationError> {
        let raw = self
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .ok_or(CommunicationError::NotConfigured)?;
        Url::parse(raw).map_err(|e| CommunicationError::InvalidUrl(e.to_string()))
    }

    /// Build the concrete transport client from this configuration.
    ///
    /// This is the only place where a transport choice is made. A future
    /// WebSocket-capable client replaces the constructor call, not the trait.
    pub fn build_client(&self) -> Result<HttpGaiaClient, CommunicationError> {
        let base = self.parsed_base()?;
        Ok(HttpGaiaClient::new(base, self.auth_token.clone()))
    }
}
