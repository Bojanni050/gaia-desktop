//! Enrolling "Hey Gaia": recording the user saying it a few times and turning that into the wake word.
//!
//! The recordings exist only in memory, only until enrolment finishes (or is cancelled), and the user
//! starts each one. What is saved is the reference file (features, never audio, see `wake.rs`), which can
//! be deleted again at any time. While a recording runs it holds the microphone tap, like push-to-talk,
//! so hands-free listening and enrolment never overlap.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::continuous::ContinuousListener;
use super::stream::Downsampler;
use super::wake::{
    check_clip, enroll, ClipCheck, WakeStore, DEFAULT_SENSITIVITY, MAX_CLIPS, MAX_SENSITIVITY, MIN_CLIPS,
    MIN_SENSITIVITY,
};
use super::{MicController, VoiceError};

/// How long one enrolment recording runs: room for a breath, the phrase, and a pause.
pub const RECORD_SECS: f32 = 3.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipReport {
    pub check: ClipCheck,
    /// Accepted recordings so far.
    pub accepted: usize,
    pub min: usize,
    pub max: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeStatus {
    /// A wake word is enrolled on this device.
    pub enrolled: bool,
    /// "Hey Gaia" wakes her (needs enrolled).
    pub enabled: bool,
    pub sensitivity: f32,
    pub min_sensitivity: f32,
    pub max_sensitivity: f32,
    /// Recordings accepted in the enrolment in progress.
    pub clips: usize,
    pub min_clips: usize,
    pub max_clips: usize,
}

/// The enrolment in progress: accepted clips, in memory.
pub struct WakeEnroller {
    mic: MicController,
    exclusive: Arc<AtomicBool>,
    clips: Mutex<Vec<Vec<i16>>>,
}

fn err(code: &str, message: impl Into<String>) -> VoiceError {
    VoiceError { code: code.into(), message: message.into() }
}

impl WakeEnroller {
    pub fn new(mic: MicController, exclusive: Arc<AtomicBool>) -> Self {
        Self { mic, exclusive, clips: Mutex::new(Vec::new()) }
    }

    pub fn count(&self) -> usize {
        self.clips.lock().expect("enrolment poisoned").len()
    }

    /// Records one clip (blocking for `secs`), judges it and keeps it when it is good.
    pub fn record(&self, secs: f32) -> Result<ClipReport, VoiceError> {
        if self.count() >= MAX_CLIPS {
            return Err(err("enough", "that is enough recordings; finish enrolling"));
        }
        let Some(sink) = self.mic.capture_sink() else {
            return Err(err("mic_off", "switch the microphone on first"));
        };
        let rate = sink.sample_rate();
        if rate == 0 {
            return Err(err("mic_off", "the microphone is not ready yet"));
        }
        if self.exclusive.swap(true, Ordering::SeqCst) {
            return Err(err("busy", "the microphone is in use for listening or a recording"));
        }
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        sink.attach_tap(tx);
        std::thread::sleep(Duration::from_secs_f32(secs));
        sink.detach_tap();
        self.exclusive.store(false, Ordering::SeqCst);

        // the microphone must still be the same one (switched off or restarted mid-recording = a hole)
        let same = self.mic.capture_sink().map(|now| Arc::ptr_eq(&now, &sink)).unwrap_or(false);
        if !same {
            return Err(err("mic_interrupted", "the microphone was interrupted during the recording"));
        }
        let mut down = Downsampler::new(rate);
        let mut audio: Vec<f32> = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            down.push(&chunk, &mut audio);
        }
        let (check, pcm) = check_clip(&audio);
        drop(audio);
        let mut clips = self.clips.lock().expect("enrolment poisoned");
        if let Some(pcm) = pcm {
            clips.push(pcm);
        }
        Ok(ClipReport { check, accepted: clips.len(), min: MIN_CLIPS, max: MAX_CLIPS })
    }

    /// Takes the accepted clips (ending the enrolment).
    pub fn take(&self) -> Vec<Vec<i16>> {
        std::mem::take(&mut *self.clips.lock().expect("enrolment poisoned"))
    }

    /// Throws the recordings away.
    pub fn cancel(&self) {
        self.clips.lock().expect("enrolment poisoned").clear();
    }
}

fn settings_error(e: crate::settings::SettingsError) -> VoiceError {
    err("settings", e.to_string())
}

