//! Communication with Gaia Server.
//!
//! The desktop depends only on the [`GaiaServerClient`] contract, never on a
//! concrete transport or on any model-specific semantics. REST lives in
//! [`http::HttpGaiaClient`]; a WebSocket layer can be added later behind the
//! same trait (`subscribe_events`) without touching callers.
//!
//! Interpretation, reasoning and memory stay server-side. The desktop sends
//! and receives opaque payloads.

mod client;
mod config;
mod events;
mod http;
mod status;

pub use client::{
    GaiaServerClient, HealthReport, ServerMethod, ServerRequest, ServerResponse, TurnDelta,
};
pub use config::ServerConfig;
pub use events::{ServerEvent, ServerEventEnvelope};
pub use http::HttpGaiaClient;
pub use status::ConnectionStatus;
pub use crate::version::{CloudBuildMeta, DesktopBuildMeta, DesktopVersion};

use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

/// Tauri event emitted whenever the connection status changes.
pub const STATUS_EVENT: &str = "server://status";

/// Tauri event emitted for each incremental piece of a streaming turn.
pub const TURN_DELTA_EVENT: &str = "server://turn-delta";

/// Tauri event emitted for each realtime [`ServerEvent`] relayed from
/// `GaiaServerClient::subscribe_events` — today, conversation-history
/// changes another client (gaia-web or another gaia-desktop instance)
/// made, so this app's History list can refresh without polling.
pub const SERVER_EVENT: &str = "server://event";

/// How often the background health loop probes Gaia Server.
const HEALTH_INTERVAL: Duration = Duration::from_secs(30);
/// Timeout for a single health probe.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
/// Delay before retrying the realtime-events connection after it drops or
/// fails to establish (server not configured yet, momentarily offline,
/// connection reset, ...) — deliberately short since reconnecting is cheap
/// and there's no backoff ceiling worth the added complexity at this scale.
const EVENTS_RECONNECT_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum CommunicationError {
    #[error("no Gaia Server configured")]
    NotConfigured,
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),
    #[error("request failed: {0}")]
    Transport(String),
    #[error("server responded with status {status}")]
    Server { status: u16 },
    #[error("realtime events require a WebSocket connection, which is not implemented yet")]
    WebSocketUnavailable,
}

/// The desktop's single link to Gaia Server.
///
/// Owns the active [`GaiaServerClient`] and the current
/// [`ConnectionStatus`], and exposes both to the frontend via Tauri events.
/// All cognition stays on the other side of this seam.
pub struct ServerLink {
    config: RwLock<ServerConfig>,
    status: RwLock<ConnectionStatus>,
    client: RwLock<Option<Arc<dyn GaiaServerClient>>>,
}

impl ServerLink {
    pub fn new(config: ServerConfig) -> Self {
        let status = RwLock::new(if config.is_configured() {
            ConnectionStatus::Connecting
        } else {
            ConnectionStatus::NotConfigured
        });
        let client = if config.is_configured() {
            config
                .build_client()
                .ok()
                .map(|c| Arc::new(c) as Arc<dyn GaiaServerClient>)
        } else {
            None
        };
        Self {
            config: RwLock::new(config),
            status,
            client: RwLock::new(client),
        }
    }

    pub fn config(&self) -> ServerConfig {
        self.config
            .read()
            .expect("server config lock poisoned")
            .clone()
    }

    /// Apply a new configuration and rebuild the transport behind it.
    pub fn reconfigure(&self, config: ServerConfig) -> Result<(), CommunicationError> {
        let next_status = if config.is_configured() {
            ConnectionStatus::Connecting
        } else {
            ConnectionStatus::NotConfigured
        };
        let client = if config.is_configured() {
            Some(Arc::new(config.build_client()?) as Arc<dyn GaiaServerClient>)
        } else {
            None
        };
        *self.client.write().expect("server client lock poisoned") = client;
        *self.config.write().expect("server config lock poisoned") = config;
        self.status
            .write()
            .expect("server status lock poisoned")
            .clone_from(&next_status);
        Ok(())
    }

    pub fn status(&self) -> ConnectionStatus {
        self.status
            .read()
            .expect("server status lock poisoned")
            .clone()
    }

    fn set_status(&self, status: ConnectionStatus) {
        let mut guard = self.status.write().expect("server status lock poisoned");
        if *guard != status {
            *guard = status;
        }
    }

    fn client(&self) -> Option<Arc<dyn GaiaServerClient>> {
        self.client
            .read()
            .expect("server client lock poisoned")
            .clone()
    }

    /// Probe the server once, update the status and notify the frontend.
    pub async fn check_once(&self, app: &AppHandle) -> ConnectionStatus {
        let previous = self.status();
        let next = match self.client() {
            None => ConnectionStatus::NotConfigured,
            Some(client) => {
                let health = tokio::time::timeout(HEALTH_TIMEOUT, client.health()).await;
                match health {
                    Ok(Ok(report)) if report.reachable => ConnectionStatus::Online,
                    Ok(Ok(_)) | Ok(Err(_)) | Err(_) => ConnectionStatus::Offline,
                }
            }
        };
        self.set_status(next.clone());
        if previous != next {
            let _ = app.emit(STATUS_EVENT, &next);
        }
        next
    }

