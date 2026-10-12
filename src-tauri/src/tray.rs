//! The system tray: how Gaia stays reachable while she listens with her window closed.
//!
//! Closing the window normally quits the app. While the microphone is on that would silently end the
//! conversation, so the close button hides the window instead and the tray icon says she is still there.
//! With the microphone off, closing quits exactly as it always did: nothing keeps running unseen.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::audio::continuous::ContinuousListener;
use crate::audio::ptt::PttController;
use crate::audio::MicController;

const MAIN: &str = "main";

/// True while the microphone is on: the window then hides instead of closing.
pub fn keeps_running(app: &AppHandle) -> bool {
    app.try_state::<MicController>()
        .map(|mic| mic.status().mic_enabled)
        .unwrap_or(false)
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// The tray's "microphone off": ends hands-free listening and any recording, then releases the device.
fn mic_off(app: &AppHandle) {
    if let Some(listener) = app.try_state::<ContinuousListener>() {
        listener.stop();
    }
    if let Some(ptt) = app.try_state::<PttController>() {
        ptt.cancel();
    }
    if let Some(mic) = app.try_state::<MicController>() {
        mic.set_enabled(false, None);
    }
}

pub fn install(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Gaia tonen", true, None::<&str>)?;
    let mic = MenuItem::with_id(app, "mic_off", "Microfoon uitzetten", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Gaia afsluiten", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &mic, &quit])?;

    let mut builder = TrayIconBuilder::with_id("gaia")
        .tooltip("Gaia")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "mic_off" => mic_off(app),
            "quit" => {
                mic_off(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
