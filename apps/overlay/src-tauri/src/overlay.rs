//! Overlay window modes:
//! - `Hidden`: window hidden, zero cost.
//! - `Open`: covers the game's monitor, takes focus and input (full map UI).
//! - `Pinned`: small click-through HUD (mini-map and/or tracking compass) while playing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use game_maps_protocol::HotkeyAction;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
};

use crate::analytics;
use crate::injector;
use crate::link::{AgentLink, HIDDEN_SNAPSHOT_HZ, SNAPSHOT_HZ};
use crate::map_window;
use crate::settings;

pub const WINDOW_LABEL: &str = "overlay";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    Hidden,
    Open,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PinConfig {
    /// Whether a HUD (mini-map and/or tracking compass) stays on screen while playing.
    pub enabled: bool,
    /// Where the HUD sits: 0..1 across the room left on the screen inside the margin.
    pub anchor_x: f64,
    pub anchor_y: f64,
    /// HUD size in logical pixels.
    pub width: u32,
    pub height: u32,
    /// Distance from the screen edges in logical pixels.
    pub margin: u32,
    /// Cover the game's whole screen (still click-through), so waypoints can be drawn
    /// over the game; the HUD then places itself.
    #[serde(default)]
    pub fullscreen: bool,
}

impl Default for PinConfig {
    fn default() -> Self {
        Self { enabled: false, anchor_x: 1.0, anchor_y: 0.0, width: 320, height: 320, margin: 24, fullscreen: false }
    }
}

pub struct OverlayState {
    mode: Mutex<OverlayMode>,
    pin: Mutex<PinConfig>,
    /// Game process and its main window, once the window exists (resolved lazily:
    /// the game may still be starting when the app attaches).
    game: Mutex<Option<(u32, Option<isize>)>>,
    /// When the game's window was last searched for: that goes through every window, and
    /// a starting game has none yet for a good while.
    window_search: Mutex<Option<Instant>>,
    /// Where the window must be while shown (see [`place`]).
    placement: Mutex<Option<Rect>>,
    /// The monitor the window was last placed for.
    monitor: Mutex<Option<Rect>>,
    /// What the game was drawing on when the HUD was last placed.
    game_area: Mutex<Option<Rect>>,
    /// Pinned HUD temporarily hidden because another app is in front of the game.
    suspended: Mutex<bool>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            mode: Mutex::new(OverlayMode::Hidden),
            pin: Mutex::new(PinConfig::default()),
            game: Mutex::new(None),
            window_search: Mutex::new(None),
            placement: Mutex::new(None),
            monitor: Mutex::new(None),
            game_area: Mutex::new(None),
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
        *st.window_search.lock().unwrap() = None;
    }
}

const WINDOW_SEARCH_INTERVAL: Duration = Duration::from_secs(1);

/// The game's main window, looked up again if it doesn't exist yet or was recreated
/// (e.g. after a display mode change).
fn game_hwnd(app: &AppHandle) -> Option<HWND> {
    let st = state(app);
    let mut game = st.game.lock().unwrap();
    let (pid, cached) = (*game)?;
    if let Some(h) = cached.filter(|&h| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool()) {
        return Some(HWND(h as *mut _));
    }
    let mut searched = st.window_search.lock().unwrap();
    if searched.is_some_and(|t| t.elapsed() < WINDOW_SEARCH_INTERVAL) {
        return None;
    }
    *searched = Some(Instant::now());
    let hwnd = injector::main_window(pid)?;
    *game = Some((pid, Some(hwnd.0 as isize)));
    Some(hwnd)
}

fn game_minimized(app: &AppHandle) -> bool {
    game_hwnd(app).is_some_and(|hwnd| unsafe { IsIconic(hwnd) }.as_bool())
}

/// The middle of the game's window, unless it's minimized (parked off every screen).
fn game_center(app: &AppHandle) -> Option<(i32, i32)> {
    let hwnd = game_hwnd(app)?;
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some(((r.left + r.right) / 2, (r.top + r.bottom) / 2))
}

