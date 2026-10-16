//! Hands-free listening: the microphone stays open, speech is found as it happens, each utterance goes
//! to Gaia Cloud for transcription, and the user can interrupt Gaia while she is speaking.
//!
//! The pipeline, per 10 ms of microphone audio:
//!
//! ```text
//! microphone ─ resample 16 kHz ─ echo canceller ◄─ what Gaia was playing (silence if she is quiet)
//!                                      │
//!                                      ▼
//!                          speech segmenter (live VAD)
//!                                      │ utterance                 │ 250 ms of speech while Gaia speaks
//!                                      ▼                           ▼
//!                      Gaia Cloud transcription            BargeIn: stop her voice
//! ```
//!
//! Everything privacy-relevant from push-to-talk holds here too: audio exists only in memory and only
//! around speech, silence and clicks are dropped before anything is sent, and switching the microphone
//! off ends it all. What is new is that nothing says "start" except the user's own voice, so:
//! - it only ever runs while the master microphone switch is on, and stops the moment it goes off;
//! - the whole time Gaia is speaking the echo canceller subtracts her voice; if barge-in is disabled
//!   (the fallback for rooms where cancellation is not good enough) her speech is simply not listened to,
//!   plus a short cooldown after she stops;
//! - transcripts that are obviously a recognizer hallucinating on noise are discarded here.
//!
//! The desktop never interprets what is said: the transcript goes to the UI, which sends it as a turn.
//!
//! **The wake word** ("Hey Gaia", see `wake.rs`) gates all of this. With continuous conversation on she is
//! always attentive. With only the wake word on, the microphone is listened to *locally and for that phrase
//! only*: the live speech detector keeps tracking the room, but whatever it finds is thrown away and nothing
//! is transcribed or sent anywhere. The phrase arms her for a conversation (a chime says so); she stays
//! attentive while the user speaks, while a reply is being thought and spoken, and for a short follow-up
//! window afterwards, then goes back to waiting. Saying the phrase while she is speaking also interrupts her.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::capture::CaptureSink;
use super::echo::{EchoCanceller, EchoGate, BLOCK};
use super::ptt::{Spawner, TranscribeError, Transcriber, Transcript, TranscriptEvent};
use super::stream::Downsampler;
use super::utterance;
use super::vad::{SegEvent, Segmenter, SegmenterConfig, FRAME_SAMPLES};
use super::wake::{RustpotterSpotter, WakeSpotter, DEFAULT_SENSITIVITY};
use super::{MicController, VoiceError};

pub const CONTINUOUS_EVENT: &str = "voice://continuous";
pub const BARGE_IN_EVENT: &str = "voice://barge-in";
pub const WAKE_EVENT: &str = "voice://wake";

/// Speech this long while Gaia is speaking is an interruption, not a cough or leftover echo.
const BARGE_IN_MS: u32 = 250;
/// How loud (rms of a cleaned 20 ms frame, 0..1) the user must be to interrupt Gaia. The speech detector
/// alone is deliberately sensitive (it hears a quiet voice in a quiet room); while she speaks that is too
/// easy to trip: a murmur, a cough, a neighbour, or a bit of her own echo is "speech" but is not someone
/// taking the floor. Normal talking into a desktop microphone is around 0.03 to 0.1.
pub const DEFAULT_BARGE_IN_LEVEL: f32 = 0.04;
/// The range the level can be set to, so a bad setting can neither disable nor jam interruption.
const BARGE_IN_LEVEL_RANGE: (f32, f32) = (0.01, 0.3);
/// Length of one speech-detector frame, in ms.
const FRAME_MS: u32 = 20;
/// How long after Gaia's voice ends her echo may still be arriving: speakers and monitors add output delay
/// on top of the room, so her last words are still sounding after the reference has run out (in 10 ms blocks).
const ECHO_TAIL_BLOCKS: u32 = 40;
/// Speech that begins in the last stretch of a reply, or in the tail after it, is her own last words coming
/// back, not the user talking over her: the canceller is least sure there (the reference has just run out,
/// and a delay it has not locked onto leaks the end of a short reply). Such an utterance is never sent for
/// transcription. In 16 kHz samples (800 ms).
const ECHO_END_ZONE_SAMPLES: usize = 12_800;
/// The same at the other end: the canceller has not yet locked onto a new reply in its first moments, so
/// speech that begins within this long of the start of a reply is her own voice too (800 ms).
const ECHO_START_ZONE_SAMPLES: usize = 12_800;
/// With barge-in off, how long after Gaia stops before listening resumes (in 10 ms blocks).
const HALF_DUPLEX_COOLDOWN_BLOCKS: u32 = 40;

/// Whether speech beginning now is probably her own voice: it begins in the first or last moments of a reply
/// (or in the tail after it), where the echo canceller is least sure. `remaining` is what is left of the
/// reply to play and `played` what has been played of it, both in 16 kHz samples.
fn in_echo_zone(speaking: bool, playing: bool, remaining: usize, played: usize) -> bool {
    speaking && (remaining < ECHO_END_ZONE_SAMPLES || (playing && played < ECHO_START_ZONE_SAMPLES))
}

/// What Gaia is playing, as the listener needs it: the reference for the echo canceller.
#[derive(Debug, Clone)]
pub enum RenderCmd {
    /// A reply started playing: its audio at 16 kHz mono.
    Start(Arc<Vec<f32>>),
    /// The reply was stopped: only the next `keep_ms` of it will still be heard (the fade-out).
    Stop { keep_ms: u32 },
}

pub type RenderSink = Arc<dyn Fn(RenderCmd) + Send + Sync>;

/// How long she stays attentive after the wake word, in 10 ms blocks.
#[derive(Debug, Clone, Copy)]
pub struct ArmTiming {
    /// After the phrase: time to start speaking.
    pub wake_window: u32,
    /// While the user is speaking: attentive until this long after they stop.
    pub speech_extend: u32,
    /// After an utterance is sent: she may be thinking for a while before she answers.
    pub think_hold: u32,
    /// While she speaks, and after: a window for a follow-up without saying the phrase again.
    pub follow_up: u32,
}

impl Default for ArmTiming {
    fn default() -> Self {
        Self { wake_window: 800, speech_extend: 500, think_hold: 6_000, follow_up: 1_000 }
    }
}

/// Builds the wake-word detector from the saved reference and the sensitivity; `None` when there is no
/// reference (not enrolled) or it cannot be used.
pub type SpotterFactory = Arc<dyn Fn(Option<&[u8]>, f32) -> Option<Box<dyn WakeSpotter>> + Send + Sync>;

fn rustpotter_factory() -> SpotterFactory {
    Arc::new(|reference, sensitivity| {
        reference
            .and_then(|bytes| RustpotterSpotter::from_reference(bytes, sensitivity).ok())
            .map(|s| Box::new(s) as Box<dyn WakeSpotter>)
    })
}

