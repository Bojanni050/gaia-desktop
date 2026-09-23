//! MCP server configuration — this device's locally configured tool servers.
//!
//! Pure configuration, no behaviour: the settings UI edits these entries,
//! the client module spawns them. What a tool *means* is never decided
//! here — Gaia Cloud owns that.

use serde::{Deserialize, Serialize};

/// The MCP slice of the desktop's settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpSettings {
    pub servers: Vec<McpServerConfig>,
}

/// One locally configured MCP server, launched over stdio.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpServerConfig {
    pub id: String,
    pub enabled: bool,
    pub command: String,
    pub args: Vec<String>,
}

impl McpServerConfig {
    /// Whether the client may launch this server: enabled and non-empty
    /// command. Disabled or blank rows are simply skipped, never an error.
    pub fn is_runnable(&self) -> bool {
        self.enabled && !self.command.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runnable_requires_enabled_and_a_command() {
        let base = || McpServerConfig {
            id: "s".to_string(),
            enabled: true,
            command: "npx".to_string(),
            args: vec!["-y".to_string()],
        };
        assert!(base().is_runnable());
        assert!(!{ let mut s = base(); s.enabled = false; s }.is_runnable());
        assert!(!{ let mut s = base(); s.command = "  ".to_string(); s }.is_runnable());
    }
}
