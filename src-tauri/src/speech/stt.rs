//! Gaia's ears, from the desktop side: sends one utterance to Gaia Cloud's `POST /speech/transcribe` and
//! returns the text.
//!
//! The same posture as the rest of `speech`: this module only carries bytes across the wire. It never
//! decides what the words mean, whether Gaia answers, or anything about memory or identity; the text goes
//! back to the UI, which sends it as an ordinary turn. The audio is the request body of one call and is
//! never written anywhere.

use async_trait::async_trait;
use tauri::Manager;

use crate::audio::ptt::{TranscribeError, Transcriber, Transcript};
use crate::communication::ServerLink;

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The real transcriber: talks to Gaia Server through the configured [`ServerLink`].
pub struct CloudTranscriber {
    app: tauri::AppHandle,
}

impl CloudTranscriber {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

/// What a failed HTTP status means to the user.
pub fn map_status(status: u16) -> TranscribeError {
    match status {
        401 | 403 => TranscribeError::new("unauthorized", "Gaia Server did not accept the token"),
        413 => TranscribeError::new("too_long", "the recording was too long"),
        503 => TranscribeError::new("not_configured", "speech recognition is not set up on Gaia Server"),
        404 | 405 => TranscribeError::new("not_supported", "this Gaia Server cannot transcribe speech yet"),
        other => TranscribeError::new("server_error", format!("Gaia Server responded with status {other}")),
    }
}

/// The `{ text, language }` body of a successful call.
pub fn parse_transcript(body: &serde_json::Value) -> Option<Transcript> {
    let text = body.get("text")?.as_str()?.trim().to_string();
    let language = body
        .get("language")
        .and_then(|l| l.as_str())
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    Some(Transcript { text, language })
}

/// The whole call: `POST {server}/speech/transcribe` with the WAV as the body. Separate from the
/// `Transcriber` impl so it can be tested against a local server without a running app.
pub async fn transcribe_wav(link: &ServerLink, wav: Vec<u8>) -> Result<Transcript, TranscribeError> {
    let base = super::base_url(link)
        .map_err(|_| TranscribeError::new("not_configured", "no Gaia Server is configured"))?;
    let request = super::authorize(reqwest::Client::new().post(format!("{base}/speech/transcribe")), link)
        .timeout(TIMEOUT)
        .header(reqwest::header::CONTENT_TYPE, "audio/wav")
        .body(wav);

    let response = request
        .send()
        .await
        .map_err(|_| TranscribeError::new("unreachable", "could not reach Gaia Server"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_status(status.as_u16()));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| TranscribeError::new("server_error", "Gaia Server sent an unreadable answer"))?;
    parse_transcript(&body).ok_or_else(|| TranscribeError::new("server_error", "Gaia Server sent no text"))
}

#[async_trait]
impl Transcriber for CloudTranscriber {
    async fn transcribe(&self, wav: Vec<u8>) -> Result<Transcript, TranscribeError> {
        let link = self.app.state::<ServerLink>();
        transcribe_wav(&link, wav).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn statuses_map_to_codes_the_ui_can_explain() {
        assert_eq!(map_status(401).code, "unauthorized");
        assert_eq!(map_status(403).code, "unauthorized");
        assert_eq!(map_status(413).code, "too_long");
        assert_eq!(map_status(503).code, "not_configured");
        assert_eq!(map_status(404).code, "not_supported", "an older server without the endpoint");
        assert_eq!(map_status(502).code, "server_error");
        assert_eq!(map_status(500).code, "server_error");
    }

    #[test]
    fn a_transcript_is_trimmed_and_may_have_no_language() {
        let t = parse_transcript(&json!({ "text": "  hallo daar \n", "language": "nl" })).unwrap();
        assert_eq!(t.text, "hallo daar");
        assert_eq!(t.language.as_deref(), Some("nl"));
        let t = parse_transcript(&json!({ "text": "hi", "language": null })).unwrap();
        assert_eq!(t.language, None);
        let t = parse_transcript(&json!({ "text": "hi", "language": "" })).unwrap();
        assert_eq!(t.language, None);
    }

    #[test]
    fn a_body_without_text_is_not_a_transcript() {
        assert!(parse_transcript(&json!({})).is_none());
        assert!(parse_transcript(&json!({ "text": 5 })).is_none());
        assert!(parse_transcript(&json!(null)).is_none());
    }

    // --- the real HTTP call against a local server -----------------------------------------------

    use crate::communication::ServerConfig;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot HTTP server on 127.0.0.1 that records the raw request and answers `status` + `body`.
    async fn local_server(status: u16, body: &'static str) -> (String, Arc<Mutex<Vec<u8>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                received.extend_from_slice(&buf[..n]);
                // the request is complete once the headers and the declared body have arrived
                if let Some(head_end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&received[..head_end]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if received.len() >= head_end + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            *seen2.lock().unwrap() = received;
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
        (url, seen)
    }

    fn link_to(url: &str, token: Option<&str>) -> ServerLink {
        ServerLink::new(ServerConfig {
            base_url: Some(url.to_string()),
            websocket_url: None,
            auth_token: token.map(str::to_string),
        })
    }

    #[tokio::test]
    async fn the_real_call_posts_raw_wav_with_the_token_and_returns_the_text() {
        let (url, seen) = local_server(200, r#"{"text":"  hallo daar ","language":"nl"}"#).await;
        let wav = b"RIFF....WAVEfake".to_vec();
        let t = transcribe_wav(&link_to(&url, Some("secret-token")), wav.clone()).await.unwrap();
        assert_eq!(t.text, "hallo daar");
        assert_eq!(t.language.as_deref(), Some("nl"));

        let raw = seen.lock().unwrap().clone();
        let head_end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..head_end]).to_lowercase();
        assert!(head.starts_with("post /speech/transcribe "), "{head}");
        assert!(head.contains("authorization: bearer secret-token"), "{head}");
        assert!(head.contains("content-type: audio/wav"), "{head}");
        assert_eq!(&raw[head_end + 4..], wav.as_slice(), "the body is the raw WAV, nothing wrapped around it");
    }

    #[tokio::test]
    async fn the_real_call_maps_server_failures_to_codes() {
        for (status, code) in [(401, "unauthorized"), (413, "too_long"), (503, "not_configured"), (404, "not_supported"), (502, "server_error")] {
            let (url, _) = local_server(status, r#"{"error":"x"}"#).await;
            let err = transcribe_wav(&link_to(&url, None), vec![1, 2, 3]).await.unwrap_err();
            assert_eq!(err.code, code, "status {status}");
        }
    }

    #[tokio::test]
    async fn the_real_call_reports_an_unreachable_server_and_an_unconfigured_link() {
        // a port nothing listens on
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", l.local_addr().unwrap())
        };
        assert_eq!(transcribe_wav(&link_to(&dead, None), vec![1]).await.unwrap_err().code, "unreachable");
        let unconfigured = ServerLink::new(ServerConfig::default());
        assert_eq!(transcribe_wav(&unconfigured, vec![1]).await.unwrap_err().code, "not_configured");
    }

    #[tokio::test]
    async fn the_real_call_rejects_an_answer_without_text() {
        let (url, _) = local_server(200, r#"{"nothing":true}"#).await;
        assert_eq!(transcribe_wav(&link_to(&url, None), vec![1]).await.unwrap_err().code, "server_error");
    }

    #[test]
    fn silence_is_an_empty_transcript_not_an_error() {
        assert_eq!(parse_transcript(&json!({ "text": "" })).unwrap().text, "");
    }
}
