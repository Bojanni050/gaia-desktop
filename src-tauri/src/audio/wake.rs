//! The wake word: "Hey Gaia", recognised on this device only.
//!
//! What the detector is: Rustpotter's reference mode. It compares the last second or so of audio with a
//! few recordings of *you* saying the phrase (their mel-frequency features, to be exact), so it is
//! enrolled by the user and speaker-dependent. Phase 0 measured it on the owner's voice and room: every
//! "Hey Gaia" caught at the loosest setting, 97% at the default, no false accepts in 10 minutes of quiet
//! room noise or 10 minutes of television dialogue, but near-misses such as "Gaia" alone or "hey Maya" do
//! fire it, which is why a short chime follows every detection (a false accept is then cheap and obvious).
//!
//! Scores are not absolute: they shift with how many clips were enrolled and how tightly they were trimmed
//! (on the owner's recordings a fixed 0.55 caught 76% with one enrolment and 7% with another). So enrolment
//! calibrates itself: every clip is scored against a reference built from the *others*, and the sensitivity
//! is set just below where the user's own phrase scores. The slider then starts from that value.
//!
//! Privacy: detection is entirely local and nothing leaves the device until the phrase is heard. The
//! enrolment recordings live in memory only; what is saved is the reference file, which holds features
//! (numbers), not audio, and can be deleted at any time.
//!
//! Rustpotter pulls a stale `candle-core` that only compiles with the `half` crate pinned to 2.4.1: the
//! pin lives in `Cargo.lock`, and `cargo update` without `--precise` would lift it.

use std::collections::HashMap;
use std::path::PathBuf;

use rustpotter::{
    AudioFmt, DetectorConfig, Endianness, Rustpotter, RustpotterConfig, SampleFormat, WakewordRef,
    WakewordRefBuildFromBuffers, WakewordSave,
};
use serde::Serialize;

use super::utterance::{self, rms, TARGET_RATE};

/// Clips needed to enrol (Rustpotter documents 3 to 8; fewer than 5 was too loose on the owner's voice).
pub const MIN_CLIPS: usize = 5;
pub const MAX_CLIPS: usize = 8;
/// The spoken phrase itself (without the 0.2 s margin trimming leaves on each side) is between these lengths.
const MIN_PHRASE_SECS: f32 = 0.35;
const MAX_PHRASE_SECS: f32 = 1.4;
/// Margin [`utterance::speech_range`] keeps on each side of the speech.
const MARGIN_SECS: f32 = 0.2;
/// Quieter than this and the microphone is too far away or too low.
const MIN_PEAK: f32 = 0.05;
/// The phrase must be at least this many times louder than the rest of the clip (12 dB).
const MIN_SNR: f32 = 4.0;
/// The loud part of the clip must stand out from its quiet part by at least this factor, or there is no
/// speech in it, just steady noise (a fan, hiss) that happens to be above the floor.
const MIN_CONTRAST: f32 = 3.0;

/// Used only when there is nothing to calibrate from; enrolment sets the real value for each user.
pub const DEFAULT_SENSITIVITY: f32 = 0.50;
/// How far below the lower end of the user's own phrase scores the threshold is set.
const CALIBRATION_MARGIN: f32 = 0.03;
/// Trimming leaves this much audio around the phrase in an enrolment clip (tighter references
/// separated the phrase from near-misses best on the owner's recordings).
const ENROL_MARGIN_SECS: f32 = 0.05;
pub const MIN_SENSITIVITY: f32 = 0.40;
pub const MAX_SENSITIVITY: f32 = 0.70;

/// Something that listens to a stream of 16 kHz audio and says when the wake word was spoken.
pub trait WakeSpotter: Send {
    /// Feeds the next audio (any length); true when the wake word was just heard.
    fn push(&mut self, audio: &[f32]) -> bool;
    fn reset(&mut self);
}

/// Detector threshold and its companion (the score against the averaged template, kept at half) for a
/// 0.40..0.70 sensitivity: higher is stricter.
pub fn thresholds(sensitivity: f32) -> (f32, f32) {
    let s = if sensitivity.is_nan() { DEFAULT_SENSITIVITY } else { sensitivity };
    let s = s.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);
    (s, s / 2.0)
}

pub struct RustpotterSpotter {
    detector: Rustpotter,
    frame: usize,
    pending: Vec<i16>,
}

