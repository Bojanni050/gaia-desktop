//! Live speech segmentation: a continuous stream of 16 kHz audio in, finished utterances out.
//!
//! This is the hands-free counterpart of [`utterance::speech_range`], which judges a finished recording.
//! The same two privacy rules hold: only audio around real speech is ever kept, and a click or a cough
//! is dropped. Everything here is pure (no clock, no threads) so it is testable frame by frame.
//!
//! How it decides: a frame is speech when it is clearly above the room. The room is the quiet end of
//! the last second and a half of audio heard while nobody was speaking (never below an absolute floor,
//! so a silent room never triggers), and it is frozen while someone speaks, so speech cannot raise the
//! bar it is judged by. Nothing can start during the first half second, while the room is first being
//! measured. Speech must hold for a moment before an utterance starts (a click does not), and ends
//! after a pause long enough to be the end of a sentence rather than a breath. A short pre-roll is kept
//! so the first word is not clipped.
//!
//! Known limit: a loud, steady noise that begins mid-session (a vacuum cleaner) looks like speech until
//! the pause rule or the length limit ends it; the transcript filter downstream discards the garbage.

use std::collections::VecDeque;

use super::utterance::{rms, TARGET_RATE};

/// Samples per analysis frame (20 ms at 16 kHz).
pub const FRAME_SAMPLES: usize = 320;
const FRAME_MS: u32 = 20;

#[derive(Debug, Clone, Copy)]
pub struct SegmenterConfig {
    /// Speech must hold this long before an utterance starts.
    pub start_ms: u32,
    /// A pause this long ends the utterance.
    pub end_silence_ms: u32,
    /// Audio kept from before the start so the first word is not clipped.
    pub preroll_ms: u32,
    /// Silence kept after the last speech.
    pub tail_ms: u32,
    /// Less speech than this is a cough, not an utterance.
    pub min_speech_ms: u32,
    /// An utterance is cut at this length (Cloud accepts about 30 s).
    pub max_ms: u32,
    /// Frames quieter than this (rms) are never speech.
    pub floor: f32,
    /// Speech is this many times louder than the noise floor.
    pub ratio: f32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            start_ms: 120,
            end_silence_ms: 700,
            preroll_ms: 300,
            tail_ms: 200,
            min_speech_ms: 200,
            max_ms: 28_000,
            floor: 0.006,
            ratio: 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SegEvent {
    /// Speech has held long enough: an utterance is in progress.
    Started,
    /// A finished utterance (16 kHz mono), trimmed to pre-roll + speech + tail.
    Utterance(Vec<f32>),
    /// Speech that began and ended too quickly to be an utterance; discarded.
    TooShort,
}

/// Frames of room-level history used to estimate the noise floor (1.5 s).
const HISTORY_FRAMES: usize = 75;
/// Frames that must be heard before the first utterance may start (0.5 s).
const CALIBRATION_FRAMES: usize = 25;

pub struct Segmenter {
    cfg: SegmenterConfig,
    noise: f32,
    /// Levels of recent frames heard while nobody was speaking.
    history: VecDeque<f32>,
    preroll: VecDeque<Vec<f32>>,
    run: u32,
    speaking: bool,
    buf: Vec<f32>,
    speech_frames: u32,
    silence_frames: u32,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig) -> Self {
        Self {
            cfg,
            noise: cfg.floor / 3.0,
            history: VecDeque::new(),
            preroll: VecDeque::new(),
            run: 0,
            speaking: false,
            buf: Vec::new(),
            speech_frames: 0,
            silence_frames: 0,
        }
    }

    pub fn in_speech(&self) -> bool {
        self.speaking
    }

    /// How much audio the utterance in progress holds so far, preroll included (0 when idle). The
    /// utterance later ends with exactly these samples first, so a position noted now stays valid.
    pub fn buffered_samples(&self) -> usize {
        if self.speaking {
            self.buf.len()
        } else {
            0
        }
    }

    /// Changes how long a pause must last to end an utterance; applies from the next frame on.
    pub fn set_end_silence_ms(&mut self, ms: u32) {
        self.cfg.end_silence_ms = ms;
    }

    /// Milliseconds of speech so far in the current utterance (0 when idle).
    pub fn speech_ms(&self) -> u32 {
        if self.speaking {
            self.speech_frames * FRAME_MS
        } else {
            self.run * FRAME_MS
        }
    }

    /// Drops everything in progress (used while the microphone must not be listening).
    pub fn reset(&mut self) {
        self.history.clear();
        self.preroll.clear();
        self.run = 0;
        self.speaking = false;
        self.buf.clear();
        self.speech_frames = 0;
        self.silence_frames = 0;
    }

    fn threshold(&self) -> f32 {
        (self.noise * self.cfg.ratio).max(self.cfg.floor)
    }

