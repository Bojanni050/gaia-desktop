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
#[derive(Default)]
pub struct CaptureSink {
    frames: AtomicU64,
    /// Peak amplitude (0..1) since the last [`CaptureSink::take_peak`], scaled by 1e6.
    peak_micro: AtomicU32,
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

    /// Called from the audio callback with one mono chunk; a no-op unless a recording is attached.
    pub fn offer_mono(&self, mono: &[f32]) {
        if !self.tapping.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(guard) = self.tap.try_lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(mono.to_vec());
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

fn info_for(device: &cpal::Device, default_id: Option<&str>) -> Option<AudioDeviceInfo> {
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

/// Peak and frame count of an interleaved chunk, converted to f32 on the fly (no allocation unless a
/// recording is attached, in which case one small mono chunk is built).
fn feed<T: Copy>(sink: &CaptureSink, data: &[T], channels: usize, to_f32: impl Fn(T) -> f32) {
    let mut peak = 0f32;
    for s in data {
        peak = peak.max(to_f32(*s).abs());
    }
    let channels = channels.max(1);
    sink.push((data.len() / channels) as u64, peak);
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
