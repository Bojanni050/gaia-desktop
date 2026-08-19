//! Audio subsystem — local microphone/input capability.
//!
//! Audio capture is an OS-close capability owned by the desktop. What the
//! audio *means* (speech, intent, emotion) is always determined server-side.
//!
//! Phase 1 ships the module boundary and permission tracking only; a
//! concrete input engine (device enumeration, capture stream) slots in
//! behind [`AudioInputEngine`] later without touching the rest of the app.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioPermission {
    /// Not yet requested on this device.
    Unknown,
    Granted,
    Denied,
}

/// Contract for a concrete audio input engine (phase 2+).
///
/// Implementations enumerate devices and produce raw audio; they do not
/// transcribe, interpret or buffer long-term.
#[allow(dead_code)]
pub trait AudioInputEngine: Send + Sync {
    /// Enumerate available input devices.
    fn input_devices(&self) -> Vec<AudioDeviceInfo>;

    /// Current permission state for microphone access.
    fn permission(&self) -> AudioPermission;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Audio subsystem state.
pub struct AudioSubsystem {
    /// No engine yet; permission starts unknown on every launch.
    permission: std::sync::RwLock<AudioPermission>,
}

impl AudioSubsystem {
    pub fn new() -> Self {
        Self {
            permission: std::sync::RwLock::new(AudioPermission::Unknown),
        }
    }

    pub fn permission(&self) -> AudioPermission {
        *self
            .permission
            .read()
            .expect("audio permission lock poisoned")
    }

    pub fn set_permission(&self, permission: AudioPermission) {
        *self
            .permission
            .write()
            .expect("audio permission lock poisoned") = permission;
    }

    /// Input devices known to this desktop. Empty until an engine is present.
    pub fn input_devices(&self) -> Vec<AudioDeviceInfo> {
        Vec::new()
    }
}

impl Default for AudioSubsystem {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub permission: AudioPermission,
    pub input_devices: Vec<AudioDeviceInfo>,
}

#[tauri::command]
pub fn audio_get_status(audio: tauri::State<'_, AudioSubsystem>) -> AudioStatus {
    AudioStatus {
        permission: audio.permission(),
        input_devices: audio.input_devices(),
    }
}

#[tauri::command]
pub fn audio_set_permission(
    audio: tauri::State<'_, AudioSubsystem>,
    permission: AudioPermission,
) -> AudioPermission {
    audio.set_permission(permission);
    audio.permission()
}
