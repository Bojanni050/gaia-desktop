//! A fake microphone shared by the audio tests (compiled for tests only).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::capture::{AudioDeviceInfo, CaptureBackend, CaptureOpenError, CaptureSink, CaptureStream};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    /// Frames flow and carry a loud tone ("speech").
    Frames,
    /// Frames flow but carry silence.
    Quiet,
    /// A pause (1.5 s), then a second of loud tone, repeating: utterances that actually end, after a
    /// quiet lead-in the speech detector can calibrate on.
    Bursts,
    /// The stream opens and delivers nothing.
    NoFrames,
    Permission,
    Fail,
}

pub(crate) const FAKE_RATE: u32 = 48_000;

pub(crate) struct Fake {
    pub devices: Vec<AudioDeviceInfo>,
    pub mode: Mutex<Mode>,
    pub alive: Arc<AtomicUsize>,
    pub opens: AtomicUsize,
    pub last_sink: Mutex<Option<Arc<CaptureSink>>>,
    pub stall: Arc<AtomicBool>,
}

pub(crate) struct FakeStream {
    stop: Arc<AtomicBool>,
    alive: Arc<AtomicUsize>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl CaptureStream for FakeStream {}
impl Drop for FakeStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.alive.fetch_sub(1, Ordering::SeqCst);
    }
}

pub(crate) fn dev(id: &str, default: bool) -> AudioDeviceInfo {
    AudioDeviceInfo {
        id: id.into(),
        name: format!("Mic {id}"),
        is_default: default,
    }
}

impl Fake {
    pub fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            devices: vec![dev("a", true), dev("b", false)],
            mode: Mutex::new(mode),
            alive: Arc::new(AtomicUsize::new(0)),
            opens: AtomicUsize::new(0),
            last_sink: Mutex::new(None),
            stall: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn alive(&self) -> usize {
        self.alive.load(Ordering::SeqCst)
    }
    pub fn set_mode(&self, m: Mode) {
        *self.mode.lock().unwrap() = m;
    }
}

impl CaptureBackend for Fake {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        self.devices.clone()
    }
    fn open(
        &self,
        device_id: Option<&str>,
        sink: Arc<CaptureSink>,
    ) -> Result<(Box<dyn CaptureStream>, AudioDeviceInfo), CaptureOpenError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let mode = *self.mode.lock().unwrap();
        match mode {
            Mode::Permission => return Err(CaptureOpenError::PermissionDenied),
            Mode::Fail => return Err(CaptureOpenError::NoDevice),
            _ => {}
        }
        let info = match device_id {
            Some(id) => self
                .devices
                .iter()
                .find(|d| d.id == id)
                .cloned()
                .ok_or_else(|| CaptureOpenError::DeviceNotFound(id.into()))?,
            None => self.devices[0].clone(),
        };
        sink.set_sample_rate(FAKE_RATE);
        *self.last_sink.lock().unwrap() = Some(sink.clone());
        self.alive.fetch_add(1, Ordering::SeqCst);
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let stall = self.stall.clone();
        let thread = std::thread::spawn(move || {
            let loud: Vec<f32> = (0..480)
                .map(|i| 0.3 * (i as f32 * 220.0 * std::f32::consts::TAU / FAKE_RATE as f32).sin())
                .collect();
            let quiet = vec![0.0f32; 480];
            let mut n: u64 = 0;
            while !s2.load(Ordering::SeqCst) {
                if !stall.load(Ordering::SeqCst) && mode != Mode::NoFrames {
                    let is_loud = match mode {
                        Mode::Frames => true,
                        Mode::Bursts => n % 250 >= 150, // 1.5 s of pause, then 1 s of speech (10 ms chunks)
                        _ => false,
                    };
                    let chunk = if is_loud { &loud } else { &quiet };
                    sink.push(480, if is_loud { 0.3 } else { 0.0 });
                    sink.offer_mono(chunk);
                    n += 1;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Ok((
            Box::new(FakeStream {
                stop,
                alive: self.alive.clone(),
                thread: Some(thread),
            }),
            info,
        ))
    }
}