impl RustpotterSpotter {
    /// Builds a detector from a saved reference (see [`WakeStore`]).
    pub fn from_reference(bytes: &[u8], sensitivity: f32) -> Result<Self, String> {
        let (threshold, avg_threshold) = thresholds(sensitivity);
        let config = RustpotterConfig {
            fmt: AudioFmt {
                sample_rate: TARGET_RATE as usize,
                sample_format: SampleFormat::I16,
                channels: 1,
                endianness: Endianness::Little,
            },
            detector: DetectorConfig { threshold, avg_threshold, ..DetectorConfig::default() },
            ..RustpotterConfig::default()
        };
        let mut detector = Rustpotter::new(&config)?;
        detector.add_wakeword_from_buffer("hey_gaia", bytes)?;
        let frame = detector.get_samples_per_frame();
        Ok(Self { detector, frame, pending: Vec::new() })
    }
}

impl WakeSpotter for RustpotterSpotter {
    fn push(&mut self, audio: &[f32]) -> bool {
        self.pending
            .extend(audio.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16));
        let mut heard = false;
        while self.pending.len() >= self.frame {
            let frame: Vec<i16> = self.pending.drain(..self.frame).collect();
            if self.detector.process_samples::<i16>(frame).is_some() {
                heard = true;
            }
        }
        heard
    }

    fn reset(&mut self) {
        self.pending.clear();
        self.detector.reset();
    }
}

// ------------------------------------------------------------------------------------------------
// Enrolment
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipProblem {
    /// Nothing was recorded.
    Empty,
    /// Too quiet: move closer to the microphone or speak up.
    TooQuiet,
    /// No speech found.
    NoSpeech,
    /// Shorter than the phrase can be: cut off.
    TooShort,
    /// Longer than the phrase: more was said than "Hey Gaia".
    TooLong,
    /// The room is too loud compared to the voice.
    Noisy,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipCheck {
    pub ok: bool,
    pub problem: Option<ClipProblem>,
    /// Length of the phrase (trimmed to the speech) in seconds.
    pub seconds: f32,
    pub peak: f32,
}

fn rejected(problem: ClipProblem, seconds: f32, peak: f32) -> (ClipCheck, Option<Vec<i16>>) {
    (ClipCheck { ok: false, problem: Some(problem), seconds, peak }, None)
}

/// Judges one enrolment recording (mono, 16 kHz). Returns the verdict and, when it is good, the clip trimmed
/// to the phrase as 16-bit samples.
pub fn check_clip(samples: &[f32]) -> (ClipCheck, Option<Vec<i16>>) {
    let peak = samples.iter().fold(0f32, |a, s| a.max(s.abs()));
    if samples.len() < (TARGET_RATE as usize) / 4 {
        return rejected(ClipProblem::Empty, 0.0, peak);
    }
    if peak < MIN_PEAK {
        return rejected(ClipProblem::TooQuiet, 0.0, peak);
    }
    // Steady noise is not speech: a voice is clearly louder than the quiet part of the same recording.
    let mut levels: Vec<f32> = samples.chunks_exact(320).map(rms).collect();
    levels.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let (quiet, loud) = (levels[levels.len() / 5], levels[levels.len() * 9 / 10]);
    if loud < MIN_CONTRAST * quiet.max(1e-4) {
        return rejected(ClipProblem::NoSpeech, 0.0, peak);
    }
    let Some(range) = utterance::speech_range(samples, TARGET_RATE) else {
        return rejected(ClipProblem::NoSpeech, 0.0, peak);
    };
    let inside = &samples[range.clone()];
    let seconds = (inside.len() as f32 / TARGET_RATE as f32 - 2.0 * MARGIN_SECS).max(0.0);
    if seconds < MIN_PHRASE_SECS {
        return rejected(ClipProblem::TooShort, seconds, peak);
    }
    if seconds > MAX_PHRASE_SECS {
        return rejected(ClipProblem::TooLong, seconds, peak);
    }
    let outside: Vec<f32> = samples[..range.start].iter().chain(&samples[range.end..]).copied().collect();
    if !outside.is_empty() && rms(inside) < MIN_SNR * rms(&outside) {
        return rejected(ClipProblem::Noisy, seconds, peak);
    }
    // Keep only a little margin: speech_range leaves 0.2 s, a reference built from tight clips tells the
    // phrase from near-misses better.
    let shave = ((MARGIN_SECS - ENROL_MARGIN_SECS) * TARGET_RATE as f32) as usize;
    let tight = if inside.len() > 2 * shave + TARGET_RATE as usize / 4 { &inside[shave..inside.len() - shave] } else { inside };
    let pcm = tight.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16).collect();
    (ClipCheck { ok: true, problem: None, seconds, peak }, Some(pcm))
}

/// The reference file for these clips (no checks).
fn reference_bytes(clips: &[Vec<i16>]) -> Result<Vec<u8>, String> {
    let samples: HashMap<String, Vec<u8>> = clips
        .iter()
        .take(MAX_CLIPS)
        .enumerate()
        .map(|(i, pcm)| (format!("hey_gaia_{i:02}.wav"), utterance::wav_bytes(pcm)))
        .collect();
    let reference = WakewordRef::new_from_sample_buffers("hey_gaia".into(), None, None, samples, 16)?;
    reference.save_to_buffer()
}