struct WakeCfg {
    reference: Option<Arc<Vec<u8>>>,
    sensitivity: f32,
    /// Bumped on every change so the listening thread rebuilds its detector.
    version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ListenState {
    Off,
    /// Only the wake word is being listened for, locally; nothing is transcribed or sent.
    Waiting,
    /// Waiting for the user to speak.
    Listening,
    /// An utterance is in progress.
    Hearing,
    /// An utterance is being transcribed (listening continues meanwhile).
    Transcribing,
    Failed { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContinuousEvent {
    State(ListenState),
    /// The user started talking over Gaia: stop her voice now.
    BargeIn,
    /// The wake word was heard. (She is armed; play the chime.)
    Wake,
    Transcript(TranscriptEvent),
}

type Emitter = Arc<dyn Fn(ContinuousEvent) + Send + Sync>;

/// A transcript that is plainly a recognizer hallucinating on noise rather than words: nothing to read,
/// or the same few words over and over (`"бла-бла-бла-бла…"`).
pub fn plausible_transcript(text: &str) -> bool {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    if words.iter().map(|w| w.chars().count()).sum::<usize>() < 2 {
        return false;
    }
    if words.len() >= 6 {
        let distinct: HashSet<&String> = words.iter().collect();
        if distinct.len() * 4 <= words.len() {
            return false;
        }
    }
    true
}

struct Active {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct Inner {
    mic: MicController,
    transcriber: Arc<dyn Transcriber>,
    emit: Emitter,
    spawn: Spawner,
    barge_in: AtomicBool,
    /// Minimum loudness of an interruption (f32 bits of an rms level).
    barge_in_level: AtomicU32,
    /// Set while hands-free mode runs; push-to-talk refuses to start meanwhile (one tap at a time).
    exclusive: Arc<AtomicBool>,
    /// Always attentive (continuous conversation).
    continuous: AtomicBool,
    /// Attentive after the wake word.
    wake: AtomicBool,
    wake_cfg: Mutex<WakeCfg>,
    factory: SpotterFactory,
    arm: ArmTiming,
    active: Mutex<Option<Active>>,
    render_tx: Mutex<Sender<RenderCmd>>,
    /// Serialises transcriptions so transcripts arrive in the order they were spoken.
    order: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone)]
pub struct ContinuousListener {
    inner: Arc<Inner>,
}

impl ContinuousListener {
    pub fn new(
        mic: MicController,
        transcriber: Arc<dyn Transcriber>,
        emit: Emitter,
        spawn: Spawner,
        exclusive: Arc<AtomicBool>,
        barge_in: bool,
    ) -> Self {
        Self::new_with_wake(mic, transcriber, emit, spawn, exclusive, barge_in, rustpotter_factory(), ArmTiming::default())
    }

    /// As [`ContinuousListener::new`], with the wake-word detector factory and the attention timing chosen
    /// (tests use a scripted detector and short timings).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_wake(
        mic: MicController,
        transcriber: Arc<dyn Transcriber>,
        emit: Emitter,
        spawn: Spawner,
        exclusive: Arc<AtomicBool>,
        barge_in: bool,
        factory: SpotterFactory,
        arm: ArmTiming,
    ) -> Self {
        let (render_tx, _unused) = mpsc::channel();
        Self {
            inner: Arc::new(Inner {
                mic,
                transcriber,
                emit,
                spawn,
                barge_in: AtomicBool::new(barge_in),
                barge_in_level: AtomicU32::new(DEFAULT_BARGE_IN_LEVEL.to_bits()),
                exclusive,
                continuous: AtomicBool::new(false),
                wake: AtomicBool::new(false),
                wake_cfg: Mutex::new(WakeCfg { reference: None, sensitivity: DEFAULT_SENSITIVITY, version: 0 }),
                factory,
                arm,
                active: Mutex::new(None),
                render_tx: Mutex::new(render_tx),
                order: Arc::new(tokio::sync::Mutex::new(())),
            }),
        }
    }

    /// Where the playback engine reports what Gaia is playing. Works whether or not listening is on.
    pub fn render_sink(&self) -> RenderSink {
        let inner = self.inner.clone();
        Arc::new(move |cmd| {
            let _ = inner.render_tx.lock().expect("render poisoned").send(cmd);
        })
    }

    pub fn is_active(&self) -> bool {
        self.inner.active.lock().expect("listener poisoned").is_some()
    }

    pub fn set_barge_in(&self, enabled: bool) {
        self.inner.barge_in.store(enabled, Ordering::SeqCst);
    }

    /// How loud the user must be to interrupt Gaia (rms of a cleaned frame). Clamped to a sane range;
    /// takes effect within a moment.
    pub fn set_barge_in_level(&self, level: f32) {
        let level = if level.is_finite() { level } else { DEFAULT_BARGE_IN_LEVEL };
        let (lo, hi) = BARGE_IN_LEVEL_RANGE;
        self.inner.barge_in_level.store(level.clamp(lo, hi).to_bits(), Ordering::SeqCst);
    }

    /// Gives the listener the enrolled wake word (`None` forgets it). Takes effect within a moment.
    pub fn set_wake_reference(&self, reference: Option<Vec<u8>>) {
        let mut cfg = self.inner.wake_cfg.lock().expect("wake cfg poisoned");
        cfg.reference = reference.map(Arc::new);
        cfg.version += 1;
    }

    pub fn set_sensitivity(&self, sensitivity: f32) {
        let mut cfg = self.inner.wake_cfg.lock().expect("wake cfg poisoned");
        cfg.sensitivity = sensitivity;
        cfg.version += 1;
    }

    /// True when a wake word is enrolled (a reference is loaded and usable).
    pub fn has_wake_word(&self) -> bool {
        (self.inner.factory)(
            self.inner.wake_cfg.lock().expect("wake cfg poisoned").reference.as_ref().map(|r| r.as_slice()),
            DEFAULT_SENSITIVITY,
        )
        .is_some()
    }

    /// Sets which of the two ways of being attentive are on, and starts or stops listening to match:
    /// `continuous` = always attentive, `wake` = attentive after "Hey Gaia". Neither means no listening at
    /// all. Starting needs the microphone on (`mic_off` otherwise; the flags are kept for later).
    pub fn apply(&self, continuous: bool, wake: bool) -> Result<(), VoiceError> {
        self.inner.continuous.store(continuous, Ordering::SeqCst);
        self.inner.wake.store(wake, Ordering::SeqCst);
        if continuous || wake {
            self.start_listening()
        } else {
            self.stop();
            Ok(())
        }
    }

    /// Starts always-attentive hands-free listening. Needs the microphone on.
    pub fn start(&self) -> Result<(), VoiceError> {
        self.inner.continuous.store(true, Ordering::SeqCst);
        self.start_listening()
    }

    fn start_listening(&self) -> Result<(), VoiceError> {
        let mut active = self.inner.active.lock().expect("listener poisoned");
        if active.is_some() {
            return Ok(());
        }
        if self.inner.exclusive.swap(true, Ordering::SeqCst) {
            return Err(VoiceError {
                code: "busy".into(),
                message: "a push-to-talk recording is in progress".into(),
            });
        }
        let Some(sink) = self.inner.mic.capture_sink() else {
            self.inner.exclusive.store(false, Ordering::SeqCst);
            return Err(VoiceError {
                code: "mic_off".into(),
                message: "switch the microphone on first".into(),
            });
        };
        if sink.sample_rate() == 0 {
            self.inner.exclusive.store(false, Ordering::SeqCst);
            return Err(VoiceError {
                code: "mic_off".into(),
                message: "the microphone is not ready yet".into(),
            });
        }
        // A fresh render channel per run, so commands sent before this start never replay into it.
        let (tx, rx) = mpsc::channel();
        *self.inner.render_tx.lock().expect("render poisoned") = tx;

        // The tap is attached here, not by the thread: once start() returns, audio is already flowing.
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
        sink.attach_tap(audio_tx);

        let stop = Arc::new(AtomicBool::new(false));
        let inner = self.inner.clone();
        let stop2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("gaia-listen".into())
            .spawn(move || run(inner, sink, audio_rx, rx, stop2))
            .map_err(|e| VoiceError {
                code: "open_failed".into(),
                message: e.to_string(),
            })?;
        *active = Some(Active {
            stop,
            thread: Some(thread),
        });
        drop(active);
        // The listening thread announces the first state itself (Listening or Waiting) on its first block.
        Ok(())
    }

    /// Stops listening (the microphone itself stays as the user left it).
    pub fn stop(&self) {
        let taken = self.inner.active.lock().expect("listener poisoned").take();
        if let Some(mut a) = taken {
            a.stop.store(true, Ordering::SeqCst);
            if let Some(t) = a.thread.take() {
                let _ = t.join();
            }
            self.inner.exclusive.store(false, Ordering::SeqCst);
            (self.inner.emit)(ContinuousEvent::State(ListenState::Off));
        }
    }
}

/// The listening thread: owns the downsampler, the echo canceller, the segmenter and the wake detector.
fn run(
    inner: Arc<Inner>,
    first_sink: Arc<CaptureSink>,
    first_audio: Receiver<Vec<f32>>,
    render_rx: Receiver<RenderCmd>,
    stop: Arc<AtomicBool>,
) {
    let mut sink = first_sink;
    let mut audio_rx = first_audio;
    let mut down = Downsampler::new(sink.sample_rate());
    let mut aec = match EchoCanceller::new() {
        Ok(a) => a,
        Err(message) => {
            (inner.emit)(ContinuousEvent::State(ListenState::Failed {
                code: "aec_failed".into(),
                message,
            }));
            inner.exclusive.store(false, Ordering::SeqCst);
            return;
        }
    };
    // The segmenter runs on everything, armed or not, so it always knows the room; while she is not armed
    // what it finds is thrown away (see below).
    let mut seg = Segmenter::new(SegmenterConfig::default());
    let mut spotter: Option<Box<dyn WakeSpotter>> = None;
    let mut spotter_version = u64::MAX;
    let arm = inner.arm;

    let mut pending: Vec<f32> = Vec::new(); // 16 kHz, awaiting a full block
    let mut half: Vec<f32> = Vec::new(); // first block of the 20 ms frame
    let mut render_queue: VecDeque<f32> = VecDeque::new();
    let mut tail = 0u32; // blocks of echo tail left after Gaia stops
    let mut cooldown = 0u32; // blocks of deafness left (barge-in off)
    let mut was_speaking = false;
    let mut gate = EchoGate::new(); // quiets what is no louder than her own leftover echo
    let mut render_half: Vec<f32> = Vec::new(); // what was played over the 20 ms frame being assembled
    let mut reply_pos = 0usize; // samples of the current reply played so far
    let mut onset_in_echo_zone = false; // the utterance in progress began while her voice was ending
    let mut barged = false;
    let mut loud_frames = 0u32; // frames of the current utterance, while she speaks, loud enough to interrupt
    let mut hearing = false;
    let mut clean = vec![0.0f32; BLOCK];
    let mut now: u64 = 0; // 10 ms blocks processed
    let mut armed_until: u64 = 0;
    let mut announced: Option<bool> = None;

    'outer: loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match audio_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => down.push(&chunk, &mut pending),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        // The microphone may have been switched off or restarted on another device.
        match inner.mic.capture_sink() {
            Some(now_sink) if !Arc::ptr_eq(&now_sink, &sink) => {
                sink.detach_tap();
                sink = now_sink;
                let (t, r) = mpsc::channel::<Vec<f32>>();
                audio_rx = r;
                sink.attach_tap(t);
                down = Downsampler::new(sink.sample_rate());
                pending.clear();
                half.clear();
                seg.reset();
                if let Some(s) = spotter.as_mut() {
                    s.reset();
                }
                hearing = false;
                continue;
            }
            Some(_) => {}
            None => {
                // switched off (or failed): listening ends with it
                seg.reset();
                break 'outer;
            }
        }

        // A changed wake word or sensitivity rebuilds the detector.
        {
            let cfg = inner.wake_cfg.lock().expect("wake cfg poisoned");
            if cfg.version != spotter_version {
                spotter = (inner.factory)(cfg.reference.as_ref().map(|r| r.as_slice()), cfg.sensitivity);
                spotter_version = cfg.version;
            }
        }

        while let Ok(cmd) = render_rx.try_recv() {
            match cmd {
                // A reply that starts while an earlier one is still fading out plays *over* it: mix, not queue.
                RenderCmd::Start(samples) => {
                    if render_queue.is_empty() {
                        reply_pos = 0; // a new reply, not one laid over another
                    }
                    for (i, s) in samples.iter().enumerate() {
                        match render_queue.get_mut(i) {
                            Some(existing) => *existing += *s,
                            None => render_queue.push_back(*s),
                        }
                    }
                }
                RenderCmd::Stop { keep_ms } => render_queue.truncate(keep_ms as usize * 16),
            }
        }

        while pending.len() >= BLOCK {
            let block: Vec<f32> = pending.drain(..BLOCK).collect();
            let render: Vec<f32> = (0..BLOCK).map(|_| render_queue.pop_front().unwrap_or(0.0)).collect();
            aec.process(&render, &block, &mut clean);
            now += 1;

            let playing = !render_queue.is_empty();
            if playing {
                reply_pos += BLOCK;
            }
            let speaking = playing || tail > 0;
            if playing {
                tail = ECHO_TAIL_BLOCKS;
            } else if tail > 0 {
                tail -= 1;
            }
            if was_speaking && !speaking {
                barged = false;
                if !inner.barge_in.load(Ordering::SeqCst) {
                    cooldown = HALF_DUPLEX_COOLDOWN_BLOCKS;
                }
            }
            was_speaking = speaking;

            // The wake word. (With continuous conversation on she is always attentive: the phrase is then
            // just speech.) Heard while she is speaking, it also interrupts her.
            let always = inner.continuous.load(Ordering::SeqCst);
            let wake_on = inner.wake.load(Ordering::SeqCst);
            let was_armed = always || now <= armed_until;
            if wake_on && !always {
                if let Some(detector) = spotter.as_mut() {
                    if detector.push(&clean) {
                        if speaking && !barged {
                            barged = true;
                            (inner.emit)(ContinuousEvent::BargeIn);
                        }
                        armed_until = now + arm.wake_window as u64;
                        (inner.emit)(ContinuousEvent::Wake);
                    }
                }
            }
            // A conversation in progress stays open while she speaks.
            if speaking && was_armed && !always {
                armed_until = armed_until.max(now + arm.follow_up as u64);
            }
            let armed = always || now <= armed_until;
            if announced != Some(armed) {
                announced = Some(armed);
                (inner.emit)(ContinuousEvent::State(if armed { ListenState::Listening } else { ListenState::Waiting }));
                if !armed {
                    hearing = false;
                }
            }

            half.extend_from_slice(&clean);
            render_half.extend_from_slice(&render);
            if half.len() < FRAME_SAMPLES {
                continue;
            }
            let mut frame: Vec<f32> = half.drain(..FRAME_SAMPLES).collect();
            let played: Vec<f32> = render_half.drain(..FRAME_SAMPLES).collect();
            // While she speaks, only what is clearly louder than her own leftover echo reaches the detector.
            gate.apply(&mut frame, &played);

            // Half-duplex: not listening while Gaia speaks (and briefly after). Anything in progress is dropped.
            let deaf = armed && ((speaking && !inner.barge_in.load(Ordering::SeqCst)) || cooldown > 0);
            if cooldown > 0 && !speaking {
                cooldown -= 1;
            }
            if deaf {
                seg.reset();
                if hearing {
                    hearing = false;
                    (inner.emit)(ContinuousEvent::State(ListenState::Listening));
                }
                continue;
            }

            let event = seg.push_frame(&frame);
            if !armed {
                // Waiting for the wake word: the room is tracked, but nothing found in it is acted on, so
                // nothing is ever transcribed or sent while she is not attentive.
                continue;
            }
            match event {
                Some(SegEvent::Started) => {
                    onset_in_echo_zone = in_echo_zone(speaking, playing, render_queue.len(), reply_pos);
                    hearing = true;
                    (inner.emit)(ContinuousEvent::State(ListenState::Hearing));
                }
                Some(SegEvent::TooShort) => {
                    hearing = false;
                    (inner.emit)(ContinuousEvent::State(ListenState::Listening));
                }
                Some(SegEvent::Utterance(samples)) => {
                    hearing = false;
                    if onset_in_echo_zone {
                        // Her own last words, heard again: not sent, not answered.
                        eprintln!(
                            "[voice] dropped {:.1} s of speech that began as Gaia's voice was ending (her own echo)",
                            samples.len() as f32 / 16_000.0
                        );
                        onset_in_echo_zone = false;
                        (inner.emit)(ContinuousEvent::State(ListenState::Listening));
                    } else {
                        eprintln!(
                            "[voice] heard {:.1} s of speech{}",
                            samples.len() as f32 / 16_000.0,
                            if speaking { " while Gaia was speaking (interruption)" } else { "" }
                        );
                        submit(&inner, samples);
                        armed_until = armed_until.max(now + arm.think_hold as u64);
                    }
                }
                None => {}
            }
            if seg.in_speech() {
                armed_until = armed_until.max(now + arm.speech_extend as u64);
            }
            // Interrupting needs volume, not only speech: the microphone is gated for the interruption on a
            // level. Only frames of the utterance that are at least that loud count, so a murmur, a cough
            // or a neighbour never takes the floor, while someone really talking over her does within
            // BARGE_IN_MS. (Normal listening, while she is silent, is not affected.)
            if speaking && seg.in_speech() {
                let barge_level = f32::from_bits(inner.barge_in_level.load(Ordering::SeqCst));
                if utterance::rms(&frame) >= barge_level {
                    loud_frames += 1;
                }
            } else {
                loud_frames = 0;
            }
            if speaking && !barged && !onset_in_echo_zone && seg.in_speech() && loud_frames * FRAME_MS >= BARGE_IN_MS {
                barged = true;
                (inner.emit)(ContinuousEvent::BargeIn);
            }
        }
    }

    sink.detach_tap();
    inner.exclusive.store(false, Ordering::SeqCst);
    let mut active = inner.active.lock().expect("listener poisoned");
    if active.as_ref().map(|a| !a.stop.load(Ordering::SeqCst)).unwrap_or(false) {
        // ended by itself (microphone off), not by stop(): clear and announce
        *active = None;
        drop(active);
        (inner.emit)(ContinuousEvent::State(ListenState::Off));
    }
}

/// Sends one finished utterance for transcription, in order, without holding up listening.
fn submit(inner: &Arc<Inner>, samples: Vec<f32>) {
    let pcm: Vec<i16> = samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16).collect();
    let wav = utterance::wav_bytes(&pcm);
    drop(samples);
    (inner.emit)(ContinuousEvent::State(ListenState::Transcribing));
    let inner = inner.clone();
    let spawn = inner.spawn.clone();
    spawn(Box::pin(async move {
        let _turn = inner.order.lock().await;
        let result: Result<Transcript, TranscribeError> = inner.transcriber.transcribe(wav).await;
        match result {
            Ok(t) if plausible_transcript(&t.text) => {
                (inner.emit)(ContinuousEvent::Transcript(TranscriptEvent::new(t, "continuous")));
            }
            Ok(_) => {} // silence or hallucination: nothing to say
            Err(e) => {
                (inner.emit)(ContinuousEvent::State(ListenState::Failed {
                    code: e.code,
                    message: e.message,
                }));
            }
        }
        if inner.active.lock().expect("listener poisoned").is_some() {
            (inner.emit)(ContinuousEvent::State(ListenState::Listening));
        }
    }));
}

