//! Gaia's voice, played from Rust.
//!
//! Playback moved out of the webview (spec section 4 / 5.5): the webview's `Audio` element hides the
//! playback signal, which echo cancellation will need as its reference, and its behaviour while the window
//! is hidden is outside our control. This engine owns the output device on a dedicated thread, applies the
//! local gain/mute (Windows lists the webview process, not "Gaia", in its volume mixer) and eases a
//! stopped reply out instead of clicking it off.
//!
//! Only one reply is ever spoken: a new reply supersedes the last, which is faded out while the new one starts.
//!
//! A reply can also be played while it is *still being synthesized* ([`PlaybackEngine::stream`]):
//! its samples are pushed in as they arrive over HTTP, so the first words are heard long before the
//! provider is done. There is no complete clip to decode then, so the source that pulls from the
//! arriving queue reports the echo canceller's reference itself, in step with what is played.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};
use serde::Serialize;
use tokio::sync::oneshot;

use cpal::traits::{DeviceTrait, HostTrait};

use crate::audio::capture::{info_for, AudioDeviceInfo};
use crate::audio::continuous::{RenderCmd, RenderSink};
use crate::audio::stream::Downsampler;

/// Where playback reports what Gaia is playing (the echo canceller's reference). Set once, after the
/// hands-free listener exists; shared with the playback thread.
type SharedRender = Arc<Mutex<Option<RenderSink>>>;

/// Default time an interrupted reply takes to ease out. Short enough to read as instant.
pub const DEFAULT_FADE_MS: u64 = 150;
/// How often the playback thread checks for the end of a clip and advances fades.
const TICK: Duration = Duration::from_millis(16);
/// The output device is released this long after the last sound, so a changed default device is
/// picked up by the next reply.
const IDLE_RELEASE: Duration = Duration::from_secs(5);
/// Samples the streaming source takes from the arriving queue at a time (about 10 ms at 24 kHz).
const STREAM_REFILL: usize = 256;
/// While nothing has arrived, this many silent samples are yielded before the queue is looked at
/// again (about 1 ms at 24 kHz), so waiting for the provider costs one lock per millisecond, not one
/// per sample.
const STREAM_SILENCE_RUN: usize = 32;
/// The echo reference is reported in blocks of this many 16 kHz samples (10 ms), the granularity the
/// canceller consumes anyway.
const REFERENCE_BLOCK: usize = 160;

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
    /// Starts a reply that is still being synthesized; the samples follow as `StreamChunk`s.
    StreamBegin {
        sample_rate: u32,
        /// `Ok` once the reply is playing, `Err` with the outcome when it could not start at all.
        started: oneshot::Sender<Result<(), PlayOutcome>>,
        done: oneshot::Sender<PlayOutcome>,
    },
    /// One decoded piece of the reply that is arriving (mono f32 at the stream's own rate).
    StreamChunk {
        samples: Vec<f32>,
    },
    /// The reply is complete: what is queued plays out and then the reply ends.
    StreamEnd,
    Stop {
        fade_ms: u64,
    },
    Gain {
        volume: f32,
        muted: bool,
    },
    /// Switches the output device (`None` = system default). Whatever is playing is cut off.
    Device(Option<String>),
}

/// The speakers/headphones Gaia's voice can be played through. `is_default` marks the system default.
pub fn list_output_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|d| d.id().ok())
        .map(|i| i.to_string());
    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|d| info_for(&d, default_id.as_deref()))
                .collect()
        })
        .unwrap_or_default()
}