/// A clip as the live detector meets it: speech in the middle of quiet, with room to finish.
fn padded(clip: &[i16]) -> Vec<f32> {
    let mut audio = vec![0.0f32; TARGET_RATE as usize];
    audio.extend(clip.iter().map(|s| *s as f32 / 32_768.0));
    audio.extend(std::iter::repeat(0.0).take(TARGET_RATE as usize * 3 / 2));
    audio
}

/// How well `reference` recognises `clip`: the best detection score (0..1), or `None` when it does not hear
/// the phrase at all. Scored at the loosest setting so the number, not a yes/no, comes back.
pub fn score_clip(reference: &[u8], clip: &[i16]) -> Option<f32> {
    let config = RustpotterConfig {
        fmt: AudioFmt {
            sample_rate: TARGET_RATE as usize,
            sample_format: SampleFormat::I16,
            channels: 1,
            endianness: Endianness::Little,
        },
        detector: DetectorConfig { threshold: 0.05, avg_threshold: 0.0, ..DetectorConfig::default() },
        ..RustpotterConfig::default()
    };
    let mut detector = Rustpotter::new(&config).ok()?;
    detector.add_wakeword_from_buffer("hey_gaia", reference).ok()?;
    let frame = detector.get_samples_per_frame();
    let audio: Vec<i16> = padded(clip).iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16).collect();
    // The detector's running best score is read directly: at a loose threshold it never settles on a
    // final detection, but the strongest window it has seen is exactly the number wanted.
    let mut best: Option<f32> = None;
    for chunk in audio.chunks_exact(frame) {
        let _ = detector.process_samples::<i16>(chunk.to_vec());
        if let Some(partial) = detector.get_partial_detection() {
            best = Some(best.map_or(partial.score, |b: f32| b.max(partial.score)));
        }
    }
    best
}

/// A finished enrolment: the reference to save and the sensitivity calibrated for this voice.
#[derive(Debug, Clone, PartialEq)]
pub struct Enrolled {
    pub reference: Vec<u8>,
    pub sensitivity: f32,
}

/// Builds the wake word from the enrolment clips and calibrates it to the user's own voice.
///
/// Each clip is scored against a reference built from all the *other* clips (how a fresh "Hey Gaia" would
/// score), and the sensitivity is set a little below the lower end of those scores. If too many clips are
/// not recognised even by their siblings, the recordings are too unlike each other to enrol.
pub fn enroll(clips: &[Vec<i16>]) -> Result<Enrolled, String> {
    if clips.len() < MIN_CLIPS {
        return Err(format!("need at least {MIN_CLIPS} recordings, have {}", clips.len()));
    }
    let clips = &clips[..clips.len().min(MAX_CLIPS)];

    // Leave-one-out, one thread per clip.
    let scores: Vec<Option<f32>> = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..clips.len())
            .map(|i| {
                scope.spawn(move || {
                    let others: Vec<Vec<i16>> =
                        clips.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| c.clone()).collect();
                    reference_bytes(&others).ok().and_then(|r| score_clip(&r, &clips[i]))
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap_or(None)).collect()
    });
    let mut heard: Vec<f32> = scores.iter().flatten().copied().collect();
    if heard.len() * 5 < clips.len() * 4 {
        return Err(format!(
            "the detector recognised only {} of {} recordings from the others: they differ too much from each other",
            heard.len(),
            clips.len()
        ));
    }
    heard.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lower_end = heard[(heard.len() - 1) / 5];
    let sensitivity = (lower_end - CALIBRATION_MARGIN).clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);

    let reference = reference_bytes(clips)?;
    Ok(Enrolled { reference, sensitivity })
}

/// Builds the reference file from the enrolment clips and checks it against its own clips at `sensitivity`
/// (kept for callers that fix the sensitivity; enrolment uses [`enroll`]).
pub fn build_reference(clips: &[Vec<i16>], sensitivity: f32) -> Result<Vec<u8>, String> {
    if clips.len() < MIN_CLIPS {
        return Err(format!("need at least {MIN_CLIPS} recordings, have {}", clips.len()));
    }
    let bytes = reference_bytes(clips)?;

    let mut heard = 0;
    for clip in clips.iter().take(MAX_CLIPS) {
        let mut spotter = RustpotterSpotter::from_reference(&bytes, sensitivity)?;
        let mut padded = vec![0.0f32; TARGET_RATE as usize];
        padded.extend(clip.iter().map(|s| *s as f32 / 32_768.0));
        padded.extend(std::iter::repeat(0.0).take(TARGET_RATE as usize * 3 / 2));
        if spotter.push(&padded) {
            heard += 1;
        }
    }
    let total = clips.len().min(MAX_CLIPS);
    if heard * 5 < total * 4 {
        return Err(format!(
            "the detector only recognised {heard} of its own {total} recordings: they differ too much from each other"
        ));
    }
    Ok(bytes)
}

