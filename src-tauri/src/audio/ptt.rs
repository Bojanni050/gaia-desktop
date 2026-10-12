//! Push-to-talk: record one utterance, have Gaia Cloud transcribe it, hand the text back.
//!
//! The desktop only moves audio and text. It never decides what the words mean or whether Gaia
//! answers: the transcript goes back to the UI, which sends it as an ordinary turn.
//!
//! Privacy, in the order it is enforced:
//! 1. Nothing records unless the microphone master switch is on **and** the user started a recording.
//! 2. Audio lives only in memory, only for the length of the recording. No file is ever written.
//! 3. A recording with no speech in it is never uploaded ([`utterance::prepare`] says `Silence`).
//! 4. Only the speech (plus a short margin) is uploaded, as one 16 kHz mono WAV.
//! 5. Switching the microphone off, or a device change, ends a recording and discards it.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;

use super::capture::CaptureSink;
use super::utterance::{self, Prepared};
use super::{MicController, VoiceError};

/// Tauri event names.
pub const PTT_EVENT: &str = "voice://ptt";
pub const TRANSCRIPT_EVENT: &str = "voice://transcript";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscribeError {
    pub code: String,
    pub message: String,
}

impl TranscribeError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

/// Where utterances go to become text (Gaia Cloud in production, a fake in tests).
#[async_trait]
pub trait Transcriber: Send + Sync + 'static {
    async fn transcribe(&self, wav: Vec<u8>) -> Result<Transcript, TranscribeError>;
}