    /// Perform a generic request against Gaia Server.
    pub async fn request(
        &self,
        request: ServerRequest,
    ) -> Result<ServerResponse, CommunicationError> {
        let client = self.client().ok_or(CommunicationError::NotConfigured)?;
        client.request(request).await
    }

    /// Perform a streaming turn, emitting [`TURN_DELTA_EVENT`] for each
    /// incremental piece as it arrives, and returning the full assistant
    /// text (content deltas only — reasoning deltas are relayed for display
    /// but not part of the reply) once the stream ends.
    pub async fn stream_turn(
        &self,
        app: &AppHandle,
        body: Value,
        request_id: String,
    ) -> Result<String, CommunicationError> {
        let client = self.client().ok_or(CommunicationError::NotConfigured)?;
        let mut deltas = client.stream_turn(body).await?;
        let mut full_reply = String::new();
        while let Some(delta) = deltas.recv().await {
            let delta = delta?;
            if let Some(content) = &delta.content {
                full_reply.push_str(content);
            }
            let _ = app.emit(
                TURN_DELTA_EVENT,
                &TurnDeltaPayload {
                    request_id: request_id.clone(),
                    content: delta.content,
                    reasoning_content: delta.reasoning_content,
                },
            );
        }
        Ok(full_reply)
    }

    /// Spawn the background health loop that keeps the status fresh.
    pub fn spawn_health_loop(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(HEALTH_INTERVAL);
            loop {
                interval.tick().await;
                let link = app.state::<ServerLink>();
                link.check_once(&app).await;
            }
        });
    }

    /// Spawn the background bridge that relays realtime [`ServerEvent`]s as
    /// [`SERVER_EVENT`] Tauri events. Reconnects on its own — whenever the
    /// subscription ends (dropped connection, not configured yet, transport
    /// error) it waits [`EVENTS_RECONNECT_DELAY`] and tries again, for the
    /// lifetime of the app.
    pub fn spawn_event_bridge(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            loop {
                let client = app.state::<ServerLink>().client();
                let Some(client) = client else {
                    tokio::time::sleep(EVENTS_RECONNECT_DELAY).await;
                    continue;
                };
                match client.subscribe_events().await {
                    Ok(mut events) => {
                        while let Some(event) = events.recv().await {
                            let _ = app.emit(SERVER_EVENT, &event);
                        }
                        // Channel closed — connection ended; fall through to
                        // the reconnect delay below and try again.
                    }
                    Err(_) => {
                        // Not configured, offline, or the endpoint rejected
                        // us — same reconnect path either way.
                    }
                }
                tokio::time::sleep(EVENTS_RECONNECT_DELAY).await;
            }
        });
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatusPayload {
    status: ConnectionStatus,
    server_url: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TurnDeltaPayload {
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[tauri::command]
pub async fn server_get_config(link: tauri::State<'_, ServerLink>) -> Result<ServerConfig, String> {
    Ok(link.config())
}

#[tauri::command]
pub async fn server_set_config(
    app: AppHandle,
    link: tauri::State<'_, ServerLink>,
    config: ServerConfig,
) -> Result<(), String> {
    link.reconfigure(config).map_err(|e| e.to_string())?;
    let _ = app.emit(STATUS_EVENT, &link.status());
    Ok(())
}

#[tauri::command]
pub async fn server_get_status(
    link: tauri::State<'_, ServerLink>,
) -> Result<ServerStatusPayload, String> {
    Ok(ServerStatusPayload {
        status: link.status(),
        server_url: link.config().base_url,
    })
}

#[tauri::command]
pub async fn server_test_connection(
    app: AppHandle,
    link: tauri::State<'_, ServerLink>,
) -> Result<ConnectionStatus, String> {
    Ok(link.check_once(&app).await)
}

#[tauri::command]
pub async fn server_request(
    link: tauri::State<'_, ServerLink>,
    request: ServerRequest,
) -> Result<ServerResponse, crate::error::DesktopError> {
    Ok(link.request(request).await?)
}

/// Streams one turn against `conversation/turn`. `body` is the same turn
/// payload `server_request` would carry (see contract.js's
/// `buildStreamTurnBody`); `request_id` lets the frontend match incoming
/// [`TURN_DELTA_EVENT`]s to this call, since it returns only once the
/// stream has ended. Resolves with the full assistant reply.
#[tauri::command]
pub async fn server_stream_turn(
    app: AppHandle,
    link: tauri::State<'_, ServerLink>,
    body: Value,
    request_id: String,
) -> Result<String, crate::error::DesktopError> {
    Ok(link.stream_turn(&app, body, request_id).await?)
}

/// Fetch Cloud build version from the server.
/// Returns the Cloud's build metadata, or an error if unavailable.
/// This is cached for the session and fetched once during initialization.
#[tauri::command]
pub async fn server_get_cloud_version(
    link: tauri::State<'_, ServerLink>,
) -> Result<CloudBuildMeta, String> {
    let client = link.client()
        .ok_or_else(|| "no Gaia Server configured".to_string())?;
    client.get_cloud_version().await
        .map_err(|e| e.to_string())
}

/// Get Desktop build metadata (from the local application).
#[tauri::command]
pub async fn version_get_desktop(
    desktop_version: tauri::State<'_, DesktopVersion>,
) -> Result<DesktopBuildMeta, String> {
    Ok(desktop_version.get().clone())
}