// ------------------------------------------------------------------------------------------------
// Tauri commands
// ------------------------------------------------------------------------------------------------

fn settings_error(e: crate::settings::SettingsError) -> VoiceError {
    VoiceError {
        code: "settings".into(),
        message: e.to_string(),
    }
}

/// Turns hands-free listening on or off, and remembers the preference. Turning it on needs the microphone
/// on (that switch stays an explicit act of its own); with the microphone off the preference is saved and
/// takes effect the next time the user switches the microphone on.
#[tauri::command]
pub fn voice_continuous_set(
    listener: tauri::State<'_, ContinuousListener>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    enabled: bool,
) -> Result<bool, VoiceError> {
    settings.update(|s| s.voice.continuous_enabled = enabled).map_err(settings_error)?;
    let voice = settings.get().voice;
    // "Hey Gaia" keeps listening for the phrase when continuous conversation is switched off.
    let wake = voice.wake_word_enabled && voice.wake_enrolled;
    match listener.apply(enabled, wake) {
        Ok(()) => {}
        Err(e) if e.code == "mic_off" => {} // saved; starts when the microphone is switched on
        Err(e) => return Err(e),
    }
    Ok(listener.is_active())
}

/// Whether the user can interrupt Gaia by talking over her (otherwise her speech is simply not listened to).
#[tauri::command]
pub fn voice_set_barge_in(
    listener: tauri::State<'_, ContinuousListener>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    enabled: bool,
) -> Result<(), VoiceError> {
    settings.update(|s| s.voice.barge_in_enabled = enabled).map_err(settings_error)?;
    listener.set_barge_in(enabled);
    Ok(())
}

