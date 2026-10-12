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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::capture::CaptureSink;
use super::echo::{EchoCanceller, BLOCK};
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
/// How long after Gaia's voice ends her echo may still be decaying in the room (in 10 ms blocks).
const ECHO_TAIL_BLOCKS: u32 = 20;
/// With barge-in off, how long after Gaia stops before listening resumes (in 10 ms blocks).
const HALF_DUPLEX_COOLDOWN_BLOCKS: u32 = 40;

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
    let mut barged = false;
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
            if half.len() < FRAME_SAMPLES {
                continue;
            }
            let frame: Vec<f32> = half.drain(..FRAME_SAMPLES).collect();

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
                    hearing = true;
                    (inner.emit)(ContinuousEvent::State(ListenState::Hearing));
                }
                Some(SegEvent::TooShort) => {
                    hearing = false;
                    (inner.emit)(ContinuousEvent::State(ListenState::Listening));
                }
                Some(SegEvent::Utterance(samples)) => {
                    hearing = false;
                    submit(&inner, samples);
                    armed_until = armed_until.max(now + arm.think_hold as u64);
                }
                None => {}
            }
            if seg.in_speech() {
                armed_until = armed_until.max(now + arm.speech_extend as u64);
            }
            if speaking && !barged && seg.in_speech() && seg.speech_ms() >= BARGE_IN_MS {
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
    if enabled {
        match listener.start() {
            Ok(()) => {}
            Err(e) if e.code == "mic_off" => {} // saved; starts when the microphone is switched on
            Err(e) => return Err(e),
        }
    } else {
        listener.stop();
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

    fn r_transcripts(log: &Log) -> usize {
        transcripts(log).len() + log.lock().unwrap().iter().filter(|e| matches!(e, ContinuousEvent::State(ListenState::Transcribing))).count()
    }
}
