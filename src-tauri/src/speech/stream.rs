//! Gaia's voice, streamed: `POST /speech/stream` answers with raw PCM as it is synthesized, so the
//! first words are heard while the rest of the reply is still being generated.
//!
//! Falls back to one complete clip when Gaia Server does not offer the streaming route (an older
//! server answers 404) and equally when the configured provider cannot stream — the route then
//! answers with a complete clip of its own, labeled with its own Content-Type.

use crate::communication::ServerLink;
use crate::error::DesktopError;

use super::{authorize, base_url, error_for_status, fetch_speech, speech_body, SpeechAudio};

/// What the server's own headers say when they say nothing.
const DEFAULT_SAMPLE_RATE: u32 = 24_000;
const DEFAULT_CHANNELS: u16 = 1;

/// How the server answered: the reply as it is being synthesized, or as one complete clip.
pub enum SpeechResponse {
    Stream(SpeechStream),
    Clip(SpeechAudio),
}

/// A reply arriving in pieces: raw PCM16LE, `channels` channels (mono in practice), at the rate the
/// server named in its headers. The caller reads it chunk by chunk and plays what has arrived.
pub struct SpeechStream {
    pub response: reqwest::Response,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Fetches the synthesized speech for `text`, asking for it as a stream.
///
/// `POST /speech/stream` answers with PCM while the reply is still being synthesized. An older Gaia
/// Server that does not know the route is asked for the whole clip instead, and a provider that
/// cannot stream answers the route with a complete clip — both arrive as [`SpeechResponse::Clip`],
/// which plays exactly as a reply always has.
pub async fn request_speech(
    link: &ServerLink,
    text: &str,
    language: Option<&str>,
) -> Result<SpeechResponse, DesktopError> {
    let base = base_url(link)?;
    let client = reqwest::Client::new();
    let request = authorize(client.post(format!("{base}/speech/stream")), link)
        .json(&speech_body(text, language));

    let response = request
        .send()
        .await
        .map_err(|e| DesktopError::Message(format!("speech request failed: {e}")))?;

    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        // No streaming route on that server: ask it for one complete clip the old way.
        return Ok(SpeechResponse::Clip(
            fetch_speech(link, text, language).await?,
        ));
    }
    if !status.is_success() {
        return Err(error_for_status(response).await);
    }
    if !is_pcm(&content_type(&response)) {
        return read_clip(response).await;
    }

    Ok(SpeechResponse::Stream(SpeechStream {
        sample_rate: header_number(&response, "x-gaia-sample-rate").unwrap_or(DEFAULT_SAMPLE_RATE),
        channels: header_number(&response, "x-gaia-channels")
            .map(|channels| channels.min(u16::MAX as u32) as u16)
            .filter(|channels| *channels > 0)
            .unwrap_or(DEFAULT_CHANNELS),
        response,
    }))
}

/// The response's own Content-Type (`""` when it sent none).
fn content_type(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// True when the server is sending the reply as raw PCM the desktop can play straight away. The
/// streaming route always labels it `audio/L16`; anything else is a complete clip to decode.
fn is_pcm(content_type: &str) -> bool {
    let kind = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        kind.as_str(),
        "audio/l16" | "audio/l16le" | "audio/pcm" | "audio/x-pcm"
    )
}

/// A positive number in a response header (`X-Gaia-Sample-Rate` and friends), when it is one.
fn header_number(response: &reqwest::Response, name: &str) -> Option<u32> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
}

/// Reads a complete clip the streaming route answered with (a provider that cannot stream).
async fn read_clip(response: reqwest::Response) -> Result<SpeechResponse, DesktopError> {
    let mime_type = content_type(&response);
    let mime_type = if mime_type.is_empty() {
        "audio/wav".to_string()
    } else {
        mime_type
    };
    let bytes = response
        .bytes()
        .await
        .map_err(|e| DesktopError::Message(format!("could not read audio response: {e}")))?;
    Ok(SpeechResponse::Clip(SpeechAudio {
        audio: bytes.to_vec(),
        mime_type,
    }))
}

/// Raw PCM16LE (what `/speech/stream` sends) turned into the mono f32 samples the player takes.
///
/// A chunk boundary can fall in the middle of a sample or of a frame, so leftover bytes are kept for
/// the next chunk: what is heard must not depend on where the network happened to split the reply.
pub struct Pcm16 {
    channels: usize,
    tail: Vec<u8>,
}

impl Pcm16 {
    pub fn new(channels: u16) -> Self {
        Self {
            channels: channels.max(1) as usize,
            tail: Vec::new(),
        }
    }