// ------------------------------------------------------------------------------------------------
// The saved reference
// ------------------------------------------------------------------------------------------------

/// Where the reference file lives. It holds features, never audio.
pub struct WakeStore {
    path: PathBuf,
}

impl WakeStore {
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self { path: config_dir.unwrap_or_else(std::env::temp_dir).join("wake").join("hey_gaia.rpw") }
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    pub fn load(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.path).ok().filter(|b| !b.is_empty())
    }

    pub fn save(&self, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.path, bytes).map_err(|e| e.to_string())
    }

    /// Forgets the wake word: the file is deleted.
    pub fn delete(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// The wake-up log
// ------------------------------------------------------------------------------------------------

/// Counts every "Hey Gaia" wake-up and writes one line per wake to `<config>/wake/wake-log.txt` (and to
/// stderr), so the false-accept rate can be measured by leaving the app running: a count of wake-ups nobody
/// asked for, over an hour. A line holds only the time and the count, never audio or words.
pub struct WakeLog {
    path: PathBuf,
    state: std::sync::Mutex<LogState>,
}

struct LogState {
    count: u64,
    started: std::time::Instant,
    last: Option<std::time::Instant>,
}

/// "4m12s", "1h03m05s", "7s".
fn span(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

impl WakeLog {
    pub fn new(config_dir: Option<PathBuf>) -> Self {
        Self {
            path: config_dir.unwrap_or_else(std::env::temp_dir).join("wake").join("wake-log.txt"),
            state: std::sync::Mutex::new(LogState { count: 0, started: std::time::Instant::now(), last: None }),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Wake-ups since this launch.
    pub fn count(&self) -> u64 {
        self.state.lock().map(|s| s.count).unwrap_or(0)
    }

    /// Records one wake-up and returns the line written, e.g.
    /// `2026-10-12 14:03:22 wake #3 (4m12s since the previous, 18m40s since start)`.
    pub fn record(&self) -> String {
        let now = std::time::Instant::now();
        let (n, since_start, since_last) = {
            let mut state = match self.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            state.count += 1;
            let since_last = state.last.map(|t| now.duration_since(t).as_secs());
            state.last = Some(now);
            (state.count, now.duration_since(state.started).as_secs(), since_last)
        };
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = match since_last {
            Some(gap) => format!("{stamp} wake #{n} ({} since the previous, {} since start)", span(gap), span(since_start)),
            None => format!("{stamp} wake #{n} ({} since start)", span(since_start)),
        };
        eprintln!("[wake] {line}");
        self.append(n == 1, &line);
        line
    }

    fn append(&self, first: bool, line: &str) {
        use std::io::Write;
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            if first {
                let _ = writeln!(file, "--- app started, wake-up log ---");
            }
            let _ = writeln!(file, "{line}");
        }
    }
}

// ------------------------------------------------------------------------------------------------
// The confirmation chime
// ------------------------------------------------------------------------------------------------

/// A short, soft two-note rise ("I heard you") as a WAV: quiet enough not to startle, brief enough not to
/// get in the way, so a false accept costs a fifth of a second of attention.
pub fn chime_wav() -> Vec<u8> {
    let note = |hz: f32, ms: usize| -> Vec<i16> {
        let n = TARGET_RATE as usize * ms / 1000;
        (0..n)
            .map(|i| {
                let t = i as f32 / TARGET_RATE as f32;
                let fade = ((i as f32 / (n as f32 * 0.15)).min(1.0)) * (((n - i) as f32 / (n as f32 * 0.5)).min(1.0));
                (0.22 * fade * (std::f32::consts::TAU * hz * t).sin() * 32_767.0) as i16
            })
            .collect()
    };
    let mut pcm = note(880.0, 80);
    pcm.extend(note(1_318.5, 110));
    utterance::wav_bytes(&pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = 16_000;

    /// A synthetic "word": harmonics of a gliding pitch under a smooth envelope, with a little room noise.
    fn word(f0: f32, vibrato: f32, secs: f32, seed: usize) -> Vec<f32> {
        let n = (SR as f32 * secs) as usize;
        let mut ph = 0f32;
        (0..n)
            .map(|i| {
                let env = (std::f32::consts::PI * i as f32 / n as f32).sin().powi(2);
                ph += std::f32::consts::TAU * f0 * (1.0 + vibrato * (i as f32 / 900.0).sin()) / SR as f32;
                let s: f32 = (1..=5).map(|h| (ph * h as f32).sin() / h as f32).sum();
                0.25 * env * s + 0.002 * (((i + seed) * 7919 % 2000) as f32 / 1000.0 - 1.0)
            })
            .collect()
    }

    /// A recording: lead-in room noise, the word, tail room noise.
    fn recording(f0: f32, vibrato: f32, word_secs: f32, seed: usize) -> Vec<f32> {
        let room = |secs: f32| -> Vec<f32> {
            (0..(SR as f32 * secs) as usize).map(|i| 0.002 * (((i + seed) * 104_729 % 2000) as f32 / 1000.0 - 1.0)).collect()
        };
        let mut v = room(0.8);
        v.extend(word(f0, vibrato, word_secs, seed));
        v.extend(room(0.8));
        v
    }

    fn good_clips(n: usize) -> Vec<Vec<i16>> {
        (0..n)
            .filter_map(|i| check_clip(&recording(150.0 + i as f32 * 1.5, 0.03, 1.0, i)).1)
            .collect()
    }

    // --- clip checks ------------------------------------------------------------------------------

    #[test]
    fn a_clean_recording_of_the_phrase_is_accepted_and_trimmed_to_it() {
        let (check, pcm) = check_clip(&recording(150.0, 0.03, 1.0, 1));
        assert!(check.ok, "{check:?}");
        let secs = pcm.unwrap().len() as f32 / SR as f32;
        assert!((0.7..1.3).contains(&secs), "the phrase with a little margin, not the whole 2.6 s: {secs}");
    }

    #[test]
    fn nothing_recorded_is_empty() {
        assert_eq!(check_clip(&[]).0.problem, Some(ClipProblem::Empty));
        assert_eq!(check_clip(&vec![0.0; 100]).0.problem, Some(ClipProblem::Empty));
    }

    #[test]
    fn a_whisper_from_across_the_room_is_too_quiet() {
        let quiet: Vec<f32> = recording(150.0, 0.03, 1.0, 1).iter().map(|s| s * 0.05).collect();
        assert_eq!(check_clip(&quiet).0.problem, Some(ClipProblem::TooQuiet));
    }

    #[test]
    fn silence_that_is_not_quite_silent_has_no_speech() {
        let hiss: Vec<f32> = (0..SR * 2).map(|i| 0.06 * ((i * 7919 % 200) as f32 / 100.0 - 1.0)).collect();
        assert_eq!(check_clip(&hiss).0.problem, Some(ClipProblem::NoSpeech));
    }

    #[test]
    fn a_cut_off_word_is_too_short() {
        let (check, _) = check_clip(&recording(150.0, 0.03, 0.25, 1));
        assert!(!check.ok);
        assert!(matches!(check.problem, Some(ClipProblem::TooShort) | Some(ClipProblem::NoSpeech)), "{check:?}");
    }

    #[test]
    fn a_whole_sentence_is_too_long_for_a_two_word_phrase() {
        assert_eq!(check_clip(&recording(150.0, 0.03, 2.4, 1)).0.problem, Some(ClipProblem::TooLong));
    }

    #[test]
    fn a_loud_room_around_a_voice_is_noisy() {
        let noisy: Vec<f32> = recording(150.0, 0.03, 1.0, 1)
            .iter()
            .enumerate()
            .map(|(i, s)| s + 0.12 * (((i * 31_337) % 2000) as f32 / 1000.0 - 1.0))
            .collect();
        let (check, _) = check_clip(&noisy);
        assert!(!check.ok, "{check:?}");
    }

    // --- thresholds ---------------------------------------------------------------------------------

    #[test]
    fn sensitivity_maps_to_the_phase_0_thresholds_and_is_clamped() {
        assert_eq!(thresholds(0.55), (0.55, 0.275));
        assert_eq!(thresholds(0.0).0, MIN_SENSITIVITY);
        assert_eq!(thresholds(5.0).0, MAX_SENSITIVITY);
        assert_eq!(thresholds(f32::NAN).0, DEFAULT_SENSITIVITY);
    }

    // --- building and detecting ---------------------------------------------------------------------

    #[test]
    fn it_refuses_to_enrol_with_too_few_recordings() {
        let clips = good_clips(MIN_CLIPS - 1);
        assert!(build_reference(&clips, DEFAULT_SENSITIVITY).unwrap_err().contains("at least"));
    }

    #[test]
    fn a_reference_built_from_good_recordings_hears_the_word_in_a_stream_and_not_another_word() {
        let clips = good_clips(MAX_CLIPS);
        assert_eq!(clips.len(), MAX_CLIPS, "all synthetic recordings pass the checks");
        let reference = build_reference(&clips, DEFAULT_SENSITIVITY).expect("builds and passes its own check");

        // the word, spoken again in the middle of a longer stream of room noise
        let mut stream = vec![0.0f32; SR];
        stream.extend(recording(152.0, 0.03, 1.0, 99));
        stream.extend(vec![0.0f32; SR * 2]);
        let mut spotter = RustpotterSpotter::from_reference(&reference, DEFAULT_SENSITIVITY).unwrap();
        assert!(spotter.push(&stream), "the enrolled word is heard");

        // something very unlike it (a flat-spectrum burst), and plain room noise. (Synthetic voiced sounds are
        // all alike to this detector; real near-miss words are measured on the owner's recordings.)
        let mut other = vec![0.0f32; SR];
        other.extend((0..SR).map(|i| 0.25 * ((i * 31_337 % 2000) as f32 / 1000.0 - 1.0)));
        other.extend(vec![0.0f32; SR * 2]);
        let mut spotter = RustpotterSpotter::from_reference(&reference, DEFAULT_SENSITIVITY).unwrap();
        assert!(!spotter.push(&other), "a different sound does not wake her");
        let mut spotter = RustpotterSpotter::from_reference(&reference, DEFAULT_SENSITIVITY).unwrap();
        let hiss: Vec<f32> = (0..SR * 5).map(|i| 0.003 * ((i * 7919 % 2000) as f32 / 1000.0 - 1.0)).collect();
        assert!(!spotter.push(&hiss), "room noise does not wake her");
    }

    #[test]
    fn a_stricter_sensitivity_rejects_what_a_looser_one_accepts() {
        let reference = build_reference(&good_clips(MAX_CLIPS), 0.5).unwrap();
        // a loose imitation: same word, different pitch glide
        let mut stream = vec![0.0f32; SR];
        stream.extend(recording(158.0, 0.07, 1.0, 7));
        stream.extend(vec![0.0f32; SR * 2]);
        let loose = RustpotterSpotter::from_reference(&reference, MIN_SENSITIVITY).unwrap().push(&stream);
        let strict = RustpotterSpotter::from_reference(&reference, MAX_SENSITIVITY).unwrap().push(&stream);
        assert!(loose || !strict, "strict must never accept what loose rejects");
    }

    #[test]
    fn recordings_that_are_too_unlike_each_other_do_not_enrol() {
        // five different "words": the detector cannot even recognise its own recordings
        let clips: Vec<Vec<i16>> = (0..MAX_CLIPS)
            .filter_map(|i| check_clip(&recording(120.0 + 60.0 * i as f32, 0.02 + 0.03 * i as f32, 1.0, i)).1)
            .collect();
        assert!(clips.len() >= MIN_CLIPS);
        // either it refuses (the intended result) or the clips happened to agree; it must never panic
        let _ = build_reference(&clips, DEFAULT_SENSITIVITY);
    }

    #[test]
    fn a_detector_cannot_be_built_from_garbage() {
        assert!(RustpotterSpotter::from_reference(&[1, 2, 3, 4], 0.5).is_err());
        assert!(RustpotterSpotter::from_reference(&[], 0.5).is_err());
    }

    #[test]
    fn push_accepts_chunks_of_any_size_and_reset_forgets() {
        let reference = build_reference(&good_clips(MAX_CLIPS), 0.5).unwrap();
        let mut stream = vec![0.0f32; SR];
        stream.extend(recording(150.0, 0.03, 1.0, 3));
        stream.extend(vec![0.0f32; SR * 2]);
        let mut spotter = RustpotterSpotter::from_reference(&reference, 0.5).unwrap();
        let mut heard = false;
        for chunk in stream.chunks(160) {
            heard |= spotter.push(chunk);
        }
        assert!(heard, "10 ms chunks work as well as one big one");
        spotter.reset();
        assert!(!spotter.push(&[]), "nothing after a reset");
    }

    // --- enrolment and calibration -----------------------------------------------------------------------

    #[test]
    fn enrolling_calibrates_a_sensitivity_inside_the_allowed_range() {
        let clips = good_clips(MAX_CLIPS);
        let enrolled = enroll(&clips).expect("enrols");
        assert!((MIN_SENSITIVITY..=MAX_SENSITIVITY).contains(&enrolled.sensitivity), "{}", enrolled.sensitivity);
        // the calibrated detector hears the phrase again, and not a flat-spectrum burst
        let mut stream = vec![0.0f32; SR];
        stream.extend(recording(152.0, 0.03, 1.0, 99));
        stream.extend(vec![0.0f32; SR * 2]);
        assert!(RustpotterSpotter::from_reference(&enrolled.reference, enrolled.sensitivity).unwrap().push(&stream));
        let mut burst = vec![0.0f32; SR];
        burst.extend((0..SR).map(|i| 0.25 * ((i * 31_337 % 2000) as f32 / 1000.0 - 1.0)));
        burst.extend(vec![0.0f32; SR * 2]);
        assert!(!RustpotterSpotter::from_reference(&enrolled.reference, enrolled.sensitivity).unwrap().push(&burst));
    }

    #[test]
    fn enrolling_refuses_too_few_recordings() {
        assert!(enroll(&good_clips(MIN_CLIPS - 1)).unwrap_err().contains("at least"));
        assert!(enroll(&[]).is_err());
    }

    #[test]
    fn recordings_that_cannot_recognise_each_other_do_not_enrol() {
        // eight unrelated sounds: noise bursts of different colour
        let clips: Vec<Vec<i16>> = (0..MAX_CLIPS)
            .map(|k| (0..SR).map(|i| (((i * (7919 + 104_729 * k)) % 20_000) as i32 - 10_000) as i16).collect())
            .collect();
        let _ = enroll(&clips); // must not panic; whether it refuses depends on the sounds
    }

    #[test]
    fn a_clip_is_scored_by_how_well_the_others_recognise_it() {
        let clips = good_clips(MAX_CLIPS);
        let reference = reference_bytes(&clips[1..]).unwrap();
        let same = score_clip(&reference, &clips[0]).expect("the phrase is heard");
        assert!(same > 0.3 && same <= 1.0, "{same}");
        let burst: Vec<i16> = (0..SR).map(|i| (((i * 31_337) % 20_000) as i32 - 10_000) as i16).collect();
        match score_clip(&reference, &burst) {
            None => {}
            Some(other) => assert!(other < same, "a burst of noise must score below the phrase ({other} vs {same})"),
        }
    }

    // --- the saved reference -------------------------------------------------------------------------

    #[test]
    fn the_store_saves_loads_and_deletes_and_holds_no_audio() {
        let dir = std::env::temp_dir().join(format!("gaia-wake-test-{}", std::process::id()));
        let store = WakeStore::new(Some(dir.clone()));
        assert!(!store.exists());
        assert!(store.load().is_none());

        let bytes = build_reference(&good_clips(MAX_CLIPS), 0.5).unwrap();
        store.save(&bytes).unwrap();
        assert!(store.exists());
        assert_eq!(store.load().unwrap(), bytes);
        // features, not audio: it is no WAV, and far smaller than eight seconds of 16-bit audio
        assert_ne!(&bytes[0..4], b"RIFF");
        assert!(bytes.len() < MAX_CLIPS * 32_000 * 2);

        store.delete().unwrap();
        assert!(!store.exists());
        store.delete().unwrap(); // deleting twice is fine
        let _ = std::fs::remove_dir_all(dir);
    }

    // --- the chime ------------------------------------------------------------------------------------

    #[test]
    fn the_chime_is_short_soft_and_starts_and_ends_gently() {
        let wav = chime_wav();
        assert_eq!(&wav[0..4], b"RIFF");
        let pcm: Vec<i16> = wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        let secs = pcm.len() as f32 / SR as f32;
        assert!((0.15..0.30).contains(&secs), "{secs} s");
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap() as f32 / 32_768.0;
        assert!(peak > 0.05 && peak < 0.3, "soft: {peak}");
        assert!(pcm[0].unsigned_abs() < 500 && pcm[pcm.len() - 1].unsigned_abs() < 500, "no click at either end");
    }

    // --- the owner's real recordings ----------------------------------------------------------------

    /// 16 kHz mono 16-bit PCM from a WAV file made by the Phase 0 harness.
    fn read_wav(path: &std::path::Path) -> Vec<f32> {
        let bytes = std::fs::read(path).unwrap();
        let at = bytes.windows(4).position(|w| w == b"data").expect("data chunk") + 8;
        bytes[at..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0).collect()
    }

    fn wavs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "wav").unwrap_or(false)).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    fn pad(clip: &[f32]) -> Vec<f32> {
        let mut v = vec![0.0f32; SR];
        v.extend_from_slice(clip);
        v.extend(std::iter::repeat(0.0).take(SR * 3 / 2));
        v
    }

    /// Opt-in: `GAIA_WAKE_DATA=<dir with hey/ neg/ bg/ bgs/> cargo test --lib real_recordings -- --ignored --nocapture`.
    /// Enrols from the owner's own "Hey Gaia" recordings through the product's enrolment (checks, leave-one-out
    /// calibration), then measures the finished detector on the rest: the Phase 4 acceptance numbers. The
    /// folders hold the owner's voice and are never committed.
    #[test]
    #[ignore = "needs the owner's recordings (GAIA_WAKE_DATA)"]
    fn real_recordings_enrol_and_detect_at_the_calibrated_sensitivity() {
        let root = std::path::PathBuf::from(
            std::env::var("GAIA_WAKE_DATA")
                .unwrap_or_else(|_| r"K:\Projects\Project Gaia\gaia-desktop\spikes\voice-phase0\recordings".into()),
        );
        // 1. which of the owner's recordings would the enrolment screen accept?
        let mut good = Vec::new();
        let mut rejected = Vec::new();
        for path in wavs(&root.join("hey")) {
            let (check, pcm) = check_clip(&read_wav(&path));
            match pcm {
                Some(pcm) => good.push(pcm),
                None => rejected.push((path.file_name().unwrap().to_string_lossy().to_string(), check.problem)),
            }
        }
        println!("hey recordings: {} accepted, {} rejected: {:?}", good.len(), rejected.len(), rejected);
        assert!(good.len() >= MAX_CLIPS + 2, "need enough good recordings to hold some out");

        // 2. enrol from eight recordings spread across the whole session (as the enrolment screen asks for:
        //    different volume, speed and distance), hold the other recordings out
        let step = good.len() / MAX_CLIPS;
        let enrol_idx: Vec<usize> = (0..MAX_CLIPS).map(|k| k * step).collect();
        let enrol_clips: Vec<Vec<i16>> = enrol_idx.iter().map(|i| good[*i].clone()).collect();
        let held_out: Vec<Vec<i16>> = good.iter().enumerate().filter(|(i, _)| !enrol_idx.contains(i)).map(|(_, c)| c.clone()).collect();
        let started = std::time::Instant::now();
        let enrolled = enroll(&enrol_clips).expect("the owner can enrol");
        println!(
            "enrolled in {:.1} s: calibrated sensitivity {:.2}, reference {} bytes (features, no audio)",
            started.elapsed().as_secs_f32(),
            enrolled.sensitivity,
            enrolled.reference.len()
        );

        let hears = |audio: &[f32]| -> bool {
            RustpotterSpotter::from_reference(&enrolled.reference, enrolled.sensitivity).unwrap().push(audio)
        };
        let hits = held_out
            .iter()
            .filter(|pcm| hears(&pad(&pcm.iter().map(|s| *s as f32 / 32_768.0).collect::<Vec<_>>())))
            .count();
        println!("held-out 'Hey Gaia': {hits} of {} heard ({:.0}%)", held_out.len(), 100.0 * hits as f32 / held_out.len() as f32);

        // 3. things that are not the phrase
        let negatives: Vec<_> = wavs(&root.join("neg"));
        let neg_fires = negatives.iter().filter(|p| hears(&pad(&read_wav(p)))).count();
        println!("near-miss phrases that woke her: {neg_fires} of {}", negatives.len());

        // 4. ambient room and television dialogue
        let mut ambient_secs = 0.0f32;
        let mut ambient_fires = 0usize;
        for folder in ["bg", "bgs"] {
            for path in wavs(&root.join(folder)) {
                let audio = read_wav(&path);
                let secs = audio.len() as f32 / SR as f32;
                let mut spotter = RustpotterSpotter::from_reference(&enrolled.reference, enrolled.sensitivity).unwrap();
                let mut fires = 0usize;
                for chunk in audio.chunks(160) {
                    if spotter.push(chunk) {
                        fires += 1;
                    }
                }
                println!("{folder}/{}: {secs:.0} s, {fires} false accept(s)", path.file_name().unwrap().to_string_lossy());
                ambient_secs += secs;
                ambient_fires += fires;
            }
        }
        let per_hour = if ambient_secs > 0.0 { ambient_fires as f32 / ambient_secs * 3600.0 } else { 0.0 };
        println!("ambient total: {ambient_secs:.0} s, {ambient_fires} false accept(s) = {per_hour:.1} per hour");

        assert!(hits as f32 >= 0.9 * held_out.len() as f32, "at least 90% of held-out phrases heard: {hits}/{}", held_out.len());
        assert!(per_hour <= 18.0, "no more than the ~18/hour Phase 0 could rule out: {per_hour}");
    }
    #[test]
    fn the_wake_log_counts_and_writes_one_line_per_wake_with_no_audio() {
        let dir = std::env::temp_dir().join(format!("gaia-wake-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = WakeLog::new(Some(dir.clone()));
        assert_eq!(log.count(), 0);
        let first = log.record();
        let second = log.record();
        assert_eq!(log.count(), 2);
        assert!(first.contains("wake #1") && second.contains("wake #2"));
        assert!(second.contains("since the previous"));
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert_eq!(text.lines().filter(|l| l.contains(" wake #")).count(), 2);
        assert!(text.starts_with("--- app started"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spans_read_naturally() {
        assert_eq!(span(7), "7s");
        assert_eq!(span(252), "4m12s");
        assert_eq!(span(3785), "1h03m05s");
    }
}