fn status_of(
    settings: &crate::settings::VoiceSettings,
    enrolled: bool,
    clips: usize,
) -> WakeStatus {
    WakeStatus {
        enrolled,
        enabled: enrolled && settings.wake_word_enabled,
        sensitivity: if settings.wake_sensitivity.is_finite() { settings.wake_sensitivity } else { DEFAULT_SENSITIVITY },
        min_sensitivity: MIN_SENSITIVITY,
        max_sensitivity: MAX_SENSITIVITY,
        clips,
        min_clips: MIN_CLIPS,
        max_clips: MAX_CLIPS,
    }
}

// ------------------------------------------------------------------------------------------------
// Tauri commands
// ------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn voice_wake_status(
    enroller: tauri::State<'_, WakeEnroller>,
    store: tauri::State<'_, WakeStore>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
) -> WakeStatus {
    status_of(&settings.get().voice, store.exists(), enroller.count())
}

/// Records one enrolment clip (takes about three seconds) and says whether it was good.
#[tauri::command]
pub async fn voice_wake_enroll_record(app: tauri::AppHandle) -> Result<ClipReport, VoiceError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let enroller = app.state::<WakeEnroller>();
        enroller.record(RECORD_SECS)
    })
    .await
    .map_err(|e| err("open_failed", e.to_string()))?
}

/// Builds and calibrates the wake word from the accepted recordings, and saves it.
#[tauri::command]
pub async fn voice_wake_enroll_finish(app: tauri::AppHandle) -> Result<WakeStatus, VoiceError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let enroller = app.state::<WakeEnroller>();
        let clips = enroller.take();
        let enrolled = enroll(&clips).map_err(|m| err("enrol_failed", m))?;
        let store = app.state::<WakeStore>();
        store.save(&enrolled.reference).map_err(|m| err("settings", m))?;
        let settings = app.state::<crate::settings::SettingsState>();
        settings
            .update(|s| {
                s.voice.wake_enrolled = true;
                s.voice.wake_sensitivity = enrolled.sensitivity;
            })
            .map_err(settings_error)?;
        let listener = app.state::<ContinuousListener>();
        listener.set_sensitivity(enrolled.sensitivity);
        listener.set_wake_reference(Some(enrolled.reference));
        Ok(status_of(&settings.get().voice, true, 0))
    })
    .await
    .map_err(|e| err("open_failed", e.to_string()))?
}

#[tauri::command]
pub fn voice_wake_enroll_cancel(enroller: tauri::State<'_, WakeEnroller>) {
    enroller.cancel();
}

/// Turns "Hey Gaia" on or off. Needs an enrolled wake word.
#[tauri::command]
pub fn voice_wake_set(
    listener: tauri::State<'_, ContinuousListener>,
    store: tauri::State<'_, WakeStore>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    enroller: tauri::State<'_, WakeEnroller>,
    enabled: bool,
) -> Result<WakeStatus, VoiceError> {
    let enrolled = store.exists();
    if enabled && !enrolled {
        return Err(err("not_enrolled", "enrol \"Hey Gaia\" first"));
    }
    settings.update(|s| s.voice.wake_word_enabled = enabled).map_err(settings_error)?;
    let voice = settings.get().voice;
    match listener.apply(voice.continuous_enabled, enabled && enrolled) {
        Ok(()) => {}
        Err(e) if e.code == "mic_off" => {} // saved; takes effect when the microphone is switched on
        Err(e) => return Err(e),
    }
    Ok(status_of(&voice, enrolled, enroller.count()))
}

/// Moves the wake-word sensitivity (higher = stricter). Takes effect at once.
#[tauri::command]
pub fn voice_wake_set_sensitivity(
    listener: tauri::State<'_, ContinuousListener>,
    store: tauri::State<'_, WakeStore>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    enroller: tauri::State<'_, WakeEnroller>,
    value: f32,
) -> Result<WakeStatus, VoiceError> {
    let value = value.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);
    settings.update(|s| s.voice.wake_sensitivity = value).map_err(settings_error)?;
    listener.set_sensitivity(value);
    Ok(status_of(&settings.get().voice, store.exists(), enroller.count()))
}

