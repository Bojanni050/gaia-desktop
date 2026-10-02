//! Gaia's voice — transports an already-finalized Gaia text reply to Gaia
//! Server's `/speech` endpoint and returns the synthesized audio bytes.
//!
//! Server-side only, same posture as `library`: this module never decides
//! *whether* Gaia should speak, never generates or alters text, and never
//! touches SOUL, memory, intent, or reasoning — it only carries bytes
//! across the wire. It isn't funneled through `server_request`'s generic
//! opaque-JSON contract because audio bytes need binary transport, which
//! that seam doesn't carry — this is its own small, purpose-built seam,
//! the same pattern `library`/`capture`/`audio` already use for concerns
//! `server_request` doesn't fit.
//!
//! Text-to-speech is always the *last* step, after Gaia Cloud's own
//! Response Engine has already produced the reply the frontend is
//! showing — this command's `text` argument is that already-received
//! reply, not a fresh prompt.

use crate::communication::{CommunicationError, ServerLink};
use crate::error::DesktopError;

fn base_url(link: &ServerLink) -> Result<String, DesktopError> {
    let config = link.config();
    let base = config
        .base_url
        .filter(|u| !u.trim().is_empty())
        .ok_or(CommunicationError::NotConfigured)?;
    Ok(base.trim_end_matches('/').to_string())
}

fn authorize(builder: reqwest::RequestBuilder, link: &ServerLink) -> reqwest::RequestBuilder {
    match link.config().auth_token {
        Some(token) => builder.bearer_auth(token),
        None => builder,
    }
}

async fn error_for_status(response: reqwest::Response) -> DesktopError {
    let status = response.status();
    DesktopError::Message(format!("Gaia Server responded with status {status}"))
}

/// Synthesized speech as the server sent it: raw audio bytes plus the
/// server's own Content-Type for correct Blob labeling upstream.
#[derive(Debug, serde::Serialize)]
pub struct SpeechAudio {
    pub audio: Vec<u8>,
    pub mime_type: String,
}

/// Synthesizes speech for `text` via Gaia Server's `POST /speech` and
/// returns the raw audio bytes plus the server's own Content-Type, so the
/// frontend can label the playback Blob correctly no matter which TTS
/// provider (and encoding) the server has configured. Falls back to
/// `audio/wav` when the server sends no Content-Type — the pre-provider
/// default, and what older servers always served.
#[tauri::command]
pub async fn speech_synthesize(
    link: tauri::State<'_, ServerLink>,
    text: String,
) -> Result<SpeechAudio, DesktopError> {
    let base = base_url(&link)?;
    let client = reqwest::Client::new();
    let request = authorize(client.post(format!("{base}/speech")), &link)
        .json(&serde_json::json!({ "text": text }));

    let response = request
        .send()
        .await
        .map_err(|e| DesktopError::Message(format!("speech request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(error_for_status(response).await);
    }

    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("audio/wav")
        .to_string();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| DesktopError::Message(format!("could not read audio response: {e}")))?;
    Ok(SpeechAudio {
        audio: bytes.to_vec(),
        mime_type,
    })
}
