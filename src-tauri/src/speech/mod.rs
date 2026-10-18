//! Gaia's voice — transports an already-finalized Gaia text reply to Gaia
//! Server and plays it: one complete clip from `POST /speech`, or the reply as
//! it is *still being synthesized* from `POST /speech/stream` (see [`stream`]),
//! which is what makes her first words audible before the provider is done.
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
mod stream;
pub mod stt;

pub use playback::{PlayOutcome, PlaybackEngine, DEFAULT_FADE_MS};

use crate::communication::{CommunicationError, ServerLink};
use crate::error::DesktopError;

use stream::{Pcm16, SpeechResponse, SpeechStream};

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

/// The `/speech` request body. `language` ("nl" or "en", detected on the desktop) lets Gaia Cloud pick the
/// matching voice; without it the server speaks with its default voice.
fn speech_body(text: &str, language: Option<&str>) -> serde_json::Value {
    match language {
        Some(language @ ("nl" | "en")) => serde_json::json!({ "text": text, "language": language }),
        _ => serde_json::json!({ "text": text }),
    }
}

/// Fetches the synthesized speech for `text` from Gaia Server's `POST /speech`: raw audio bytes plus the
/// server's own Content-Type. Falls back to `audio/wav` when the server sends no Content-Type — the
/// pre-provider default, and what older servers always served.
async fn fetch_speech(
    link: &ServerLink,
    text: &str,
    language: Option<&str>,
) -> Result<SpeechAudio, DesktopError> {
    let base = base_url(link)?;
    let client = reqwest::Client::new();
    let request = authorize(client.post(format!("{base}/speech")), link)
        .json(&speech_body(text, language));

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
    language: Option<String>,
) -> Result<SpeechAudio, DesktopError> {
    if !settings.get().audio.speak_replies {
        return Err(DesktopError::Message("speaking replies is switched off".into()));
    }
    fetch_speech(&link, &text, language.as_deref()).await
}

/// Synthesizes `text` and plays it through the speakers, starting as soon as the provider has
/// synthesized the first of it. Resolves when the reply ends or is stopped.
///
/// A newer `speech_play`, or any `speech_stop`, supersedes this one: if that happens while the reply
/// is still arriving, nothing more is played, the provider request is hung up and the call resolves
/// as stopped.
#[tauri::command]
pub async fn speech_play(
    link: tauri::State<'_, ServerLink>,
    playback: tauri::State<'_, PlaybackEngine>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    text: String,
    language: Option<String>,
) -> Result<PlayOutcome, DesktopError> {
    // Switched off: nothing is requested from Gaia Cloud, so no provider is called.
    if !settings.get().audio.speak_replies {
        return Ok(PlayOutcome::Stopped);
    }
    let epoch = playback.begin();
    // Anything still speaking is superseded by this reply.
    playback.stop_current(DEFAULT_FADE_MS);
    match stream::request_speech(&link, &text, language.as_deref()).await? {
        SpeechResponse::Clip(speech) => {
            if !playback.is_current(epoch) {
                return Ok(PlayOutcome::Stopped);
            }
            Ok(playback.play(speech.audio).await)
        }
        SpeechResponse::Stream(speech) => {
            if !playback.is_current(epoch) {
                return Ok(PlayOutcome::Stopped);
            }
            play_stream(&playback, epoch, speech).await
        }
    }
}

/// Plays a reply that is still arriving: reads the PCM the server sends as it is synthesized and
/// hands it to the player chunk by chunk. Resolves when the reply ends, is stopped, or the stream
/// breaks.
async fn play_stream(
    playback: &PlaybackEngine,
    epoch: u64,
    speech: SpeechStream,
) -> Result<PlayOutcome, DesktopError> {
    let handle = match playback.stream(speech.sample_rate).await {
        Ok(handle) => handle,
        // No output device (or no playback thread): the reply cannot be spoken at all.
        Err(outcome) => return Ok(outcome),
    };
    let mut pcm = Pcm16::new(speech.channels);
    let mut response = speech.response;
    loop {
        if !playback.is_current(epoch) {
            // Stopped or superseded. Finishing the reply here and dropping the response ends what is
            // left locally *and* hangs the provider request up, so it stops synthesizing a sentence
            // nobody will hear.
            handle.finish();
            return Ok(PlayOutcome::Stopped);
        }
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(e) => {
                // The rest of the reply never arrives. What she has already said is not taken back,
                // so what is playing runs out and the failure is reported after it.
                handle.finish();
                let _ = handle.wait().await;
                return Err(DesktopError::Message(format!("speech stream failed: {e}")));
            }
        };
        let mut samples = Vec::new();
        pcm.push(&chunk, &mut samples);
        handle.push(samples);
    }
    handle.finish();
    Ok(handle.wait().await)
}

/// Stops the current reply (easing it out over `fade_ms`, default 150) and cancels any reply still
/// being synthesised.
#[tauri::command]
pub fn speech_stop(playback: tauri::State<'_, PlaybackEngine>, fade_ms: Option<u64>) {
    playback.stop(fade_ms.unwrap_or(DEFAULT_FADE_MS));
}

/// The output devices Gaia's voice can be played through.
#[tauri::command]
pub fn speech_list_output_devices() -> Vec<crate::audio::capture::AudioDeviceInfo> {
    playback::list_output_devices()
}

/// Chooses the output device (`None` = system default), saves the choice and switches playback to it.
#[tauri::command]
pub fn speech_select_output_device(
    playback: tauri::State<'_, PlaybackEngine>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    id: Option<String>,
) -> Result<(), DesktopError> {
    settings
        .update(|s| s.audio.output_device = id.clone())
        .map_err(DesktopError::Settings)?;
    playback.set_device(id);
    Ok(())
}

/// Sets the local playback gain (volume 0..1, mute).
#[tauri::command]
pub fn speech_set_gain(playback: tauri::State<'_, PlaybackEngine>, volume: f32, muted: bool) {
    playback.set_gain(volume, muted);
}

#[cfg(test)]
mod speech_body_tests {
    use super::speech_body;

    #[test]
    fn sends_the_language_only_when_known() {
        assert_eq!(speech_body("hoi", Some("nl")), serde_json::json!({ "text": "hoi", "language": "nl" }));
        assert_eq!(speech_body("hi", Some("en"))["language"], "en");
        assert_eq!(speech_body("hi", None), serde_json::json!({ "text": "hi" }));
        assert_eq!(speech_body("hi", Some("xx")), serde_json::json!({ "text": "hi" }));
    }
}
