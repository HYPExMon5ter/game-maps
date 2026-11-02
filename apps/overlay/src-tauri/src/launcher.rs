//! The launcher: a regular window listing supported games, their status and map
//! downloads, plus app settings. Closing it keeps the app running in the tray.

use tauri::{AppHandle, Manager, Window, WindowEvent};

pub const WINDOW_LABEL: &str = "launcher";

pub fn show(app: &AppHandle) {
    let Some(win) = app.get_webview_window(WINDOW_LABEL) else { return };
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        // The app keeps watching for games from the tray.
        api.prevent_close();
        let _ = window.hide();
    }
}
