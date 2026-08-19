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
pub mod notifications;
pub mod presence;
pub mod settings;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
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
            app.manage(audio::AudioSubsystem::new());
            app.manage(presence::PresenceController::new());

            // Keep connection status and presence fresh in the background.
            communication::ServerLink::spawn_health_loop(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // communication
            communication::server_get_config,
            communication::server_set_config,
            communication::server_get_status,
            communication::server_test_connection,
            communication::server_request,
            // capture
            capture::capture_list_sources,
            capture::capture_now,
            capture::capture_submit,
            // audio
            audio::audio_get_status,
            audio::audio_set_permission,
            // notifications
            notifications::notify,
            // settings
            settings::settings_get,
            settings::settings_save,
            // presence
            presence::presence_get,
            presence::presence_set_quiet,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gaia");
}
