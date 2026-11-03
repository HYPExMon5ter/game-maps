//! Overlay window modes:
//! - `Hidden`: window hidden, zero cost.
//! - `Open`: covers the game's monitor, takes focus and input (full map UI).
//! - `Pinned`: small click-through HUD (mini-map and/or tracking compass) while playing.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsWindow, SetForegroundWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
};

use crate::injector;

pub const WINDOW_LABEL: &str = "overlay";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    Hidden,
    Open,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PinConfig {
    /// Whether a HUD (mini-map and/or tracking compass) stays on screen while playing.
    pub enabled: bool,
    pub corner: Corner,
    /// HUD size in logical pixels.
    pub width: u32,
    pub height: u32,
    /// Distance from the screen edges in logical pixels.
    pub margin: u32,
    /// Cover the game's whole screen (still click-through), so waypoints can be drawn
    /// over the game; the HUD then places itself in its corner.
    #[serde(default)]
    pub fullscreen: bool,
}

impl Default for PinConfig {
    fn default() -> Self {
        Self { enabled: false, corner: Corner::TopRight, width: 320, height: 320, margin: 24, fullscreen: false }
    }
}

pub struct OverlayState {
    mode: Mutex<OverlayMode>,
    pin: Mutex<PinConfig>,
    /// Game process and its main window, once the window exists (resolved lazily:
    /// the game may still be starting when the app attaches).
    game: Mutex<Option<(u32, Option<isize>)>>,
    /// Where the window must be while shown (see [`place`]).
    placement: Mutex<Option<Rect>>,
    /// Pinned HUD temporarily hidden because another app is in front of the game.
    suspended: Mutex<bool>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            mode: Mutex::new(OverlayMode::Hidden),
            pin: Mutex::new(PinConfig::default()),
            game: Mutex::new(None),
            placement: Mutex::new(None),
            suspended: Mutex::new(false),
        }
    }
}

fn state(app: &AppHandle) -> tauri::State<'_, OverlayState> {
    app.state::<OverlayState>()
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WINDOW_LABEL)
}

pub fn mode(app: &AppHandle) -> OverlayMode {
    *state(app).mode.lock().unwrap()
}

pub fn is_visible(app: &AppHandle) -> bool {
    mode(app) != OverlayMode::Hidden
}

/// Remembers the game's process so the overlay can follow its monitor and hand focus back.
pub fn set_game_window(app: &AppHandle, pid: u32) {
    let st = state(app);
    let mut game = st.game.lock().unwrap();
    if game.is_none_or(|(p, _)| p != pid) {
        *game = Some((pid, None));
    }
}

/// The game's main window, looked up again if it doesn't exist yet or was recreated
/// (e.g. after a display mode change).
fn game_hwnd(app: &AppHandle) -> Option<HWND> {
    let st = state(app);
    let mut game = st.game.lock().unwrap();
    let (pid, cached) = (*game)?;
    if let Some(h) = cached.filter(|&h| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool()) {
        return Some(HWND(h as *mut _));
    }
    let hwnd = injector::main_window(pid)?;
    *game = Some((pid, Some(hwnd.0 as isize)));
    Some(hwnd)
}

/// Physical rect (x, y, w, h) and scale factor of the monitor showing the game,
/// falling back to the primary monitor.
fn target_monitor(app: &AppHandle, win: &WebviewWindow) -> Option<(i32, i32, u32, u32, f64)> {
    let monitor = game_hwnd(app)
        .and_then(|hwnd| {
            let mut r = RECT::default();
            unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
            win.monitor_from_point(((r.left + r.right) / 2) as f64, ((r.top + r.bottom) / 2) as f64).ok().flatten()
        })
        .or_else(|| win.primary_monitor().ok().flatten())?;
    let p = monitor.position();
    let s = monitor.size();
    Some((p.x, p.y, s.width, s.height, monitor.scale_factor()))
}

/// Physical screen rect (x, y, width, height).
type Rect = (i32, i32, i32, i32);