/// Opens the chosen output device, or the system default when none is chosen or the chosen one is
/// gone or will not open (a saved device that was unplugged must not silence Gaia).
fn open_sink(wanted: Option<&str>) -> Result<MixerDeviceSink, rodio::DeviceSinkError> {
    let chosen = wanted.and_then(|id| {
        let device = cpal::default_host().device_by_id(&id.parse::<cpal::DeviceId>().ok()?)?;
        DeviceSinkBuilder::from_device(device)
            .ok()?
            .open_sink_or_fallback()
            .ok()
    });
    match chosen {
        Some(sink) => Ok(sink),
        None => DeviceSinkBuilder::open_default_sink(),
    }
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

/// The samples of a reply that is still arriving: the HTTP task pushes what the provider has
/// synthesized so far, the mixer pulls them as the speakers need them.
struct StreamQueue {
    pending: Mutex<VecDeque<f32>>,
    finished: AtomicBool,
}

impl StreamQueue {
    fn new() -> Self {
        Self {
            pending: Mutex::new(VecDeque::new()),
            finished: AtomicBool::new(false),
        }
    }

    fn push(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        self.pending
            .lock()
            .expect("stream poisoned")
            .extend(samples.iter().copied());
    }

    /// The reply will not grow any further: the source ends once what is queued has played.
    fn finish(&self) {
        self.finished.store(true, Ordering::SeqCst);
    }

    /// Moves up to `limit` queued samples into `out`; returns how many it moved (nothing means
    /// nothing has arrived *yet*).
    fn take(&self, out: &mut VecDeque<f32>, limit: usize) -> usize {
        let mut pending = self.pending.lock().expect("stream poisoned");
        let take = pending.len().min(limit);
        out.extend(pending.drain(..take));
        take
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

/// The audio of a reply that is played while it is still being synthesized.
///
/// The mixer pulls samples from the arriving queue; while nothing has arrived this yields silence —
/// the reply has not ended, it just is not complete yet — and it ends only once the task has said
/// the reply is over and everything queued has played. Because there is no complete clip to report
/// in advance, this is also where the echo canceller's reference comes from: exactly the samples the
/// speakers are handed, silence included, so the canceller's timeline matches the room's.
struct StreamSource {
    queue: Arc<StreamQueue>,
    rate: u32,
    buffer: VecDeque<f32>,
    /// Samples waiting to go through the downsampler.
    feed: Vec<f32>,
    /// 16 kHz reference samples not reported yet.
    reference: Vec<f32>,
    downsampler: Downsampler,
    /// Silent samples still to be yielded before the queue is looked at again.
    silent_run: usize,
    /// The first block reported *starts* the reply (it plays from now on, possibly over a reply that
    /// is still fading out); every block after it follows on the same timeline.
    first: bool,
    report: Arc<dyn Fn(RenderCmd) + Send + Sync>,
}

impl StreamSource {
    fn new(queue: Arc<StreamQueue>, rate: u32, report: Arc<dyn Fn(RenderCmd) + Send + Sync>) -> Self {
        let rate = rate.max(1);
        Self {
            queue,
            rate,
            buffer: VecDeque::new(),
            feed: Vec::new(),
            reference: Vec::new(),
            downsampler: Downsampler::new(rate),
            silent_run: 0,
            first: true,
            report,
        }
    }

    /// Hands one sample to the caller, keeping the canceller's reference in step with it.
    fn played(&mut self, sample: f32) -> f32 {
        self.feed.push(sample);
        if self.feed.len() >= REFERENCE_BLOCK {
            self.flush_feed();
        }
        if self.reference.len() >= REFERENCE_BLOCK {
            self.report_reference();
        }
        sample
    }

    /// Pushes whatever still waits in `feed` through the downsampler.
    fn flush_feed(&mut self) {
        if self.feed.is_empty() {
            return;
        }
        let feed = std::mem::take(&mut self.feed);
        self.downsampler.push(&feed, &mut self.reference);
    }

    fn report_reference(&mut self) {
        if self.reference.is_empty() {
            return;
        }
        let block: Vec<f32> = self.reference.drain(..).collect();
        let cmd = if self.first {
            self.first = false;
            RenderCmd::Start(Arc::new(block))
        } else {
            RenderCmd::Append(Arc::new(block))
        };
        (self.report)(cmd);
    }
}

impl Iterator for StreamSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        loop {
            if let Some(sample) = self.buffer.pop_front() {
                return Some(self.played(sample));
            }
            // Nothing buffered. The queue is only looked at when no silent run is in progress, so
            // waiting for the provider costs one lock per run rather than one per sample.
            if self.silent_run == 0 {
                if self.queue.take(&mut self.buffer, STREAM_REFILL) > 0 {
                    continue;
                }
                if self.queue.is_finished() {
                    // Everything the reply held has played: report the last of it and end.
                    self.flush_feed();
                    self.report_reference();
                    return None;
                }
                self.silent_run = STREAM_SILENCE_RUN;
            }
            // Still being synthesized: hold the speakers' timeline with silence rather than ending
            // the reply mid-sentence.
            self.silent_run -= 1;
            return Some(self.played(0.0));
        }
    }
}

impl Source for StreamSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).expect("one channel")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.rate).expect("a non-zero sample rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// A reply that is still arriving, from the caller's side: push what has been decoded, then say the
/// reply is complete. Dropping the handle does not stop playback — the reply plays out what it has.
pub struct StreamHandle {
    tx: mpsc::Sender<Cmd>,
    done: oneshot::Receiver<PlayOutcome>,
}

impl StreamHandle {
    /// Hands one decoded chunk (mono f32 at the stream's own rate) to the player.
    pub fn push(&self, samples: Vec<f32>) {
        let _ = self.tx.send(Cmd::StreamChunk { samples });
    }

    /// The reply is complete: what is queued plays out, then the reply ends.
    pub fn finish(&self) {
        let _ = self.tx.send(Cmd::StreamEnd);
    }

    /// Resolves when the reply played to its end or was stopped.
    pub async fn wait(self) -> PlayOutcome {
        self.done.await.unwrap_or(PlayOutcome::Stopped)
    }
}

/// Handle to the playback thread. Cheap to share; commands are queued to the thread.
pub struct PlaybackEngine {
    tx: mpsc::Sender<Cmd>,
    render: SharedRender,
    /// Bumped by every `begin` and `cancel`; a synthesis that finishes after the epoch moved on is dropped.
    epoch: Arc<AtomicU64>,
}

impl PlaybackEngine {
    pub fn new(volume: f32, muted: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let render: SharedRender = Arc::new(Mutex::new(None));
        let thread_render = render.clone();
        std::thread::Builder::new()
            .name("gaia-playback".into())
            .spawn(move || run(rx, volume, muted, thread_render))
            .expect("playback thread");
        Self {
            tx,
            render,
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

    /// Starts a reply that is still being synthesized: [`StreamHandle::push`] feeds it the samples as
    /// they arrive and [`StreamHandle::finish`] says the reply is complete, after which it plays out.
    ///
    /// `rate` is the stream's own sample rate (chunks are mono f32 at that rate). Resolves with the
    /// outcome when the reply cannot start at all — no output device, no playback thread; a reply
    /// that does play resolves through [`StreamHandle::wait`].
    pub async fn stream(&self, rate: u32) -> Result<StreamHandle, PlayOutcome> {
        let (started, wait_started) = oneshot::channel();
        let (done, wait_done) = oneshot::channel();
        if self
            .tx
            .send(Cmd::StreamBegin {
                sample_rate: rate,
                started,
                done,
            })
            .is_err()
        {
            return Err(PlayOutcome::Failed("playback thread is gone".into()));
        }
        match wait_started.await {
            Ok(Ok(())) => Ok(StreamHandle {
                tx: self.tx.clone(),
                done: wait_done,
            }),
            Ok(Err(outcome)) => Err(outcome),
            Err(_) => Err(PlayOutcome::Failed("playback thread is gone".into())),
        }
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

    /// Reports what is being played to `sink` (the hands-free listener's echo canceller).
    pub fn set_render_sink(&self, sink: RenderSink) {
        *self.render.lock().expect("render poisoned") = Some(sink);
    }

    pub fn set_gain(&self, volume: f32, muted: bool) {
        let _ = self.tx.send(Cmd::Gain { volume, muted });
    }

    /// Plays through the output device `id` (`None` = system default) from now on.
    pub fn set_device(&self, id: Option<String>) {
        let _ = self.tx.send(Cmd::Device(id));
    }
}

/// The decoded reply as the echo canceller wants it: mono, 16 kHz, at the same loudness it is played
/// at (the canceller learns the gain of the room itself, so the clip is not scaled here).
pub fn reference_16k(audio: &[u8]) -> Option<Vec<f32>> {
    let decoder = Decoder::new(Cursor::new(audio.to_vec())).ok()?;
    let channels = decoder.channels().get() as usize;
    let rate = decoder.sample_rate().get();
    let mut mono: Vec<f32> = Vec::new();
    let mut frame_sum = 0.0f32;
    let mut in_frame = 0usize;
    for s in decoder {
        frame_sum += s;
        in_frame += 1;
        if in_frame == channels {
            mono.push(frame_sum / channels as f32);
            frame_sum = 0.0;
            in_frame = 0;
        }
    }
    let mut out = Vec::with_capacity(mono.len() * 16_000 / rate.max(1) as usize + 1);
    Downsampler::new(rate).push(&mono, &mut out);
    Some(out)
}

/// Ends a reply that was still arriving: it will not grow any further, so the player ends after what
/// is already queued has played.
fn end_stream(stream: &mut Option<Arc<StreamQueue>>) {
    if let Some(queue) = stream.take() {
        queue.finish();
    }
}

fn run(rx: mpsc::Receiver<Cmd>, volume: f32, muted: bool, render: SharedRender) {
    let report: Arc<dyn Fn(RenderCmd) + Send + Sync> = Arc::new(move |cmd: RenderCmd| {
        if let Some(sink) = render.lock().expect("render poisoned").as_ref() {
            sink(cmd);
        }
    });
    let mut gain = effective_volume(volume, muted);
    let mut sink: Option<MixerDeviceSink> = None;
    let mut device: Option<String> = None;
    let mut current: Option<Playing> = None;
    let mut fading: Vec<Fading> = Vec::new();
    // The reply that is still arriving, if one is (chunks land in its queue).
    let mut stream: Option<Arc<StreamQueue>> = None;
    let mut last_active = Instant::now();

    let start_fade = |p: Playing, ms: u64, gain: f32, fading: &mut Vec<Fading>| {
        // Only the fade-out is still audible: that is all the echo canceller should still expect to hear.
        report(RenderCmd::Stop { keep_ms: ms as u32 });
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
                end_stream(&mut stream);
                if let Some(old) = current.take() {
                    start_fade(old, DEFAULT_FADE_MS, gain, &mut fading);
                }
                if sink.is_none() {
                    match open_sink(device.as_deref()) {
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
                let reference = reference_16k(&audio);
                match Decoder::new(Cursor::new(audio)) {
                    Ok(source) => {
                        let player = Player::connect_new(mixer);
                        player.set_volume(gain);
                        if let Some(samples) = reference {
                            report(RenderCmd::Start(Arc::new(samples)));
                        }
                        player.append(source);
                        current = Some(Playing { player, done });
                        last_active = Instant::now();
                    }
                    Err(e) => {
                        let _ = done.send(PlayOutcome::Failed(format!("could not decode the audio: {e}")));
                    }
                }
            }
            Ok(Cmd::StreamBegin {
                sample_rate,
                started,
                done,
            }) => {
                // A reply that is still arriving takes over from whatever is playing now.
                end_stream(&mut stream);
                if let Some(old) = current.take() {
                    start_fade(old, DEFAULT_FADE_MS, gain, &mut fading);
                }
                if sink.is_none() {
                    match open_sink(device.as_deref()) {
                        Ok(mut s) => {
                            s.log_on_drop(false);
                            sink = Some(s);
                        }
                        Err(e) => {
                            let _ = started
                                .send(Err(PlayOutcome::Failed(format!("could not open the speakers: {e}"))));
                            continue;
                        }
                    }
                }
                let mixer = sink.as_ref().expect("sink just opened").mixer();
                let queue = Arc::new(StreamQueue::new());
                let player = Player::connect_new(mixer);
                player.set_volume(gain);
                player.append(StreamSource::new(queue.clone(), sample_rate, report.clone()));
                current = Some(Playing { player, done });
                stream = Some(queue);
                last_active = Instant::now();
                let _ = started.send(Ok(()));
            }
            Ok(Cmd::StreamChunk { samples }) => {
                if let Some(queue) = &stream {
                    queue.push(&samples);
                }
            }
            Ok(Cmd::StreamEnd) => {
                end_stream(&mut stream);
            }
            Ok(Cmd::Stop { fade_ms }) => {
                end_stream(&mut stream);
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
            Ok(Cmd::Device(id)) => {
                if id != device {
                    device = id;
                    // The old device's mixer goes away with its sink, so what plays on it ends now.
                    if let Some(p) = current.take() {
                        start_fade(p, 0, gain, &mut fading);
                    }
                    for f in fading.drain(..) {
                        f.player.stop();
                        if let Some(done) = f.done {
                            let _ = done.send(PlayOutcome::Stopped);
                        }
                    }
                    sink = None;
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

    /// An echo reference that keeps what it was told, in order.
    fn recording_report() -> (Arc<dyn Fn(RenderCmd) + Send + Sync>, Arc<Mutex<Vec<RenderCmd>>>) {
        let cmds: Arc<Mutex<Vec<RenderCmd>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = cmds.clone();
        let report: Arc<dyn Fn(RenderCmd) + Send + Sync> = Arc::new(move |cmd| {
            sink.lock().expect("commands poisoned").push(cmd);
        });
        (report, cmds)
    }

    /// How many 16 kHz samples a recorded reference holds.
    fn reference_samples(cmds: &Arc<Mutex<Vec<RenderCmd>>>) -> usize {
        cmds.lock()
            .expect("commands poisoned")
            .iter()
            .map(|cmd| match cmd {
                RenderCmd::Start(samples) | RenderCmd::Append(samples) => samples.len(),
                RenderCmd::Stop { .. } => 0,
            })
            .sum()
    }

    /// Pulls until `expected` comes out of the source, at most one silent run later (while nothing
    /// has arrived it hands out silence between looks at the queue).
    fn expect_sample(source: &mut StreamSource, expected: f32) -> bool {
        for _ in 0..=STREAM_SILENCE_RUN {
            if source.next() == Some(expected) {
                return true;
            }
        }
        false
    }

    #[test]
    fn a_streamed_reply_plays_what_arrived_and_ends_when_it_is_complete() {
        let queue = Arc::new(StreamQueue::new());
        let (report, _) = recording_report();
        let mut source = StreamSource::new(queue.clone(), 24_000, report);

        queue.push(&[0.1, 0.2, 0.3]);
        assert_eq!(source.next(), Some(0.1));
        assert_eq!(source.next(), Some(0.2));
        assert_eq!(source.next(), Some(0.3));

        // Nothing has arrived yet: not the end of the reply, so it holds the timeline with silence.
        assert_eq!(source.next(), Some(0.0));

        queue.push(&[0.4]);
        assert!(expect_sample(&mut source, 0.4), "the rest of the reply follows the silence");

        queue.push(&[0.5]);
        queue.finish();
        assert!(expect_sample(&mut source, 0.5));
        assert_eq!(source.next(), None, "over once everything it held has played");
    }

    #[test]
    fn a_starved_reply_keeps_playing_silence_instead_of_ending() {
        let queue = Arc::new(StreamQueue::new());
        let (report, _) = recording_report();
        let mut source = StreamSource::new(queue.clone(), 24_000, report);

        for _ in 0..10_000 {
            assert_eq!(source.next(), Some(0.0), "incomplete is not the same as over");
        }
        queue.finish();
        let ended = (0..=STREAM_SILENCE_RUN).any(|_| source.next().is_none());
        assert!(ended, "and it ends within one silent run of the reply being over");
    }

    #[test]
    fn the_echo_reference_covers_everything_played_silence_included() {
        let queue = Arc::new(StreamQueue::new());
        let (report, cmds) = recording_report();
        let mut source = StreamSource::new(queue.clone(), 24_000, report);

        // 100 ms of audio, then 100 ms of waiting, then 100 ms more: 300 ms played in all.
        let tone: Vec<f32> = (0..2_400).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        queue.push(&tone);
        for _ in 0..2_400 {
            source.next();
        }
        for _ in 0..2_400 {
            assert_eq!(source.next(), Some(0.0));
        }
        queue.push(&tone);
        queue.finish();
        while source.next().is_some() {}

        let seen = cmds.lock().expect("commands poisoned");
        assert!(
            matches!(seen.first(), Some(RenderCmd::Start(_))),
            "the reply is announced before it is followed: {} blocks",
            seen.len()
        );
        assert!(
            seen.iter().skip(1).all(|cmd| matches!(cmd, RenderCmd::Append(_))),
            "everything after the first block continues the same reply"
        );
        drop(seen);
        let expected = 7_200 * 16 / 24; // 300 ms at 16 kHz
        let total = reference_samples(&cmds);
        assert!((total as i32 - expected as i32).abs() <= 4, "{total} vs {expected}");
    }

    #[test]
    fn the_queue_hands_out_no_more_than_it_was_asked_for() {
        let queue = StreamQueue::new();
        queue.push(&[0.1, 0.2, 0.3]);
        let mut out = VecDeque::new();
        assert_eq!(queue.take(&mut out, 2), 2);
        assert_eq!(out, VecDeque::from([0.1, 0.2]));
        assert_eq!(queue.take(&mut out, 10), 1);
        assert_eq!(out, VecDeque::from([0.1, 0.2, 0.3]));
        assert_eq!(queue.take(&mut out, 10), 0);
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

    #[test]
    fn the_echo_reference_is_the_reply_as_mono_16k() {
        let wav = tone_wav(1.0); // 16 kHz mono already
        let reference = reference_16k(&wav).expect("decodes");
        assert!((reference.len() as i32 - 16_000).abs() <= 4, "{} samples", reference.len());
        let peak = reference.iter().fold(0f32, |a, s| a.max(s.abs()));
        assert!(peak > 0.05 && peak < 0.2, "tone amplitude 3000/32768 survives: {peak}");
        let crossings = reference.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((crossings as i32 - 880).abs() < 30, "440 Hz stays 440 Hz: {crossings} crossings");
    }

    #[test]
    fn a_reply_that_cannot_be_decoded_has_no_reference() {
        assert!(reference_16k(&[1, 2, 3, 4]).is_none());
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
