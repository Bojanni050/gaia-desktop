//! Finishing a Desktop Commander search inside one connection.
//!
//! `start_search` only *starts* a session: it returns at once with
//! `Status: RUNNING` and no results, and the matches come from
//! `get_more_search_results` on that session id. The desktop opens a fresh
//! server process per tool call, so a session started in one call is gone by the
//! next. The start and the polling therefore have to share a connection.
//!
//! This is protocol mechanics, not interpretation: the text the server returns is
//! relayed as it came, and what it means stays with Gaia Cloud.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::client::StdioClient;
use super::McpError;

/// How long a search may keep running before the results so far are returned.
pub const SEARCH_BUDGET: Duration = Duration::from_secs(15);
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

fn first_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// The session id a `start_search` answer announces (`search_<n>_<ts>`).
fn session_id(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ':')
        .find(|w| w.starts_with("search_") && w.len() > "search_".len())
        .map(str::to_string)
}

/// Whether the session reports anything other than still working. The server says `RUNNING` when
/// a search starts and `IN PROGRESS` while polling it, so both (and similar) count as not done.
fn is_finished(text: &str) -> bool {
    match text.lines().find_map(|l| l.trim().strip_prefix("Status:")) {
        Some(status) => {
            let status = status.trim().to_ascii_uppercase();
            !["RUNNING", "PROGRESS", "STARTED", "PENDING", "SEARCHING"]
                .iter()
                .any(|working| status.contains(working))
        }
        None => true,
    }
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// `start_search`, then poll the same session until it finishes or `budget` runs out. Returns a
/// `tools/call`-shaped result so callers read it like any other tool result.
pub fn run_search(
    client: &mut StdioClient,
    arguments: Value,
    budget: Duration,
    interval: Duration,
) -> Result<Value, McpError> {
    let page = arguments
        .get("maxResults")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or(50);
    let started = client.call_tool("start_search", arguments)?;
    let start_text = first_text(&started);
    if started.get("isError").and_then(Value::as_bool) == Some(true) {
        return Ok(text_result(start_text, true));
    }
    let Some(session) = session_id(&start_text) else {
        // No session to follow: hand back what the server said.
        return Ok(started);
    };

    let deadline = Instant::now() + budget;
    let mut latest = start_text;
    let mut finished = false;
    while Instant::now() < deadline {
        std::thread::sleep(interval);
        let polled = client.call_tool(
            "get_more_search_results",
            json!({ "sessionId": session, "length": page }),
        )?;
        latest = first_text(&polled);
        if polled.get("isError").and_then(Value::as_bool) == Some(true) {
            return Ok(text_result(latest, true));
        }
        if is_finished(&latest) {
            finished = true;
            break;
        }
    }
    // A search still running is stopped, so no process is left scanning the disk.
    let _ = client.call_tool("stop_search", json!({ "sessionId": session }));
    if !finished {
        latest.push_str("\n\n(De zoekopdracht was nog bezig; dit zijn de resultaten tot nu toe.)");
    }
    Ok(text_result(latest, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::config::McpServerConfig;

    #[test]
    fn session_id_is_found_in_the_start_text() {
        let text = "Started file search session: search_1_1791944495619\nPattern: \"txt\"";
        assert_eq!(session_id(text).as_deref(), Some("search_1_1791944495619"));
        assert_eq!(session_id("no session here"), None);
        assert_eq!(session_id("search_ alone"), None);
    }

    #[test]
    fn finished_means_status_is_not_running() {
        assert!(!is_finished("Search session: s\nStatus: RUNNING\nTotal results: 0"));
        // what the server really says while it is polled mid-search
        assert!(!is_finished("Search session: s\nStatus: IN PROGRESS\nTotal results: 0"));
        assert!(is_finished("Status: COMPLETED\nTotal results: 2"));
        assert!(is_finished("Status: ERROR"));
        assert!(is_finished("no status line at all"));
    }

    /// A stdio server that starts a search, reports RUNNING for the first
    /// polls and then COMPLETED with a match — and refuses polls for an unknown session, the way
    /// a server that lost its state would.
    const SEARCH_SCRIPT: &str = r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
let polls = 0;
rl.on('line', (line) => {
  const frame = JSON.parse(line);
  const send = (result) =>
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: frame.id, result }) + '\n');
  const text = (t) => send({ content: [{ type: 'text', text: t }] });
  if (frame.method === 'initialize') {
    send({ protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'search', version: '1' } });
  } else if (frame.method === 'tools/call') {
    const { name, arguments: args } = frame.params;
    if (name === 'start_search') text('Started file search session: search_1_42\nStatus: RUNNING\nTotal results: 0 matches');
    else if (name === 'get_more_search_results') {
      if (args.sessionId !== 'search_1_42') return text('Status: ERROR\nunknown session');
      polls += 1;
      if (process.env.NEVER_DONE || polls < 3) text('Search session: search_1_42\nStatus: IN PROGRESS\nTotal results: 0 matches');
      else text('Search session: search_1_42\nStatus: COMPLETED\nTotal results: 1 matches\nC:\\txtjes\\notes.txt');
    } else if (name === 'stop_search') text('stopped');
  }
});
"#;

    fn connect(script: &str) -> StdioClient {
        let config = McpServerConfig {
            enabled: true,
            command: "node".to_string(),
            args: vec!["-e".to_string(), script.to_string()],
            ..Default::default()
        };
        StdioClient::connect(&config).expect("connect")
    }

    #[test]
    fn polls_the_same_session_until_it_completes() {
        let mut client = connect(SEARCH_SCRIPT);
        let result = run_search(
            &mut client,
            json!({ "path": "C:\\txtjes", "pattern": "txt" }),
            Duration::from_secs(10),
            Duration::from_millis(10),
        )
        .expect("search");
        let text = first_text(&result);
        assert!(text.contains("Status: COMPLETED"), "{text}");
        assert!(text.contains("notes.txt"), "{text}");
        assert!(!text.contains("nog bezig"), "{text}");
    }

    #[test]
    fn a_search_that_never_finishes_returns_what_it_has_and_says_so() {
        let mut client = connect(&format!("process.env.NEVER_DONE = '1';{SEARCH_SCRIPT}"));
        let result = run_search(
            &mut client,
            json!({ "path": "C:\\txtjes", "pattern": "txt" }),
            Duration::from_millis(150),
            Duration::from_millis(20),
        )
        .expect("search");
        assert!(first_text(&result).contains("nog bezig"));
    }
}
