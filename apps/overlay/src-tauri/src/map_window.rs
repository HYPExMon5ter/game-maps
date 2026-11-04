//! The map window: an ordinary, interactive window with the full map, kept open on a
//! monitor of the user's choice (typically a second screen) while they play. It runs the
//! same UI as the overlay in its "window" view (`index.html?view=window`).

use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

use crate::settings;

pub const WINDOW_LABEL: &str = "map-window";

/// A monitor the map window can go on.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// The system's name for it (stable while it stays plugged into the same port).
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

pub fn monitors(app: &AppHandle) -> Vec<MonitorInfo> {
    let primary = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    app.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            let name = m.name()?.clone();
            Some(MonitorInfo { primary: primary.as_ref() == Some(&name), width: m.size().width, height: m.size().height, name })
        })
        .collect()
}

pub fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(WINDOW_LABEL).is_some()
}

/// Opens the map window on `monitor` (by name; another one if it's gone), moves it
/// there if it's open, or closes it (`None`).
pub fn apply(app: &AppHandle, monitor: Option<&str>) -> Result<(), String> {
    let Some(wanted) = monitor else {
        if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
            win.destroy().map_err(|e| e.to_string())?;
        }
        return Ok(());
    };
    let all = app.available_monitors().map_err(|e| e.to_string())?;
    let primary = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    // The chosen monitor, else a secondary one, else whatever there is.
    let target = all
        .iter()
        .find(|m| m.name().is_some_and(|n| n == wanted))
        .or_else(|| all.iter().find(|m| m.name() != primary.as_ref()))
        .or(all.first())
        .ok_or("no monitor found")?;
    let PhysicalPosition { x, y } = *target.position();
    let scale = target.scale_factor();
    let win = match app.get_webview_window(WINDOW_LABEL) {
        Some(win) => {
            let _ = win.unmaximize();
            win
        }
        None => WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("index.html?view=window".into()))
            .title("Game Maps")
            .inner_size(1280.0, 800.0)
            .min_inner_size(480.0, 360.0)
            .background_color(tauri::window::Color(14, 18, 22, 255))
            .focused(false)
            .visible(false)
            .build()
            .inspect(crate::recovery::watch_webview)
            .map_err(|e| e.to_string())?,
    };
    win.set_position(PhysicalPosition::new(x + (40.0 * scale) as i32, y + (40.0 * scale) as i32)).map_err(|e| e.to_string())?;
    win.maximize().map_err(|e| e.to_string())?;
    win.show().map_err(|e| e.to_string())?;
    Ok(())
}

/// Opens the window at startup if the user left it on.
pub fn restore(app: &AppHandle) {
    let monitor = settings::get(app).map_window;
    if let Some(m) = monitor {
        if let Err(e) = apply(app, Some(&m)) {
            log::warn!("map window: {e}");
        }
    }
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    // Closing the window turns it off until it's turned on again in the settings.
    if let WindowEvent::CloseRequested { .. } = event {
        if let Err(e) = settings::update(window.app_handle(), |s| s.map_window = None) {
            log::warn!("map window: {e}");
        }
    }
}
