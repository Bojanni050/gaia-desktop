//! The CaptureSource contract.
//!
//! Every future capture capability — text, screenshots, audio, clipboard,
//! files, other local context — plugs in here. Sources are local capability
//! adapters: they *collect*, they never interpret. What the context means is
//! decided by Gaia Server.

use serde_json::Value;

use super::types::{CaptureError, CaptureItem, CaptureKind};

/// A provider of one kind of local context.
pub trait CaptureSource: Send + Sync {
    /// Stable identifier, e.g. `clipboard`, `screen`, `microphone`.
    fn id(&self) -> &str;

    /// Human-readable name for UI display.
    fn name(&self) -> &str;

    fn kind(&self) -> CaptureKind;

    /// Whether this source can currently run on this device
    /// (permissions granted, hardware present).
    fn available(&self) -> bool;

    /// Collect one item of context.
    ///
    /// Phase 1 ships no concrete sources; this contract exists so they can
    /// be added one by one without touching the manager or the frontend.
    fn capture(&self, options: Option<Value>) -> Result<CaptureItem, CaptureError>;
}