/// The physical screen rect the game draws on, unless it's minimized.
fn game_area(app: &AppHandle) -> Option<Rect> {
    let hwnd = game_hwnd(app)?;
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client) }.ok()?;
    let mut origin = POINT::default();
    unsafe { ClientToScreen(hwnd, &mut origin) }.ok().ok()?;
    let (w, h) = (client.right - client.left, client.bottom - client.top);
    (w > 0 && h > 0).then_some((origin.x, origin.y, w, h))
}

/// Physical rect (x, y, w, h) and scale factor of the monitor showing the game,
/// falling back to the primary monitor. Remembered for [`follow_game`].
fn target_monitor(app: &AppHandle, win: &WebviewWindow) -> Option<(i32, i32, u32, u32, f64)> {
    let monitor = game_center(app)
        .and_then(|(x, y)| win.monitor_from_point(x as f64, y as f64).ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten())?;
    let (x, y, w, h) = monitor_rect(&monitor);
    *state(app).monitor.lock().unwrap() = Some((x, y, w, h));
    Some((x, y, w as u32, h as u32, monitor.scale_factor()))
}

fn monitor_rect(monitor: &tauri::Monitor) -> Rect {
    let (p, s) = (monitor.position(), monitor.size());
    (p.x, p.y, s.width as i32, s.height as i32)
}

/// Physical screen rect (x, y, width, height).
type Rect = (i32, i32, i32, i32);

fn contains((x, y, w, h): Rect, (px, py): (i32, i32)) -> bool {
    px >= x && px < x + w && py >= y && py < y + h
}

/// Moves the overlay along when the game is on another monitor than the one it was placed
/// for: the game can open or move there after the overlay is shown.
fn follow_game(app: &AppHandle, mode: OverlayMode) {
    if mode == OverlayMode::Hidden {
        return;
    }
    let Some(center) = game_center(app) else { return };
    let placed = *state(app).monitor.lock().unwrap();
    if placed.is_some_and(|m| contains(m, center)) {
        // Same monitor; a windowed game may still have moved or changed size.
        let area = game_area(app);
        if mode == OverlayMode::Pinned && area.is_some() && area != *state(app).game_area.lock().unwrap() {
            apply_with(app, mode, false);
        }
        return;
    }
    let Some(win) = window(app) else { return };
    let now = win.monitor_from_point(center.0 as f64, center.1 as f64).ok().flatten();
    // Off every screen: nowhere better to go.
    if now.is_some_and(|m| Some(monitor_rect(&m)) != placed) {
        log::info!("the game moved to another monitor; following it");
        apply_with(app, mode, false);
    }
}

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
    // Leaving a map opened over a minimized game: the HUD stays away until it's back.
    *state(app).suspended.lock().unwrap() = next != OverlayMode::Open && game_minimized(app);
    apply_with(app, next, true);
}

/// Switches mode. `return_focus`: when leaving the full map, give focus back to the
/// game (not when the user switched to another app themselves).
fn apply_with(app: &AppHandle, next: OverlayMode, return_focus: bool) {
    let Some(win) = window(app) else { return };
    // First: the focus watcher runs alongside this, and acting on the old mode it could
    // hide the window this is showing.
    let previous = std::mem::replace(&mut *state(app).mode.lock().unwrap(), next);
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
            if let Some((mx, my, mw, mh, scale)) = target_monitor(app, &win) {
                // What the game draws on: a windowed game doesn't fill its monitor, and
                // markers over the game are placed from this window's size.
                let area = game_area(app).unwrap_or((mx, my, mw as i32, mh as i32));
                *state(app).game_area.lock().unwrap() = Some(area);
                let (ax, ay, aw, ah) = area;
                if pin.fullscreen {
                    // Waypoints: cover the game; the HUD places itself.
                    place(app, &win, ax, ay, aw as u32, ah as u32);
                } else {
                    let w = (pin.width as f64 * scale).round() as i32;
                    let h = (pin.height as f64 * scale).round() as i32;
                    let margin = (pin.margin as f64 * scale).round() as i32;
                    let room = |screen: i32, size: i32| (screen - size - 2 * margin).max(0) as f64;
                    let x = ax + margin + (pin.anchor_x * room(aw, w)).round() as i32;
                    let y = ay + margin + (pin.anchor_y * room(ah, h)).round() as i32;
                    place(app, &win, x, y, w.max(1) as u32, h.max(1) as u32);
                }
            }
        }
    }
    if return_focus && previous == OverlayMode::Open && next != OverlayMode::Open {
        focus_game(app);
    }
    if next == OverlayMode::Open && previous != OverlayMode::Open {
        if let Some(link) = app.try_state::<Arc<AgentLink>>() {
            link.map_opened();
        }
        crate::worldmaps::map_opened(app);
    }
    let _ = app.emit("overlay-mode", next);
    update_stream_rate(app);
}

