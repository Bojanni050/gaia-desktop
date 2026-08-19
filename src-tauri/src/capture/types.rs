//! Capture data model.
//!
//! Capture items are *offered* to Gaia Server as context. The desktop
//! records what was captured, where and when — never what it means.
//! Interpretation and intent stay with Gaia.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The kinds of local context Capture can eventually provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureKind {
    Text,
    Screenshot,
    Audio,
    Clipboard,
    File,
}

/// Payload of a captured item. Kept as plain data; no interpretation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum CapturePayload {
    /// Plain text content.
    Text { content: String },
    /// Binary content, base64-encoded, with its media type.
    Binary { media_type: String, data: String },
    /// Structured local context (device info, window titles, ...).
    Structured { content: Value },
}

/// A single captured unit of local context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureItem {
    /// Unique id, generated locally.
    pub id: String,
    /// Id of the source that produced the item.
    pub source_id: String,
    pub kind: CaptureKind,
    /// Unix epoch milliseconds.
    pub captured_at: u64,
    pub payload: CapturePayload,
    /// Optional source-specific metadata (window title, file name, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture source '{0}' is not registered")]
    UnknownSource(String),
    #[error("capture source '{0}' is not available on this device")]
    Unavailable(String),
    #[error("capture failed: {0}")]
    Failed(String),
    #[error("capture is not implemented in this phase")]
    NotImplemented,
}