/// Places the overlay at an exact physical screen rect and keeps it there.
///
/// Tauri's `set_position`/`set_size` (and its own later style and first-show updates)
/// adjust for a title bar this undecorated window doesn't draw, shifting it by the
/// caption and border size (20 px left and down, 88 px short). So the rect is set
/// directly, and [`enforce_placement`] restores it whenever the window drifts.
fn place(app: &AppHandle, win: &WebviewWindow, x: i32, y: i32, w: u32, h: u32) {
    let rect = (x, y, w.max(1) as i32, h.max(1) as i32);
    *state(app).placement.lock().unwrap() = Some(rect);
    let Ok(hwnd) = win.hwnd() else { return };
    let hwnd = hwnd.0 as usize;
    // Queued after any pending window updates on the main thread.
    let _ = win.run_on_main_thread(move || set_rect(HWND(hwnd as _), rect));
}

fn set_rect(hwnd: HWND, (x, y, w, h): Rect) {
    let flags = SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;
    if let Err(e) = unsafe { SetWindowPos(hwnd, None, x, y, w, h, flags) } {
        log::warn!("placing overlay at {x},{y} {w}x{h} failed: {e}");
    }
}

/// Puts the window back at its intended rect if something moved or resized it.
/// Called from the window's move/resize events (main thread).
pub fn enforce_placement(app: &AppHandle) {
    let Some(target) = *state(app).placement.lock().unwrap() else { return };
    let Some(win) = window(app) else { return };
    let Ok(hwnd) = win.hwnd() else { return };
    let hwnd = HWND(hwnd.0 as _);
    let mut r = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut r) }.is_err() {
        return;
    }
    if (r.left, r.top, r.right - r.left, r.bottom - r.top) != target {
        set_rect(hwnd, target);
    }
}

