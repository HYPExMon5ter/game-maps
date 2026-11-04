//! Keeps the game well-behaved while the overlay has focus (the full map is open).
//!
//! - **Minimize guard.** Some Unity games (e.g. Sunkenland) minimize their borderless
//!   fullscreen window as soon as it loses focus. Opening the map gives the overlay
//!   focus, so the game would vanish from under the map and Windows would hand focus to
//!   another app. The game's requests to minimize its own windows are skipped while a
//!   Game Maps window is in front; switching to any other app minimizes as usual.
//! - **Input guard.** Unity's legacy `Input` API keeps reading the keyboard and mouse
//!   while the game is in the background, so typing in the map's search box would open
//!   the inventory or walk the player around. Its native functions report "nothing
//!   pressed" (and zero axes) while a Game Maps window is in front.
//!
//! - **Map hotkey.** While the app is connected, the game never sees the map hotkey, so
//!   a game that binds the same key (its own map or GPS) doesn't react and only the
//!   overlay opens. It is removed where every game's input arrives: window messages
//!   (`GetMessage`/`PeekMessage`) and raw input (`GetRawInputData`/`GetRawInputBuffer`),
//!   plus Unity's legacy `Input` functions. Without the app the game sees it as usual.
//!   (The agent detects the hotkey from the physical key state, which isn't filtered.)
//! - **Background input.** While a Game Maps window is in front, raw keyboard and mouse
//!   input still delivered to the game in the background is neutralized too.
//!
//! "In front" means the foreground window belongs to the app connected to the agent's
//! pipe, so neither guard ever affects the game when the app isn't running.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use game_maps_protocol::Hotkey;
use minhook::MinHook;
use unity_runtime::Runtime;
use windows::core::{s, w, BOOL, PCSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Input::{HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, MSG, SHOW_WINDOW_CMD, SW_FORCEMINIMIZE, SW_MINIMIZE, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE,
    WM_KEYDOWN, WM_KEYUP, WM_NULL, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Process id of the connected app (0 while none is connected).
static APP_PID: AtomicU32 = AtomicU32::new(0);
/// Every hooked address, for [`remove`].
static HOOKED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Called by the pipe server when an app connects (its pid) or disconnects (0).
pub fn set_app_pid(pid: u32) {
    APP_PID.store(pid, Ordering::SeqCst);
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Whether the connected app's window (the open map) is in front.
fn app_in_front() -> bool {
    let app = APP_PID.load(Ordering::Relaxed);
    app != 0 && window_pid(unsafe { GetForegroundWindow() }) == app
}

unsafe fn hook(target: *mut c_void, detour: *mut c_void, original: &AtomicUsize) -> Result<(), String> {
    let trampoline = MinHook::create_hook(target, detour).map_err(|e| format!("{e:?}"))?;
    original.store(trampoline as usize, Ordering::SeqCst);
    MinHook::enable_hook(target).map_err(|e| format!("{e:?}"))?;
    HOOKED.lock().unwrap().push(target as usize);
    Ok(())
}

// --- hidden map hotkey ---------------------------------------------------------------

/// Unity `KeyCode`, virtual key and modifiers of the hidden hotkey, packed (see
/// [`pack`]); 0 = none.
static HIDDEN_KEY: AtomicU64 = AtomicU64::new(0);
const HAS_KEY: u64 = 1 << 40;
const VK_SHIFT_BITS: u32 = 48;
const CTRL: u64 = 1 << 32;
const SHIFT: u64 = 1 << 33;
const ALT: u64 = 1 << 34;

fn pack(keycode: u32, hotkey: &Hotkey) -> u64 {
    let mut packed = HAS_KEY | keycode as u64 | (hotkey.vk as u64) << VK_SHIFT_BITS;
    if hotkey.ctrl {
        packed |= CTRL;
    }
    if hotkey.shift {
        packed |= SHIFT;
    }
    if hotkey.alt {
        packed |= ALT;
    }
    packed
}

/// Sets (or clears) the hotkey the game must not see while the app is connected.
pub fn set_hidden_hotkey(hotkey: Option<&Hotkey>) {
    // Keys without a Unity key code are still hidden from messages and raw input.
    let packed = hotkey.map(|h| pack(unity_keycode(h.vk).unwrap_or(u32::MAX), h)).unwrap_or(0);
    HIDDEN_KEY.store(packed, Ordering::SeqCst);
}

/// Unity `KeyCode` for a Windows virtual key, for the keys a hotkey is likely to use.
pub fn unity_keycode(vk: u16) -> Option<u32> {
    let vk = vk as u32;
    Some(match vk {
        0x41..=0x5A => 97 + (vk - 0x41),  // A-Z (KeyCode values are lowercase ASCII)
        0x30..=0x39 => 48 + (vk - 0x30),  // 0-9
        0x60..=0x69 => 256 + (vk - 0x60), // numpad 0-9
        0x70..=0x7E => 282 + (vk - 0x70), // F1-F15
        0x6A => 268,                      // numpad *
        0x6B => 270,                      // numpad +
        0x6D => 269,                      // numpad -
        0x6E => 266,                      // numpad .
        0x6F => 267,                      // numpad /
        0x09 => 9,                        // tab
        0x20 => 32,                       // space
        0x21 => 280,                      // page up
        0x22 => 281,                      // page down
        0x23 => 279,                      // end
        0x24 => 278,                      // home
        0x2D => 277,                      // insert
        0xBA => 59,                       // semicolon
        0xBB => 61,                       // equals
        0xBC => 44,                       // comma
        0xBD => 45,                       // minus
        0xBE => 46,                       // period
        0xBF => 47,                       // slash
        0xC0 => 96,                       // backquote
        0xDB => 91,                       // left bracket
        0xDC => 92,                       // backslash
        0xDD => 93,                       // right bracket
        0xDE => 39,                       // quote
        _ => return None,
    })
}

/// The hidden hotkey, if the app is connected and the hotkey's modifiers are held.
fn active_hidden_key() -> Option<u64> {
    let packed = HIDDEN_KEY.load(Ordering::Relaxed);
    if packed & HAS_KEY == 0 || APP_PID.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let held = |vk: u16| (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0;
    modifiers_match(packed, held).then_some(packed)
}

/// Whether the modifiers held (per `held(vk)`) are exactly the hotkey's.
fn modifiers_match(packed: u64, held: impl Fn(u16) -> bool) -> bool {
    let wants = |flag: u64| (packed & flag) != 0;
    held(VK_CONTROL.0) == wants(CTRL) && held(VK_SHIFT.0) == wants(SHIFT) && held(VK_MENU.0) == wants(ALT)
}

/// Whether Unity `keycode` is the hidden hotkey right now.
fn is_hidden_key(keycode: usize) -> bool {
    let packed = HIDDEN_KEY.load(Ordering::Relaxed);
    packed as u32 as usize == keycode && active_hidden_key().is_some()
}

/// Whether virtual key `vk` is the hidden hotkey right now.
fn is_hidden_vk(vk: u32) -> bool {
    let packed = HIDDEN_KEY.load(Ordering::Relaxed);
    (packed >> VK_SHIFT_BITS) as u32 == vk && active_hidden_key().is_some()
}

// --- window messages and raw input ---------------------------------------------------------

static PEEK_W: AtomicUsize = AtomicUsize::new(0);
static PEEK_A: AtomicUsize = AtomicUsize::new(0);
static GET_W: AtomicUsize = AtomicUsize::new(0);
static GET_A: AtomicUsize = AtomicUsize::new(0);
static RAW_DATA: AtomicUsize = AtomicUsize::new(0);
static RAW_BUFFER: AtomicUsize = AtomicUsize::new(0);

type PeekMessageFn = unsafe extern "system" fn(*mut MSG, HWND, u32, u32, u32) -> BOOL;
type GetMessageFn = unsafe extern "system" fn(*mut MSG, HWND, u32, u32) -> BOOL;
type GetRawInputDataFn = unsafe extern "system" fn(HRAWINPUT, u32, *mut c_void, *mut u32, u32) -> u32;
type GetRawInputBufferFn = unsafe extern "system" fn(*mut RAWINPUT, *mut u32, u32) -> u32;

/// Turns a retrieved key message for the hidden hotkey into `WM_NULL`.
unsafe fn filter_message(msg: *mut MSG) {
    if msg.is_null() {
        return;
    }
    let m = &mut *msg;
    if matches!(m.message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP) && is_hidden_vk(m.wParam.0 as u32) {
        m.message = WM_NULL;
    }
}

unsafe extern "system" fn peek_message_w(msg: *mut MSG, hwnd: HWND, min: u32, max: u32, remove: u32) -> BOOL {
    crate::hook::pump_tick();
    let original: PeekMessageFn = std::mem::transmute(PEEK_W.load(Ordering::Relaxed));
    let r = original(msg, hwnd, min, max, remove);
    if r.as_bool() {
        filter_message(msg);
    }
    r
}

unsafe extern "system" fn peek_message_a(msg: *mut MSG, hwnd: HWND, min: u32, max: u32, remove: u32) -> BOOL {
    crate::hook::pump_tick();
    let original: PeekMessageFn = std::mem::transmute(PEEK_A.load(Ordering::Relaxed));
    let r = original(msg, hwnd, min, max, remove);
    if r.as_bool() {
        filter_message(msg);
    }
    r
}

unsafe extern "system" fn get_message_w(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> BOOL {
    let original: GetMessageFn = std::mem::transmute(GET_W.load(Ordering::Relaxed));
    let r = original(msg, hwnd, min, max);
    if r.0 > 0 {
        filter_message(msg);
    }
    r
}

unsafe extern "system" fn get_message_a(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> BOOL {
    let original: GetMessageFn = std::mem::transmute(GET_A.load(Ordering::Relaxed));
    let r = original(msg, hwnd, min, max);
    if r.0 > 0 {
        filter_message(msg);
    }
    r
}

/// The "no key" virtual key Windows uses for fake key events; input systems ignore it.
const VK_NONE: u16 = 0xFF;

/// Neutralizes one raw input record: the hidden hotkey always, and all keyboard and mouse
/// input while the map is in front.
unsafe fn filter_raw(raw: *mut RAWINPUT, in_front: bool) {
    let r = &mut *raw;
    if r.header.dwType == RIM_TYPEKEYBOARD.0 {
        let kb = &mut r.data.keyboard;
        if in_front || is_hidden_vk(kb.VKey as u32) {
            kb.VKey = VK_NONE;
            kb.MakeCode = 0;
        }
    } else if r.header.dwType == RIM_TYPEMOUSE.0 && in_front {
        let mouse = &mut r.data.mouse;
        mouse.lLastX = 0;
        mouse.lLastY = 0;
        mouse.Anonymous.ulButtons = 0;
    }
}

unsafe extern "system" fn get_raw_input_data(input: HRAWINPUT, command: u32, data: *mut c_void, size: *mut u32, header_size: u32) -> u32 {
    let original: GetRawInputDataFn = std::mem::transmute(RAW_DATA.load(Ordering::Relaxed));
    let r = original(input, command, data, size, header_size);
    if command == RID_INPUT.0 && !data.is_null() && r != u32::MAX && r as usize >= std::mem::size_of::<RAWINPUTHEADER>() {
        filter_raw(data as *mut RAWINPUT, app_in_front());
    }
    r
}

unsafe extern "system" fn get_raw_input_buffer(data: *mut RAWINPUT, size: *mut u32, header_size: u32) -> u32 {
    let original: GetRawInputBufferFn = std::mem::transmute(RAW_BUFFER.load(Ordering::Relaxed));
    let count = original(data, size, header_size);
    if !data.is_null() && count != u32::MAX {
        let in_front = app_in_front();
        let mut record = data as *mut u8;
        for _ in 0..count {
            let raw = record as *mut RAWINPUT;
            filter_raw(raw, in_front);
            // NEXTRAWINPUTBLOCK: records are 8-byte aligned.
            let next = record as usize + (*raw).header.dwSize as usize;
            record = ((next + 7) & !7) as *mut u8;
        }
    }
    count
}

/// Hooks message retrieval and raw input in user32 (every game reads input through these).
pub fn install_input_filters() -> Result<(), String> {
    unsafe {
        let user32 = GetModuleHandleW(w!("user32.dll")).map_err(|e| e.to_string())?;
        let hooks: [(PCSTR, *mut c_void, &AtomicUsize); 6] = [
            (s!("PeekMessageW"), peek_message_w as *mut c_void, &PEEK_W),
            (s!("PeekMessageA"), peek_message_a as *mut c_void, &PEEK_A),
            (s!("GetMessageW"), get_message_w as *mut c_void, &GET_W),
            (s!("GetMessageA"), get_message_a as *mut c_void, &GET_A),
            (s!("GetRawInputData"), get_raw_input_data as *mut c_void, &RAW_DATA),
            (s!("GetRawInputBuffer"), get_raw_input_buffer as *mut c_void, &RAW_BUFFER),
        ];
        for (name, detour, original) in hooks {
            let target = GetProcAddress(user32, name).ok_or_else(|| format!("user32!{} not found", name.display()))?;
            hook(target as *mut c_void, detour, original)?;
        }
    }
    Ok(())
}

// --- minimize guard ------------------------------------------------------------------

static SHOW_WINDOW: AtomicUsize = AtomicUsize::new(0);
type ShowWindowFn = unsafe extern "system" fn(HWND, SHOW_WINDOW_CMD) -> BOOL;

fn is_minimize(cmd: SHOW_WINDOW_CMD) -> bool {
    [SW_MINIMIZE, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE, SW_FORCEMINIMIZE].contains(&cmd)
}

unsafe extern "system" fn show_window(hwnd: HWND, cmd: SHOW_WINDOW_CMD) -> BOOL {
    if is_minimize(cmd) && window_pid(hwnd) == GetCurrentProcessId() && app_in_front() {
        // `ShowWindow` returns whether the window was visible before; it still is.
        return BOOL(1);
    }
    let original: ShowWindowFn = std::mem::transmute(SHOW_WINDOW.load(Ordering::SeqCst));
    original(hwnd, cmd)
}

/// Hooks `user32!ShowWindow` in the game process.
pub fn install_minimize_guard() -> Result<(), String> {
    unsafe {
        let user32 = GetModuleHandleW(w!("user32.dll")).map_err(|e| e.to_string())?;
        let target = GetProcAddress(user32, s!("ShowWindow")).ok_or("user32!ShowWindow not found")?;
        hook(target as *mut c_void, show_window as *mut c_void, &SHOW_WINDOW)
    }
}

// --- input guard ---------------------------------------------------------------------

/// Native signature of a guarded `UnityEngine.Input` function. Each takes at most one
/// argument (a key code, button index or string), so these cover them all.
#[derive(Clone, Copy)]
enum Kind {
    /// `bool f(KeyCode)`: GetKey(Down/Up) by key code; also hides the map hotkey.
    Key,
    /// `bool f(arg)`: GetKey* by name, GetButton*, GetMouseButton*.
    Bool1,
    /// `float f(arg)`: GetAxis, GetAxisRaw.
    Float1,
    /// `bool f()`: anyKey, anyKeyDown.
    Bool0,
}

const INPUT_FUNCTIONS: &[(&str, Kind)] = &[
    ("UnityEngine.Input::GetKeyInt(UnityEngine.KeyCode)", Kind::Key),
    ("UnityEngine.Input::GetKeyDownInt(UnityEngine.KeyCode)", Kind::Key),
    ("UnityEngine.Input::GetKeyUpInt(UnityEngine.KeyCode)", Kind::Key),
    ("UnityEngine.Input::GetKeyString(System.String)", Kind::Bool1),
    ("UnityEngine.Input::GetKeyDownString(System.String)", Kind::Bool1),
    ("UnityEngine.Input::GetKeyUpString(System.String)", Kind::Bool1),
    ("UnityEngine.Input::GetButton(System.String)", Kind::Bool1),
    ("UnityEngine.Input::GetButtonDown(System.String)", Kind::Bool1),
    ("UnityEngine.Input::GetButtonUp(System.String)", Kind::Bool1),
    ("UnityEngine.Input::GetMouseButton(System.Int32)", Kind::Bool1),
    ("UnityEngine.Input::GetMouseButtonDown(System.Int32)", Kind::Bool1),
    ("UnityEngine.Input::GetMouseButtonUp(System.Int32)", Kind::Bool1),
    ("UnityEngine.Input::GetAxis(System.String)", Kind::Float1),
    ("UnityEngine.Input::GetAxisRaw(System.String)", Kind::Float1),
    ("UnityEngine.Input::get_anyKey()", Kind::Bool0),
    ("UnityEngine.Input::get_anyKeyDown()", Kind::Bool0),
];
const INPUT_SLOTS: usize = 16;
static INPUT_ORIGINALS: [AtomicUsize; INPUT_SLOTS] = [const { AtomicUsize::new(0) }; INPUT_SLOTS];

macro_rules! input_detours {
    ($($slot:literal => $key:ident, $b1:ident, $f1:ident, $b0:ident;)*) => {
        $(
            unsafe extern "C" fn $key(keycode: usize) -> bool {
                if app_in_front() || is_hidden_key(keycode) {
                    return false;
                }
                let original: unsafe extern "C" fn(usize) -> bool = std::mem::transmute(INPUT_ORIGINALS[$slot].load(Ordering::Relaxed));
                original(keycode)
            }
            unsafe extern "C" fn $b1(arg: usize) -> bool {
                if app_in_front() {
                    return false;
                }
                let original: unsafe extern "C" fn(usize) -> bool = std::mem::transmute(INPUT_ORIGINALS[$slot].load(Ordering::Relaxed));
                original(arg)
            }
            unsafe extern "C" fn $f1(arg: usize) -> f32 {
                if app_in_front() {
                    return 0.0;
                }
                let original: unsafe extern "C" fn(usize) -> f32 = std::mem::transmute(INPUT_ORIGINALS[$slot].load(Ordering::Relaxed));
                original(arg)
            }
            unsafe extern "C" fn $b0() -> bool {
                if app_in_front() {
                    return false;
                }
                let original: unsafe extern "C" fn() -> bool = std::mem::transmute(INPUT_ORIGINALS[$slot].load(Ordering::Relaxed));
                original()
            }
        )*
        fn input_detour(slot: usize, kind: Kind) -> *mut c_void {
            match (slot, kind) {
                $(
                    ($slot, Kind::Key) => $key as *mut c_void,
                    ($slot, Kind::Bool1) => $b1 as *mut c_void,
                    ($slot, Kind::Float1) => $f1 as *mut c_void,
                    ($slot, Kind::Bool0) => $b0 as *mut c_void,
                )*
                _ => unreachable!("input slot out of range"),
            }
        }
    };
}

input_detours! {
    0 => key_0, b1_0, f1_0, b0_0;
    1 => key_1, b1_1, f1_1, b0_1;
    2 => key_2, b1_2, f1_2, b0_2;
    3 => key_3, b1_3, f1_3, b0_3;
    4 => key_4, b1_4, f1_4, b0_4;
    5 => key_5, b1_5, f1_5, b0_5;
    6 => key_6, b1_6, f1_6, b0_6;
    7 => key_7, b1_7, f1_7, b0_7;
    8 => key_8, b1_8, f1_8, b0_8;
    9 => key_9, b1_9, f1_9, b0_9;
    10 => key_10, b1_10, f1_10, b0_10;
    11 => key_11, b1_11, f1_11, b0_11;
    12 => key_12, b1_12, f1_12, b0_12;
    13 => key_13, b1_13, f1_13, b0_13;
    14 => key_14, b1_14, f1_14, b0_14;
    15 => key_15, b1_15, f1_15, b0_15;
}

/// Hooks the native functions behind `UnityEngine.Input`. Returns how many were
/// guarded; games that don't use the legacy input module simply have fewer.
pub fn install_input_guard(rt: &Runtime) -> usize {
    let mut done: Vec<usize> = Vec::new();
    for (slot, &(name, kind)) in INPUT_FUNCTIONS.iter().enumerate() {
        let Ok(code) = (unsafe { rt.icall::<*const c_void>(name) }) else { continue };
        let code = code as usize;
        // Identical bodies are shared (e.g. two getters folded together); hook once.
        if code == 0 || done.contains(&code) {
            continue;
        }
        if unsafe { hook(code as *mut c_void, input_detour(slot, kind), &INPUT_ORIGINALS[slot]) }.is_ok() {
            done.push(code);
        }
    }
    done.len()
}

/// Removes every guard hook (before the agent unloads).
pub fn remove() {
    for target in HOOKED.lock().unwrap().drain(..) {
        unsafe {
            let _ = MinHook::disable_hook(target as *mut c_void);
            let _ = MinHook::remove_hook(target as *mut c_void);
        }
    }
}

#[cfg(test)]
mod tests {
    use game_maps_protocol::HotkeyAction;

    use super::*;

    #[test]
    fn maps_virtual_keys_to_unity_keycodes() {
        assert_eq!(unity_keycode(0x4D), Some(109)); // M -> KeyCode.M
        assert_eq!(unity_keycode(0x41), Some(97));
        assert_eq!(unity_keycode(0x31), Some(49));
        assert_eq!(unity_keycode(0x70), Some(282)); // F1
        assert_eq!(unity_keycode(0x6B), Some(270)); // numpad +
        assert_eq!(unity_keycode(0x01), None); // mouse button
    }

    fn m_key(ctrl: bool) -> Hotkey {
        Hotkey { action: HotkeyAction::ToggleMap, vk: 0x4D, ctrl, shift: false, alt: false }
    }

    #[test]
    fn packs_key_code_and_virtual_key() {
        let packed = pack(unity_keycode(0x4D).unwrap(), &m_key(false));
        assert_eq!(packed as u32, 109);
        assert_eq!((packed >> VK_SHIFT_BITS) as u32, 0x4D);
        assert!(packed & HAS_KEY != 0);
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let plain = pack(109, &m_key(false));
        let with_ctrl = pack(109, &m_key(true));
        let none = |_: u16| false;
        let ctrl = |vk: u16| vk == VK_CONTROL.0;
        assert!(modifiers_match(plain, none));
        assert!(!modifiers_match(plain, ctrl), "Ctrl+M is not M");
        assert!(modifiers_match(with_ctrl, ctrl));
        assert!(!modifiers_match(with_ctrl, none));
    }
}
