//! Gaia Desktop — the local presence and interface layer of the Gaia system.
//!
//! Architectural rule, kept absolute:
//!
//! > Desktop = presence, interface and local capabilities.
//! > Server  = cognition, memory, reasoning and agency.
//!
//! The Rust layer therefore owns only OS-close concerns: application
//! lifecycle, window management, secure server communication, local
//! filesystem access, notifications, audio and capture. Every module here
//! is a local capability; none of them reason, remember or interpret.
//! The frontend stays model-agnostic: whatever LLM Gaia Server orchestrates
//! is invisible to this shell.

pub mod audio;
pub mod capture;
pub mod communication;
pub mod error;
pub mod history;
pub mod library;
pub mod mcp;
pub mod notifications;
pub mod presence;
pub mod settings;
pub mod speech;
pub mod tray;
pub mod version;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            // While the microphone is on, closing the window hides it: she keeps listening, the tray says so.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if tray::keeps_running(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .map_err(|e| format!("could not resolve config directory: {e}"))?;

            // Settings first: everything else is configured from them.
            let settings_state = settings::SettingsState::open(Some(config_dir));
            let loaded = settings_state.get();
            app.manage(settings_state);

            // The single link to Gaia Server, configured from settings.
            let link = communication::ServerLink::new(loaded.server.clone());
            app.manage(link);

            // Local capability subsystems.
            app.manage(capture::CaptureManager::new());
            // The microphone engine. The master switch is runtime-only, so every launch starts
            // with the microphone off; nothing opens until the user switches it on.
            let handle = app.handle().clone();
            let mic = audio::MicController::new(
                std::sync::Arc::new(audio::CpalBackend),
                std::sync::Arc::new(move |event| {
                    use tauri::Emitter;
                    let _ = match &event {
                        audio::VoiceEvent::State(s) => handle.emit(audio::VOICE_STATE_EVENT, s),
                        audio::VoiceEvent::Error(e) => handle.emit(audio::VOICE_ERROR_EVENT, e),
                        audio::VoiceEvent::Level(l) => handle.emit(audio::VOICE_LEVEL_EVENT, l),
                    };
                }),
                audio::Timing::default(),
            );
            app.manage(mic.clone());

            // Push-to-talk: record one utterance, have Gaia Cloud transcribe it, emit the text. The
            // desktop never interprets it; the UI sends it as an ordinary turn.
            let ptt_handle = app.handle().clone();
            // One microphone tap at a time: push-to-talk and hands-free listening share this claim.
            let mic_claim = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let ptt = audio::ptt::PttController::new(
                mic,
                std::sync::Arc::new(speech::stt::CloudTranscriber::new(app.handle().clone())),
                std::sync::Arc::new(move |event| {
                    use tauri::Emitter;
                    let _ = match &event {
                        audio::ptt::PttEvent::State(s) => ptt_handle.emit(audio::ptt::PTT_EVENT, s),
                        audio::ptt::PttEvent::Transcript(t) => ptt_handle.emit(
                            audio::ptt::TRANSCRIPT_EVENT,
                            audio::ptt::TranscriptEvent::new(t.clone(), "ptt"),
                        ),
                    };
                }),
                std::sync::Arc::new(|future| {
                    tauri::async_runtime::spawn(future);
                }),
                std::time::Duration::from_secs(audio::utterance::MAX_RECORDING_SECS),
                mic_claim.clone(),
            );
            app.manage(ptt);
            app.manage(presence::PresenceController::new());

            // Gaia's voice: playback lives in Rust (spec: AEC needs the playback signal), with the
            // saved volume/mute applied from the first reply.
            let playback = speech::PlaybackEngine::new(loaded.audio.volume, loaded.audio.muted);
            playback.set_device(loaded.audio.output_device.clone());
            app.manage(playback);

            // Hands-free listening. Gaia's own voice is the echo canceller's reference, so playback
            // reports what it plays to the listener. A barge-in stops her voice here, in Rust, at once;
            // the UI is told so it can cancel the turn that is still streaming.
            let listen_handle = app.handle().clone();
            let continuous = audio::continuous::ContinuousListener::new(
                app.state::<audio::MicController>().inner().clone(),
                std::sync::Arc::new(speech::stt::CloudTranscriber::new(app.handle().clone())),
                std::sync::Arc::new(move |event| {
                    use tauri::Emitter;
                    match &event {
                        audio::continuous::ContinuousEvent::State(s) => {
                            let _ = listen_handle.emit(audio::continuous::CONTINUOUS_EVENT, s);
                        }
                        audio::continuous::ContinuousEvent::BargeIn => {
                            if let Some(playback) = listen_handle.try_state::<speech::PlaybackEngine>() {
                                playback.stop(speech::DEFAULT_FADE_MS);
                            }
                            let _ = listen_handle.emit(audio::continuous::BARGE_IN_EVENT, ());
                        }
                        audio::continuous::ContinuousEvent::Wake => {
                            // Every wake-up is counted and logged (time only), so the false-accept rate can be
                            // measured by leaving the app running.
                            if let Some(log) = listen_handle.try_state::<audio::wake::WakeLog>() {
                                log.record();
                            }
                            // "Hey Gaia" was heard: a soft chime says so (a false accept is then cheap and
                            // obvious), and the UI is told she is listening.
                            let h = listen_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Some(playback) = h.try_state::<speech::PlaybackEngine>() {
                                    let _ = playback.play(audio::wake::chime_wav()).await;
                                }
                            });
                            let _ = listen_handle.emit(audio::continuous::WAKE_EVENT, ());
                        }
                        audio::continuous::ContinuousEvent::Transcript(t) => {
                            let _ = listen_handle.emit(audio::ptt::TRANSCRIPT_EVENT, t);
                        }
                    }
                }),
                std::sync::Arc::new(|future| {
                    tauri::async_runtime::spawn(future);
                }),
                mic_claim.clone(),
                loaded.voice.barge_in_enabled,
            );
            app.state::<speech::PlaybackEngine>().set_render_sink(continuous.render_sink());

            // The enrolled wake word: its reference (features, never audio) is loaded from the config
            // directory. A settings flag without a file, or the other way round, is put right.
            let wake_store = audio::wake::WakeStore::new(app.path().app_config_dir().ok());
            if loaded.voice.wake_enrolled {
                match wake_store.load() {
                    Some(reference) => {
                        continuous.set_sensitivity(loaded.voice.wake_sensitivity);
                        continuous.set_wake_reference(Some(reference));
                    }
                    None => {
                        let _ = app.state::<settings::SettingsState>().update(|s| {
                            s.voice.wake_enrolled = false;
                            s.voice.wake_word_enabled = false;
                        });
                    }
                }
            } else if wake_store.exists() {
                let _ = wake_store.delete();
            }
            app.manage(continuous);
            app.manage(audio::enroll::WakeEnroller::new(
                app.state::<audio::MicController>().inner().clone(),
                mic_claim.clone(),
            ));
            app.manage(wake_store);
            app.manage(audio::wake::WakeLog::new(app.path().app_config_dir().ok()));

            // The tray keeps Gaia reachable while she listens with the window closed.
            tray::install(app)?;
            
            // Desktop version metadata
            app.manage(version::DesktopVersion::new());

            // Keep connection status and presence fresh in the background.
            communication::ServerLink::spawn_health_loop(app.handle().clone());
            // Relay realtime server events (e.g. conversation-history
            // changes from another client) as they arrive.
            communication::ServerLink::spawn_event_bridge(app.handle().clone());
            // Relay freshly-synthesised Kairos episodes for the live timeline.
            communication::ServerLink::spawn_episode_bridge(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // communication
            communication::server_get_config,
            communication::server_set_config,
            communication::server_get_status,
            communication::server_test_connection,
            communication::server_request,
            communication::server_stream_turn,
            communication::server_cancel_turn,
            communication::server_get_cloud_version,
            communication::version_get_desktop,
            // capture
            capture::capture_list_sources,
            capture::capture_now,
            capture::capture_submit,
            // audio
            audio::audio_get_status,
            audio::voice_get_status,
            audio::voice_list_devices,
            audio::voice_set_mic_enabled,
            audio::voice_select_device,
            audio::ptt::voice_ptt_start,
            audio::ptt::voice_ptt_stop,
            audio::ptt::voice_ptt_cancel,
            audio::ptt::voice_ptt_state,
            audio::continuous::voice_continuous_set,
            audio::continuous::voice_set_barge_in,
            audio::continuous::voice_continuous_state,
            audio::enroll::voice_wake_status,
            audio::enroll::voice_wake_enroll_record,
            audio::enroll::voice_wake_enroll_finish,
            audio::enroll::voice_wake_enroll_cancel,
            audio::enroll::voice_wake_set,
            audio::enroll::voice_wake_set_sensitivity,
            audio::enroll::voice_wake_forget,
            // history
            history::history_export_conversation,
            // library
            library::library_upload_file,
            library::library_list_files,
            library::library_download_file,
            library::library_delete_file,
            // speech
            speech::speech_synthesize,
            speech::speech_play,
            speech::speech_stop,
            speech::speech_set_gain,
            speech::speech_list_output_devices,
            speech::speech_select_output_device,
            // notifications
            notifications::notify,
            // settings
            settings::settings_get,
            settings::settings_save,
            // mcp — local tool servers, technical capability only
            mcp::mcp_list_tools,
            mcp::mcp_test_server,
            mcp::mcp_call_tool,
            // presence
            presence::presence_get,
            presence::presence_set_quiet,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gaia");
}
