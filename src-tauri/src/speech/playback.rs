//! Gaia's voice, played from Rust.
//!
//! Playback moved out of the webview (spec section 4 / 5.5): the webview's `Audio` element hides the
//! playback signal, which echo cancellation will need as its reference, and its behaviour while the window
//! is hidden is outside our control. This engine owns the output device on a dedicated thread, applies the
//! local gain/mute (Windows lists the webview process, not "Gaia", in its volume mixer) and eases a
//! stopped reply out instead of clicking it off.
//!
//! Only one reply is ever spoken: a new reply supersedes the last, which is faded out while the new one starts.

use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use serde::Serialize;
use tokio::sync::oneshot;

/// Default time an interrupted reply takes to ease out. Short enough to read as instant.
pub const DEFAULT_FADE_MS: u64 = 150;
/// How often the playback thread checks for the end of a clip and advances fades.
const TICK: Duration = Duration::from_millis(16);
/// The output device is released this long after the last sound, so a changed default device is
/// picked up by the next reply.
const IDLE_RELEASE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "message", rename_all = "camelCase")]
pub enum PlayOutcome {
    /// The clip played to the end.
    Ended,
    /// The clip was stopped (new reply, new turn, user action). Not a failure.
    Stopped,
    /// The clip could not be decoded or the device could not be opened.
    Failed(String),
}

enum Cmd {
    Play {
        audio: Vec<u8>,
        done: oneshot::Sender<PlayOutcome>,
    },
    Stop {
        fade_ms: u64,
    },
    Gain {
        volume: f32,
        muted: bool,
    },
}

/// Volume at `elapsed` into a linear fade from `from` to silence over `fade`.
pub fn faded_volume(from: f32, elapsed: Duration, fade: Duration) -> f32 {
    if fade.is_zero() || elapsed >= fade {
        return 0.0;
    }
    from * (1.0 - elapsed.as_secs_f32() / fade.as_secs_f32())
}

/// The gain actually applied: muted or non-positive volume is silence; volume is clamped to 0..1.
pub fn effective_volume(volume: f32, muted: bool) -> f32 {
    if muted || volume.is_nan() {
        0.0
    } else {
        volume.clamp(0.0, 1.0)
    }
}

struct Playing {
    player: Player,
    done: oneshot::Sender<PlayOutcome>,
}

struct Fading {
    player: Player,
    done: Option<oneshot::Sender<PlayOutcome>>,
    began: Instant,
    from: f32,
    fade: Duration,
}

/// Handle to the playback thread. Cheap to share; commands are queued to the thread.
pub struct PlaybackEngine {
    tx: mpsc::Sender<Cmd>,
    /// Bumped by every `begin` and `cancel`; a synthesis that finishes after the epoch moved on is dropped.
    epoch: Arc<AtomicU64>,
}

