//! Minimal MCP client — the desktop's technical tool-execution layer.
//!
//! This is a *capability*, not a cognition: the client speaks MCP's
//! JSON-RPC protocol over a local stdio server's stdin/stdout, and nothing
//! more. Which tool to call, with which arguments, and what the result
//! means is decided by Gaia Cloud — this layer only offers the technical
//! possibility and relays bytes verbatim.
//!
//! Scope: `initialize`, `tools/list` and `tools/call` from the MCP spec,
//! against stdio servers configured in the desktop's settings. One fresh
//! connection per operation — simple and predictable; pooling or remote
//! transports (HTTP/SSE) can be added later behind the same commands.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::config::McpServerConfig;
use super::McpError;

/// How long any single request may wait for its JSON-RPC response.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the spawned server gets to complete `initialize`.
const INIT_TIMEOUT: Duration = Duration::from_secs(10);

type SharedReader = Arc<Mutex<BufReader<ChildStdout>>>;

/// A connected stdio MCP server. The process is killed on drop — the
/// desktop never leaves tool servers running unattended.
pub struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    reader: SharedReader,
    next_id: u64,
}

impl StdioClient {
    /// Launch the configured server and perform MCP `initialize`.
    pub fn connect(config: &McpServerConfig) -> Result<Self, McpError> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::Spawn(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("server stdin closed".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("server stdout closed".to_string()))?;
        let reader: SharedReader = Arc::new(Mutex::new(BufReader::new(stdout)));
        let mut client = Self {
            child,
            stdin,
            reader,
            next_id: 0,
        };
        client.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "gaia-desktop", "version": "0.2.1" },
            }),
            INIT_TIMEOUT,
        )?;
        client.notify("notifications/initialized", json!({}))?;
        Ok(client)
    }

    /// `tools/list` — the server's tools, verbatim.
    pub fn list_tools(&mut self) -> Result<Vec<Value>, McpError> {
        let result = self.request("tools/list", json!({}), REQUEST_TIMEOUT)?;
        Ok(match result.get("tools") {
            Some(Value::Array(tools)) => tools.clone(),
            _ => Vec::new(),
        })
    }

    /// `tools/call` — execute one tool with opaque arguments, verbatim.
    /// The desktop never interprets the result.
    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
            REQUEST_TIMEOUT,
        )
    }

    fn allocate_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Send one JSON-RPC request and read the matching response. Responses
    /// are matched by id, so interleaved notifications never
    /// desynchronize the stream.
    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, McpError> {
        let id = self.allocate_id();
        let frame = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_frame(&frame)?;
        self.read_response(id, timeout)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), McpError> {
        let frame = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_frame(&frame)
    }

    fn write_frame(&mut self, frame: &Value) -> Result<(), McpError> {
        let mut line = serde_json::to_string(frame)
            .map_err(|e| McpError::Transport(e.to_string()))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| McpError::Transport(e.to_string()))?;
        self.stdin
            .flush()
            .map_err(|e| McpError::Transport(e.to_string()))
    }

    /// Read lines until the response for `id` arrives, bounded by
    /// `timeout`. Reading runs on a thread so the timeout is enforced even
    /// when the server never answers; the shared reader keeps any buffered
    /// data available for the next request.
    fn read_response(&mut self, id: u64, timeout: Duration) -> Result<Value, McpError> {
        let (tx, rx) = mpsc::channel();
        let reader = Arc::clone(&self.reader);
        let handle = std::thread::spawn(move || loop {
            let mut guard = match reader.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    let _ = tx.send(Err(McpError::Transport("reader lock poisoned".to_string())));
                    return;
                }
            };
            let mut line = String::new();
            match guard.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(Err(McpError::Transport("server closed".to_string())));
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(trimmed) {
                        Ok(frame) if frame.get("id").and_then(Value::as_u64) == Some(id) => {
                            let _ = tx.send(Ok(frame));
                            return;
                        }
                        _ => continue,
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(McpError::Transport(e.to_string())));
                    return;
                }
            }
        });
        let frame = rx
            .recv_timeout(timeout)
            .map_err(|_| McpError::Timeout)?;
        let _ = handle.join();
        match frame {
            Ok(frame) => {
                if let Some(error) = frame.get("error") {
                    return Err(McpError::Server(error.clone()));
                }
                Ok(frame.get("result").cloned().unwrap_or_else(|| json!({})))
            }
            Err(e) => Err(e),
        }
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::config::McpServerConfig;

    /// A tiny stdio MCP echo server as the test double: it answers the
    /// same JSON-RPC methods the real protocol uses, one frame per line.
    const ECHO_SCRIPT: &str = r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const frame = JSON.parse(line);
  const send = (result) =>
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id, result }) + '\n');
  if (frame.method === 'initialize') {
    send({ protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'echo', version: '1.0' } });
  } else if (frame.method === 'tools/list') {
    send({ tools: [{ name: 'echo', description: 'echoes its arguments' }] });
  } else if (frame.method === 'tools/call') {
    send({ content: [{ type: 'text', text: JSON.stringify(frame.params.arguments) }], isError: false });
  }
});
"#;

    fn echo_config() -> McpServerConfig {
        McpServerConfig {
            id: "echo".to_string(),
            enabled: true,
            command: "node".to_string(),
            args: vec!["-e".to_string(), ECHO_SCRIPT.to_string()],
        }
    }

    #[test]
    fn stdio_round_trip_lists_and_calls_tools() {
        let mut client = StdioClient::connect(&echo_config()).expect("connect must succeed");
        let tools = client.list_tools().expect("tools/list must succeed");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");

        let result = client
            .call_tool("echo", serde_json::json!({ "message": "hallo" }))
            .expect("tools/call must succeed");
        assert_eq!(
            result["content"][0]["text"],
            serde_json::json!({ "message": "hallo" }).to_string()
        );
    }

    #[test]
    fn unknown_command_fails_to_spawn() {
        let config = McpServerConfig {
            id: "missing".to_string(),
            enabled: true,
            command: "definitely-not-a-real-command-xyz".to_string(),
            args: vec![],
        };
        assert!(matches!(
            StdioClient::connect(&config),
            Err(McpError::Spawn(_))
        ));
    }

    #[test]
    fn dead_server_surfaces_a_transport_error() {
        // Prints one garbage frame, then exits before ever answering
        // `initialize`: the garbage is skipped and the end-of-stream must
        // surface as a transport error, never a hang or a panic.
        let config = McpServerConfig {
            id: "dies".to_string(),
            enabled: true,
            command: "node".to_string(),
            args: vec![
                "-e".to_string(),
                "process.stdout.write('not-json\n'); process.exit(0)".to_string(),
            ],
        };
        assert!(matches!(
            StdioClient::connect(&config),
            Err(McpError::Transport(_))
        ));
    }
}
