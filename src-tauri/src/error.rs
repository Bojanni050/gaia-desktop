//! Shared error surface for the desktop's Tauri command boundary.
//!
//! Desktop errors describe *local* failures (transport, permissions, IO).
//! They never carry cognitive or model-specific semantics — interpretation
//! belongs to Gaia Server.

use serde::ser::SerializeStruct;

#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("communication error: {0}")]
    Communication(#[from] crate::communication::CommunicationError),

    #[error("capture error: {0}")]
    Capture(#[from] crate::capture::CaptureError),

    #[error("settings error: {0}")]
    Settings(#[from] crate::settings::SettingsError),

    #[error("notification error: {0}")]
    Notification(String),

    #[error("{0}")]
    Message(String),
}

impl DesktopError {
    pub fn kind(&self) -> &'static str {
        match self {
            DesktopError::Communication(_) => "communication",
            DesktopError::Capture(_) => "capture",
            DesktopError::Settings(_) => "settings",
            DesktopError::Notification(_) => "notification",
            DesktopError::Message(_) => "message",
        }
    }
}

impl serde::Serialize for DesktopError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("DesktopError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<String> for DesktopError {
    fn from(message: String) -> Self {
        DesktopError::Message(message)
    }
}

impl From<&str> for DesktopError {
    fn from(message: &str) -> Self {
        DesktopError::Message(message.to_string())
    }
}

pub type DesktopResult<T> = Result<T, DesktopError>;