/// What the UI shows. `Failed` and `NoSpeech` are terminal for one recording; the next `start` begins afresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum PttState {
    Idle,
    Recording,
    Transcribing,
    NoSpeech,
    Failed { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PttEvent {
    State(PttState),
    Transcript(Transcript),
}

type Emitter = Arc<dyn Fn(PttEvent) + Send + Sync>;
/// Runs a future to completion in the background (the Tauri runtime in production).
pub type Spawner = Arc<dyn Fn(Pin<Box<dyn Future<Output = ()> + Send>>) + Send + Sync>;

struct Recording {
    id: u64,
    rx: Receiver<Vec<f32>>,
    sink: Arc<CaptureSink>,
}

enum Phase {
    Idle,
    Recording(Recording),
    Transcribing,
}

struct Inner {
    mic: MicController,
    transcriber: Arc<dyn Transcriber>,
    emit: Emitter,
    spawn: Spawner,
    max_duration: Duration,
    phase: Mutex<Phase>,
    next_id: AtomicU64,
}

#[derive(Clone)]
pub struct PttController {
    inner: Arc<Inner>,
}

impl PttController {
    pub fn new(
        mic: MicController,
        transcriber: Arc<dyn Transcriber>,
        emit: Emitter,
        spawn: Spawner,
        max_duration: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                mic,
                transcriber,
                emit,
                spawn,
                max_duration,
                phase: Mutex::new(Phase::Idle),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    pub fn state(&self) -> PttState {
        match &*self.inner.phase.lock().expect("ptt poisoned") {
            Phase::Idle => PttState::Idle,
            Phase::Recording(_) => PttState::Recording,
            Phase::Transcribing => PttState::Transcribing,
        }
    }

    /// Starts recording. Fails with `mic_off` unless the microphone is on and healthy, `busy` unless idle.
    pub fn start(&self) -> Result<(), VoiceError> {
        let id = {
            let mut phase = self.inner.phase.lock().expect("ptt poisoned");
            if !matches!(*phase, Phase::Idle) {
                return Err(VoiceError {
                    code: "busy".into(),
                    message: "a recording is already in progress".into(),
                });
            }
            let Some(sink) = self.inner.mic.capture_sink() else {
                return Err(VoiceError {
                    code: "mic_off".into(),
                    message: "switch the microphone on first".into(),
                });
            };
            if sink.sample_rate() == 0 {
                return Err(VoiceError {
                    code: "mic_off".into(),
                    message: "the microphone is not ready yet".into(),
                });
            }
            let (tx, rx) = mpsc::channel();
            sink.attach_tap(tx);
            let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
            *phase = Phase::Recording(Recording { id, rx, sink });
            id
        };
        self.inner.emit_state(PttState::Recording);

        // A recording ends by itself: Cloud accepts about 30 s of audio.
        let this = self.clone();
        let max = self.inner.max_duration;
        std::thread::Builder::new()
            .name("gaia-ptt-limit".into())
            .spawn(move || {
                std::thread::sleep(max);
                let still_this_recording = matches!(
                    &*this.inner.phase.lock().expect("ptt poisoned"),
                    Phase::Recording(r) if r.id == id
                );
                if still_this_recording {
                    this.stop();
                }
            })
            .ok();
        Ok(())
    }

    /// Ends the recording and sends it for transcription. The result arrives as events. No-op unless recording.
    pub fn stop(&self) {
        let recording = {
            let mut phase = self.inner.phase.lock().expect("ptt poisoned");
            match std::mem::replace(&mut *phase, Phase::Transcribing) {
                Phase::Recording(r) => r,
                other => {
                    *phase = other;
                    return;
                }
            }
        };
        recording.sink.detach_tap();

        // The microphone must be the same one we started on: switched off, failed or restarted
        // (a new sink) means the audio has a hole in it and is thrown away.
        let same = self
            .inner
            .mic
            .capture_sink()
            .map(|now| Arc::ptr_eq(&now, &recording.sink))
            .unwrap_or(false);
        if !same {
            drop(recording);
            self.inner.finish_with(PttState::Failed {
                code: "mic_interrupted".into(),
                message: "the microphone was interrupted during the recording".into(),
            });
            return;
        }

        self.inner.emit_state(PttState::Transcribing);
        let inner = self.inner.clone();
        (self.inner.spawn)(Box::pin(async move {
            let mut samples: Vec<f32> = Vec::new();
            while let Ok(chunk) = recording.rx.try_recv() {
                samples.extend_from_slice(&chunk);
            }
            let rate = recording.sink.sample_rate();
            drop(recording);
            inner.transcribe(samples, rate).await;
        }));
    }

    /// Throws a recording in progress away (nothing is sent). No-op unless recording.
    pub fn cancel(&self) {
        let recording = {
            let mut phase = self.inner.phase.lock().expect("ptt poisoned");
            match std::mem::replace(&mut *phase, Phase::Idle) {
                Phase::Recording(r) => Some(r),
                other => {
                    *phase = other;
                    None
                }
            }
        };
        if let Some(r) = recording {
            r.sink.detach_tap();
            drop(r);
            self.inner.emit_state(PttState::Idle);
        }
    }
}

impl Inner {
    fn emit_state(&self, state: PttState) {
        (self.emit)(PttEvent::State(state));
    }

    /// Returns to idle first (so a listener may start again at once), then announces how it ended.
    fn finish_with(&self, state: PttState) {
        *self.phase.lock().expect("ptt poisoned") = Phase::Idle;
        self.emit_state(state);
    }

    async fn transcribe(&self, samples: Vec<f32>, rate: u32) {
        let wav = match utterance::prepare(&samples, rate) {
            Prepared::Silence => return self.finish_with(PttState::NoSpeech),
            Prepared::Speech { wav, .. } => wav,
        };
        drop(samples);
        match self.transcriber.transcribe(wav).await {
            Ok(t) if t.text.trim().is_empty() => self.finish_with(PttState::NoSpeech),
            Ok(t) => {
                *self.phase.lock().expect("ptt poisoned") = Phase::Idle;
                (self.emit)(PttEvent::Transcript(t));
                self.emit_state(PttState::Idle);
            }
            Err(e) => self.finish_with(PttState::Failed {
                code: e.code,
                message: e.message,
            }),
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Tauri commands
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn voice_ptt_start(ptt: tauri::State<'_, PttController>) -> Result<(), VoiceError> {
    ptt.start()
}

#[tauri::command]
pub fn voice_ptt_stop(ptt: tauri::State<'_, PttController>) {
    ptt.stop();
}

#[tauri::command]
pub fn voice_ptt_cancel(ptt: tauri::State<'_, PttController>) {
    ptt.cancel();
}

#[tauri::command]
pub fn voice_ptt_state(ptt: tauri::State<'_, PttController>) -> PttState {
    ptt.state()
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, Mode};
    use super::super::Timing;
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::runtime::Handle;

    struct FakeTranscriber {
        calls: AtomicUsize,
        last_wav: Mutex<Option<Vec<u8>>>,
        reply: Mutex<Result<Transcript, TranscribeError>>,
    }

    impl FakeTranscriber {
        fn replying(text: &str) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                last_wav: Mutex::new(None),
                reply: Mutex::new(Ok(Transcript {
                    text: text.into(),
                    language: Some("nl".into()),
                })),
            })
        }
        fn failing(code: &str) -> Arc<Self> {
            let t = Self::replying("");
            *t.reply.lock().unwrap() = Err(TranscribeError::new(code, "boom"));
            t
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Transcriber for FakeTranscriber {
        async fn transcribe(&self, wav: Vec<u8>) -> Result<Transcript, TranscribeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_wav.lock().unwrap() = Some(wav);
            self.reply.lock().unwrap().clone()
        }
    }

    type Log = Arc<Mutex<Vec<PttEvent>>>;

    struct Rig {
        fake: Arc<Fake>,
        mic: MicController,
        ptt: PttController,
        log: Log,
        transcriber: Arc<FakeTranscriber>,
    }

    fn rig(mode: Mode, transcriber: Arc<FakeTranscriber>, max: Duration) -> Rig {
        let fake = Fake::new(mode);
        let mic = MicController::new(
            fake.clone(),
            Arc::new(|_| {}),
            Timing {
                poll: Duration::from_millis(5),
                first_frames_timeout: Duration::from_millis(500),
                no_frames_timeout: Duration::from_millis(500),
                max_restarts: 3,
                restart_window: Duration::from_secs(60),
            },
        );
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let handle = Handle::current();
        let ptt = PttController::new(
            mic.clone(),
            transcriber.clone(),
            Arc::new(move |e| l2.lock().unwrap().push(e)),
            Arc::new(move |f| {
                handle.spawn(f);
            }),
            max,
        );
        Rig { fake, mic, ptt, log, transcriber }
    }

    async fn wait_for(what: &str, cond: impl Fn() -> bool) {
        for _ in 0..600 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    fn states(log: &Log) -> Vec<PttState> {
        log.lock()
            .unwrap()
            .iter()
            .filter_map(|e| if let PttEvent::State(s) = e { Some(s.clone()) } else { None })
            .collect()
    }

    fn transcripts(log: &Log) -> Vec<Transcript> {
        log.lock()
            .unwrap()
            .iter()
            .filter_map(|e| if let PttEvent::Transcript(t) = e { Some(t.clone()) } else { None })
            .collect()
    }

    /// Opt-in hardware test: `cargo test --lib real_microphone_recording -- --ignored --nocapture`.
    /// Records two seconds from the real microphone through the tap and checks the audio is the right
    /// length at the device's own rate and survives the trip to a 16 kHz WAV. Run from a normal terminal.
    #[test]
    #[ignore = "needs a real microphone"]
    fn real_microphone_recording_delivers_two_seconds_of_audio() {
        use super::super::{AudioPermission, CpalBackend};
        let mic = MicController::new(Arc::new(CpalBackend), Arc::new(|_| {}), Timing::default());
        let status = mic.set_enabled(true, None);
        assert_eq!(status.state, super::super::MicState::Listening, "{status:?}");
        let t0 = std::time::Instant::now();
        while mic.status().permission != AudioPermission::Granted && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let sink = mic.capture_sink().expect("sink while listening");
        let rate = sink.sample_rate();
        assert!(rate >= 8_000, "sample rate {rate}");

        let (tx, rx) = mpsc::channel();
        sink.attach_tap(tx);
        std::thread::sleep(Duration::from_secs(2));
        sink.detach_tap();
        mic.set_enabled(false, None);

        let mut samples: Vec<f32> = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            samples.extend_from_slice(&chunk);
        }
        let secs = samples.len() as f32 / rate as f32;
        let peak = samples.iter().fold(0f32, |a, s| a.max(s.abs()));
        println!("rate {rate} Hz, {} samples = {secs:.2} s, peak {peak:.4}", samples.len());
        assert!((1.7..2.3).contains(&secs), "expected about 2 s of audio, got {secs}");
        match utterance::prepare(&samples, rate) {
            Prepared::Speech { wav, seconds } => println!("heard speech: {seconds:.2} s -> {} byte WAV", wav.len()),
            Prepared::Silence => println!("no speech in those two seconds (silence is never uploaded)"),
        }
    }

    async fn record(r: &Rig, ms: u64) {
        r.ptt.start().expect("start");
        tokio::time::sleep(Duration::from_millis(ms)).await;
        r.ptt.stop();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn it_cannot_start_while_the_microphone_is_off() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("x"), Duration::from_secs(30));
        let err = r.ptt.start().unwrap_err();
        assert_eq!(err.code, "mic_off");
        assert_eq!(r.ptt.state(), PttState::Idle);
        assert!(states(&r.log).is_empty(), "nothing happened, nothing was announced");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_spoken_recording_becomes_a_transcript_through_the_expected_states() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("hallo daar"), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        record(&r, 600).await;

        wait_for("the transcript", || !transcripts(&r.log).is_empty()).await;
        assert_eq!(transcripts(&r.log)[0].text, "hallo daar");
        wait_for("back to idle", || states(&r.log).last() == Some(&PttState::Idle)).await;
        assert_eq!(
            states(&r.log),
            vec![PttState::Recording, PttState::Transcribing, PttState::Idle]
        );
        assert_eq!(r.transcriber.calls(), 1);
        // What was uploaded is one canonical 16 kHz mono WAV of the speech
        let wav = r.transcriber.last_wav.lock().unwrap().clone().unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_silent_recording_is_never_uploaded() {
        let r = rig(Mode::Quiet, FakeTranscriber::replying("should never be asked"), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        record(&r, 400).await;

        wait_for("no speech", || states(&r.log).contains(&PttState::NoSpeech)).await;
        assert_eq!(r.transcriber.calls(), 0, "silence must not leave the machine");
        assert!(transcripts(&r.log).is_empty());
        assert_eq!(r.ptt.state(), PttState::Idle);
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_transcript_is_reported_as_no_speech() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("   "), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        record(&r, 500).await;
        wait_for("no speech", || states(&r.log).contains(&PttState::NoSpeech)).await;
        assert!(transcripts(&r.log).is_empty());
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_transcription_is_reported_and_the_next_recording_works() {
        let transcriber = FakeTranscriber::failing("unreachable");
        let r = rig(Mode::Frames, transcriber.clone(), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        record(&r, 500).await;
        wait_for("failure", || {
            states(&r.log).iter().any(|s| matches!(s, PttState::Failed { code, .. } if code == "unreachable"))
        })
        .await;
        assert_eq!(r.ptt.state(), PttState::Idle);

        *transcriber.reply.lock().unwrap() = Ok(Transcript { text: "tweede keer".into(), language: None });
        record(&r, 500).await;
        wait_for("second transcript", || !transcripts(&r.log).is_empty()).await;
        assert_eq!(transcripts(&r.log)[0].text, "tweede keer");
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelling_discards_the_recording_and_detaches_the_tap() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("x"), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        r.ptt.start().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        r.ptt.cancel();

        assert_eq!(r.ptt.state(), PttState::Idle);
        assert_eq!(states(&r.log), vec![PttState::Recording, PttState::Idle]);
        assert!(!r.fake.last_sink.lock().unwrap().as_ref().unwrap().is_tapping(), "audio stops flowing to us");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(r.transcriber.calls(), 0);
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_start_while_recording_is_refused() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("x"), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        r.ptt.start().unwrap();
        assert_eq!(r.ptt.start().unwrap_err().code, "busy");
        r.ptt.cancel();
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_and_cancel_do_nothing_when_idle() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("x"), Duration::from_secs(30));
        r.ptt.stop();
        r.ptt.cancel();
        assert!(states(&r.log).is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_recording_ends_by_itself_at_the_time_limit() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("lang verhaal"), Duration::from_millis(250));
        r.mic.set_enabled(true, None);
        r.ptt.start().unwrap();
        wait_for("automatic transcript", || !transcripts(&r.log).is_empty()).await;
        assert_eq!(transcripts(&r.log)[0].text, "lang verhaal");
        r.mic.set_enabled(false, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn switching_the_microphone_off_mid_recording_discards_it() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("x"), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        r.ptt.start().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        r.mic.set_enabled(false, None);
        r.ptt.stop();

        wait_for("interrupted", || {
            states(&r.log).iter().any(|s| matches!(s, PttState::Failed { code, .. } if code == "mic_interrupted"))
        })
        .await;
        assert_eq!(r.transcriber.calls(), 0, "audio from an interrupted microphone is never sent");
        assert_eq!(r.ptt.state(), PttState::Idle);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_device_change_mid_recording_discards_it() {
        let r = rig(Mode::Frames, FakeTranscriber::replying("x"), Duration::from_secs(30));
        r.mic.set_enabled(true, None);
        r.ptt.start().unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        // the stream fails and the controller restarts it on a fresh sink
        r.fake.last_sink.lock().unwrap().as_ref().unwrap().fail();
        tokio::time::sleep(Duration::from_millis(150)).await;
        r.ptt.stop();

        wait_for("interrupted", || {
            states(&r.log).iter().any(|s| matches!(s, PttState::Failed { code, .. } if code == "mic_interrupted"))
        })
        .await;
        assert_eq!(r.transcriber.calls(), 0);
        r.mic.set_enabled(false, None);
    }
}
