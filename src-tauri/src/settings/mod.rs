//! Settings — the desktop's persisted local configuration.
//!
//! Settings describe *this device's* behaviour only (which server to talk
//! to, notification and audio preferences, enabled capture sources). They
//! never encode anything cognitive; Gaia's long-term state lives server-side.

mod secrets;
mod store;

use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::communication::ServerConfig;

pub use store::SettingsError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub server: ServerConfig,
    pub notifications: NotificationSettings,
    pub audio: AudioSettings,
    pub capture: CaptureSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    pub enabled: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// Preferred input device id, if the user chose one.
    pub preferred_input: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CaptureSettings {
    /// Ids of capture sources the user has enabled.
    pub enabled_sources: Vec<String>,
}

/// Managed settings state: persisted copy plus in-memory current value.
pub struct SettingsState {
    current: RwLock<Settings>,
    store: store::FileSettingsStore,
}

impl SettingsState {
    /// Opens (and loads) the settings state from the given config directory.
    /// Falls back to defaults when no settings file exists yet.
    pub fn open(config_dir: Option<std::path::PathBuf>) -> Self {
        let store = store::FileSettingsStore::open(config_dir);
        let mut current = store.load().unwrap_or_default();

        // One-time migration: a token written to settings.json by an
        // earlier version moves into the OS credential store and out of
        // the plain-JSON file. If the keyring is unavailable the token
        // stays where it is (legacy behaviour, no worse than before).
        if let Some(token) = current.server.auth_token.clone() {
            if secrets::store_token(Some(&token)).is_ok() {
                current.server.auth_token = None;
                let _ = store.save(&current);
            }
        }

        Self {
            current: RwLock::new(current),
            store,
        }
    }

    pub fn get(&self) -> Settings {
        let mut settings = self.current.read().expect("settings lock poisoned").clone();
        // The token lives in the credential store; hydrate it for callers
        // (the settings UI and the server link) when one is stored there.
        if let Some(token) = secrets::load_token() {
            settings.server.auth_token = Some(token);
        }
        settings
    }

    fn replace(&self, settings: Settings) {
        *self.current.write().expect("settings lock poisoned") = settings;
    }
}

#[tauri::command]
pub fn settings_get(settings: tauri::State<'_, SettingsState>) -> Settings {
    settings.get()
}

#[tauri::command]
pub async fn settings_save(
    app: tauri::AppHandle,
    settings: tauri::State<'_, SettingsState>,
    new_settings: Settings,
) -> Result<Settings, crate::error::DesktopError> {
    // The token moves to the OS credential store; settings.json stays
    // secret-free. If the keyring is unavailable, the token is persisted
    // in the file instead (legacy behaviour) so saving never breaks.
    let mut to_persist = new_settings.clone();
    if secrets::store_token(new_settings.server.auth_token.as_deref()).is_ok() {
        to_persist.server.auth_token = None;
    }

    settings
        .store
        .save(&to_persist)
        .map_err(crate::error::DesktopError::Settings)?;
    settings.replace(new_settings.clone());

    // Keep the server link in step with the new configuration.
    if let Some(link) = app.try_state::<crate::communication::ServerLink>() {
        link.reconfigure(new_settings.server.clone())
            .map_err(crate::error::DesktopError::Communication)?;
    }
    Ok(new_settings)
}