/// How often the agent should sample the game: it does that on the game's main thread, so
/// only at full rate while something on screen shows it.
pub fn stream_rate(app: &AppHandle) -> u32 {
    let shown = is_visible(app) && !*state(app).suspended.lock().unwrap();
    if shown || map_window::is_open(app) {
        SNAPSHOT_HZ
    } else {
        HIDDEN_SNAPSHOT_HZ
    }
}

/// Tells the agent after anything that changes [`stream_rate`].
pub fn update_stream_rate(app: &AppHandle) {
    if let Some(link) = app.try_state::<Arc<AgentLink>>() {
        link.set_stream_rate(stream_rate(app));
    }
}

pub fn open(app: &AppHandle) {
    if mode(app) != OverlayMode::Open {
        analytics::count(app, analytics::Feature::MapOpen);
    }
    apply(app, OverlayMode::Open);
}

/// What's left on screen without the full map. The HUD belongs over a game, so with none
/// attached (map browsed from the tray or launcher) nothing is.
fn resting_mode(hud_enabled: bool, game_attached: bool) -> OverlayMode {
    if hud_enabled && game_attached {
        OverlayMode::Pinned
    } else {
        OverlayMode::Hidden
    }
}

fn current_resting_mode(app: &AppHandle) -> OverlayMode {
    let st = state(app);
    let hud_enabled = st.pin.lock().unwrap().enabled;
    let game_attached = st.game.lock().unwrap().is_some();
    resting_mode(hud_enabled, game_attached)
}

/// Leaves the full map.
pub fn close(app: &AppHandle) {
    apply(app, current_resting_mode(app));
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
/// - game minimized: the same as another app being in front;
/// - game closed or crashed: hide everything.
pub fn watch_focus(app: AppHandle) {
    std::thread::Builder::new()
        .name("overlay-focus".into())
        .spawn(move || loop {
            std::thread::sleep(FOCUS_POLL);
            // Must keep running: it's what hides the overlay when the game goes away.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sync_with_game(&app)));
        })
        .expect("spawn overlay focus watcher");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    Game,
    /// One of this app's windows.
    App,
    Other,
    /// No foreground window (e.g. during alt-tab or a UAC prompt): change nothing.
    Unknown,
}

fn front(game_pid: u32) -> Front {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return Front::Unknown;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    if pid == game_pid {
        Front::Game
    } else if pid == std::process::id() {
        Front::App
    } else {
        Front::Other
    }
}

/// What the overlay follows. Minimizing the game often leaves one of this app's windows in
/// front, so it counts as switching away. A map opened after that, from the launcher or
/// tray, was asked for and stays.
fn followed_front(real: Front, minimized: bool, just_minimized: bool, mode: OverlayMode) -> Front {
    if minimized && (mode != OverlayMode::Open || just_minimized) {
        Front::Other
    } else {
        real
    }
}

/// The map key while the agent can't report it (not connected, or not working): the
/// map then opens as a plain map over the game. Polled, so the game still sees the key.
fn poll_map_key(app: &AppHandle, in_front: bool) {
    static WAS_DOWN: AtomicBool = AtomicBool::new(false);
    let Some(link) = app.try_state::<Arc<AgentLink>>() else { return };
    let Some(registry) = app.try_state::<crate::Registry>() else { return };
    let Some(game) = link.status().game_id.and_then(|id| registry.0.iter().find(|g| g.id == id).cloned()) else { return };
    let Some(key) = settings::get(app).hotkeys(&game).into_iter().find(|h| h.action == HotkeyAction::ToggleMap) else { return };
    let state = unsafe { GetAsyncKeyState(key.vk as i32) };
    let (down, tapped) = (state < 0, state & 1 != 0);
    let was_down = WAS_DOWN.swap(down, Ordering::Relaxed);
    if !(down || tapped) || was_down || !in_front || link.is_live() || mode(app) == OverlayMode::Open {
        return;
    }
    let held = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(vk.0 as i32) < 0 };
    if held(VK_CONTROL) == key.ctrl && held(VK_SHIFT) == key.shift && held(VK_MENU) == key.alt {
        log::info!("opening the map without the agent");
        open(app);
    }
}

