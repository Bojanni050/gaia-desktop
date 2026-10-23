//! Capture backends: the OS-facing half of the microphone engine.
//!
//! A [`CaptureBackend`] enumerates input devices and opens one capture stream. The audio callback only
//! touches a [`CaptureSink`] (atomics, no allocation, no locks), so a slow consumer can never cause an
//! overrun (a growing buffer behind a mutex did exactly that during Phase 0).
//!
//! Dropping a [`CaptureStream`] stops capture and releases the device: that is how the master switch
//! turns the microphone *off*, not by pausing a stream.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::Serialize;

/// Where the noise gate sits when it has never been configured: just above a quiet room's floor, so it
/// closes on hiss without eating a soft voice. The settings carry the real value.
pub const DEFAULT_GATE_LEVEL: f32 = 0.006;
/// The range the gate can be set to. Below the floor a microphone would gate on nothing but its own
/// noise; above the top a normal voice would never open it.
pub const GATE_LEVEL_RANGE: (f32, f32) = (0.002, 0.2);

/// The gate's level follows speech over roughly this window, so the meter does not flicker per chunk.
const GATE_ENVELOPE_SECS: f32 = 0.05;
/// A gate that snaps shut clicks. It fades over this long instead.
const GATE_RAMP_SECS: f32 = 0.005;
/// A gap between words is not silence: once open, the gate stays open this long before it closes.
const GATE_HOLD_SECS: f32 = 0.25;
/// The gate closes at this fraction of the level that opened it (hysteresis), so a level hovering at the
/// threshold does not chatter open and shut.
const GATE_CLOSE_RATIO: f32 = 0.6;

/// One input device as shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    /// Stable id (`DeviceId` string, e.g. `wasapi:{...}`); persisted in settings.
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// What the audio callback writes to. The counters are lock-free; the optional *tap* is only touched
/// with `try_lock`, so the callback never waits: a recording that is being started or stopped at that
/// very instant simply misses one 10 ms chunk.
/// What the audio callback writes to. The counters are lock-free; the optional *tap* is only touched
/// with `try_lock`, so the callback never waits: a recording that is being started or stopped at that
/// very instant simply misses one 10 ms chunk.
pub struct CaptureSink {
    frames: AtomicU64,
    /// Peak amplitude (0..1) since the last [`CaptureSink::take_peak`], scaled by 1e6.
    peak_micro: AtomicU32,
    /// Mean square of the input (f32 bits), smoothed over ~50 ms: the gate's level and the meter's source.
    level_ms: AtomicU32,
    /// The noise gate: below `threshold` the gate shuts and tapped audio is silent.
    gate_enabled: AtomicBool,
    /// Its threshold: a mean square level, scaled by 1e6.
    gate_threshold_micro: AtomicU32,
    /// Whether the gate is currently letting audio through.
    gate_open: AtomicBool,
    /// The gain applied to tapped audio right now (f32 bits): 0 shut, 1 open, ramped in between.
    gate_gain: AtomicU32,
    /// Samples left before an open gate may shut after the level dropped.
    gate_hold: AtomicU32,
    glitches: AtomicU32,
    stream_failed: AtomicBool,
    /// Input sample rate in Hz, set when the stream opens (0 until then).
    sample_rate: AtomicU32,
    /// True while a tap is attached: the callback only builds a mono chunk when someone is listening.
    tapping: AtomicBool,
    /// Receives mono f32 chunks at `sample_rate` while a recording is in progress. Audio exists only in
    /// memory, only for the length of a recording, and only because the user asked for one.
    tap: Mutex<Option<mpsc::Sender<Vec<f32>>>>,
}

impl Default for CaptureSink {
    fn default() -> Self {
        Self {
            frames: AtomicU64::new(0),
            peak_micro: AtomicU32::new(0),
            level_ms: AtomicU32::new(0),
            gate_enabled: AtomicBool::new(false),
            gate_threshold_micro: AtomicU32::new((DEFAULT_GATE_LEVEL * 1_000_000.0) as u32),
            gate_open: AtomicBool::new(true),
            // Wide open until the gate has seen a chunk and decided otherwise: audio must never be
            // silenced just because the gate was not consulted.
            gate_gain: AtomicU32::new(1.0f32.to_bits()),
            gate_hold: AtomicU32::new(0),
            glitches: AtomicU32::new(0),
            stream_failed: AtomicBool::new(false),
            sample_rate: AtomicU32::new(0),
            tapping: AtomicBool::new(false),
            tap: Mutex::new(None),
        }
    }
}

