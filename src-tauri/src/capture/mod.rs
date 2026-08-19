//! Capture subsystem — the desktop's local context layer.
//!
//! Capture is a *capability*, not a cognition: sources collect local context
//! and the subsystem offers it to Gaia Server verbatim. Meaning, relevance
//! and intent are determined server-side, never here.
//!
//! Phase 1 ships only the architecture: the [`CaptureSource`] contract, the
//! [`CaptureManager`] registry and the offer-to-server seam. No concrete
//! capture engines are implemented yet.

mod source;
mod types;

pub use source::CaptureSource;
pub use types::{CaptureError, CaptureItem, CaptureKind, CapturePayload};

use serde_json::{json, Value};

use crate::communication::{ServerLink, ServerMethod, ServerRequest};

#[tauri::command]
pub fn capture_list_sources(manager: tauri::State<'_, CaptureManager>) -> Vec<CaptureSourceInfo> {
    manager.list()
}

/// Registry of available capture sources.
///
/// Sources register at startup (or later, dynamically); the frontend lists
/// them and requests capture by id. Adding a capability means adding a
/// source — nothing else changes.
pub struct CaptureManager {
    sources: Vec<Box<dyn CaptureSource>>,
}

impl CaptureManager {
    pub fn new() -> Self {
        // Phase 1: intentionally empty. Concrete sources (text, screenshot,
        // audio, clipboard, file) are added one by one behind the same trait.
        Self {
            sources: Vec::new(),
        }
    }

    pub fn register(&mut self, source: Box<dyn CaptureSource>) {
        self.sources.push(source);
    }

    pub fn list(&self) -> Vec<CaptureSourceInfo> {
        self.sources
            .iter()
            .map(|s| CaptureSourceInfo {
                id: s.id().to_string(),
                name: s.name().to_string(),
                kind: s.kind(),
                available: s.available(),
            })
            .collect()
    }

    pub fn capture(
        &self,
        source_id: &str,
        options: Option<Value>,
    ) -> Result<CaptureItem, CaptureError> {
        let source = self
            .sources
            .iter()
            .find(|s| s.id() == source_id)
            .ok_or_else(|| CaptureError::UnknownSource(source_id.to_string()))?;
        if !source.available() {
            return Err(CaptureError::Unavailable(source_id.to_string()));
        }
        source.capture(options)
    }

    /// Offer a captured item to Gaia Server as context.
    ///
    /// A pure hand-off: the envelope is forwarded verbatim; the server owns
    /// every decision about interpretation, retention and use.
    pub async fn offer_to_server(
        &self,
        link: &ServerLink,
        item: &CaptureItem,
    ) -> Result<(), crate::error::DesktopError> {
        let request = ServerRequest {
            method: ServerMethod::Post,
            path: "capture".to_string(),
            body: Some(json!({ "item": item })),
        };
        link.request(request).await?;
        Ok(())
    }
}

impl Default for CaptureManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSourceInfo {
    pub id: String,
    pub name: String,
    pub kind: CaptureKind,
    pub available: bool,
}

#[tauri::command]
pub async fn capture_now(
    manager: tauri::State<'_, CaptureManager>,
    source_id: String,
    options: Option<Value>,
) -> Result<CaptureItem, crate::error::DesktopError> {
    Ok(manager.capture(&source_id, options)?)
}

#[tauri::command]
pub async fn capture_submit(
    manager: tauri::State<'_, CaptureManager>,
    link: tauri::State<'_, ServerLink>,
    item: CaptureItem,
) -> Result<(), crate::error::DesktopError> {
    manager.offer_to_server(&link, &item).await
}
