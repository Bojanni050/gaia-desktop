//! Opt-in capture of what Gaia's voice actually received and played, for diagnosing
//! provider-side problems (a voice that fades out, clips, or changes level) instead of guessing.
//!
//! Nothing is written unless `GAIA_SPEECH_DUMP` is set: `1`/`true` dumps to
//! `<temp>/gaia-speech-dump`, any other non-empty value is used as the directory. Every spoken reply
//! then leaves three files there:
//!
//! - `<stamp>-source.wav`: the bytes the server sent, wrapped as a WAV (`audio/L16` PCM or a
//!   complete clip), i.e. what the provider produced;
//! - `<stamp>-played.wav`: the mono samples handed to the player after decoding/averaging, i.e. what
//!   the desktop itself played;
//! - `<stamp>-meta.json`: the reply text, language, rate, channels and sizes, to tell recordings
//!   apart.
//!
//! Comparing the first two answers "is the fade in the source or in the app?" without listening to
//! a provider dashboard. The same env var is only read when a reply is spoken, so turning it off
//! takes a restart, not a rebuild.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Local;

/// The reply that is about to be spoken, captured to disk when dumping is switched on.
pub(crate) struct SpeechDump {
    dir: PathBuf,
    stem: String,
    text: String,
    language: Option<String>,
}

impl SpeechDump {
    /// Opens a dump for one reply, or `None` when `GAIA_SPEECH_DUMP` is unset/off.
    pub(crate) fn from_env(text: &str, language: Option<&str>) -> Option<Self> {
        let dir = dump_dir(&std::env::var("GAIA_SPEECH_DUMP").ok()?)?;
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("gaia speech dump: cannot create {}: {e}", dir.display());
            return None;
        }
        let stamp = Local::now().format("%Y%m%d-%H%M%S-%3f");
        let language = language.map(str::to_string);
        let stem = match &language {
            Some(language) => format!("{stamp}-{language}"),
            None => stamp.to_string(),
        };
        Some(Self {
            dir,
            stem,
            text: text.to_string(),
            language,
        })
    }

    /// A complete clip as the server sent it (wav, mp3, …).
    pub(crate) fn write_clip(&self, audio: &[u8], mime_type: &str) {
        let extension = match mime_type.split(';').next().unwrap_or("").trim() {
            "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
            "audio/mpeg" | "audio/mp3" => "mp3",
            "audio/ogg" | "audio/opus" => "ogg",
            "audio/flac" => "flac",
            _ => "bin",
        };
        let path = self.dir.join(format!("{}-source.{extension}", self.stem));
        self.write_bytes(&path, audio);
        self.write_meta(
            serde_json::json!({
                "kind": "clip",
                "mime_type": mime_type,
                "bytes": audio.len(),
            }),
        );
    }

    /// A streamed reply: the raw PCM16 the server sent plus the samples the player was given.
    pub(crate) fn write_stream(
        &self,
        source: &[u8],
        played: &[f32],
        sample_rate: u32,
        channels: u16,
        outcome: &str,
    ) {
        let source_path = self.dir.join(format!("{}-source.wav", self.stem));
        self.write_bytes(&source_path, &wav_from_pcm16(source, sample_rate, channels));

        let played_path = self.dir.join(format!("{}-played.wav", self.stem));
        self.write_bytes(&played_path, &wav_from_f32(played, sample_rate));

        self.write_meta(
            serde_json::json!({
                "kind": "stream",
                "sample_rate": sample_rate,
                "channels": channels,
                "source_bytes": source.len(),
                "played_samples": played.len(),
                "played_seconds": played.len() as f64 / sample_rate.max(1) as f64,
                "outcome": outcome,
            }),
        );
    }

    fn write_meta(&self, details: serde_json::Value) {
        let path = self.dir.join(format!("{}-meta.json", self.stem));
        let mut meta = details;
        meta["created"] = serde_json::Value::String(Local::now().to_rfc3339());
        meta["text"] = serde_json::Value::String(self.text.clone());
        meta["language"] = match &self.language {
            Some(language) => serde_json::Value::String(language.clone()),
            None => serde_json::Value::Null,
        };
        let body = serde_json::to_vec_pretty(&meta).unwrap_or_default();
        self.write_bytes(&path, &body);
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) {
        match std::fs::File::create(path).and_then(|mut file| file.write_all(bytes)) {
            Ok(()) => eprintln!("gaia speech dump: wrote {}", path.display()),
            Err(e) => eprintln!("gaia speech dump: cannot write {}: {e}", path.display()),
        }
    }
}

/// Where a dump goes: unset/off is `None`, `1`/`true`/`on` is a folder in the temp directory, and
/// anything else is the folder itself.
fn dump_dir(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("0") || value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("off") {
        return None;
    }
    if value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("on") {
        return Some(std::env::temp_dir().join("gaia-speech-dump"));
    }
    Some(PathBuf::from(value))
}

/// A mono 16-bit WAV of the samples the player was given.
fn wav_from_f32(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16).to_le_bytes())
        .collect();
    wav_bytes(&pcm, sample_rate, 1, 16)
}

/// A WAV around raw PCM16LE as the server sent it, keeping its own rate and channel count — so the
/// file plays at the speed it was meant to, even when the desktop later resamples it.
fn wav_from_pcm16(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let channels = channels.max(1);
    wav_bytes(pcm, sample_rate, channels, 16)
}

fn wav_bytes(data: &[u8], sample_rate: u32, channels: u16, bits: u16) -> Vec<u8> {
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * block_align as u32;
    let data_len = data.len() as u32;
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumping_is_off_unless_asked_for() {
        assert!(dump_dir("").is_none());
        assert!(dump_dir("0").is_none());
        assert!(dump_dir("false").is_none());
        assert_eq!(dump_dir("1"), Some(std::env::temp_dir().join("gaia-speech-dump")));
        assert_eq!(dump_dir(" C:\\gaia-audio "), Some(PathBuf::from("C:\\gaia-audio")));
    }

    #[test]
    fn a_played_clip_is_a_mono_16_bit_wav() {
        let wav = wav_from_f32(&[0.0, 0.5, -0.5], 24_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 24_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16, "16-bit");
        let data = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        assert_eq!(data, 6, "three samples, two bytes each");
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), 0);
        assert!((i16::from_le_bytes([wav[46], wav[47]]) - 16_384).abs() <= 1);
    }

    #[test]
    fn a_stream_wav_keeps_the_servers_rate_and_channels() {
        let wav = wav_from_pcm16(&[1, 0, 2, 0], 48_000, 2);
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 2);
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 48_000);
        assert_eq!(u16::from_le_bytes([wav[32], wav[33]]), 4, "block align");
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 4);
    }
}
