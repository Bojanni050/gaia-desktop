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

mod playback;
pub mod stt;

pub use playback::{PlayOutcome, PlaybackEngine, DEFAULT_FADE_MS};

use crate::communication::{CommunicationError, ServerLink};
use crate::error::DesktopError;

pub(crate) fn base_url(link: &ServerLink) -> Result<String, DesktopError> {
    let config = link.config();
    let base = config
        .base_url
        .filter(|u| !u.trim().is_empty())
        .ok_or(CommunicationError::NotConfigured)?;
    Ok(base.trim_end_matches('/').to_string())
}

pub(crate) fn authorize(builder: reqwest::RequestBuilder, link: &ServerLink) -> reqwest::RequestBuilder {
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

/// Fetches the synthesized speech for `text` from Gaia Server's `POST /speech`: raw audio bytes plus the
/// server's own Content-Type. Falls back to `audio/wav` when the server sends no Content-Type — the
/// pre-provider default, and what older servers always served.
async fn fetch_speech(link: &ServerLink, text: &str) -> Result<SpeechAudio, DesktopError> {
    let base = base_url(link)?;
    let client = reqwest::Client::new();
    let request = authorize(client.post(format!("{base}/speech")), link)
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

/// Synthesizes speech for `text` and returns the audio bytes (kept for callers that want the bytes;
/// the desktop itself now plays through [`speech_play`]).
#[tauri::command]
pub async fn speech_synthesize(
    link: tauri::State<'_, ServerLink>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    text: String,
) -> Result<SpeechAudio, DesktopError> {
    if !settings.get().audio.speak_replies {
        return Err(DesktopError::Message("speaking replies is switched off".into()));
    }
    fetch_speech(&link, &text).await
}

/// Synthesizes `text` and plays it through the speakers. Resolves when the reply ends or is stopped.
///
/// A newer `speech_play`, or any `speech_stop`, supersedes this one: if that happens while the audio is
/// still being fetched, nothing is played and the call resolves as stopped.
#[tauri::command]
pub async fn speech_play(
    link: tauri::State<'_, ServerLink>,
    playback: tauri::State<'_, PlaybackEngine>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    text: String,
) -> Result<PlayOutcome, DesktopError> {
    // Switched off: nothing is requested from Gaia Cloud, so no provider is called.
    if !settings.get().audio.speak_replies {
        return Ok(PlayOutcome::Stopped);
    }
    let epoch = playback.begin();
    // Anything still speaking is superseded by this reply.
    playback.stop_current(DEFAULT_FADE_MS);
    let speech = fetch_speech(&link, &text).await?;
    if !playback.is_current(epoch) {
        return Ok(PlayOutcome::Stopped);
    }
    Ok(playback.play(speech.audio).await)
}

/// Stops the current reply (easing it out over `fade_ms`, default 150) and cancels any reply still
/// being synthesised.
#[tauri::command]
pub fn speech_stop(playback: tauri::State<'_, PlaybackEngine>, fade_ms: Option<u64>) {
    playback.stop(fade_ms.unwrap_or(DEFAULT_FADE_MS));
}

/// Sets the local playback gain (volume 0..1, mute).
#[tauri::command]
pub fn speech_set_gain(playback: tauri::State<'_, PlaybackEngine>, volume: f32, muted: bool) {
    playback.set_gain(volume, muted);
}
