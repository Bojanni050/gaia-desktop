//! Build version and metadata for Gaia Desktop.
//!
//! This module provides access to the Desktop application's build metadata,
//! which is generated at compile time and embedded in the binary.

use serde::Serialize;
use std::env;

/// Desktop build metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBuildMeta {
    pub name: String,
    pub version: String,
    pub build: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// Cached Desktop build metadata, loaded once at startup.
pub struct DesktopVersion {
    meta: DesktopBuildMeta,
}

impl DesktopVersion {
    /// Create a new DesktopVersion instance.
    pub fn new() -> Self {
        Self {
            meta: load_desktop_build_meta(),
        }
    }

    /// Get the Desktop build metadata.
    pub fn get(&self) -> &DesktopBuildMeta {
        &self.meta
    }
}

/// Load Desktop build metadata.
/// 
/// Tries to load from the embedded build-meta.json file that was generated
/// at compile time. Falls back to a development build ID if not available.
fn load_desktop_build_meta() -> DesktopBuildMeta {
    // Try to read from the embedded file path set by build.rs
    if let Ok(path) = env::var("GAIA_DESKTOP_BUILD_META") {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(meta) = serde_json::from_str(&content) {
                return meta;
            }
        }
    }

    // Fallback for development: generate a dev build ID
    use chrono::Utc;
    let build_id = Utc::now().format("%Y%m%d%H%M-dev").to_string();
    
    DesktopBuildMeta {
        name: "Gaia Desktop".to_string(),
        version: env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string()),
        build: build_id,
        commit: None,
    }
}

/// Cloud build metadata fetched from the server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudBuildMeta {
    pub name: String,
    pub version: String,
    pub build: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// Combined version information for display.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub desktop: DesktopBuildMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud: Option<CloudBuildMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_status: Option<String>, // "connected", "unavailable", etc.
}
