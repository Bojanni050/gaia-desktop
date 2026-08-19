//! Notifications — the desktop's OS-level attention channel.
//!
//! The desktop decides *how* to surface a notification locally; what
//! deserves attention is decided by Gaia (server-side) or by explicit user
//! action in the interface.

use serde::{Deserialize, Serialize};
use tauri_plugin_notification::NotificationExt;

use crate::settings::SettingsState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationOptions {
    pub title: String,
    pub body: String,
}

/// Send an OS notification. Respects the user's notification settings.
#[tauri::command]
pub fn notify(
    app: tauri::AppHandle,
    settings: tauri::State<'_, SettingsState>,
    options: NotificationOptions,
) -> Result<(), crate::error::DesktopError> {
    if !settings.get().notifications.enabled {
        return Ok(());
    }
    app.notification()
        .builder()
        .title(&options.title)
        .body(&options.body)
        .show()
        .map_err(|e| crate::error::DesktopError::Notification(e.to_string()))
}