    /// Feeds one [`FRAME_SAMPLES`]-long frame. At most one event per frame.
    pub fn push_frame(&mut self, frame: &[f32]) -> Option<SegEvent> {
        debug_assert_eq!(frame.len(), FRAME_SAMPLES);
        let level = rms(frame);

        // The room is the quiet end of what was heard while nobody was speaking. While someone is
        // speaking it is frozen, so speech cannot raise the bar it is judged by.
        if !self.speaking {
            self.history.push_back(level);
            if self.history.len() > HISTORY_FRAMES {
                self.history.pop_front();
            }
            if self.history.len() >= 10 {
                let mut sorted: Vec<f32> = self.history.iter().copied().collect();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                self.noise = sorted[sorted.len() / 5];
            }
        }
        let speech = level > self.threshold();
        let calibrated = self.history.len() >= CALIBRATION_FRAMES || self.speaking;

        if !self.speaking {
            self.preroll.push_back(frame.to_vec());
            let keep = ((self.cfg.preroll_ms + self.cfg.start_ms) / FRAME_MS) as usize + 1;
            while self.preroll.len() > keep {
                self.preroll.pop_front();
            }
            self.run = if speech && calibrated { self.run + 1 } else { 0 };
            if self.run * FRAME_MS >= self.cfg.start_ms {
                self.speaking = true;
                self.speech_frames = self.run;
                self.silence_frames = 0;
                self.buf = self.preroll.drain(..).flatten().collect();
                self.run = 0;
                return Some(SegEvent::Started);
            }
            return None;
        }

        self.buf.extend_from_slice(frame);
        if speech {
            self.speech_frames += 1;
            self.silence_frames = 0;
        } else {
            self.silence_frames += 1;
        }
        let too_long = self.buf.len() as u64 * 1000 / TARGET_RATE as u64 >= self.cfg.max_ms as u64;
        if self.silence_frames * FRAME_MS >= self.cfg.end_silence_ms || too_long {
            return Some(self.finish());
        }
        None
    }

    fn finish(&mut self) -> SegEvent {
        let speech_ms = self.speech_frames * FRAME_MS;
        let tail_frames = self.cfg.tail_ms / FRAME_MS;
        let drop_frames = self.silence_frames.saturating_sub(tail_frames) as usize;
        let keep = self.buf.len().saturating_sub(drop_frames * FRAME_SAMPLES);
        self.buf.truncate(keep);
        let utterance = std::mem::take(&mut self.buf);
        self.speaking = false;
        self.speech_frames = 0;
        self.silence_frames = 0;
        self.run = 0;
        if speech_ms < self.cfg.min_speech_ms {
            SegEvent::TooShort
        } else {
            SegEvent::Utterance(utterance)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> Vec<f32> {
        (0..FRAME_SAMPLES).map(|i| 0.001 * ((i * 7919 % 200) as f32 / 100.0 - 1.0)).collect()
    }

    fn voice(seed: usize) -> Vec<f32> {
        (0..FRAME_SAMPLES)
            .map(|i| 0.25 * ((i + seed * FRAME_SAMPLES) as f32 * 220.0 * std::f32::consts::TAU / 16_000.0).sin())
            .collect()
    }

    /// Feeds `frames` of speech or quiet; returns every event with the frame index it occurred on.
    fn feed(seg: &mut Segmenter, plan: &[(bool, usize)]) -> Vec<(usize, SegEvent)> {
        let mut events = Vec::new();
        let mut n = 0;
        for &(is_voice, count) in plan {
            for k in 0..count {
                let f = if is_voice { voice(n + k) } else { quiet() };
                if let Some(e) = seg.push_frame(&f) {
                    events.push((n + k, e));
                }
            }
            n += count;
        }
        events
    }

    #[test]
    fn a_quiet_room_produces_nothing_for_a_long_time() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        assert!(feed(&mut seg, &[(false, 2_000)]).is_empty());
        assert!(!seg.in_speech());
    }

    #[test]
    fn a_sentence_becomes_one_utterance_with_preroll_and_a_short_tail() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let events = feed(&mut seg, &[(false, 50), (true, 100), (false, 60)]);
        assert!(matches!(events[0].1, SegEvent::Started));
        let SegEvent::Utterance(samples) = &events[1].1 else { panic!("{events:?}") };
        let secs = samples.len() as f32 / 16_000.0;
        // 2.0 s of speech + 0.3 s preroll + 0.2 s tail, give or take a frame or two
        assert!((secs - 2.5).abs() < 0.12, "{secs} s");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn it_reports_that_speech_started_after_the_hold_time_not_before() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let events = feed(&mut seg, &[(false, 50), (true, 30)]);
        let (at, e) = &events[0];
        assert!(matches!(e, SegEvent::Started));
        assert_eq!(*at, 50 + 6 - 1, "120 ms of speech = 6 frames");
    }

