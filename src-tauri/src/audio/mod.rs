//! Audio subsystem — the desktop's microphone capability.
//!
//! Audio capture is an OS-close capability owned by the desktop. What the audio *means* (speech,
//! intent, emotion) is always determined server-side.
//!
//! Phase 1 scope: device enumeration and selection, a real capture stream behind the master switch,
//! a level meter, and the watchdogs that keep "listening" honest. Nothing is recorded, buffered or
//! sent anywhere yet — later phases consume the frames.
//!
//! The master switch semantics (spec section 5.6): *on* opens a capture stream, *off* **drops** it, so
//! Windows releases the device and its microphone indicator disappears. The switch is runtime-only:
//! every launch starts with the microphone off.

mod capture;

pub use capture::{
    AudioDeviceInfo, CaptureBackend, CaptureOpenError, CaptureSink, CaptureStream, CpalBackend,
};

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Tauri event names (spec 5.3).
pub const VOICE_STATE_EVENT: &str = "voice://state";
pub const VOICE_ERROR_EVENT: &str = "voice://error";
pub const VOICE_LEVEL_EVENT: &str = "voice://level";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioPermission {
    /// No frames have arrived yet on this device this session.
    Unknown,
    /// Frames are arriving.
    Granted,
    /// The OS refused the microphone.
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MicState {
    Off,
    Listening,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceError {
    /// Stable machine code: `no_frames`, `device_changed`, `device_fallback`, `permission_denied`,
    /// `no_device`, `open_failed`, `too_many_restarts`.
    pub code: String,
    pub message: String,
}

impl VoiceError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    fn from_open(e: &CaptureOpenError) -> Self {
        match e {
            CaptureOpenError::NoDevice => Self::new("no_device", e.to_string()),
            CaptureOpenError::DeviceNotFound(_) => Self::new("device_fallback", e.to_string()),
            CaptureOpenError::PermissionDenied => Self::new("permission_denied", e.to_string()),
            CaptureOpenError::Other(_) => Self::new("open_failed", e.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub state: MicState,
    /// The master switch as the user sees it. False whenever the stream is not actually open.
    pub mic_enabled: bool,
    pub device: Option<AudioDeviceInfo>,
    pub permission: AudioPermission,
    pub error: Option<VoiceError>,
    /// Capture glitches (xruns) since the stream opened.
    pub glitches: u32,
}

#[derive(Debug, Clone)]
pub enum VoiceEvent {
    State(VoiceStatus),
    /// A failure or notice (`code` tells which). Always accompanied by a `State` where state changed.
    Error(VoiceError),
    /// Peak input level 0..1 since the previous level event.
    Level(f32),
}

/// Timing knobs (shortened in tests).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    /// A stream that has not delivered its first frames after this long is dead. Generous: a USB
    /// microphone can take seconds to wake on first open (seen once in Phase 1 testing).
    pub first_frames_timeout: Duration,
    /// A stream that delivered frames and then goes quiet for this long is dead.
    pub no_frames_timeout: Duration,
    pub max_restarts: usize,
    pub restart_window: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(250),
            first_frames_timeout: Duration::from_secs(5),
            no_frames_timeout: Duration::from_secs(2),
            max_restarts: 3,
            restart_window: Duration::from_secs(60),
        }
    }
}

type Emitter = Arc<dyn Fn(VoiceEvent) + Send + Sync>;

struct Shared {
    /// Bumped on every start/stop/restart; a watchdog exits when its generation is stale.
    generation: u64,
    enabled: bool,
    preferred: Option<String>,
    stream: Option<Box<dyn CaptureStream>>,
    sink: Option<Arc<CaptureSink>>,
    device: Option<AudioDeviceInfo>,
    permission: AudioPermission,
    error: Option<VoiceError>,
    restarts: VecDeque<Instant>,
}

struct Inner {
    backend: Arc<dyn CaptureBackend>,
    emit: Emitter,
    timing: Timing,
    shared: Mutex<Shared>,
}

/// Owns the capture stream and everything that keeps it honest.
#[derive(Clone)]
pub struct MicController {
    inner: Arc<Inner>,
}

impl MicController {
    pub fn new(backend: Arc<dyn CaptureBackend>, emit: Emitter, timing: Timing) -> Self {
        Self {
            inner: Arc::new(Inner {
                backend,
                emit,
                timing,
                shared: Mutex::new(Shared {
                    generation: 0,
                    enabled: false,
                    preferred: None,
                    stream: None,
                    sink: None,
                    device: None,
                    permission: AudioPermission::Unknown,
                    error: None,
                    restarts: VecDeque::new(),
                }),
            }),
        }
    }

    pub fn devices(&self) -> Vec<AudioDeviceInfo> {
        self.inner.backend.list_input_devices()
    }

    pub fn status(&self) -> VoiceStatus {
        status_of(&self.inner.shared.lock().expect("mic state poisoned"))
    }

    /// The master microphone switch. `preferred` is the saved device id (`None` = system default).
    pub fn set_enabled(&self, enabled: bool, preferred: Option<String>) -> VoiceStatus {
        let mut events = Vec::new();
        let status = {
            let mut sh = self.inner.shared.lock().expect("mic state poisoned");
            sh.generation += 1;
            // Release the device first: it must be free before it is reopened, and when switching
            // off this is the moment Windows' microphone indicator goes away.
            sh.stream = None;
            sh.sink = None;
            sh.device = None;
            sh.error = None;
            sh.restarts.clear();
            sh.enabled = false;
            if enabled {
                sh.preferred = preferred;
                match self.inner.start_locked(&mut sh, &mut events) {
                    Ok(()) => sh.enabled = true,
                    Err(e) => {
                        events.push(VoiceEvent::Error(e.clone()));
                        sh.error = Some(e);
                    }
                }
            }
            status_of(&sh)
        };
        events.push(VoiceEvent::State(status.clone()));
        self.inner.dispatch(events);
        status
    }

    /// Changes the preferred device; restarts capture on it when the microphone is on.
    pub fn select_device(&self, id: Option<String>) -> VoiceStatus {
        let enabled = {
            let mut sh = self.inner.shared.lock().expect("mic state poisoned");
            sh.preferred = id.clone();
            sh.enabled
        };
        if enabled {
            self.set_enabled(true, id)
        } else {
            self.status()
        }
    }

    /// Stops everything (app exit).
    pub fn shutdown(&self) {
        self.set_enabled(false, None);
    }
}

fn status_of(sh: &Shared) -> VoiceStatus {
    let state = if sh.error.is_some() {
        MicState::Error
    } else if sh.stream.is_some() {
        MicState::Listening
    } else {
        MicState::Off
    };
    VoiceStatus {
        state,
        mic_enabled: sh.enabled && sh.stream.is_some(),
        device: sh.device.clone(),
        permission: sh.permission,
        error: sh.error.clone(),
        glitches: sh.sink.as_ref().map(|s| s.glitches()).unwrap_or(0),
    }
}

impl Inner {
    fn dispatch(&self, events: Vec<VoiceEvent>) {
        for e in events {
            (self.emit)(e);
        }
    }

    /// Opens the preferred device, falling back to the system default when it is gone, then starts the
    /// watchdog for the new generation. Caller holds the lock.
    fn start_locked(
        self: &Arc<Self>,
        sh: &mut Shared,
        events: &mut Vec<VoiceEvent>,
    ) -> Result<(), VoiceError> {
        let sink = CaptureSink::new();
        let attempt = match self.backend.open(sh.preferred.as_deref(), sink.clone()) {
            Err(CaptureOpenError::DeviceNotFound(missing)) if sh.preferred.is_some() => {
                events.push(VoiceEvent::Error(VoiceError::new(
                    "device_fallback",
                    format!("saved input device {missing} is gone; using the system default"),
                )));
                self.backend.open(None, sink.clone())
            }
            other => other,
        };
        match attempt {
            Ok((stream, info)) => {
                sh.stream = Some(stream);
                sh.sink = Some(sink.clone());
                sh.device = Some(info);
                sh.error = None;
                let generation = sh.generation;
                let inner = self.clone();
                std::thread::Builder::new()
                    .name("gaia-mic-watchdog".into())
                    .spawn(move || inner.watchdog(generation, sink))
                    .map_err(|e| VoiceError::new("open_failed", e.to_string()))?;
                Ok(())
            }
            Err(e) => {
                if e == CaptureOpenError::PermissionDenied {
                    sh.permission = AudioPermission::Denied;
                }
                Err(VoiceError::from_open(&e))
            }
        }
    }

    fn fail_locked(&self, sh: &mut Shared, error: VoiceError, events: &mut Vec<VoiceEvent>) {
        sh.generation += 1;
        sh.stream = None;
        sh.device = None;
        sh.enabled = false;
        sh.error = Some(error.clone());
        events.push(VoiceEvent::Error(error));
        events.push(VoiceEvent::State(status_of(sh)));
    }

    fn watchdog(self: Arc<Self>, generation: u64, sink: Arc<CaptureSink>) {
        let mut last_frames = 0u64;
        let mut last_progress = Instant::now();
        loop {
            std::thread::sleep(self.timing.poll);
            let mut events = Vec::new();
            {
                let mut sh = self.shared.lock().expect("mic state poisoned");
                if sh.generation != generation {
                    return;
                }
                let frames = sink.frames();
                if frames > last_frames {
                    last_frames = frames;
                    last_progress = Instant::now();
                    if sh.permission != AudioPermission::Granted {
                        sh.permission = AudioPermission::Granted;
                        events.push(VoiceEvent::State(status_of(&sh)));
                    }
                }
                events.push(VoiceEvent::Level(sink.take_peak()));

                if sink.has_failed() {
                    // The device changed or vanished: drop the dead stream, then try again on the
                    // default device — but not forever.
                    let now = Instant::now();
                    sh.restarts
                        .retain(|t| now.duration_since(*t) < self.timing.restart_window);
                    if sh.restarts.len() >= self.timing.max_restarts {
                        self.fail_locked(
                            &mut sh,
                            VoiceError::new(
                                "too_many_restarts",
                                "the input device keeps failing; microphone switched off",
                            ),
                            &mut events,
                        );
                    } else {
                        sh.restarts.push_back(now);
                        sh.generation += 1;
                        sh.stream = None;
                        sh.sink = None;
                        sh.device = None;
                        events.push(VoiceEvent::Error(VoiceError::new(
                            "device_changed",
                            "the input device changed; reconnecting",
                        )));
                        match self.start_locked(&mut sh, &mut events) {
                            Ok(()) => events.push(VoiceEvent::State(status_of(&sh))),
                            Err(e) => self.fail_locked(&mut sh, e, &mut events),
                        }
                    }
                    drop(sh);
                    self.dispatch(events);
                    return; // a restart spawned its own watchdog
                } else if last_progress.elapsed()
                    > if last_frames == 0 {
                        self.timing.first_frames_timeout
                    } else {
                        self.timing.no_frames_timeout
                    }
                {
                    // The stream opened without error but delivers nothing: never report "listening".
                    self.fail_locked(
                        &mut sh,
                        VoiceError::new(
                            "no_frames",
                            "the microphone is not delivering audio (security software such as Norton can block it); switched off",
                        ),
                        &mut events,
                    );
                    drop(sh);
                    self.dispatch(events);
                    return;
                }
            }
            self.dispatch(events);
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Tauri commands
// ------------------------------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub permission: AudioPermission,
    pub input_devices: Vec<AudioDeviceInfo>,
}

#[tauri::command]
pub fn audio_get_status(mic: tauri::State<'_, MicController>) -> AudioStatus {
    AudioStatus {
        permission: mic.status().permission,
        input_devices: mic.devices(),
    }
}

#[tauri::command]
pub fn voice_get_status(mic: tauri::State<'_, MicController>) -> VoiceStatus {
    mic.status()
}

#[tauri::command]
pub fn voice_list_devices(mic: tauri::State<'_, MicController>) -> Vec<AudioDeviceInfo> {
    mic.devices()
}

/// The master microphone switch. Runtime only: never persisted.
#[tauri::command]
pub fn voice_set_mic_enabled(
    mic: tauri::State<'_, MicController>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    enabled: bool,
) -> VoiceStatus {
    let preferred = settings.get().voice.input_device;
    mic.set_enabled(enabled, preferred)
}

#[tauri::command]
pub fn voice_select_device(
    mic: tauri::State<'_, MicController>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    id: Option<String>,
) -> Result<VoiceStatus, crate::error::DesktopError> {
    settings
        .update(|s| s.voice.input_device = id.clone())
        .map_err(crate::error::DesktopError::Settings)?;
    Ok(mic.select_device(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Frames,
        NoFrames,
        Permission,
        Fail,
    }

    struct Fake {
        devices: Vec<AudioDeviceInfo>,
        mode: Mutex<Mode>,
        alive: Arc<AtomicUsize>,
        opens: AtomicUsize,
        last_sink: Mutex<Option<Arc<CaptureSink>>>,
        stall: Arc<AtomicBool>,
    }

    struct FakeStream {
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

    fn dev(id: &str, default: bool) -> AudioDeviceInfo {
        AudioDeviceInfo {
            id: id.into(),
            name: format!("Mic {id}"),
            is_default: default,
        }
    }

    impl Fake {
        fn new(mode: Mode) -> Arc<Self> {
            Arc::new(Self {
                devices: vec![dev("a", true), dev("b", false)],
                mode: Mutex::new(mode),
                alive: Arc::new(AtomicUsize::new(0)),
                opens: AtomicUsize::new(0),
                last_sink: Mutex::new(None),
                stall: Arc::new(AtomicBool::new(false)),
            })
        }
        fn alive(&self) -> usize {
            self.alive.load(Ordering::SeqCst)
        }
        fn set_mode(&self, m: Mode) {
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
            *self.last_sink.lock().unwrap() = Some(sink.clone());
            self.alive.fetch_add(1, Ordering::SeqCst);
            let stop = Arc::new(AtomicBool::new(false));
            let s2 = stop.clone();
            let stall = self.stall.clone();
            let thread = std::thread::spawn(move || {
                while !s2.load(Ordering::SeqCst) {
                    if mode == Mode::Frames && !stall.load(Ordering::SeqCst) {
                        sink.push(480, 0.25);
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

    fn fast() -> Timing {
        Timing {
            poll: Duration::from_millis(5),
            first_frames_timeout: Duration::from_millis(200),
            no_frames_timeout: Duration::from_millis(80),
            max_restarts: 3,
            restart_window: Duration::from_secs(60),
        }
    }

    type Log = Arc<Mutex<Vec<VoiceEvent>>>;

    fn controller(fake: &Arc<Fake>) -> (MicController, Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let c = MicController::new(
            fake.clone(),
            Arc::new(move |e| l2.lock().unwrap().push(e)),
            fast(),
        );
        (c, log)
    }

    fn wait_until(what: &str, cond: impl Fn() -> bool) {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            if cond() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for: {what}");
    }

    fn has_error(log: &Log, code: &str) -> bool {
        log.lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, VoiceEvent::Error(err) if err.code == code))
    }

    #[test]
    fn starts_off_with_no_stream() {
        let fake = Fake::new(Mode::Frames);
        let (c, _) = controller(&fake);
        let s = c.status();
        assert_eq!(s.state, MicState::Off);
        assert!(!s.mic_enabled);
        assert_eq!(fake.alive(), 0);
        assert_eq!(
            fake.opens.load(Ordering::SeqCst),
            0,
            "nothing opens before the user switches on"
        );
    }

    #[test]
    fn switching_on_listens_and_off_drops_the_stream() {
        let fake = Fake::new(Mode::Frames);
        let (c, _) = controller(&fake);
        let s = c.set_enabled(true, None);
        assert_eq!(s.state, MicState::Listening);
        assert!(s.mic_enabled);
        assert_eq!(fake.alive(), 1);
        wait_until("permission granted once frames arrive", || {
            c.status().permission == AudioPermission::Granted
        });

        let off = c.set_enabled(false, None);
        assert_eq!(off.state, MicState::Off);
        assert!(!off.mic_enabled);
        assert_eq!(fake.alive(), 0, "off must drop the capture stream, not pause it");
    }

    #[test]
    fn levels_are_reported_while_listening() {
        let fake = Fake::new(Mode::Frames);
        let (c, log) = controller(&fake);
        c.set_enabled(true, None);
        wait_until("a level event", || {
            log.lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, VoiceEvent::Level(l) if *l > 0.2))
        });
        c.set_enabled(false, None);
    }

    #[test]
    fn a_stream_that_delivers_no_frames_is_never_reported_as_listening() {
        let fake = Fake::new(Mode::NoFrames);
        let (c, log) = controller(&fake);
        c.set_enabled(true, None);
        wait_until("no_frames error", || has_error(&log, "no_frames"));
        let s = c.status();
        assert_eq!(s.state, MicState::Error);
        assert!(!s.mic_enabled);
        assert_eq!(s.error.unwrap().code, "no_frames");
        assert_eq!(fake.alive(), 0, "the dead stream is dropped");
    }

    #[test]
    fn a_stream_that_stalls_after_working_is_switched_off() {
        let fake = Fake::new(Mode::Frames);
        let (c, log) = controller(&fake);
        c.set_enabled(true, None);
        wait_until("frames flowing", || c.status().permission == AudioPermission::Granted);
        fake.stall.store(true, Ordering::SeqCst);
        wait_until("no_frames after the stall", || has_error(&log, "no_frames"));
        assert_eq!(c.status().state, MicState::Error);
        assert_eq!(fake.alive(), 0);
    }

    #[test]
    fn first_frames_get_a_longer_grace_than_a_mid_stream_stall() {
        let t = Timing::default();
        assert!(t.first_frames_timeout > t.no_frames_timeout);
    }

    #[test]
    fn permission_denied_stays_off_and_does_not_retry() {
        let fake = Fake::new(Mode::Permission);
        let (c, _) = controller(&fake);
        let s = c.set_enabled(true, None);
        assert_eq!(s.state, MicState::Error);
        assert_eq!(s.permission, AudioPermission::Denied);
        assert_eq!(s.error.unwrap().code, "permission_denied");
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(fake.opens.load(Ordering::SeqCst), 1, "no retry loop");
    }

    #[test]
    fn a_missing_saved_device_falls_back_to_the_default() {
        let fake = Fake::new(Mode::Frames);
        let (c, log) = controller(&fake);
        let s = c.set_enabled(true, Some("gone".into()));
        assert_eq!(s.state, MicState::Listening);
        assert_eq!(s.device.unwrap().id, "a", "default device");
        assert!(has_error(&log, "device_fallback"));
        c.set_enabled(false, None);
    }

    #[test]
    fn selecting_a_device_while_on_restarts_on_it() {
        let fake = Fake::new(Mode::Frames);
        let (c, _) = controller(&fake);
        c.set_enabled(true, None);
        let s = c.select_device(Some("b".into()));
        assert_eq!(s.device.unwrap().id, "b");
        assert_eq!(fake.alive(), 1, "the old stream was released first");
        c.set_enabled(false, None);
    }

    #[test]
    fn selecting_a_device_while_off_does_not_open_anything() {
        let fake = Fake::new(Mode::Frames);
        let (c, _) = controller(&fake);
        c.select_device(Some("b".into()));
        assert_eq!(fake.opens.load(Ordering::SeqCst), 0);
        assert_eq!(c.status().state, MicState::Off);
    }

    #[test]
    fn a_failed_stream_reconnects_on_the_default_device() {
        let fake = Fake::new(Mode::Frames);
        let (c, log) = controller(&fake);
        c.set_enabled(true, Some("b".into()));
        fake.last_sink.lock().unwrap().as_ref().unwrap().fail();
        wait_until("reconnect notice", || has_error(&log, "device_changed"));
        wait_until("listening again", || {
            let s = c.status();
            s.state == MicState::Listening && fake.opens.load(Ordering::SeqCst) == 2
        });
        assert_eq!(fake.alive(), 1);
        c.set_enabled(false, None);
    }

    #[test]
    fn a_device_that_keeps_failing_ends_up_off() {
        let fake = Fake::new(Mode::Frames);
        let (c, log) = controller(&fake);
        c.set_enabled(true, None);
        for _ in 0..4 {
            let sink = fake.last_sink.lock().unwrap().clone().unwrap();
            sink.fail();
            let before = fake.opens.load(Ordering::SeqCst);
            wait_until("handled", || {
                fake.opens.load(Ordering::SeqCst) > before || c.status().state == MicState::Error
            });
            if c.status().state == MicState::Error {
                break;
            }
        }
        wait_until("too_many_restarts", || has_error(&log, "too_many_restarts"));
        assert!(!c.status().mic_enabled);
        assert_eq!(fake.alive(), 0);
    }

    #[test]
    fn switching_off_after_an_error_clears_it() {
        let fake = Fake::new(Mode::Fail);
        let (c, _) = controller(&fake);
        assert_eq!(c.set_enabled(true, None).state, MicState::Error);
        fake.set_mode(Mode::Frames);
        let s = c.set_enabled(false, None);
        assert_eq!(s.state, MicState::Off);
        assert!(s.error.is_none());
        assert_eq!(c.set_enabled(true, None).state, MicState::Listening);
        c.set_enabled(false, None);
    }

    /// Windows records microphone use per program; `LastUsedTimeStop` is 0 while the program holds the
    /// microphone open and is set when it releases it. The system microphone indicator follows this, so it
    /// is an objective check that "off" really released the device.
    fn consent_last_stop() -> Option<u64> {
        let exe = std::env::current_exe().ok()?.to_string_lossy().replace('\\', "#");
        let exe = exe.trim_start_matches("?#").to_string();
        let key = format!(
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged\{exe}"
        );
        let out = std::process::Command::new("reg")
            .args(["query", &key, "/v", "LastUsedTimeStop"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let hex = text.split_whitespace().last()?.trim_start_matches("0x").to_string();
        u64::from_str_radix(&hex, 16).ok()
    }

    /// Opt-in hardware test: `cargo test --lib real_microphone_delivers -- --ignored --nocapture`.
    /// Run it from a normal terminal (a sandboxed one can open the stream but receive no audio). Security
    /// software such as Norton may block a freshly built test binary until you allow it.
    #[test]
    #[ignore = "needs a real microphone"]
    fn real_microphone_delivers_frames_and_releases_on_off() {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let l2 = log.clone();
        let c = MicController::new(
            Arc::new(CpalBackend),
            Arc::new(move |e| l2.lock().unwrap().push(e)),
            Timing::default(),
        );
        println!("devices: {:?}", c.devices());
        let s = c.set_enabled(true, None);
        println!("status after on: {s:?}");
        assert_eq!(s.state, MicState::Listening, "{s:?}");
        std::thread::sleep(Duration::from_millis(1200));
        println!("status after 1.2 s: {:?}", c.status());
        println!("events: {:?}", log.lock().unwrap().iter().filter(|e| !matches!(e, VoiceEvent::Level(_))).collect::<Vec<_>>());
        println!("levels: {:?}", log.lock().unwrap().iter().filter_map(|e| if let VoiceEvent::Level(l) = e { Some(*l) } else { None }).take(6).collect::<Vec<_>>());
        wait_until("frames from the real mic", || c.status().permission == AudioPermission::Granted);
        let in_use = consent_last_stop();
        println!("Windows consent record while on (LastUsedTimeStop, 0 = in use): {in_use:?}");
        let off = c.set_enabled(false, None);
        assert_eq!(off.state, MicState::Off);
        std::thread::sleep(Duration::from_millis(500));
        let released = consent_last_stop();
        println!("Windows consent record after off: {released:?}");
        if let (Some(during), Some(after)) = (in_use, released) {
            assert_eq!(during, 0, "the microphone must show as in use while on");
            assert!(after > 0, "off must release the microphone (system indicator off)");
        } else {
            println!("(no consent record found for this test binary; indicator check skipped)");
        }
        let glitches = log.lock().unwrap().iter().filter(|e| matches!(e, VoiceEvent::Error(_))).count();
        println!("error/notice events: {glitches}");
    }

    /// Measures how long the real microphone takes to deliver its first frame after open, repeatedly.
    /// `cargo test --lib real_microphone_first -- --ignored --nocapture` (run from a normal terminal).
    #[test]
    #[ignore = "needs a real microphone"]
    fn real_microphone_first_frame_latency() {
        let backend = CpalBackend;
        let mut times = Vec::new();
        for round in 0..6 {
            let sink = CaptureSink::new();
            let t0 = Instant::now();
            let opened = backend.open(None, sink.clone());
            let open_ms = t0.elapsed().as_millis();
            let Ok((stream, _)) = opened else {
                println!("round {round}: open failed after {open_ms} ms");
                continue;
            };
            let mut first = None;
            while t0.elapsed() < Duration::from_secs(8) {
                if sink.frames() > 0 {
                    first = Some(t0.elapsed().as_millis());
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            println!("round {round}: open {open_ms} ms, first frame {first:?} ms, failed={}", sink.has_failed());
            times.push(first);
            drop(stream);
            std::thread::sleep(Duration::from_millis(1000));
        }
        let slow = times.iter().filter(|t| t.map(|v| v > 2000).unwrap_or(true)).count();
        println!("{} of {} rounds took longer than 2 s or never delivered", slow, times.len());
    }

    #[test]
    fn toggling_quickly_never_leaves_more_than_one_stream() {
        let fake = Fake::new(Mode::Frames);
        let (c, _) = controller(&fake);
        for _ in 0..10 {
            c.set_enabled(true, None);
            assert!(fake.alive() <= 1);
            c.set_enabled(false, None);
            assert_eq!(fake.alive(), 0);
        }
    }
}