fn sync_with_game(app: &AppHandle) {
    let mode = mode(app);
    // No game attached (map opened from the tray or launcher): nothing to follow, but the
    // map mustn't stay on top of the app the user switched to.
    let Some((pid, _)) = *state(app).game.lock().unwrap() else {
        if mode == OverlayMode::Open && front(0) == Front::Other {
            apply_with(app, OverlayMode::Hidden, false);
        }
        return;
    };
    if !injector::is_alive(pid) {
        *state(app).game.lock().unwrap() = None;
        if mode != OverlayMode::Hidden {
            apply_with(app, OverlayMode::Hidden, false);
        }
        return;
    }
    let in_front = front(pid);
    // Only the game: typing that key in one of this app's windows isn't opening the map.
    poll_map_key(app, in_front == Front::Game);
    follow_game(app, mode);
    static WAS_MINIMIZED: AtomicBool = AtomicBool::new(false);
    let minimized = game_minimized(app);
    let was_minimized = WAS_MINIMIZED.swap(minimized, Ordering::Relaxed);
    match (mode, followed_front(in_front, minimized, minimized && !was_minimized, mode)) {
        // A game attached (app start, game start, reconnect): bring the mini-map and
        // tracking HUD back once the game is in front, as it was left.
        (OverlayMode::Hidden, Front::Game | Front::App) if state(app).pin.lock().unwrap().enabled => apply(app, OverlayMode::Pinned),
        (OverlayMode::Open, Front::Other) => {
            // The HUD must not appear over the app the user switched to.
            *state(app).suspended.lock().unwrap() = true;
            apply_with(app, current_resting_mode(app), false);
        }
        (OverlayMode::Pinned, Front::Other) => suspend(app, true),
        (OverlayMode::Pinned, Front::Game | Front::App) => suspend(app, false),
        _ => {}
    }
}

fn suspend(app: &AppHandle, suspended: bool) {
    // The caller read the mode a moment ago; the map may have opened since.
    if mode(app) != OverlayMode::Pinned {
        return;
    }
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
    update_stream_rate(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_map_keeps_the_hud_only_over_a_game() {
        assert_eq!(resting_mode(true, true), OverlayMode::Pinned);
        assert_eq!(resting_mode(true, false), OverlayMode::Hidden);
        assert_eq!(resting_mode(false, true), OverlayMode::Hidden);
        assert_eq!(resting_mode(false, false), OverlayMode::Hidden);
    }

    #[test]
    fn a_minimized_game_counts_as_switching_away() {
        // The launcher left in front must not keep the HUD or the map up.
        assert_eq!(followed_front(Front::App, true, true, OverlayMode::Pinned), Front::Other);
        assert_eq!(followed_front(Front::App, true, false, OverlayMode::Pinned), Front::Other);
        assert_eq!(followed_front(Front::App, true, true, OverlayMode::Open), Front::Other);
        assert_eq!(followed_front(Front::Unknown, true, true, OverlayMode::Pinned), Front::Other);
    }

    #[test]
    fn a_map_opened_over_a_minimized_game_stays() {
        assert_eq!(followed_front(Front::App, true, false, OverlayMode::Open), Front::App);
        assert_eq!(followed_front(Front::Other, true, false, OverlayMode::Open), Front::Other);
    }

    #[test]
    fn a_visible_game_is_followed_as_it_is() {
        assert_eq!(followed_front(Front::Game, false, false, OverlayMode::Pinned), Front::Game);
        assert_eq!(followed_front(Front::App, false, false, OverlayMode::Open), Front::App);
    }

    #[test]
    fn a_game_on_the_monitor_to_the_left_is_outside_the_primary_one() {
        let primary = (0, 0, 3440, 1440);
        assert!(contains(primary, (1720, 720)));
        assert!(!contains(primary, (-960, 540)));
        assert!(!contains(primary, (3440, 720)));
    }
}
