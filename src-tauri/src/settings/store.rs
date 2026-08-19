//! File-backed settings persistence.
//!
//! Stored as plain JSON in the app's config directory so users can inspect
//! and back it up. Secrets (auth tokens) will move to the OS credential
//! store in a later phase.

use std::fs;
use std::path::PathBuf;

use super::Settings;

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not read settings: {0}")]
    Read(String),
    #[error("could not write settings: {0}")]
    Write(String),
}

pub struct FileSettingsStore {
    path: Option<PathBuf>,
}

impl FileSettingsStore {
    /// Creates the store against the app config directory. A missing
    /// directory (rare, headless tests) degrades to in-memory only.
    pub fn open(dir: Option<PathBuf>) -> Self {
        Self {
            path: dir.map(|d| d.join(SETTINGS_FILE)),
        }
    }

    pub fn load(&self) -> Result<Settings, SettingsError> {
        let Some(path) = &self.path else {
            return Ok(Settings::default());
        };
        match fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str(&raw).map_err(|e| SettingsError::Read(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(SettingsError::Read(e.to_string())),
        }
    }

    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| SettingsError::Write(e.to_string()))?;
        }
        let raw = serde_json::to_string_pretty(settings)
            .map_err(|e| SettingsError::Write(e.to_string()))?;
        fs::write(path, raw).map_err(|e| SettingsError::Write(e.to_string()))
    }
}