    /// Appends the samples `chunk` completes to `out`.
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<f32>) {
        let frame_bytes = self.channels * 2;
        let mut data = std::mem::take(&mut self.tail);
        data.extend_from_slice(chunk);
        let whole = data.len() - data.len() % frame_bytes;
        for frame in data[..whole].chunks_exact(frame_bytes) {
            let mut sum = 0i32;
            for sample in frame.chunks_exact(2) {
                sum += i16::from_le_bytes([sample[0], sample[1]]) as i32;
            }
            out.push(sum as f32 / (self.channels as f32 * 32_768.0));
        }
        self.tail = data.split_off(whole);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One 16-bit little-endian sample.
    fn sample(value: i16) -> [u8; 2] {
        value.to_le_bytes()
    }

    /// Several samples in a row, as the wire carries them.
    fn samples(values: &[i16]) -> Vec<u8> {
        values.iter().flat_map(|value| value.to_le_bytes()).collect()
    }

    #[test]
    fn pcm_becomes_samples_at_their_own_level() {
        let mut out = Vec::new();
        let mut pcm = Pcm16::new(1);
        pcm.push(&samples(&[1000, -1000]), &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 1000.0 / 32_768.0).abs() < 1e-6, "{:?}", out[0]);
        assert!((out[1] + 1000.0 / 32_768.0).abs() < 1e-6, "{:?}", out[1]);

        out.clear();
        pcm.push(&samples(&[i16::MAX, i16::MIN]), &mut out);
        assert!((out[0] - 1.0).abs() < 1e-4, "{:?}", out[0]);
        assert!((out[1] + 1.0).abs() < 1e-4, "{:?}", out[1]);
    }

    #[test]
    fn a_sample_split_across_two_chunks_survives() {
        let mut out = Vec::new();
        let mut pcm = Pcm16::new(1);
        pcm.push(&[sample(1000)[0]], &mut out); // half a sample
        assert!(out.is_empty(), "a half sample is not a sample");
        pcm.push(&[sample(1000)[1], sample(-1000)[0]], &mut out); // its other half
        assert_eq!(out.len(), 1);
        assert!((out[0] - 1000.0 / 32_768.0).abs() < 1e-6);
        pcm.push(&[sample(-1000)[1]], &mut out);
        assert_eq!(out.len(), 2);
        assert!((out[1] + 1000.0 / 32_768.0).abs() < 1e-6);
    }

    #[test]
    fn a_frame_split_across_two_chunks_survives() {
        let mut out = Vec::new();
        let mut pcm = Pcm16::new(2);
        pcm.push(&[sample(i16::MAX)[0], sample(i16::MAX)[1], 0], &mut out); // left + half of right
        assert!(out.is_empty(), "half a frame is not a frame");
        pcm.push(&[0], &mut out);
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.5).abs() < 1e-3, "{:?}", out[0]);
    }

    #[test]
    fn several_channels_become_one() {
        let mut out = Vec::new();
        let mut pcm = Pcm16::new(2);
        pcm.push(&samples(&[i16::MAX, 0]), &mut out);
        assert_eq!(out.len(), 1, "two channels are averaged into one sample");
        assert!((out[0] - 0.5).abs() < 1e-3, "{:?}", out[0]);
    }

    #[test]
    fn chunk_boundaries_do_not_change_the_samples() {
        let bytes: Vec<u8> = (0..480i32).flat_map(|i| ((i * 137 % 601) as i16 - 300).to_le_bytes()).collect();
        let whole = {
            let mut out = Vec::new();
            Pcm16::new(1).push(&bytes, &mut out);
            out
        };
        for size in [1usize, 3, 64, 137, 512] {
            let mut out = Vec::new();
            let mut pcm = Pcm16::new(1);
            for chunk in bytes.chunks(size) {
                pcm.push(chunk, &mut out);
            }
            assert_eq!(out, whole, "chunk size {size}");
        }
    }

    #[test]
    fn a_zero_channel_count_does_not_panic() {
        let mut out = Vec::new();
        Pcm16::new(0).push(&[1, 2], &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn only_raw_pcm_is_taken_as_a_stream() {
        assert!(is_pcm("audio/L16"));
        assert!(is_pcm("audio/l16; rate=24000"));
        assert!(is_pcm("audio/pcm"));
        assert!(!is_pcm("audio/wav"));
        assert!(!is_pcm("audio/mpeg"));
        assert!(!is_pcm("application/json"));
        assert!(!is_pcm(""));
    }
}
