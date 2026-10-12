//! Turning a recording into what Gaia Cloud's speech recognition expects: one utterance, trimmed to the
//! speech, as 16 kHz mono 16-bit WAV.
//!
//! Everything here is a pure function over in-memory samples. Two privacy rules live in this file:
//! a recording with no speech in it is [`Prepared::Silence`] and is **never uploaded**, and what *is*
//! uploaded is only the part of the recording around the speech, not the room noise before and after it.

use std::ops::Range;

/// The rate Cloud's recognition is fed (and the desktop's whole voice pipeline works in).
pub const TARGET_RATE: u32 = 16_000;
/// A recording ends by itself after this long (Cloud accepts about 32 s of this format).
pub const MAX_RECORDING_SECS: u64 = 30;

const FRAME_MS: usize = 20;
/// Audio kept around the speech so a word's soft start and end are not clipped.
const MARGIN_MS: usize = 200;
/// Less speech than this is a cough or a click, not an utterance.
const MIN_SPEECH_MS: usize = 120;
/// Frames quieter than this (rms, full scale 1.0) are never speech, however quiet the room.
const ABSOLUTE_FLOOR: f32 = 0.006;

/// What a recording became.
#[derive(Debug, Clone, PartialEq)]
pub enum Prepared {
    /// No speech found: nothing is sent anywhere.
    Silence,
    /// The trimmed utterance, ready to upload.
    Speech { wav: Vec<u8>, seconds: f32 },
}

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// The sample range holding the speech (plus margin), or `None` when there is not enough of it.
///
/// A frame counts as speech when it is clearly above the room: three times the quiet end of the recording
/// (its 20th percentile), but never more than half of the loudest frame, so continuous speech with no gaps
/// still counts, and never below an absolute floor, so a silent room never does.
pub fn speech_range(samples: &[f32], rate: u32) -> Option<Range<usize>> {
    let frame = (rate as usize * FRAME_MS / 1000).max(1);
    let levels: Vec<f32> = samples.chunks_exact(frame).map(rms).collect();
    if levels.is_empty() {
        return None;
    }
    let mut sorted = levels.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise = sorted[(sorted.len() - 1) / 5];
    let peak = *sorted.last().expect("not empty");
    let threshold = (noise * 3.0).min(peak * 0.5).max(ABSOLUTE_FLOOR);

    let loud: Vec<usize> = levels
        .iter()
        .enumerate()
        .filter(|(_, l)| **l > threshold)
        .map(|(i, _)| i)
        .collect();
    if loud.len() * FRAME_MS < MIN_SPEECH_MS {
        return None;
    }
    let margin = rate as usize * MARGIN_MS / 1000;
    let start = (loud[0] * frame).saturating_sub(margin);
    let end = ((loud[loud.len() - 1] + 1) * frame + margin).min(samples.len());
    Some(start..end)
}

/// Mono samples at `rate` → 16 kHz 16-bit: a box filter (so higher rates do not alias) and linear interpolation.
pub fn to_16k_i16(samples: &[f32], rate: u32) -> Vec<i16> {
    if samples.is_empty() || rate == 0 {
        return Vec::new();
    }
    let width = (rate as f32 / TARGET_RATE as f32).round().max(1.0) as usize;
    let filtered: Vec<f32> = if width > 1 {
        let half = width / 2;
        (0..samples.len())
            .map(|i| {
                let a = i.saturating_sub(half);
                let b = (i + half + 1).min(samples.len());
                samples[a..b].iter().sum::<f32>() / (b - a) as f32
            })
            .collect()
    } else {
        samples.to_vec()
    };
    let out_len = (filtered.len() as u64 * TARGET_RATE as u64 / rate as u64) as usize;
    (0..out_len)
        .map(|k| {
            let pos = k as f64 * rate as f64 / TARGET_RATE as f64;
            let i = pos as usize;
            let frac = (pos - pos.floor()) as f32;
            let a = filtered[i.min(filtered.len() - 1)];
            let b = filtered[(i + 1).min(filtered.len() - 1)];
            ((a + (b - a) * frac).clamp(-1.0, 1.0) * 32_767.0) as i16
        })
        .collect()
}

