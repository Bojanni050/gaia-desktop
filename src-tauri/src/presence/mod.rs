//! Presence — Gaia's local, device-bound availability state.
//!
//! Presence is a *desktop* concept: it describes this device's connection
//! and the user's local availability, and it can be *reported* to Gaia
//! Server. It is never a claim about Gaia's cognitive state — that the
//! desktop cannot and should not know.

use serde::{Deserialize, Serialize};

use crate::communication::ConnectionStatus;

/// Tauri event emitted whenever local presence changes.
pub const PRESENCE_EVENT: &str = "presence://changed";

/// Local presence of Gaia on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DesktopPresence {
    /// No server configured — Gaia is present locally, but silent.
    LocalOnly,
    /// Reaching out to Gaia Server.
    Connecting,
    /// Connected and available on this device.
    Available,
    /// Server unreachable; Gaia is present but cannot act.
    Disconnected,
    /// The user muted Gaia locally (do-not-disturb).
    Quiet,
}

impl DesktopPresence {
    /// Derive local presence from the server connection status.
    ///
    /// A pure mapping: connectivity describes presence, it never describes
    /// cognition.
    pub fn from_connection(status: &ConnectionStatus) -> Self {
        match status {
            ConnectionStatus::NotConfigured => DesktopPresence::LocalOnly,
            ConnectionStatus::Connecting => DesktopPresence::Connecting,
            ConnectionStatus::Online => DesktopPresence::Available,
            ConnectionStatus::Offline => DesktopPresence::Disconnected,
        }
    }
}

/// The presence controller keeps local presence and reports it onward.
pub struct PresenceController {
    /// User-controlled quiet mode on this device.
    quiet: std::sync::atomic::AtomicBool,
}

impl PresenceController {
    pub fn new() -> Self {
        Self {
            quiet: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn quiet(&self) -> bool {
        self.quiet.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_quiet(&self, quiet: bool) {
        self.quiet
            .store(quiet, std::sync::atomic::Ordering::Relaxed);
    }

    /// Current local presence, quiet mode applied.
    pub fn current(&self, connection: &ConnectionStatus) -> DesktopPresence {
        if self.quiet() {
            return DesktopPresence::Quiet;
        }
        DesktopPresence::from_connection(connection)
    }
}

impl Default for PresenceController {
    fn default() -> Self {
        Self::new()
    }
}

#[tauri::command]
pub fn presence_get(
    presence: tauri::State<'_, PresenceController>,
    link: tauri::State<'_, crate::communication::ServerLink>,
) -> DesktopPresence {
    presence.current(&link.status())
}

#[tauri::command]
pub fn presence_set_quiet(
    app: tauri::AppHandle,
    presence: tauri::State<'_, PresenceController>,
    link: tauri::State<'_, crate::communication::ServerLink>,
    quiet: bool,
) -> Result<DesktopPresence, String> {
    presence.set_quiet(quiet);
    let current = presence.current(&link.status());
    use tauri::Emitter;
    let _ = app.emit(PRESENCE_EVENT, &current);
    Ok(current)
}