/// How loud the user must be to interrupt Gaia by talking over her (rms level, 0.01 to 0.3).
#[tauri::command]
pub fn voice_set_barge_in_level(
    listener: tauri::State<'_, ContinuousListener>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    level: f32,
) -> Result<(), VoiceError> {
    let (lo, hi) = BARGE_IN_LEVEL_RANGE;
    let level = if level.is_finite() { level.clamp(lo, hi) } else { DEFAULT_BARGE_IN_LEVEL };
    settings.update(|s| s.voice.barge_in_level = level).map_err(settings_error)?;
    listener.set_barge_in_level(level);
    Ok(())
}

#[tauri::command]
pub fn voice_continuous_state(listener: tauri::State<'_, ContinuousListener>) -> bool {
    listener.is_active()
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, Mode};
    use super::super::Timing;
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use tokio::runtime::Handle;

    struct FakeTranscriber {
        calls: AtomicUsize,
        reply: Mutex<Result<Transcript, TranscribeError>>,
    }

    impl FakeTranscriber {
        fn saying(text: &str) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                reply: Mutex::new(Ok(Transcript { text: text.into(), language: Some("nl".into()) })),
            })
        }
    }

    #[async_trait]
    impl Transcriber for FakeTranscriber {
        async fn transcribe(&self, _wav: Vec<u8>) -> Result<Transcript, TranscribeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.reply.lock().unwrap().clone()
        }
    }

    type Log = Arc<Mutex<Vec<ContinuousEvent>>>;

    struct Rig {
        fake: Arc<Fake>,
        mic: MicController,
        listener: ContinuousListener,
        log: Log,
        transcriber: Arc<FakeTranscriber>,
        exclusive: Arc<AtomicBool>,
    }

    fn rig(mode: Mode, barge_in: bool) -> Rig {
        // (Mode::Bursts: a second of speech, then a pause: utterances that actually end.)
        let fake = Fake::new(mode);
        let mic = MicController::new(
            fake.clone(),
            Arc::new(|_| {}),
            Timing {
                poll: Duration::from_millis(5),
                first_frames_timeout: Duration::from_secs(5),
                no_frames_timeout: Duration::from_secs(5),
                max_restarts: 3,
                restart_window: Duration::from_secs(60),
            },
        );
        let transcriber = FakeTranscriber::saying("goedemorgen Gaia");
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let handle = Handle::current();
        let exclusive = Arc::new(AtomicBool::new(false));
        let listener = ContinuousListener::new(
            mic.clone(),
            transcriber.clone(),
            Arc::new(move |e| l2.lock().unwrap().push(e)),
            Arc::new(move |f| {
                handle.spawn(f);
            }),
            exclusive.clone(),
            barge_in,
        );
        Rig { fake, mic, listener, log, transcriber, exclusive }
    }

    async fn wait_for(what: &str, cond: impl Fn() -> bool) {
        for _ in 0..800 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    fn transcripts(log: &Log) -> Vec<TranscriptEvent> {
        log.lock().unwrap().iter().filter_map(|e| if let ContinuousEvent::Transcript(t) = e { Some(t.clone()) } else { None }).collect()
    }

    fn barge_ins(log: &Log) -> usize {
        log.lock().unwrap().iter().filter(|e| matches!(e, ContinuousEvent::BargeIn)).count()
    }

    fn states(log: &Log) -> Vec<ListenState> {
        log.lock().unwrap().iter().filter_map(|e| if let ContinuousEvent::State(s) = e { Some(s.clone()) } else { None }).collect()
    }

    /// A reply Gaia is "playing": quiet noise that the microphone does not actually echo (the fake mic's
    /// own tone stands in for the user talking over her).
    fn reply_audio(secs: usize) -> Arc<Vec<f32>> {
        Arc::new((0..16_000 * secs).map(|i| 0.1 * ((i * 31) % 100) as f32 / 100.0 - 0.05).collect())
    }

    #[test]
    fn speech_in_the_first_or_last_moments_of_a_reply_is_her_echo_the_middle_is_not() {
        let at = |remaining: usize, played: usize| in_echo_zone(true, remaining > 0, remaining, played);
        assert!(at(100_000, 4_000), "just after a reply began");
        assert!(at(5_000, 100_000), "as it is ending");
        assert!(at(0, 100_000), "in the tail after it");
        assert!(!at(100_000, 100_000), "in the middle of it");
        assert!(!in_echo_zone(false, false, 0, 0), "nothing is playing");
    }

    #[test]
    fn nonsense_transcripts_are_recognised_and_real_ones_are_not() {
        assert!(plausible_transcript("Wat is het weer vandaag?"));
        assert!(plausible_transcript("ja"));
        assert!(plausible_transcript("Hey Gaia, kun je dit voor me opzoeken, alsjeblieft"));
        assert!(!plausible_transcript(""));
        assert!(!plausible_transcript("   "));
        assert!(!plausible_transcript("..."));
        assert!(!plausible_transcript("a"));
        let babble = "Бла-".repeat(40);
        assert!(!plausible_transcript(&babble), "repeated babble");
        assert!(!plausible_transcript("ja ja ja ja ja ja ja ja ja ja"));
        assert!(plausible_transcript("nee nee nee, dat bedoelde ik niet"), "emphasis is not babble");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn it_cannot_start_while_the_microphone_is_off() {
        let r = rig(Mode::Frames, true);
        assert_eq!(r.listener.start().unwrap_err().code, "mic_off");
        assert!(!r.listener.is_active());
        assert!(!r.exclusive.load(Ordering::SeqCst), "a refused start leaves no claim on the microphone");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn speech_becomes_a_transcript_labelled_as_hands_free() {
        let r = rig(Mode::Bursts, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        wait_for("the transcript", || !transcripts(&r.log).is_empty()).await;
        let t = &transcripts(&r.log)[0];
        assert_eq!(t.text, "goedemorgen Gaia");
        assert_eq!(t.source, "continuous");
        let s = states(&r.log);
        assert_eq!(s[0], ListenState::Listening);
        assert!(s.contains(&ListenState::Hearing) && s.contains(&ListenState::Transcribing));
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_quiet_room_is_never_sent_anywhere() {
        let r = rig(Mode::Quiet, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert_eq!(r.transcriber.calls.load(Ordering::SeqCst), 0);
        assert!(transcripts(&r.log).is_empty());
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_hallucinated_transcript_is_dropped() {
        let r = rig(Mode::Bursts, true);
        *r.transcriber.reply.lock().unwrap() = Ok(Transcript { text: "Бла-".repeat(40), language: None });
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        wait_for("it was asked", || r.transcriber.calls.load(Ordering::SeqCst) >= 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(transcripts(&r.log).is_empty());
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn talking_over_gaia_is_a_barge_in_exactly_once() {
        let r = rig(Mode::Bursts, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        (r.listener.render_sink())(RenderCmd::Start(reply_audio(20)));
        wait_for("a barge-in", || barge_ins(&r.log) >= 1).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(barge_ins(&r.log), 1, "one interruption per spoken reply, not one per frame");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn speech_quieter_than_the_barge_in_level_does_not_interrupt_her() {
        let r = rig(Mode::Bursts, true);
        // The fake microphone's "speech" has an rms of about 0.2: a level above that is a voice too quiet.
        r.listener.set_barge_in_level(0.3);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        (r.listener.render_sink())(RenderCmd::Start(reply_audio(20)));
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        assert_eq!(barge_ins(&r.log), 0, "too quiet to take the floor");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[test]
    fn the_barge_in_level_is_kept_within_its_range() {
        let r = {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let _g = rt.enter();
            rig(Mode::Quiet, true)
        };
        let level = |l: &ContinuousListener| f32::from_bits(l.inner.barge_in_level.load(Ordering::SeqCst));
        assert!((level(&r.listener) - DEFAULT_BARGE_IN_LEVEL).abs() < f32::EPSILON);
        r.listener.set_barge_in_level(5.0);
        assert!((level(&r.listener) - BARGE_IN_LEVEL_RANGE.1).abs() < f32::EPSILON);
        r.listener.set_barge_in_level(0.0);
        assert!((level(&r.listener) - BARGE_IN_LEVEL_RANGE.0).abs() < f32::EPSILON);
        r.listener.set_barge_in_level(f32::NAN);
        assert!((level(&r.listener) - DEFAULT_BARGE_IN_LEVEL).abs() < f32::EPSILON);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_barge_in_off_gaias_speech_is_not_listened_to() {
        let r = rig(Mode::Bursts, false);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        (r.listener.render_sink())(RenderCmd::Start(reply_audio(20)));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(barge_ins(&r.log), 0);
        assert_eq!(r.transcriber.calls.load(Ordering::SeqCst), 0, "nothing heard while she speaks");
        assert!(transcripts(&r.log).is_empty());
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn listening_resumes_after_gaia_stops_when_barge_in_is_off() {
        let r = rig(Mode::Bursts, false);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        let sink = r.listener.render_sink();
        sink(RenderCmd::Start(reply_audio(1)));
        sink(RenderCmd::Stop { keep_ms: 0 });
        wait_for("listening again after the cooldown", || !transcripts(&r.log).is_empty()).await;
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn switching_the_microphone_off_ends_hands_free_listening() {
        let r = rig(Mode::Quiet, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        assert!(r.listener.is_active());
        r.mic.set_enabled(false, None);
        wait_for("listening ended", || !r.listener.is_active()).await;
        assert_eq!(states(&r.log).last(), Some(&ListenState::Off));
        assert!(!r.exclusive.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_device_change_does_not_end_listening() {
        let r = rig(Mode::Bursts, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        wait_for("first transcript", || !transcripts(&r.log).is_empty()).await;
        let before = transcripts(&r.log).len();
        // the stream fails; the controller reconnects on a new sink
        r.fake.last_sink.lock().unwrap().as_ref().unwrap().fail();
        wait_for("a transcript after the reconnect", || transcripts(&r.log).len() > before).await;
        assert!(r.listener.is_active());
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_detaches_the_microphone_tap_and_frees_push_to_talk() {
        let r = rig(Mode::Frames, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        assert!(r.exclusive.load(Ordering::SeqCst));
        let sink = r.fake.last_sink.lock().unwrap().clone().unwrap();
        assert!(sink.is_tapping());
        r.listener.stop();
        assert!(!sink.is_tapping());
        assert!(!r.exclusive.load(Ordering::SeqCst));
        assert!(!r.listener.is_active());
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn starting_twice_is_harmless() {
        let r = rig(Mode::Quiet, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        r.listener.start().unwrap();
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_transcription_failure_is_reported_and_listening_continues() {
        let r = rig(Mode::Bursts, true);
        *r.transcriber.reply.lock().unwrap() = Err(TranscribeError::new("unreachable", "x"));
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        wait_for("a failure", || {
            states(&r.log).iter().any(|s| matches!(s, ListenState::Failed { code, .. } if code == "unreachable"))
        })
        .await;
        assert!(r.listener.is_active());
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    // --- the wake word ----------------------------------------------------------------------------------

    /// A stand-in for the real detector: "hears" the wake word at the start of the Nth loud burst.
    struct ScriptedSpotter {
        fire_on: usize,
        bursts: usize,
        was_loud: bool,
    }

    impl WakeSpotter for ScriptedSpotter {
        fn push(&mut self, audio: &[f32]) -> bool {
            let loud = rms(audio) > 0.1;
            let rising = loud && !self.was_loud;
            self.was_loud = loud;
            if rising {
                self.bursts += 1;
                return self.bursts == self.fire_on;
            }
            false
        }
        fn reset(&mut self) {
            self.was_loud = false;
        }
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Short attention windows (10 ms blocks) so the tests fit between the fake microphone's bursts.
    fn quick_arm() -> ArmTiming {
        ArmTiming { wake_window: 150, speech_extend: 30, think_hold: 30, follow_up: 30 }
    }

    struct WakeRig {
        rig: Rig,
        built: Arc<AtomicUsize>,
    }

    /// A listener whose detector fires on the `fire_on`-th burst. `enrolled` decides whether a reference
    /// exists at all (without one the factory returns no detector, as in production).
    fn wake_rig(mode: Mode, fire_on: usize, enrolled: bool) -> WakeRig {
        let fake = Fake::new(mode);
        let mic = MicController::new(
            fake.clone(),
            Arc::new(|_| {}),
            Timing {
                poll: Duration::from_millis(5),
                first_frames_timeout: Duration::from_secs(5),
                no_frames_timeout: Duration::from_secs(5),
                max_restarts: 3,
                restart_window: Duration::from_secs(60),
            },
        );
        let transcriber = FakeTranscriber::saying("hoe laat is het");
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let handle = Handle::current();
        let exclusive = Arc::new(AtomicBool::new(false));
        let built = Arc::new(AtomicUsize::new(0));
        let built2 = built.clone();
        let factory: SpotterFactory = Arc::new(move |reference, _sensitivity| {
            built2.fetch_add(1, Ordering::SeqCst);
            reference.map(|_| Box::new(ScriptedSpotter { fire_on, bursts: 0, was_loud: false }) as Box<dyn WakeSpotter>)
        });
        let listener = ContinuousListener::new_with_wake(
            mic.clone(),
            transcriber.clone(),
            Arc::new(move |e| l2.lock().unwrap().push(e)),
            Arc::new(move |f| {
                handle.spawn(f);
            }),
            exclusive.clone(),
            true,
            factory,
            quick_arm(),
        );
        if enrolled {
            listener.set_wake_reference(Some(vec![1, 2, 3]));
        }
        WakeRig { rig: Rig { fake, mic, listener, log, transcriber, exclusive }, built }
    }

    fn wakes(log: &Log) -> usize {
        log.lock().unwrap().iter().filter(|e| matches!(e, ContinuousEvent::Wake)).count()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn waiting_for_the_wake_word_transcribes_nothing_until_it_is_heard() {
        let w = wake_rig(Mode::Bursts, 2, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();

        wait_for("the wake word", || wakes(&r.log) >= 1).await;
        wait_for("the question after it", || !transcripts(&r.log).is_empty()).await;

        // The first burst was speech too, but she was not attentive: it never left the device.
        assert_eq!(r.transcriber.calls.load(Ordering::SeqCst), 1, "only the burst after the wake word was sent");
        let events = r.log.lock().unwrap().clone();
        let wake_at = events.iter().position(|e| matches!(e, ContinuousEvent::Wake)).unwrap();
        let first_transcript = events.iter().position(|e| matches!(e, ContinuousEvent::Transcript(_))).unwrap();
        assert!(wake_at < first_transcript, "the phrase comes first");
        let waiting_before_wake = events[..wake_at]
            .iter()
            .any(|e| matches!(e, ContinuousEvent::State(ListenState::Waiting)));
        assert!(waiting_before_wake, "she announced that she was waiting, not listening");
        assert_eq!(transcripts(&r.log)[0].source, "continuous");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn attention_ends_after_the_conversation_and_later_speech_is_ignored_again() {
        let w = wake_rig(Mode::Bursts, 2, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();
        wait_for("the transcript", || !transcripts(&r.log).is_empty()).await;
        wait_for("back to waiting", || {
            let s = states(&r.log);
            let last_listening = s.iter().rposition(|x| *x == ListenState::Listening);
            let last_waiting = s.iter().rposition(|x| *x == ListenState::Waiting);
            matches!((last_listening, last_waiting), (Some(l), Some(w)) if w > l)
        })
        .await;
        // two more bursts go by while she is not attentive
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert_eq!(transcripts(&r.log).len(), 1, "speech after the conversation ended is not sent");
        assert_eq!(r.transcriber.calls.load(Ordering::SeqCst), 1);
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn saying_the_phrase_while_gaia_speaks_interrupts_her() {
        let w = wake_rig(Mode::Bursts, 1, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();
        (r.listener.render_sink())(RenderCmd::Start(reply_audio(20)));
        wait_for("the wake word", || wakes(&r.log) >= 1).await;
        let events = r.log.lock().unwrap().clone();
        let barge = events.iter().position(|e| matches!(e, ContinuousEvent::BargeIn)).expect("an interruption");
        let wake = events.iter().position(|e| matches!(e, ContinuousEvent::Wake)).unwrap();
        assert!(barge < wake, "her voice is stopped first, then the chime");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn continuous_conversation_makes_the_phrase_unnecessary() {
        let w = wake_rig(Mode::Bursts, 99, true); // the detector never fires
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(true, true).unwrap();
        wait_for("a transcript without any wake word", || !transcripts(&r.log).is_empty()).await;
        assert_eq!(wakes(&r.log), 0);
        assert!(!states(&r.log).contains(&ListenState::Waiting), "always attentive: never waiting");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_neither_mode_on_nothing_listens() {
        let w = wake_rig(Mode::Bursts, 1, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, false).unwrap();
        assert!(!r.listener.is_active());
        let sink = r.fake.last_sink.lock().unwrap().clone().unwrap();
        assert!(!sink.is_tapping(), "no tap, no audio");
        r.listener.apply(false, true).unwrap();
        assert!(r.listener.is_active());
        r.listener.apply(false, false).unwrap();
        assert!(!r.listener.is_active());
        assert!(!r.exclusive.load(Ordering::SeqCst));
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn starting_needs_the_microphone_on() {
        let w = wake_rig(Mode::Bursts, 1, true);
        let r = &w.rig;
        assert_eq!(r.listener.apply(false, true).unwrap_err().code, "mic_off");
        assert!(!r.listener.is_active());
        assert!(!r.exclusive.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn switching_the_microphone_off_ends_wake_listening_too() {
        let w = wake_rig(Mode::Quiet, 1, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();
        wait_for("waiting", || states(&r.log).contains(&ListenState::Waiting)).await;
        r.mic.set_enabled(false, None);
        wait_for("ended", || !r.listener.is_active()).await;
        assert_eq!(states(&r.log).last(), Some(&ListenState::Off));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_new_wake_word_or_sensitivity_rebuilds_the_detector() {
        let w = wake_rig(Mode::Quiet, 99, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();
        wait_for("the first detector", || w.built.load(Ordering::SeqCst) >= 1).await;
        let before = w.built.load(Ordering::SeqCst);
        r.listener.set_sensitivity(0.65);
        wait_for("a rebuild after the sensitivity changed", || w.built.load(Ordering::SeqCst) > before).await;
        let before = w.built.load(Ordering::SeqCst);
        r.listener.set_wake_reference(Some(vec![9, 9]));
        wait_for("a rebuild after re-enrolling", || w.built.load(Ordering::SeqCst) > before).await;
        r.listener.set_wake_reference(None);
        assert!(!r.listener.has_wake_word(), "forgotten means no detector can be built");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_wake_word_that_is_not_enrolled_never_wakes_and_never_sends() {
        let w = wake_rig(Mode::Bursts, 1, false);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();
        tokio::time::sleep(Duration::from_millis(1_200)).await;
        assert_eq!(wakes(&r.log), 0);
        assert_eq!(r.transcriber.calls.load(Ordering::SeqCst), 0);
        assert!(states(&r.log).contains(&ListenState::Waiting));
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn push_to_talk_cannot_start_while_the_wake_word_is_listening() {
        let w = wake_rig(Mode::Bursts, 99, true);
        let r = &w.rig;
        r.mic.set_enabled(true, None);
        r.listener.apply(false, true).unwrap();
        assert!(r.exclusive.load(Ordering::SeqCst), "the microphone tap is claimed");
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    // --- real speakers + real microphone ----------------------------------------------------------

    /// Speech-like synthetic reply (harmonics of a wandering pitch under a syllable envelope), 16 kHz.
    fn speechlike_reply(secs: usize) -> Vec<i16> {
        let mut seed = 99u64;
        let mut rnd = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        };
        let (mut ph, mut env, mut pitch) = (0f32, 0f32, 130f32);
        (0..16_000 * secs)
            .map(|i| {
                if i % 4_000 == 0 {
                    env = if rnd() > -0.3 { 0.6 + 0.4 * rnd().abs() } else { 0.0 };
                }
                if i % 320 == 0 {
                    pitch = 130.0 * (1.0 + 0.15 * rnd());
                }
                ph += std::f32::consts::TAU * pitch / 16_000.0;
                let v: f32 = (1..=8).map(|h| (ph * h as f32).sin() / h as f32).sum();
                (0.5 * env * v * 32_767.0).clamp(-32_000.0, 32_000.0) as i16
            })
            .collect()
    }

    /// Opt-in hardware test: `cargo test --lib real_speakers_do_not_make_gaia -- --ignored --nocapture`.
    /// PLAYS about 10 s of speech-like sound through the default speakers twice and listens on the real
    /// microphone: **stay silent while it runs**. The first run has the echo reference connected (the
    /// product); the second, a control, does not. The product run must produce no interruption of her
    /// own voice; the control shows whether the room actually echoes enough for the test to mean anything.
    #[test]
    #[ignore = "plays sound through the speakers and listens on the microphone"]
    fn real_speakers_do_not_make_gaia_interrupt_herself() {
        use super::super::CpalBackend;
        use crate::speech::PlaybackEngine;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let wav = utterance::wav_bytes(&speechlike_reply(10));
        let mut results = Vec::new();
        for connected in [true, false] {
            let log: Log = Arc::new(Mutex::new(Vec::new()));
            let l2 = log.clone();
            let mic = MicController::new(Arc::new(CpalBackend), Arc::new(|_| {}), Timing::default());
            let handle = rt.handle().clone();
            let transcriber = FakeTranscriber::saying("(heard something)");
            let listener = ContinuousListener::new(
                mic.clone(),
                transcriber.clone(),
                Arc::new(move |e| l2.lock().unwrap().push(e)),
                Arc::new(move |f| {
                    handle.spawn(f);
                }),
                Arc::new(AtomicBool::new(false)),
                true,
            );
            let playback = PlaybackEngine::new(0.5, false);
            if connected {
                playback.set_render_sink(listener.render_sink());
            }
            let status = mic.set_enabled(true, None);
            assert_eq!(status.state, super::super::MicState::Listening, "{status:?}");
            listener.start().expect("start");
            std::thread::sleep(Duration::from_millis(1_500)); // the room is measured while quiet

            let outcome = rt.block_on(playback.play(wav.clone()));
            std::thread::sleep(Duration::from_millis(800));
            let barge = barge_ins(&log);
            let heard = r_transcripts(&log);
            println!(
                "echo reference {}: playback {:?}, interruptions of her own voice: {barge}, utterances sent for transcription: {heard}",
                if connected { "CONNECTED (the product)" } else { "disconnected (control)" },
                outcome
            );
            listener.stop();
            mic.set_enabled(false, None);
            results.push((connected, barge, heard));
        }
        let (_, product_barge, product_heard) = results[0];
        assert_eq!(product_barge, 0, "Gaia must not interrupt herself");
        assert_eq!(product_heard, 0, "Gaia's own voice must not be sent for transcription");
        // Without the reference she is not known to be speaking, so the control cannot "interrupt"; what it
        // shows is her own voice being heard as the user's: utterances that would have been transcribed.
        let control_heard = results[1].2;
        println!(
            "control heard her own voice as {control_heard} utterance(s): {}",
            if control_heard > 0 { "the room echoes, so this test is meaningful" } else { "no echo reached the microphone, so this room proves little" }
        );
        assert!(control_heard > 0, "the control must hear her own voice, or this test proves nothing in this room");
    }

    // The fake microphone is quiet for 1.5 s and then speaks for 1 s, over and over, and runs faster than real
    // time; the speech first begins about 1.5 s into the reply (the reply is consumed with the microphone).

    #[tokio::test(flavor = "multi_thread")]
    async fn speech_that_begins_as_her_reply_is_ending_is_her_own_echo_and_is_not_sent() {
        let r = rig(Mode::Bursts, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        // 1.8 s of reply: the first burst begins with only a fraction of a second of it left (and stays in the
        // end zone even if the listener picks the reply up a little late); the next burst is after it.
        (r.listener.render_sink())(RenderCmd::Start(Arc::new(reply_audio(2)[..28_800].to_vec())));
        wait_for("a transcript", || !transcripts(&r.log).is_empty()).await;
        let heard_before_first_transcription = {
            let s = states(&r.log);
            let first = s.iter().position(|x| *x == ListenState::Transcribing).expect("a transcription began");
            s[..first].iter().filter(|x| **x == ListenState::Hearing).count()
        };
        assert!(
            heard_before_first_transcription >= 2,
            "the burst that began as her reply was ending must be skipped, not transcribed ({heard_before_first_transcription} heard)"
        );
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn speech_in_the_middle_of_her_reply_is_still_an_interruption_and_is_sent() {
        let r = rig(Mode::Bursts, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        (r.listener.render_sink())(RenderCmd::Start(reply_audio(20)));
        wait_for("the transcript of the interruption", || !transcripts(&r.log).is_empty()).await;
        assert!(barge_ins(&r.log) >= 1);
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn speech_well_after_her_reply_has_ended_is_sent() {
        let r = rig(Mode::Bursts, true);
        r.mic.set_enabled(true, None);
        r.listener.start().unwrap();
        (r.listener.render_sink())(RenderCmd::Start(reply_audio(1))); // over long before the speech begins
        wait_for("the transcript", || !transcripts(&r.log).is_empty()).await;
        r.listener.stop();
        r.mic.set_enabled(false, None);
    }

    /// Opt-in hardware test: `cargo test --lib real_speakers_three_replies -- --ignored --nocapture`.
    /// PLAYS three replies of different length through the default speakers, one after the other, with the
    /// echo reference connected like the product, and reports for each how many times she interrupted herself
    /// and how many pieces of her own voice were sent for transcription. **Stay silent while it runs**
    /// (about 40 s), and switch the microphone off in the running app first, or it will answer the test.
    #[test]
    #[ignore = "plays sound through the speakers and listens on the microphone"]
    fn real_speakers_three_replies_do_not_make_gaia_answer_herself() {
        use super::super::CpalBackend;
        use crate::speech::PlaybackEngine;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mic = MicController::new(Arc::new(CpalBackend), Arc::new(|_| {}), Timing::default());
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let handle = rt.handle().clone();
        let transcriber = FakeTranscriber::saying("(heard something)");
        let listener = ContinuousListener::new(
            mic.clone(),
            transcriber.clone(),
            Arc::new(move |e| l2.lock().unwrap().push(e)),
            Arc::new(move |f| {
                handle.spawn(f);
            }),
            Arc::new(AtomicBool::new(false)),
            true,
        );
        let playback = PlaybackEngine::new(0.5, false);
        playback.set_render_sink(listener.render_sink());
        let status = mic.set_enabled(true, None);
        assert_eq!(status.state, super::super::MicState::Listening, "{status:?}");
        listener.start().expect("start");
        std::thread::sleep(Duration::from_millis(2_000)); // the room is measured while quiet

        let mut all_ok = true;
        for (name, secs) in [("long reply (8 s)", 8usize), ("short reply (2 s)", 2), ("very short reply (1 s)", 1)] {
            let barge_before = barge_ins(&log);
            let heard_before = r_transcripts(&log);
            let wav = utterance::wav_bytes(&speechlike_reply(secs));
            let outcome = rt.block_on(playback.play(wav));
            std::thread::sleep(Duration::from_millis(3_000)); // what comes back after she stops
            let barge = barge_ins(&log) - barge_before;
            let heard = r_transcripts(&log) - heard_before;
            println!("{name}: playback {outcome:?}, interruptions of her own voice: {barge}, pieces of her own voice sent for transcription: {heard}");
            all_ok &= barge == 0 && heard == 0;
        }
        listener.stop();
        mic.set_enabled(false, None);
        assert!(all_ok, "she must neither interrupt nor answer herself, for any reply length");
    }

    /// Opt-in hardware test: `cargo test --lib measure_the_speaker_to_microphone_delay -- --ignored --nocapture`.
    /// PLAYS four replies through the default speakers and, for each, measures how long after the playback
    /// command her voice reaches the microphone (cross-correlation of what was played with what was heard)
    /// and how loudly. **Stay silent while it runs** (about 30 s) and switch the microphone off in the app.
    #[test]
    #[ignore = "plays sound through the speakers and listens on the microphone"]
    fn measure_the_speaker_to_microphone_delay() {
        use super::super::stream::Downsampler;
        use super::super::CpalBackend;
        use crate::speech::PlaybackEngine;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mic = MicController::new(Arc::new(CpalBackend), Arc::new(|_| {}), Timing::default());
        let status = mic.set_enabled(true, None);
        assert_eq!(status.state, super::super::MicState::Listening, "{status:?}");
        let sink = mic.capture_sink().expect("capture sink");
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        sink.attach_tap(tx);
        let heard: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let heard2 = heard.clone();
        let rate = sink.sample_rate();
        let collector = std::thread::spawn(move || {
            let mut down = Downsampler::new(rate);
            while let Ok(chunk) = rx.recv() {
                let mut out = Vec::new();
                down.push(&chunk, &mut out);
                heard2.lock().unwrap().extend(out);
            }
        });

        // What was played, and where in the microphone stream it began.
        let starts: Arc<Mutex<Vec<(usize, Arc<Vec<f32>>)>>> = Arc::new(Mutex::new(Vec::new()));
        let (s2, h3) = (starts.clone(), heard.clone());
        let playback = PlaybackEngine::new(0.5, false);
        playback.set_render_sink(Arc::new(move |cmd| {
            if let RenderCmd::Start(samples) = cmd {
                s2.lock().unwrap().push((h3.lock().unwrap().len(), samples));
            }
        }));
        std::thread::sleep(Duration::from_millis(1_500));

        let secs_list = [6usize, 2, 2, 3];
        for secs in secs_list {
            let wav = utterance::wav_bytes(&speechlike_reply(secs));
            let _ = rt.block_on(playback.play(wav));
            std::thread::sleep(Duration::from_millis(3_000));
        }
        mic.set_enabled(false, None);
        drop(sink);
        let _ = collector.join();

        let mic16 = heard.lock().unwrap().clone();
        // Keep the recording (what the microphone heard and what was played, on one timeline) so the canceller
        // can be tuned offline against this room: GAIA_ECHO_DUMP is the directory, default the temp directory.
        {
            let dir = std::path::PathBuf::from(std::env::var("GAIA_ECHO_DUMP").unwrap_or_else(|_| std::env::temp_dir().display().to_string()));
            let mut render = vec![0.0f32; mic16.len()];
            for (at, reference) in starts.lock().unwrap().iter() {
                for (i, v) in reference.iter().enumerate() {
                    if let Some(slot) = render.get_mut(at + i) {
                        *slot += *v;
                    }
                }
            }
            let bytes = |x: &[f32]| x.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
            std::fs::write(dir.join("echo-mic16.f32"), bytes(&mic16)).unwrap();
            std::fs::write(dir.join("echo-render16.f32"), bytes(&render)).unwrap();
            println!("recording kept in {}", dir.display());
        }
        let quiet_rms = (mic16[..16_000].iter().map(|v| v * v).sum::<f32>() / 16_000.0).sqrt();
        println!("room noise before playing: rms {quiet_rms:.5}");
        for (n, (at, reference)) in starts.lock().unwrap().iter().enumerate() {
            let len = (reference.len()).min(16_000 * 2); // correlate over the first 2 s
            let norm_ref: f32 = reference[..len].iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
            let (mut best_lag, mut best) = (0usize, 0f32);
            for lag in (0..24_000).step_by(16) {
                if at + lag + len > mic16.len() {
                    break;
                }
                let m = &mic16[at + lag..at + lag + len];
                let dot: f32 = reference[..len].iter().zip(m).map(|(a, b)| a * b).sum();
                let norm_m: f32 = m.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
                let c = (dot / (norm_ref * norm_m)).abs();
                if c > best {
                    best = c;
                    best_lag = lag;
                }
            }
            let loud = &mic16[at + best_lag..(at + best_lag + len).min(mic16.len())];
            let rms = (loud.iter().map(|v| v * v).sum::<f32>() / loud.len().max(1) as f32).sqrt();
            println!(
                "reply {} ({} s): her voice reaches the microphone {} ms after the play command (match {:.2}), level there rms {:.4} = {:.0}x the room",
                n + 1,
                secs_list[n],
                best_lag / 16,
                best,
                rms,
                rms / quiet_rms.max(1e-6)
            );
        }
    }

    /// Offline replay of a recording made by `measure_the_speaker_to_microphone_delay`: the same canceller and
    /// the same speech detector as the product, over what this room really did, with different settings.
    /// Nobody spoke during the recording, so every piece of speech the detector finds is Gaia hearing herself.
    #[test]
    #[ignore = "needs the recording written by measure_the_speaker_to_microphone_delay (GAIA_ECHO_DUMP)"]
    fn replay_the_recorded_room_with_different_canceller_settings() {
        use super::super::echo::relaxed_config;
        use aec3::api::config::EchoCanceller3Config;

        let dir = std::path::PathBuf::from(std::env::var("GAIA_ECHO_DUMP").unwrap_or_else(|_| std::env::temp_dir().display().to_string()));
        let load = |name: &str| -> Vec<f32> {
            std::fs::read(dir.join(name)).unwrap().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
        };
        let (mic, render) = (load("echo-mic16.f32"), load("echo-render16.f32"));
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt();
        let playing: Vec<bool> = render.chunks(BLOCK).map(|b| b.iter().any(|v| v.abs() > 1e-6)).collect();

        let mut variants: Vec<(String, EchoCanceller3Config)> = vec![
            ("relaxed x4 (the product now)".into(), relaxed_config(4.0)),
            ("relaxed x2".into(), relaxed_config(2.0)),
            ("relaxed x1".into(), relaxed_config(1.0)),
            ("AEC3 defaults".into(), EchoCanceller3Config::default()),
        ];
        for (blocks, k) in [(24usize, 4.0f32), (32, 4.0), (48, 4.0), (32, 2.0)] {
            let mut cfg = relaxed_config(k);
            cfg.filter.main.length_blocks = blocks;
            cfg.filter.main_initial.length_blocks = blocks.min(cfg.filter.main_initial.length_blocks.max(blocks / 2));
            cfg.filter.shadow.length_blocks = blocks.saturating_sub(2).max(12);
            variants.push((format!("relaxed x{k}, filter {blocks} blocks ({} ms)", blocks * 4), cfg));
        }

        for (name, config) in variants {
            let mut aec = EchoCanceller::with_config(config).expect("canceller");
            let mut clean = vec![0.0f32; mic.len()];
            for b in 0..mic.len() / BLOCK {
                let r = &render[b * BLOCK..(b + 1) * BLOCK];
                let m = &mic[b * BLOCK..(b + 1) * BLOCK];
                aec.process(r, m, &mut clean[b * BLOCK..(b + 1) * BLOCK]);
            }
            // how much of her voice is left while she plays
            let (mut raw_e, mut out_e, mut n) = (0f32, 0f32, 0usize);
            for (b, p) in playing.iter().enumerate() {
                if *p && (b + 1) * BLOCK <= mic.len() {
                    raw_e += mic[b * BLOCK..(b + 1) * BLOCK].iter().map(|v| v * v).sum::<f32>();
                    out_e += clean[b * BLOCK..(b + 1) * BLOCK].iter().map(|v| v * v).sum::<f32>();
                    n += BLOCK;
                }
            }
            let erle = 10.0 * (raw_e.max(1e-12) / out_e.max(1e-12)).log10();
            // what the product's detector makes of the cleaned signal
            let mut seg = Segmenter::new(SegmenterConfig::default());
            let (mut started, mut barge_like, mut was_long) = (0usize, 0usize, false);
            let mut utterances = 0usize;
            // where in the recording (seconds from the first sound she played) the detector finds speech
            let first_sound = render.iter().position(|v| v.abs() > 1e-6).unwrap_or(0);
            let mut at_secs: Vec<String> = Vec::new();
            for (fi, frame) in clean.chunks_exact(FRAME_SAMPLES).enumerate() {
                match seg.push_frame(frame) {
                    Some(SegEvent::Started) => {
                        started += 1;
                        let pos = fi * FRAME_SAMPLES;
                        // distance into the reply that is playing (or since the last sample of the last reply)
                        let last_play = render[..pos.min(render.len())].iter().rposition(|v| v.abs() > 1e-6);
                        at_secs.push(format!(
                            "{:.1}s{}",
                            pos.saturating_sub(first_sound) as f32 / 16_000.0,
                            match last_play {
                                Some(l) if pos > l => format!(" (+{:.1}s after she paused)", (pos - l) as f32 / 16_000.0),
                                _ => String::new(),
                            }
                        ));
                    }
                    Some(SegEvent::Utterance(_)) => utterances += 1,
                    _ => {}
                }
                let long = seg.in_speech() && seg.speech_ms() >= BARGE_IN_MS;
                if long && !was_long {
                    barge_like += 1;
                }
                was_long = long;
            }
            println!(
                "{name:<34} echo removed {erle:>5.1} dB over {:.0} s of playing | detector: {started} speech starts, {barge_like} would interrupt her, {utterances} utterances sent",
                n as f32 / 16_000.0
            );
            println!("    speech found at: {}", at_secs.join(", "));
            let _ = rms(&clean);
        }
    }

    #[test]
    #[ignore = "needs the recording written by measure_the_speaker_to_microphone_delay (GAIA_ECHO_DUMP)"]
    fn replay_the_recorded_room_with_the_echo_gate() {
        use super::super::echo::{relaxed_config, EchoGate};

        let dir = std::path::PathBuf::from(std::env::var("GAIA_ECHO_DUMP").unwrap_or_else(|_| std::env::temp_dir().display().to_string()));
        let load = |name: &str| -> Vec<f32> {
            std::fs::read(dir.join(name)).unwrap().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
        };
        let (mic, render) = (load("echo-mic16.f32"), load("echo-render16.f32"));
        let first_sound = render.iter().position(|v| v.abs() > 1e-6).unwrap_or(0);

        // Cleans `mic`, gates it like the product, runs the detector: (speech starts, would-interrupt count, start times in s)
        let run = |mic: &[f32], k: f32, gated: bool| -> (usize, usize, Vec<f32>) {
            let mut aec = EchoCanceller::with_config(super::super::echo::relaxed_config(k)).expect("canceller");
            let mut clean = vec![0.0f32; mic.len()];
            for b in 0..mic.len() / BLOCK {
                aec.process(&render[b * BLOCK..(b + 1) * BLOCK], &mic[b * BLOCK..(b + 1) * BLOCK], &mut clean[b * BLOCK..(b + 1) * BLOCK]);
            }
            let mut gate = EchoGate::new();
            let mut seg = Segmenter::new(SegmenterConfig::default());
            let (mut started, mut barge_like, mut was_long) = (0usize, 0usize, false);
            let mut times = Vec::new();
            for (fi, frame) in clean.chunks_exact(FRAME_SAMPLES).enumerate() {
                let mut frame = frame.to_vec();
                if gated {
                    gate.apply(&mut frame, &render[fi * FRAME_SAMPLES..(fi + 1) * FRAME_SAMPLES]);
                }
                if let Some(SegEvent::Started) = seg.push_frame(&frame) {
                    started += 1;
                    times.push((fi * FRAME_SAMPLES).saturating_sub(first_sound) as f32 / 16_000.0);
                }
                let long = seg.in_speech() && seg.speech_ms() >= BARGE_IN_MS;
                if long && !was_long {
                    barge_like += 1;
                }
                was_long = long;
            }
            let _ = relaxed_config(1.0);
            (started, barge_like, times)
        };

        // Nobody spoke in the recording: every detection is Gaia hearing herself.
        for k in [4.0f32, 2.0] {
            for gated in [false, true] {
                let (started, barge, times) = run(&mic, k, gated);
                println!("quiet room, relaxed x{k}, gate {}: {started} speech starts, {barge} would interrupt her, at {:?} s", if gated { "ON " } else { "off" }, times.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>());
            }
        }

        // The user talks over her: speech-like voice added at the microphone for 3 s in the middle of the long reply.
        let user = {
            let mut seed = 7u64;
            let mut rnd = move || {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
            };
            let (mut ph, mut env, mut pitch) = (0f32, 0f32, 210f32);
            (0..16_000 * 3)
                .map(|i| {
                    if i % 4_000 == 0 {
                        env = 0.7 + 0.3 * rnd().abs();
                    }
                    if i % 320 == 0 {
                        pitch = 210.0 * (1.0 + 0.15 * rnd());
                    }
                    ph += std::f32::consts::TAU * pitch / 16_000.0;
                    env * (1..=8).map(|h| (ph * h as f32).sin() / h as f32).sum::<f32>()
                })
                .collect::<Vec<f32>>()
        };
        let user_rms = (user.iter().map(|v| v * v).sum::<f32>() / user.len() as f32).sqrt();
        for target in [0.02f32, 0.04, 0.08] {
            let mut with_user = mic.clone();
            let from = first_sound + 16_000 * 2; // 2 s into the long reply
            for (i, v) in user.iter().enumerate() {
                with_user[from + i] += v * target / user_rms;
            }
            for gated in [false, true] {
                let (_, _, times) = run(&with_user, 4.0, gated);
                let caught = times.iter().any(|x| *x >= 2.0 && *x <= 5.5);
                println!("user talking over her at rms {target}, gate {}: caught = {caught} (detections at {:?} s)", if gated { "ON " } else { "off" }, times.iter().map(|x| (x * 10.0).round() / 10.0).collect::<Vec<_>>());
            }
        }
    }

    fn r_transcripts(log: &Log) -> usize {
        transcripts(log).len() + log.lock().unwrap().iter().filter(|e| matches!(e, ContinuousEvent::State(ListenState::Transcribing))).count()
    }
}
