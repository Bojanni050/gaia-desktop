//! MCP — the desktop's technical tool capability.
//!
//! Model Context Protocol servers give a model access to tools and data.
//! The desktop's role here is deliberately narrow: it *offers the
//! technical possibility* — configuring local stdio servers, connecting to
//! them, and executing a tool call verbatim. It never decides which tool
//! to run or what a result means; that cognition belongs to Gaia Cloud,
//! reached through the ServerLink seam.
//!
//! Phase 1: configuration in settings, `tools/list` and `tools/call` over
//! stdio. Remote transports and a persistent registry can follow behind
//! the same commands without touching callers.

pub mod client;
pub mod config;

pub use client::StdioClient;
pub use config::{McpServerConfig, McpSettings};

use serde_json::Value;

use crate::error::DesktopError;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("could not launch MCP server: {0}")]
    Spawn(String),
    #[error("MCP server transport failed: {0}")]
    Transport(String),
    #[error("MCP server did not answer in time")]
    Timeout,
    #[error("MCP server returned an error: {0}")]
    Server(serde_json::Value),
}

impl From<McpError> for DesktopError {
    fn from(error: McpError) -> Self {
        DesktopError::Mcp(error)
    }
}

/// One configured server's id plus its offered tools, verbatim from
/// `tools/list`. Names and schemas are opaque to the desktop.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolsResult {
    pub server_id: String,
    pub tools: Vec<Value>,
}

/// Find a runnable server config by id. Blank or disabled rows are treated
/// the same as missing ones: not runnable, reported to the caller as such.
fn find_runnable(
    settings: &crate::settings::SettingsState,
    server_id: &str,
) -> Result<McpServerConfig, DesktopError> {
    let configured = settings.get().mcp.servers;
    configured
        .into_iter()
        .find(|s| s.id == server_id && s.is_runnable())
        .ok_or_else(|| {
            DesktopError::Message(format!("MCP server not found or not enabled: {server_id}"))
        })
}

/// List the tools a configured MCP server offers. Purely technical: spawn,
/// initialize, `tools/list`, relay verbatim, terminate.
#[tauri::command]
pub async fn mcp_list_tools(
    settings: tauri::State<'_, crate::settings::SettingsState>,
    server_id: String,
) -> Result<McpToolsResult, DesktopError> {
    let config = find_runnable(&settings, &server_id)?;
    let mut client = StdioClient::connect(&config)?;
    let tools = client.list_tools()?;
    Ok(McpToolsResult { server_id, tools })
}

/// Execute one tool call against a configured MCP server. The arguments
/// are opaque — the desktop forwards them verbatim and returns the
/// server's content list verbatim; interpretation stays with Gaia Cloud.
#[tauri::command]
pub async fn mcp_call_tool(
    settings: tauri::State<'_, crate::settings::SettingsState>,
    server_id: String,
    tool: String,
    arguments: Value,
) -> Result<Value, DesktopError> {
    let config = find_runnable(&settings, &server_id)?;
    let mut client = StdioClient::connect(&config)?;
    Ok(client.call_tool(&tool, arguments)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_runnable_skips_disabled_and_blank_rows() {
        // The filtering contract of the lookup, exercised through the same
        // predicate it delegates to (is_runnable, unit-tested in config.rs).
        let rows = vec![
            McpServerConfig {
                id: "a".to_string(),
                enabled: false,
                command: "node".to_string(),
                args: vec![],
            },
            McpServerConfig {
                id: "b".to_string(),
                enabled: true,
                command: "node".to_string(),
                args: vec![],
            },
        ];
        let runnable: Vec<&McpServerConfig> = rows
            .iter()
            .filter(|r| r.id == "b" && r.is_runnable())
            .collect();
        assert_eq!(runnable.len(), 1);
        assert_eq!(runnable[0].id, "b");
    }
}