/// A canonical 44-byte-header WAV (mono, 16 kHz, 16-bit PCM), built by hand: no file is ever involved.
pub fn wav_bytes(pcm: &[i16]) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + pcm.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&TARGET_RATE.to_le_bytes());
    wav.extend_from_slice(&(TARGET_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

/// A whole recording → the utterance to upload (or [`Prepared::Silence`]).
pub fn prepare(samples: &[f32], rate: u32) -> Prepared {
    let Some(range) = speech_range(samples, rate) else {
        return Prepared::Silence;
    };
    let pcm = to_16k_i16(&samples[range], rate);
    if pcm.is_empty() {
        return Prepared::Silence;
    }
    Prepared::Speech {
        seconds: pcm.len() as f32 / TARGET_RATE as f32,
        wav: wav_bytes(&pcm),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (RATE as f32 * secs) as usize]
    }

    /// Low-level deterministic room noise.
    fn hum(secs: f32, level: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize)
            .map(|i| level * ((i * 7919 % 2000) as f32 / 1000.0 - 1.0))
            .collect()
    }

    fn tone(secs: f32, hz: f32, level: f32) -> Vec<f32> {
        (0..(RATE as f32 * secs) as usize)
            .map(|i| level * (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin())
            .collect()
    }

    fn concat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.iter().flatten().copied().collect()
    }

    #[test]
    fn an_empty_or_silent_recording_is_silence_and_never_uploaded() {
        assert_eq!(prepare(&[], RATE), Prepared::Silence);
        assert_eq!(prepare(&silence(2.0), RATE), Prepared::Silence);
    }

    #[test]
    fn quiet_room_noise_alone_is_silence() {
        assert_eq!(prepare(&hum(3.0, 0.002), RATE), Prepared::Silence);
    }

    #[test]
    fn a_click_is_not_an_utterance() {
        let click = concat(&[silence(1.0), tone(0.04, 800.0, 0.5), silence(1.0)]);
        assert_eq!(prepare(&click, RATE), Prepared::Silence);
    }

    #[test]
    fn speech_in_a_quiet_room_is_trimmed_to_the_speech_plus_margin() {
        let rec = concat(&[hum(2.0, 0.002), tone(1.0, 220.0, 0.3), hum(2.0, 0.002)]);
        let range = speech_range(&rec, RATE).expect("speech found");
        let secs = (range.end - range.start) as f32 / RATE as f32;
        // 1.0 s of speech + 0.2 s margin on each side (give or take a frame)
        assert!((secs - 1.4).abs() < 0.1, "trimmed to {secs} s");
        assert!(range.start as f32 / RATE as f32 > 1.6, "most of the lead-in noise is dropped");
    }

    #[test]
    fn continuous_loud_speech_is_not_mistaken_for_noise() {
        let rec = tone(3.0, 180.0, 0.25);
        let range = speech_range(&rec, RATE).expect("speech found");
        assert!((range.end - range.start) as f32 / RATE as f32 > 2.8);
    }

    #[test]
    fn speech_over_a_noisy_room_is_still_found() {
        let rec = concat(&[hum(1.0, 0.02), tone(1.0, 220.0, 0.3), hum(1.0, 0.02)]);
        let range = speech_range(&rec, RATE).expect("speech found");
        let secs = (range.end - range.start) as f32 / RATE as f32;
        assert!(secs > 1.0 && secs < 2.0, "{secs}");
    }

    #[test]
    fn resampling_changes_the_rate_but_not_the_pitch() {
        let one_second = tone(1.0, 400.0, 0.5);
        let pcm = to_16k_i16(&one_second, RATE);
        assert!((pcm.len() as i32 - TARGET_RATE as i32).abs() <= 2, "{} samples", pcm.len());
        let crossings = pcm.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        // 400 Hz = 800 zero crossings per second
        assert!((crossings as i32 - 800).abs() < 20, "{crossings} zero crossings");
    }

    #[test]
    fn resampling_handles_the_rates_real_microphones_use() {
        for rate in [16_000u32, 44_100, 48_000] {
            let samples = vec![0.1f32; rate as usize / 2];
            let pcm = to_16k_i16(&samples, rate);
            assert!((pcm.len() as i32 - 8_000).abs() <= 2, "{rate}: {}", pcm.len());
        }
        assert!(to_16k_i16(&[], 48_000).is_empty());
        assert!(to_16k_i16(&[0.1], 0).is_empty());
    }

    #[test]
    fn the_wav_header_is_canonical_16k_mono_16_bit() {
        let wav = wav_bytes(&[1, -2, 3]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1, "PCM");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16, "bits");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]), 36 + 6);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), -2);
    }

    #[test]
    fn prepare_returns_a_small_trimmed_wav_for_real_speech() {
        let rec = concat(&[hum(3.0, 0.002), tone(1.5, 200.0, 0.3), hum(3.0, 0.002)]);
        match prepare(&rec, RATE) {
            Prepared::Speech { wav, seconds } => {
                assert!((seconds - 1.9).abs() < 0.15, "{seconds} s");
                // 16 kHz mono 16-bit: about 32 KB per second, nowhere near the 7.5 s that was recorded
                assert!(wav.len() < 70_000, "{} bytes", wav.len());
                assert_eq!(&wav[0..4], b"RIFF");
            }
            Prepared::Silence => panic!("speech must not be silence"),
        }
    }

    #[test]
    fn the_longest_recording_fits_cloud_s_upload_limit() {
        // 30 s at 16 kHz mono 16-bit = 960,000 bytes of PCM + header, under Cloud's 1 MiB (1,048,576)
        let bytes = MAX_RECORDING_SECS as usize * TARGET_RATE as usize * 2 + 44;
        assert!(bytes < 1024 * 1024);
    }
}