fn focus_game(app: &AppHandle) {
    if let Some(hwnd) = game_hwnd(app) {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

/// Mode change requested by the user (hotkey, tray, UI).
fn apply(app: &AppHandle, next: OverlayMode) {
    *state(app).suspended.lock().unwrap() = false;
    apply_with(app, next, true);
}

/// Switches mode. `return_focus`: when leaving the full map, give focus back to the
/// game (not when the user switched to another app themselves).
fn apply_with(app: &AppHandle, next: OverlayMode, return_focus: bool) {
    let Some(win) = window(app) else { return };
    match next {
        OverlayMode::Hidden => {
            *state(app).placement.lock().unwrap() = None;
            let _ = win.hide();
        }
        OverlayMode::Open => {
            let _ = win.set_ignore_cursor_events(false);
            let _ = win.show();
            let _ = win.set_focus();
            if let Some((x, y, w, h, _)) = target_monitor(app, &win) {
                place(app, &win, x, y, w, h);
            }
        }
        OverlayMode::Pinned => {
            let _ = win.set_ignore_cursor_events(true);
            if *state(app).suspended.lock().unwrap() {
                let _ = win.hide();
            } else {
                let _ = win.show();
            }
            let pin = *state(app).pin.lock().unwrap();
            if let Some((mx, my, mw, mh, scale)) = target_monitor(app, &win).filter(|_| !pin.fullscreen) {
                let w = (pin.width as f64 * scale).round() as i32;
                let h = (pin.height as f64 * scale).round() as i32;
                let margin = (pin.margin as f64 * scale).round() as i32;
                let (x, y) = match pin.corner {
                    Corner::TopLeft => (mx + margin, my + margin),
                    Corner::TopRight => (mx + mw as i32 - w - margin, my + margin),
                    Corner::BottomLeft => (mx + margin, my + mh as i32 - h - margin),
                    Corner::BottomRight => (mx + mw as i32 - w - margin, my + mh as i32 - h - margin),
                };
                place(app, &win, x, y, w.max(1) as u32, h.max(1) as u32);
            } else if let Some((mx, my, mw, mh, _)) = target_monitor(app, &win) {
                // Waypoints: cover the game's screen; the HUD places itself in its corner.
                place(app, &win, mx, my, mw, mh);
            }
        }
    }
    let previous = std::mem::replace(&mut *state(app).mode.lock().unwrap(), next);
    if return_focus && previous == OverlayMode::Open && next != OverlayMode::Open {
        focus_game(app);
    }
    let _ = app.emit("overlay-mode", next);
}

pub fn open(app: &AppHandle) {
    apply(app, OverlayMode::Open);
}

/// Leaves the full map: back to the pinned mini-map if enabled, otherwise hidden.
pub fn close(app: &AppHandle) {
    let pinned = state(app).pin.lock().unwrap().enabled;
    apply(app, if pinned { OverlayMode::Pinned } else { OverlayMode::Hidden });
}

pub fn toggle(app: &AppHandle) {
    if mode(app) == OverlayMode::Open {
        close(app)
    } else {
        open(app)
    }
}

/// Zoom key pressed in the game: zooms the mini-map by `steps` (the full map has focus
/// while open and handles its own keys).
pub fn zoom(app: &AppHandle, steps: i32) {
    if mode(app) == OverlayMode::Pinned {
        let _ = app.emit_to(WINDOW_LABEL, "map-zoom", steps);
    }
}

pub fn set_pin_config(app: &AppHandle, config: PinConfig) {
    *state(app).pin.lock().unwrap() = config;
    match (mode(app), config.enabled) {
        (OverlayMode::Pinned, true) => apply_with(app, OverlayMode::Pinned, false),
        (OverlayMode::Pinned, false) => apply_with(app, OverlayMode::Hidden, false),
        _ => {}
    }
}

/// How often the overlay checks which window is in front.
const FOCUS_POLL: std::time::Duration = std::time::Duration::from_millis(150);

/// Keeps the overlay in step with the game window, so it never sits on top of other
/// apps or outlives the game:
/// - full map open and the user switches to another app: close it (focus stays there);
/// - mini-map pinned: hidden while another app is in front, back when the game is;
/// - game closed or crashed: hide everything.
pub fn watch_focus(app: AppHandle) {
    std::thread::Builder::new()
        .name("overlay-focus".into())
        .spawn(move || loop {
            std::thread::sleep(FOCUS_POLL);
            sync_with_game(&app);
        })
        .expect("spawn overlay focus watcher");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    /// The game or this app.
    Ours,
    Other,
    /// No foreground window (e.g. during alt-tab or a UAC prompt): change nothing.
    Unknown,
}

fn front(app: &AppHandle, game_pid: u32) -> Front {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return Front::Unknown;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    if pid == game_pid || pid == std::process::id() || window(app).and_then(|w| w.hwnd().ok()).is_some_and(|h| h.0 as isize == fg.0 as isize) {
        Front::Ours
    } else {
        Front::Other
    }
}

fn sync_with_game(app: &AppHandle) {
    let mode = mode(app);
    // No game attached (map opened from the tray): nothing to follow.
    let Some((pid, _)) = *state(app).game.lock().unwrap() else { return };
    if !injector::is_alive(pid) {
        *state(app).game.lock().unwrap() = None;
        if mode != OverlayMode::Hidden {
            apply_with(app, OverlayMode::Hidden, false);
        }
        return;
    }
    match (mode, front(app, pid)) {
        // A game attached (app start, game start, reconnect): bring the mini-map and
        // tracking HUD back once the game is in front, as it was left.
        (OverlayMode::Hidden, Front::Ours) if state(app).pin.lock().unwrap().enabled => apply(app, OverlayMode::Pinned),
        (OverlayMode::Open, Front::Other) => {
            // The HUD must not appear over the app the user switched to.
            *state(app).suspended.lock().unwrap() = true;
            let pinned = state(app).pin.lock().unwrap().enabled;
            apply_with(app, if pinned { OverlayMode::Pinned } else { OverlayMode::Hidden }, false);
        }
        (OverlayMode::Pinned, Front::Other) => suspend(app, true),
        (OverlayMode::Pinned, Front::Ours) => suspend(app, false),
        _ => {}
    }
}

fn suspend(app: &AppHandle, suspended: bool) {
    let st = state(app);
    let mut current = st.suspended.lock().unwrap();
    if *current == suspended {
        return;
    }
    *current = suspended;
    drop(current);
    if let Some(win) = window(app) {
        let _ = if suspended { win.hide() } else { win.show() };
    }
}