/// Forgets the wake word: the reference file is deleted and "Hey Gaia" stops waking her.
#[tauri::command]
pub fn voice_wake_forget(
    listener: tauri::State<'_, ContinuousListener>,
    store: tauri::State<'_, WakeStore>,
    settings: tauri::State<'_, crate::settings::SettingsState>,
    enroller: tauri::State<'_, WakeEnroller>,
) -> Result<WakeStatus, VoiceError> {
    store.delete().map_err(|m| err("settings", m))?;
    settings
        .update(|s| {
            s.voice.wake_enrolled = false;
            s.voice.wake_word_enabled = false;
        })
        .map_err(settings_error)?;
    listener.set_wake_reference(None);
    let voice = settings.get().voice;
    let _ = listener.apply(voice.continuous_enabled, false);
    enroller.cancel();
    Ok(status_of(&voice, false, 0))
}

#[cfg(test)]
mod tests {
    use super::super::testing::{Fake, Mode};
    use super::super::Timing;
    use super::*;

    fn rig(mode: Mode) -> (Arc<Fake>, MicController, WakeEnroller, Arc<AtomicBool>) {
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
        let exclusive = Arc::new(AtomicBool::new(false));
        let enroller = WakeEnroller::new(mic.clone(), exclusive.clone());
        (fake, mic, enroller, exclusive)
    }

    #[test]
    fn it_cannot_record_while_the_microphone_is_off() {
        let (_fake, _mic, enroller, exclusive) = rig(Mode::Bursts);
        assert_eq!(enroller.record(0.05).unwrap_err().code, "mic_off");
        assert!(!exclusive.load(Ordering::SeqCst));
        assert_eq!(enroller.count(), 0);
    }

    #[test]
    fn a_quiet_clip_is_judged_and_not_kept() {
        let (_fake, mic, enroller, exclusive) = rig(Mode::Quiet);
        mic.set_enabled(true, None);
        let report = enroller.record(0.4).expect("a report");
        assert!(!report.check.ok);
        assert_eq!(report.accepted, 0);
        assert_eq!(enroller.count(), 0);
        assert!(!exclusive.load(Ordering::SeqCst), "the microphone is released");
        mic.set_enabled(false, None);
    }

    #[test]
    fn the_recording_holds_the_tap_and_lets_go_of_it() {
        let (fake, mic, enroller, _) = rig(Mode::Bursts);
        mic.set_enabled(true, None);
        let sink = fake.last_sink.lock().unwrap().clone().unwrap();
        let _ = enroller.record(0.3);
        assert!(!sink.is_tapping());
        mic.set_enabled(false, None);
    }

    #[test]
    fn it_refuses_to_record_while_something_else_holds_the_microphone() {
        let (_fake, mic, enroller, exclusive) = rig(Mode::Bursts);
        mic.set_enabled(true, None);
        exclusive.store(true, Ordering::SeqCst); // hands-free listening, say
        assert_eq!(enroller.record(0.1).unwrap_err().code, "busy");
        assert!(exclusive.load(Ordering::SeqCst), "the other claim is not released by the refusal");
        mic.set_enabled(false, None);
    }

    #[test]
    fn switching_the_microphone_off_mid_recording_discards_it() {
        let (_fake, mic, enroller, _) = rig(Mode::Bursts);
        mic.set_enabled(true, None);
        let mic2 = mic.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            mic2.set_enabled(false, None);
        });
        let result = enroller.record(0.5);
        stopper.join().unwrap();
        assert_eq!(result.unwrap_err().code, "mic_interrupted");
        assert_eq!(enroller.count(), 0);
    }

    #[test]
    fn cancel_and_take_empty_the_recordings() {
        let (_fake, _mic, enroller, _) = rig(Mode::Bursts);
        enroller.clips.lock().unwrap().push(vec![1, 2, 3]);
        assert_eq!(enroller.count(), 1);
        assert_eq!(enroller.take().len(), 1);
        assert_eq!(enroller.count(), 0);
        enroller.clips.lock().unwrap().push(vec![1]);
        enroller.cancel();
        assert_eq!(enroller.count(), 0);
    }

    #[test]
    fn the_status_reports_enrolment_and_never_claims_enabled_without_one() {
        let mut voice = crate::settings::VoiceSettings::default();
        voice.wake_word_enabled = true;
        let s = status_of(&voice, false, 3);
        assert!(!s.enrolled && !s.enabled, "a stored 'enabled' without a wake word means nothing");
        assert_eq!(s.clips, 3);
        let s = status_of(&voice, true, 0);
        assert!(s.enrolled && s.enabled);
        voice.wake_sensitivity = f32::NAN;
        assert_eq!(status_of(&voice, true, 0).sensitivity, DEFAULT_SENSITIVITY);
    }
}