impl CaptureSink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Called from the audio callback.
    pub fn push(&self, frames: u64, peak: f32) {
        self.frames.fetch_add(frames, Ordering::Relaxed);
        let micro = (peak.clamp(0.0, 1.0) * 1_000_000.0) as u32;
        self.peak_micro.fetch_max(micro, Ordering::Relaxed);
    }

    /// Called from the stream error callback for a recoverable glitch (xrun).
    pub fn glitch(&self) {
        self.glitches.fetch_add(1, Ordering::Relaxed);
    }

    /// Called from the stream error callback when the stream cannot continue (device changed or removed).
    pub fn fail(&self) {
        self.stream_failed.store(true, Ordering::Relaxed);
    }

    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn set_sample_rate(&self, hz: u32) {
        self.sample_rate.store(hz, Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// True while a recording is attached.
    pub fn is_tapping(&self) -> bool {
        self.tapping.load(Ordering::SeqCst)
    }

    /// Starts forwarding mono chunks to `tx` (a recording). Replaces any earlier tap.
    pub fn attach_tap(&self, tx: mpsc::Sender<Vec<f32>>) {
        *self.tap.lock().expect("tap poisoned") = Some(tx);
        self.tapping.store(true, Ordering::SeqCst);
    }

    /// Stops forwarding. Safe to call when nothing is attached.
    pub fn detach_tap(&self) {
        self.tapping.store(false, Ordering::SeqCst);
        *self.tap.lock().expect("tap poisoned") = None;
    }

    /// Called from the audio callback with one mono chunk; a no-op unless a recording is attached. The
    /// chunk passes through the noise gate, so everything downstream (push-to-talk, hands-free, the wake
    /// word) hears the same gated microphone.
    pub fn offer_mono(&self, mono: &[f32]) {
        if !self.tapping.load(Ordering::Relaxed) {
            return;
        }
        let gain = self.gate_gain();
        if let Ok(guard) = self.tap.try_lock() {
            if let Some(tx) = guard.as_ref() {
                let chunk: Vec<f32> = if gain >= 0.999 {
                    mono.to_vec()
                } else {
                    mono.iter().map(|s| s * gain).collect()
                };
                let _ = tx.send(chunk);
            }
        }
    }

    pub fn glitches(&self) -> u32 {
        self.glitches.load(Ordering::Relaxed)
    }

    /// Peak since the previous call, then reset.
    pub fn take_peak(&self) -> f32 {
        self.peak_micro.swap(0, Ordering::Relaxed) as f32 / 1_000_000.0
    }

    /// Configures the noise gate. Takes effect from the next chunk; the level already measured is kept,
    /// so the meter does not jump when the slider moves.
    pub fn set_gate(&self, enabled: bool, level: f32) {
        let (lo, hi) = GATE_LEVEL_RANGE;
        let level = if level.is_finite() { level.clamp(lo, hi) } else { DEFAULT_GATE_LEVEL };
        self.gate_threshold_micro
            .store((level * 1_000_000.0).round() as u32, Ordering::Relaxed);
        self.gate_enabled.store(enabled, Ordering::Relaxed);
        // Switching the gate off must pass sound at once, not fade in over the old gain.
        if !enabled {
            self.gate_open.store(true, Ordering::Relaxed);
            self.gate_gain.store(1.0f32.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn gate_enabled(&self) -> bool {
        self.gate_enabled.load(Ordering::Relaxed)
    }

    /// The threshold, as a level (0..1, the same scale as [`CaptureSink::level`]).
    pub fn gate_threshold(&self) -> f32 {
        self.gate_threshold_micro.load(Ordering::Relaxed) as f32 / 1_000_000.0
    }

    /// Whether the gate is letting sound through. True while the gate is switched off.
    pub fn gate_open(&self) -> bool {
        self.gate_open.load(Ordering::Relaxed)
    }

    /// The gain a tapped chunk would get right now (0 shut, 1 open).
    pub fn gate_gain(&self) -> f32 {
        f32::from_bits(self.gate_gain.load(Ordering::Relaxed))
    }

    /// The smoothed input level (0..1, rms-like). This is the scale the gate works on, so the meter and
    /// the threshold it shows mean the same thing.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level_ms.load(Ordering::Relaxed)).max(0.0).sqrt()
    }

    /// Called from the audio callback with the mean square of one chunk. Tracks the level the meter shows
    /// and decides whether the gate is open, returning the gain for that chunk's tapped audio.
    ///
    /// One writer only (the capture callback), so the state lives in plain atomics: no lock, no
    /// allocation, nothing the realtime thread can block on.
    pub fn observe(&self, mean_square: f32, samples: u32) -> f32 {
        let rate = self.sample_rate().max(8_000) as f32;
        let seconds = samples.max(1) as f32 / rate;

        let previous = f32::from_bits(self.level_ms.load(Ordering::Relaxed));
        let mean_square = if mean_square.is_finite() { mean_square.clamp(0.0, 1.0) } else { 0.0 };
        let envelope = previous + (mean_square - previous) * (1.0 - (-seconds / GATE_ENVELOPE_SECS).exp());
        self.level_ms.store(envelope.to_bits(), Ordering::Relaxed);
        let level = envelope.max(0.0).sqrt();

        let mut open = self.gate_open.load(Ordering::Relaxed);
        if self.gate_enabled.load(Ordering::Relaxed) {
            let threshold = self.gate_threshold();
            let hold = self.gate_hold.load(Ordering::Relaxed);
            if level >= threshold {
                // Open on real sound, and stay open across the gaps between words.
                open = true;
                self.gate_hold
                    .store((GATE_HOLD_SECS * rate).round() as u32, Ordering::Relaxed);
            } else if open && level < threshold * GATE_CLOSE_RATIO {
                let left = hold.saturating_sub(samples);
                self.gate_hold.store(left, Ordering::Relaxed);
                if left == 0 {
                    open = false;
                }
            }
        } else {
            open = true;
        }
        self.gate_open.store(open, Ordering::Relaxed);

        let target = if open { 1.0 } else { 0.0 };
        let gain = self.gate_gain();
        let gain = gain + (target - gain) * (1.0 - (-seconds / GATE_RAMP_SECS).exp());
        self.gate_gain.store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        gain
    }

    pub fn has_failed(&self) -> bool {
        self.stream_failed.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureOpenError {
    #[error("no input device available")]
    NoDevice,
    #[error("input device not found: {0}")]
    DeviceNotFound(String),
    #[error("microphone access denied")]
    PermissionDenied,
    #[error("could not open input device: {0}")]
    Other(String),
}

/// A running capture. Dropping it stops capture and releases the device.
pub trait CaptureStream: Send {}

pub trait CaptureBackend: Send + Sync + 'static {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo>;

    /// Opens `device_id` (or the system default when `None`) and starts writing into `sink`.
    fn open(
        &self,
        device_id: Option<&str>,
        sink: Arc<CaptureSink>,
    ) -> Result<(Box<dyn CaptureStream>, AudioDeviceInfo), CaptureOpenError>;
}

// ------------------------------------------------------------------------------------------------
// cpal / WASAPI
// ------------------------------------------------------------------------------------------------

pub struct CpalBackend;

/// The cpal stream lives on its own thread (so the handle is `Send` on every platform); dropping the
/// handle tells that thread to drop the stream and waits for it.
struct CpalStream {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CaptureStream for CpalStream {}

impl Drop for CpalStream {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(crate) fn info_for(device: &cpal::Device, default_id: Option<&str>) -> Option<AudioDeviceInfo> {
    let id = device.id().ok()?.to_string();
    let name = device
        .description()
        .ok()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|| id.clone());
    Some(AudioDeviceInfo {
        is_default: default_id == Some(id.as_str()),
        id,
        name,
    })
}

fn classify_open_error(message: &str) -> CaptureOpenError {
    let lower = message.to_lowercase();
    if lower.contains("denied") || lower.contains("permission") || lower.contains("access is") {
        CaptureOpenError::PermissionDenied
    } else {
        CaptureOpenError::Other(message.to_string())
    }
}

/// Peak, level and frame count of an interleaved chunk, converted to f32 on the fly (no allocation unless
/// a recording is attached, in which case one small mono chunk is built).
fn feed<T: Copy>(sink: &CaptureSink, data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) {
    let mut peak = 0f32;
    let mut sum_sq = 0f32;
    for s in data {
        let v = to_f32(*s);
        peak = peak.max(v.abs());
        sum_sq += v * v;
    }
    let channels = channels.max(1);
    let frames = (data.len() / channels) as u64;
    sink.push(frames, peak);
    // The gate works on the level of the microphone itself, tapped or not: the meter stays live and the
    // gate has already decided by the time a chunk is offered below.
    if !data.is_empty() {
        sink.observe(sum_sq / data.len() as f32, frames as u32);
    }
    if sink.tapping.load(Ordering::Relaxed) {
        let mono: Vec<f32> = data
            .chunks_exact(channels)
            .map(|frame| frame.iter().map(|s| to_f32(*s)).sum::<f32>() / channels as f32)
            .collect();
        sink.offer_mono(&mono);
    }
}

fn build_stream(
    wanted: Option<&str>,
    sink: Arc<CaptureSink>,
) -> Result<(cpal::Stream, AudioDeviceInfo), CaptureOpenError> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|d| d.id().ok())
        .map(|i| i.to_string());
    let device = match wanted {
        Some(id) => {
            let parsed = id
                .parse::<cpal::DeviceId>()
                .map_err(|_| CaptureOpenError::DeviceNotFound(id.to_string()))?;
            host.device_by_id(&parsed)
                .ok_or_else(|| CaptureOpenError::DeviceNotFound(id.to_string()))?
        }
        None => host.default_input_device().ok_or(CaptureOpenError::NoDevice)?,
    };
    let info = info_for(&device, default_id.as_deref())
        .ok_or_else(|| CaptureOpenError::Other("device has no id".into()))?;
    let config = device
        .default_input_config()
        .map_err(|e| classify_open_error(&e.to_string()))?;
    let channels = config.channels() as usize;
    sink.set_sample_rate(config.sample_rate());

    let err_sink = sink.clone();
    let err_fn = move |e: cpal::Error| match e.kind() {
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied => err_sink.glitch(),
        _ => err_sink.fail(),
    };
    let stream = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            config.into(),
            move |d: &[f32], _: &_| feed(&sink, d, channels, |s| s),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config.into(),
            move |d: &[i16], _: &_| feed(&sink, d, channels, |s| s as f32 / 32_768.0),
            err_fn,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            config.into(),
            move |d: &[i32], _: &_| feed(&sink, d, channels, |s| s as f32 / 2_147_483_648.0),
            err_fn,
            None,
        ),
        other => {
            return Err(CaptureOpenError::Other(format!(
                "unsupported sample format {other:?}"
            )))
        }
    }
    .map_err(|e| classify_open_error(&e.to_string()))?;
    stream
        .play()
        .map_err(|e| classify_open_error(&e.to_string()))?;
    Ok((stream, info))
}

impl CaptureBackend for CpalBackend {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        let host = cpal::default_host();
        let default_id = host
            .default_input_device()
            .and_then(|d| d.id().ok())
            .map(|i| i.to_string());
        host.input_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| info_for(&d, default_id.as_deref()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn open(
        &self,
        device_id: Option<&str>,
        sink: Arc<CaptureSink>,
    ) -> Result<(Box<dyn CaptureStream>, AudioDeviceInfo), CaptureOpenError> {
        let wanted = device_id.map(str::to_string);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<AudioDeviceInfo, CaptureOpenError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("gaia-mic".into())
            .spawn(move || match build_stream(wanted.as_deref(), sink) {
                Ok((stream, info)) => {
                    let _ = ready_tx.send(Ok(info));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| CaptureOpenError::Other(e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(info)) => Ok((
                Box::new(CpalStream {
                    stop: Some(stop_tx),
                    thread: Some(thread),
                }),
                info,
            )),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = stop_tx.send(());
                Err(CaptureOpenError::Other("timed out opening the input device".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One 10 ms chunk at the fake rate.
    const CHUNK: u32 = 480;
    /// The mean square of a 0.3 sine: what a normal voice looks like on this scale.
    const SPEECH: f32 = 0.3 * 0.3 / 2.0;

    fn sink() -> Arc<CaptureSink> {
        let sink = CaptureSink::new();
        sink.set_sample_rate(48_000);
        sink
    }

    #[test]
    fn the_level_is_an_rms_like_number_on_the_gates_own_scale() {
        let sink = sink();
        for _ in 0..20 {
            sink.observe(SPEECH, CHUNK);
        }
        // sqrt(0.045) = 0.212: a 0.3 sine is 0.21 rms, not 0.3. The threshold uses the same scale.
        assert!(sink.level() > 0.2 && sink.level() < 0.22, "level {}", sink.level());
    }

    #[test]
    fn the_gate_opens_on_sound_and_shuts_on_silence() {
        let sink = sink();
        sink.set_gate(true, 0.02);
        assert!(sink.gate_open(), "a gate that has never run must not silence audio");

        for _ in 0..20 {
            sink.observe(SPEECH, CHUNK);
        }
        assert!(sink.gate_open());
        assert!(sink.gate_gain() > 0.99);

        // Silent for a second: the gate closes and its gain fades to zero.
        for _ in 0..100 {
            sink.observe(0.0, CHUNK);
        }
        assert!(!sink.gate_open());
        assert!(sink.gate_gain() < 0.01);
    }

    #[test]
    fn a_gap_between_words_does_not_shut_the_gate() {
        let sink = sink();
        sink.set_gate(true, 0.02);
        for _ in 0..20 {
            sink.observe(SPEECH, CHUNK);
        }
        // 100 ms of silence is a pause, not the end of the sentence.
        for _ in 0..10 {
            sink.observe(0.0, CHUNK);
        }
        assert!(sink.gate_open());
    }

    #[test]
    fn a_quiet_room_never_opens_the_gate() {
        let sink = sink();
        sink.set_gate(true, 0.02);
        for _ in 0..100 {
            sink.observe(0.0001, CHUNK);
        }
        assert!(!sink.gate_open());
        assert!(sink.gate_gain() < 0.01);
    }

    #[test]
    fn a_shut_gate_delivers_silence_and_an_open_one_delivers_the_microphone() {
        let sink = sink();
        let (tx, rx) = mpsc::channel();
        sink.attach_tap(tx);

        sink.set_gate(true, 0.02);
        for _ in 0..100 {
            sink.observe(0.0, CHUNK);
        }
        sink.offer_mono(&[0.5, -0.5]);
        assert!(rx.recv().unwrap().iter().all(|s| s.abs() < 1e-6), "a shut gate passes nothing");

        for _ in 0..20 {
            sink.observe(SPEECH, CHUNK);
        }
        sink.offer_mono(&[0.5, -0.5]);
        assert!(rx.recv().unwrap().iter().all(|s| (*s - 0.5).abs() < 0.01 || (*s + 0.5).abs() < 0.01));
    }

    #[test]
    fn switching_the_gate_off_passes_sound_immediately() {
        let sink = sink();
        sink.set_gate(true, 0.02);
        for _ in 0..100 {
            sink.observe(0.0, CHUNK);
        }
        assert!(sink.gate_gain() < 0.01);
        sink.set_gate(false, 0.02);
        assert!(sink.gate_open());
        assert!(sink.gate_gain() > 0.99);
    }

    #[test]
    fn the_threshold_is_kept_within_its_range() {
        let sink = sink();
        sink.set_gate(true, 5.0);
        assert!((sink.gate_threshold() - GATE_LEVEL_RANGE.1).abs() < 1e-6);
        sink.set_gate(true, 0.0);
        assert!((sink.gate_threshold() - GATE_LEVEL_RANGE.0).abs() < 1e-6);
        sink.set_gate(true, f32::NAN);
        assert!((sink.gate_threshold() - DEFAULT_GATE_LEVEL).abs() < 1e-6);
    }
}
