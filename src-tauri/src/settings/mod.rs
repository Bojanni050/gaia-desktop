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
    pub voice: VoiceSettings,
    pub capture: CaptureSettings,
    pub mcp: crate::mcp::McpSettings,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// Legacy: preferred input device. Migrated into `voice.input_device` on load
    /// (see [`migrate_legacy_input`]) and never written back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_input: Option<String>,
    /// Playback volume for Gaia's voice, 0.0 (silent) to 1.0 (full). Applied
    /// locally at playback — WebView2 gives Windows no per-app slider of its
    /// own, so this is the only volume control Gaia's voice actually has.
    pub volume: f32,
    /// Whether Gaia's voice is muted on this device (playback silenced
    /// without discarding the transcript).
    pub muted: bool,
    /// Whether Gaia speaks her replies at all. Off means no speech is ever requested from Gaia Cloud
    /// (so nothing is synthesised and no text-to-speech provider is called), unlike `muted`, which
    /// still fetches the audio and only silences it.
    pub speak_replies: bool,
    /// Output device id (`DeviceId` string) Gaia's voice plays through; `None` = system default.
    /// An id that no longer exists falls back to the default device.
    pub output_device: Option<String>,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            preferred_input: None,
            output_device: None,
            volume: 1.0,
            muted: false,
            speak_replies: true,
        }
    }
}

/// Voice (background conversation) preferences for this device.
///
/// The master microphone switch is deliberately **not** persisted: every launch
/// starts with the microphone off, and only an explicit user action turns it on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceSettings {
    /// Master microphone switch. Runtime only (`serde(skip)`): always false after launch.
    #[serde(skip)]
    pub mic_enabled: bool,
    /// Continuous conversation mode (Phase 3).
    pub continuous_enabled: bool,
    /// Local "Hey Gaia" wake word (Phase 4).
    pub wake_word_enabled: bool,
    /// Input device id (`DeviceId` string, e.g. `wasapi:{...}`); `None` = system default.
    /// An id that no longer exists falls back to the default device.
    pub input_device: Option<String>,
    /// True once a wake word has been enrolled on this device (Phase 4).
    pub wake_enrolled: bool,
    /// Wake word detector sensitivity (maps to the detector threshold).
    pub wake_sensitivity: f32,
    /// Voice interruption while Gaia speaks (Phase 3).
    pub barge_in_enabled: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            mic_enabled: false,
            continuous_enabled: false,
            wake_word_enabled: false,
            input_device: None,
            wake_enrolled: false,
            wake_sensitivity: 0.55,
            barge_in_enabled: true,
        }
    }
}

/// One-time migration of the legacy `audio.preferredInput` into `voice.inputDevice`.
/// Returns true when anything changed (the caller then persists the settings).
pub fn migrate_legacy_input(settings: &mut Settings) -> bool {
    let Some(legacy) = settings.audio.preferred_input.take() else {
        return false;
    };
    if settings.voice.input_device.is_none() && !legacy.trim().is_empty() {
        settings.voice.input_device = Some(legacy);
    }
    true
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

        // Legacy audio.preferredInput → voice.inputDevice.
        if migrate_legacy_input(&mut current) {
            let _ = store.save(&current);
        }

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

    /// Applies `f` to the current settings and persists them (used by commands that change one
    /// preference, e.g. the selected input device). The auth token never goes to the JSON file when
    /// it lives in the OS credential store.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<(), SettingsError> {
        let mut guard = self.current.write().expect("settings lock poisoned");
        f(&mut guard);
        let mut to_persist = guard.clone();
        if secrets::load_token().is_some() {
            to_persist.server.auth_token = None;
        }
        self.store.save(&to_persist)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_defaults_to_full_and_unmuted() {
        let audio = AudioSettings::default();
        assert_eq!(audio.volume, 1.0);
        assert!(!audio.muted);
        assert!(audio.speak_replies, "she speaks unless switched off");
    }

    #[test]
    fn settings_saved_before_the_speak_switch_existed_still_let_her_speak() {
        let old: AudioSettings = serde_json::from_str(r#"{"volume":0.5,"muted":true}"#).unwrap();
        assert!(old.speak_replies);
        assert!(old.muted);
        let off: AudioSettings = serde_json::from_str(r#"{"speakReplies":false}"#).unwrap();
        assert!(!off.speak_replies);
    }

    #[test]
    fn voice_defaults_are_off_and_unenrolled() {
        let voice = VoiceSettings::default();
        assert!(!voice.mic_enabled);
        assert!(!voice.continuous_enabled);
        assert!(!voice.wake_word_enabled);
        assert!(!voice.wake_enrolled);
        assert!(voice.input_device.is_none());
        assert!(voice.barge_in_enabled);
        assert!((voice.wake_sensitivity - 0.55).abs() < f32::EPSILON);
    }

    #[test]
    fn settings_without_a_voice_block_still_parse() {
        let settings: Settings =
            serde_json::from_str(r#"{ "audio": { "volume": 0.5 } }"#).expect("legacy settings parse");
        assert!(!settings.voice.mic_enabled);
        assert!(settings.voice.input_device.is_none());
    }

    #[test]
    fn the_master_mic_switch_is_never_persisted_or_restored() {
        let mut settings = Settings::default();
        settings.voice.mic_enabled = true;
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("micEnabled"), "mic state must not be written: {json}");
        let restored: Settings =
            serde_json::from_str(r#"{ "voice": { "micEnabled": true, "continuousEnabled": true } }"#).unwrap();
        assert!(!restored.voice.mic_enabled, "a stored micEnabled must be ignored");
        assert!(restored.voice.continuous_enabled);
    }

    #[test]
    fn legacy_preferred_input_migrates_once_and_is_not_written_back() {
        let mut settings: Settings =
            serde_json::from_str(r#"{ "audio": { "preferredInput": "mic-1" } }"#).unwrap();
        assert!(migrate_legacy_input(&mut settings));
        assert_eq!(settings.voice.input_device.as_deref(), Some("mic-1"));
        assert!(settings.audio.preferred_input.is_none());
        assert!(!serde_json::to_string(&settings).unwrap().contains("preferredInput"));
        assert!(!migrate_legacy_input(&mut settings), "second run changes nothing");
    }

    #[test]
    fn migration_keeps_an_explicit_voice_device() {
        let mut settings: Settings = serde_json::from_str(
            r#"{ "audio": { "preferredInput": "old" }, "voice": { "inputDevice": "new" } }"#,
        )
        .unwrap();
        migrate_legacy_input(&mut settings);
        assert_eq!(settings.voice.input_device.as_deref(), Some("new"));
    }

    #[test]
    fn legacy_settings_without_volume_still_parse_at_full_volume() {
        // A settings.json written before the volume field existed must not
        // come back silent: missing fields fill from Default (volume 1.0).
        let settings: Settings =
            serde_json::from_str(r#"{ "audio": { "preferredInput": "mic-1" } }"#)
                .expect("legacy settings should still parse");
        assert_eq!(settings.audio.preferred_input.as_deref(), Some("mic-1"));
        assert_eq!(settings.audio.volume, 1.0);
        assert!(!settings.audio.muted);
    }
}