    #[test]
    fn a_click_is_not_even_a_start() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        assert!(feed(&mut seg, &[(false, 50), (true, 3), (false, 100)]).is_empty());
    }

    #[test]
    fn a_cough_that_starts_an_utterance_but_is_too_short_is_dropped() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        // 8 frames = 160 ms: long enough to start, shorter than the 200 ms minimum
        let events = feed(&mut seg, &[(false, 50), (true, 8), (false, 80)]);
        assert!(matches!(events.first().map(|e| &e.1), Some(SegEvent::Started)));
        assert!(matches!(events.last().map(|e| &e.1), Some(SegEvent::TooShort)), "{events:?}");
    }

    #[test]
    fn a_breath_inside_a_sentence_does_not_split_it() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        // 400 ms pause (20 frames) is shorter than the 700 ms that ends an utterance
        let events = feed(&mut seg, &[(false, 50), (true, 50), (false, 20), (true, 50), (false, 60)]);
        let utterances: Vec<_> = events.iter().filter(|e| matches!(e.1, SegEvent::Utterance(_))).collect();
        assert_eq!(utterances.len(), 1, "{events:?}");
    }

    #[test]
    fn the_buffer_length_noted_mid_utterance_stays_valid_when_it_ends() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let events = feed(&mut seg, &[(false, 50), (true, 50)]);
        assert!(matches!(events.last().map(|e| &e.1), Some(SegEvent::Started)) || seg.in_speech());
        let noted = seg.buffered_samples();
        assert!(noted > 0);
        let events = feed(&mut seg, &[(true, 30), (false, 80)]);
        let SegEvent::Utterance(samples) = &events.iter().find(|e| matches!(e.1, SegEvent::Utterance(_))).expect("utterance").1 else { unreachable!() };
        assert!(samples.len() > noted, "the utterance holds everything noted before it ended");
    }

    #[test]
    fn a_longer_pause_setting_keeps_a_slow_thinker_in_one_utterance() {
        // 1.0 s of silence (50 frames) ends an utterance at the default 700 ms, but not at 1.5 s.
        let plan = [(false, 50), (true, 50), (false, 50), (true, 50), (false, 120)];
        let utterances = |seg: &mut Segmenter| feed(seg, &plan).iter().filter(|e| matches!(e.1, SegEvent::Utterance(_))).count();
        assert_eq!(utterances(&mut Segmenter::new(SegmenterConfig::default())), 2);
        let mut patient = Segmenter::new(SegmenterConfig::default());
        patient.set_end_silence_ms(1_500);
        assert_eq!(utterances(&mut patient), 1);
    }

    #[test]
    fn two_sentences_with_a_real_pause_are_two_utterances() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let events = feed(&mut seg, &[(false, 50), (true, 60), (false, 60), (true, 60), (false, 60)]);
        let utterances: Vec<_> = events.iter().filter(|e| matches!(e.1, SegEvent::Utterance(_))).collect();
        assert_eq!(utterances.len(), 2, "{events:?}");
    }

    #[test]
    fn an_endless_monologue_is_cut_at_the_maximum_length() {
        let mut seg = Segmenter::new(SegmenterConfig { max_ms: 2_000, ..SegmenterConfig::default() });
        let events = feed(&mut seg, &[(false, 50), (true, 400)]);
        let cut = events.iter().find(|e| matches!(e.1, SegEvent::Utterance(_))).expect("cut");
        let SegEvent::Utterance(samples) = &cut.1 else { unreachable!() };
        assert!(samples.len() as f32 / 16_000.0 <= 2.1);
    }

    #[test]
    fn speech_ms_grows_while_speaking_and_resets_after() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        feed(&mut seg, &[(false, 50), (true, 25)]);
        assert!(seg.in_speech());
        assert!(seg.speech_ms() >= 400, "{}", seg.speech_ms());
        feed(&mut seg, &[(false, 60)]);
        assert!(!seg.in_speech());
        assert_eq!(seg.speech_ms(), 0);
    }

    #[test]
    fn reset_drops_an_utterance_in_progress() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        feed(&mut seg, &[(false, 50), (true, 30)]);
        assert!(seg.in_speech());
        seg.reset();
        assert!(!seg.in_speech());
        assert!(feed(&mut seg, &[(false, 60)]).is_empty(), "nothing leaks out after a reset");
    }

    #[test]
    fn a_noisy_room_raises_the_bar_but_speech_still_gets_through() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let noisy = |n: usize| -> Vec<f32> {
            (0..FRAME_SAMPLES).map(|i| 0.02 * (((i + n * 13) * 7919 % 200) as f32 / 100.0 - 1.0)).collect()
        };
        let mut events = Vec::new();
        for n in 0..150 {
            events.extend(seg.push_frame(&noisy(n)));
        }
        assert!(events.is_empty(), "room noise alone is not speech");
        for n in 0..60 {
            let f: Vec<f32> = voice(n).iter().zip(noisy(n)).map(|(a, b)| a + b).collect();
            events.extend(seg.push_frame(&f));
        }
        for n in 0..60 {
            events.extend(seg.push_frame(&noisy(n)));
        }
        assert!(events.iter().any(|e| matches!(e, SegEvent::Utterance(_))), "{events:?}");
    }
}