impl PlaybackEngine {
    pub fn new(volume: f32, muted: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("gaia-playback".into())
            .spawn(move || run(rx, volume, muted))
            .expect("playback thread");
        Self {
            tx,
            epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Marks the start of a new reply (it supersedes any synthesis still in flight) and returns its epoch.
    pub fn begin(&self) -> u64 {
        self.epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// True while `epoch` is still the newest: nothing started or stopped since.
    pub fn is_current(&self, epoch: u64) -> bool {
        self.epoch.load(Ordering::SeqCst) == epoch
    }

    /// Plays `audio` (any format Symphonia can probe: wav, mp3, ...). Resolves when it ends or is stopped.
    pub async fn play(&self, audio: Vec<u8>) -> PlayOutcome {
        let (done, wait) = oneshot::channel();
        if self.tx.send(Cmd::Play { audio, done }).is_err() {
            return PlayOutcome::Failed("playback thread is gone".into());
        }
        wait.await.unwrap_or(PlayOutcome::Stopped)
    }

    /// Stops what is playing (easing it out over `fade_ms`) and cancels any reply still being synthesised.
    pub fn stop(&self, fade_ms: u64) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Stop { fade_ms });
    }

    /// Eases out what is playing without touching the epoch (used when a new reply supersedes it).
    pub fn stop_current(&self, fade_ms: u64) {
        let _ = self.tx.send(Cmd::Stop { fade_ms });
    }

    pub fn set_gain(&self, volume: f32, muted: bool) {
        let _ = self.tx.send(Cmd::Gain { volume, muted });
    }
}

fn run(rx: mpsc::Receiver<Cmd>, volume: f32, muted: bool) {
    let mut gain = effective_volume(volume, muted);
    let mut sink: Option<MixerDeviceSink> = None;
    let mut current: Option<Playing> = None;
    let mut fading: Vec<Fading> = Vec::new();
    let mut last_active = Instant::now();

    let start_fade = |p: Playing, ms: u64, gain: f32, fading: &mut Vec<Fading>| {
        if ms == 0 {
            p.player.stop();
            let _ = p.done.send(PlayOutcome::Stopped);
        } else {
            fading.push(Fading {
                player: p.player,
                done: Some(p.done),
                began: Instant::now(),
                from: gain,
                fade: Duration::from_millis(ms),
            });
        }
    };

    loop {
        match rx.recv_timeout(TICK) {
            Ok(Cmd::Play { audio, done }) => {
                if let Some(old) = current.take() {
                    start_fade(old, DEFAULT_FADE_MS, gain, &mut fading);
                }
                if sink.is_none() {
                    match DeviceSinkBuilder::open_default_sink() {
                        Ok(mut s) => {
                            s.log_on_drop(false);
                            sink = Some(s);
                        }
                        Err(e) => {
                            let _ = done.send(PlayOutcome::Failed(format!("could not open the speakers: {e}")));
                            continue;
                        }
                    }
                }
                let mixer = sink.as_ref().expect("sink just opened").mixer();
                match Decoder::new(Cursor::new(audio)) {
                    Ok(source) => {
                        let player = Player::connect_new(mixer);
                        player.set_volume(gain);
                        player.append(source);
                        current = Some(Playing { player, done });
                        last_active = Instant::now();
                    }
                    Err(e) => {
                        let _ = done.send(PlayOutcome::Failed(format!("could not decode the audio: {e}")));
                    }
                }
            }
            Ok(Cmd::Stop { fade_ms }) => {
                if let Some(p) = current.take() {
                    start_fade(p, fade_ms, gain, &mut fading);
                }
            }
            Ok(Cmd::Gain { volume, muted }) => {
                gain = effective_volume(volume, muted);
                if let Some(p) = &current {
                    p.player.set_volume(gain);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Natural end of the current clip.
        if current.as_ref().map(|p| p.player.empty()).unwrap_or(false) {
            let p = current.take().expect("checked above");
            let _ = p.done.send(PlayOutcome::Ended);
            last_active = Instant::now();
        }
        // Advance fades; a finished fade stops its player and settles the waiting call as Stopped.
        fading.retain_mut(|f| {
            let elapsed = f.began.elapsed();
            if elapsed >= f.fade {
                f.player.stop();
                if let Some(done) = f.done.take() {
                    let _ = done.send(PlayOutcome::Stopped);
                }
                last_active = Instant::now();
                false
            } else {
                f.player.set_volume(faded_volume(f.from, elapsed, f.fade));
                true
            }
        });
        if current.is_none() && fading.is_empty() && sink.is_some() && last_active.elapsed() > IDLE_RELEASE {
            sink = None; // release the output device
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_goes_linearly_from_the_current_gain_to_silence() {
        let fade = Duration::from_millis(100);
        assert!((faded_volume(1.0, Duration::ZERO, fade) - 1.0).abs() < 1e-6);
        assert!((faded_volume(1.0, Duration::from_millis(50), fade) - 0.5).abs() < 1e-3);
        assert!((faded_volume(0.4, Duration::from_millis(50), fade) - 0.2).abs() < 1e-3);
        assert_eq!(faded_volume(1.0, fade, fade), 0.0);
        assert_eq!(faded_volume(1.0, Duration::from_secs(5), fade), 0.0);
    }

    #[test]
    fn a_zero_length_fade_is_an_immediate_stop() {
        assert_eq!(faded_volume(1.0, Duration::ZERO, Duration::ZERO), 0.0);
    }

    #[test]
    fn effective_volume_clamps_and_honours_mute() {
        assert_eq!(effective_volume(0.4, false), 0.4);
        assert_eq!(effective_volume(5.0, false), 1.0);
        assert_eq!(effective_volume(-3.0, false), 0.0);
        assert_eq!(effective_volume(0.9, true), 0.0);
        assert_eq!(effective_volume(f32::NAN, false), 0.0);
    }

    #[test]
    fn stop_and_begin_invalidate_a_synthesis_still_in_flight() {
        let engine = PlaybackEngine::new(1.0, false);
        let first = engine.begin();
        assert!(engine.is_current(first));
        engine.stop(0);
        assert!(!engine.is_current(first), "a stop cancels the pending reply");
        let second = engine.begin();
        let third = engine.begin();
        assert!(!engine.is_current(second), "a newer reply supersedes an older one");
        assert!(engine.is_current(third));
    }

    /// 0.6 s of 440 Hz mono 16-bit PCM WAV, built by hand.
    fn tone_wav(secs: f32) -> Vec<u8> {
        let rate = 16_000u32;
        let n = (rate as f32 * secs) as usize;
        let mut pcm = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 3000.0) as i16;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        wav
    }

    #[tokio::test]
    async fn undecodable_audio_fails_calmly_without_touching_the_speakers() {
        let engine = PlaybackEngine::new(1.0, false);
        // The output device is opened before decoding, so on a machine without speakers this is a
        // device failure; either way the call settles as Failed and never panics or hangs.
        let outcome = tokio::time::timeout(Duration::from_secs(5), engine.play(vec![1, 2, 3])).await;
        assert!(matches!(outcome, Ok(PlayOutcome::Failed(_))), "{outcome:?}");
    }

    /// Opt-in hardware test (plays a 0.5 s tone quietly): `cargo test --lib real_speakers -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "plays sound through the default speakers"]
    async fn real_speakers_play_to_the_end_and_stop_with_a_fade() {
        let engine = PlaybackEngine::new(0.1, false);
        let ended = engine.play(tone_wav(0.5)).await;
        println!("natural end: {ended:?}");
        assert_eq!(ended, PlayOutcome::Ended);

        let long = tone_wav(5.0);
        let started = Instant::now();
        let pending = {
            let e = &engine;
            async move { e.play(long).await }
        };
        let (outcome, _) = tokio::join!(pending, async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            engine.stop(150);
        });
        println!("stopped after {:?}: {outcome:?}", started.elapsed());
        assert_eq!(outcome, PlayOutcome::Stopped);
        assert!(started.elapsed() < Duration::from_secs(2), "the fade must end the clip promptly");
    }
}
