//! Per-frame main-thread callback via detours on engine/game methods that Unity calls
//! every frame. Running on the main thread is what makes Unity API calls safe.
//!
//! Several methods are hooked because none runs in every situation: UI event systems
//! may be disabled in gameplay, render-loop entry points only exist in scriptable
//! render pipelines, and game-specific `Update`s only run in the world. The agent's
//! tick dedupes by frame number, so extra hooks cost one cheap check each.
//!
//! Each detour is a small assembly thunk: it saves the argument registers, runs the
//! agent's tick, restores them and *jumps* into the original method. The agent's code
//! is therefore never on the stack while game code runs, so a managed exception thrown
//! by the hooked method unwinds exactly as it would without the agent (neither Mono's
//! unwinder nor IL2CPP's C++ exceptions can cross an unknown native frame safely).
//! Passing every register through also makes the thunk independent of the method's
//! signature and of backend conventions (IL2CPP's trailing `MethodInfo*`, Mono's
//! hidden arguments in r10/r11).
//!
//! On Mono the hooked code is JIT-compiled, and some game launches leave no free memory
//! near it for a detour. Then the tick runs from the message pump instead (see
//! [`enable_pump_ticks`]): Unity's main thread polls `PeekMessage` every frame, and that
//! hook lives in user32, where there's always room.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use minhook::MinHook;
use unity_runtime::Runtime;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible};

use crate::SHARED;

/// Kind of hooked method, used to find the right overload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Instance method without parameters.
    Instance0,
    /// Static method with three parameters.
    Static3,
}

#[derive(Clone, Copy, Debug)]
pub struct HookTarget {
    pub namespace: &'static str,
    pub class: &'static str,
    pub method: &'static str,
    pub shape: Shape,
}

pub const fn target(namespace: &'static str, class: &'static str, method: &'static str, shape: Shape) -> HookTarget {
    HookTarget { namespace, class, method, shape }
}

/// Engine-level candidates useful for any Unity game.
pub const ENGINE_TARGETS: &[HookTarget] = &[
    // Scriptable render pipelines (URP/HDRP): called every rendered frame.
    target("UnityEngine.Rendering", "RenderPipelineManager", "DoRenderLoop_Internal", Shape::Static3),
    // uGUI: runs while an EventSystem is active (menus, often gameplay too).
    target("UnityEngine.EventSystems", "EventSystem", "Update", Shape::Instance0),
];

/// Frame hook slots, plus one for the quit hook.
const SLOTS: usize = 6;
const QUIT_SLOT: usize = SLOTS;
/// Hooked code addresses, and the trampolines to the original code the thunks jump to.
static TARGETS: [AtomicUsize; SLOTS + 1] = [const { AtomicUsize::new(0) }; SLOTS + 1];
static GAME_MAPS_HOOK_ORIGINALS: [AtomicUsize; SLOTS + 1] = [const { AtomicUsize::new(0) }; SLOTS + 1];

/// Called by every thunk, with all argument registers saved, before the original runs.
extern "C" fn thunk_entry(slot: usize) {
    #[cfg(test)]
    tests::ENTRIES.fetch_add(1, Ordering::SeqCst);
    let Some(s) = SHARED.get() else { return };
    if slot == QUIT_SLOT {
        // Unity is quitting (menu quit, Alt+F4, window close): from now on the agent
        // does no more work on the main thread, so it never touches objects or runtime
        // state that are being torn down.
        s.stopping.store(true, Ordering::SeqCst);
        return;
    }
    run_tick(s);
}

fn run_tick(s: &crate::agent::Shared) {
    s.in_tick.fetch_add(1, Ordering::SeqCst);
    if catch_unwind(AssertUnwindSafe(|| s.tick())).is_err() {
        // The game keeps running either way. Ticks go on, unless they keep failing.
        let panics = TICK_PANICS.fetch_add(1, Ordering::SeqCst) + 1;
        crate::filelog::write(&format!("agent tick panicked ({panics})"));
        if panics >= MAX_TICK_PANICS {
            s.log(game_maps_protocol::LogLevel::Error, "agent tick keeps panicking; live features disabled");
            s.stopping.store(true, Ordering::SeqCst);
        } else {
            s.log(game_maps_protocol::LogLevel::Error, "agent tick panicked; continuing");
        }
    }
    s.in_tick.fetch_sub(1, Ordering::SeqCst);
}

// Windows x64: on entry rsp = 8 (mod 16). Six pushes (48 bytes) plus 0x68 keep the call
// below 16-byte aligned with 32 bytes of shadow space; xmm0-3 (float arguments) are
// saved above the shadow space. r10/r11 carry Mono's hidden arguments in some calls.
macro_rules! thunks {
    ($($slot:literal => $name:ident),* $(,)?) => {
        $(
            std::arch::global_asm!(
                concat!(".globl ", stringify!($name)),
                concat!(stringify!($name), ":"),
                "push rcx",
                "push rdx",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "sub rsp, 0x68",
                "movdqu [rsp + 0x20], xmm0",
                "movdqu [rsp + 0x30], xmm1",
                "movdqu [rsp + 0x40], xmm2",
                "movdqu [rsp + 0x50], xmm3",
                concat!("mov ecx, ", stringify!($slot)),
                "call {entry}",
                "movdqu xmm0, [rsp + 0x20]",
                "movdqu xmm1, [rsp + 0x30]",
                "movdqu xmm2, [rsp + 0x40]",
                "movdqu xmm3, [rsp + 0x50]",
                "add rsp, 0x68",
                "pop r11",
                "pop r10",
                "pop r9",
                "pop r8",
                "pop rdx",
                "pop rcx",
                concat!("jmp qword ptr [rip + {originals} + ", stringify!($slot), " * 8]"),
                entry = sym thunk_entry,
                originals = sym GAME_MAPS_HOOK_ORIGINALS,
            );
        )*
        extern "C" {
            $( fn $name(); )*
        }
        fn thunk_for(slot: usize) -> *mut std::ffi::c_void {
            match slot {
                $( $slot => $name as *mut std::ffi::c_void, )*
                _ => unreachable!("slot out of range"),
            }
        }
    };
}

thunks! {
    0 => game_maps_hook_thunk_0,
    1 => game_maps_hook_thunk_1,
    2 => game_maps_hook_thunk_2,
    3 => game_maps_hook_thunk_3,
    4 => game_maps_hook_thunk_4,
    5 => game_maps_hook_thunk_5,
    6 => game_maps_hook_thunk_quit,
}

fn param_count(shape: Shape) -> i32 {
    match shape {
        Shape::Instance0 => 0,
        Shape::Static3 => 3,
    }
}

/// Detours `code` to the thunk of `slot`.
unsafe fn hook(slot: usize, code: usize) -> Result<(), String> {
    let target = code as *mut std::ffi::c_void;
    let original = MinHook::create_hook(target, thunk_for(slot)).map_err(|e| format!("hook failed {e:?}"))?;
    // The thunk may run as soon as the hook is enabled, so publish the trampoline first.
    GAME_MAPS_HOOK_ORIGINALS[slot].store(original as usize, Ordering::SeqCst);
    if let Err(e) = MinHook::enable_hook(target) {
        let _ = MinHook::remove_hook(target);
        return Err(format!("enable failed {e:?}"));
    }
    TARGETS[slot].store(code, Ordering::SeqCst);
    Ok(())
}

/// Installs every resolvable target. Returns the installed method names; errors for
/// targets that couldn't be hooked are returned alongside. On Mono, resolving native
/// code JIT-compiles methods, so call this on an attached thread.
pub fn install(rt: &Runtime, targets: &[HookTarget]) -> (Vec<String>, Vec<String>) {
    let mut installed = Vec::new();
    let mut errors = Vec::new();
    let mut slot = 0;
    for t in targets {
        let label = format!("{}.{}::{}", t.namespace, t.class, t.method).trim_start_matches('.').to_owned();
        if slot >= SLOTS {
            errors.push(format!("{label}: no free hook slot"));
            continue;
        }
        let m = match rt.class(t.namespace, t.class).and_then(|c| rt.method(c, t.method, param_count(t.shape))) {
            Ok(m) => m,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        if m.is_static != (t.shape == Shape::Static3) {
            errors.push(format!("{label}: unexpected static/instance kind"));
            continue;
        }
        let code = rt.method_pointer(&m) as usize;
        if code == 0 {
            errors.push(format!("{label}: no code"));
            continue;
        }
        // Identical code folding can make several methods share one body; hook it once.
        if TARGETS.iter().any(|t| t.load(Ordering::SeqCst) == code) {
            installed.push(format!("{label} (shared code)"));
            continue;
        }
        match unsafe { hook(slot, code) } {
            Ok(()) => {
                slot += 1;
                installed.push(label);
            }
            Err(e) => errors.push(format!("{label}: {e}")),
        }
    }
    (installed, errors)
}

/// Hooks `UnityEngine.Application.Internal_ApplicationQuit`, which Unity calls once
/// when the application is quitting (see [`thunk_entry`]).
pub fn install_quit_hook(rt: &Runtime) -> Result<(), String> {
    let m = rt
        .class("UnityEngine", "Application")
        .and_then(|c| rt.method(c, "Internal_ApplicationQuit", 0))
        .map_err(|e| e.to_string())?;
    let code = rt.method_pointer(&m) as usize;
    if code == 0 || !m.is_static {
        return Err("Application.Internal_ApplicationQuit has no static code".into());
    }
    unsafe { hook(QUIT_SLOT, code) }
}

static TICK_PANICS: AtomicUsize = AtomicUsize::new(0);
/// Give up on live features after this many panics, rather than risk the game.
const MAX_TICK_PANICS: usize = 10;

/// Ticks from the message pump are on; see the module docs.
static PUMP_TICKS: AtomicBool = AtomicBool::new(false);
/// The game's main thread (the one with its window), once found.
static PUMP_THREAD: AtomicU32 = AtomicU32::new(0);
static PUMP_LAST_MS: AtomicU64 = AtomicU64::new(0);
/// Shortest time between pump ticks: the pump is polled several times a frame.
const PUMP_INTERVAL_MS: u64 = 8;
/// How often the game's window is looked for until it's found.
const PUMP_SEARCH_MS: u64 = 500;

/// Runs the tick from the message pump, for when no frame hook could be installed.
pub fn enable_pump_ticks() {
    PUMP_TICKS.store(true, Ordering::SeqCst);
}

unsafe extern "system" fn find_game_thread(hwnd: HWND, found: LPARAM) -> BOOL {
    let mut pid = 0u32;
    let thread = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let mut class = [0u16; 32];
    let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
    if pid == GetCurrentProcessId() && IsWindowVisible(hwnd).as_bool() && String::from_utf16_lossy(&class[..len]) == "UnityWndClass" {
        *(found.0 as *mut u32) = thread;
        return BOOL(0);
    }
    BOOL(1)
}

/// Called on every `PeekMessage`, from any thread.
pub fn pump_tick() {
    if !PUMP_TICKS.load(Ordering::Relaxed) {
        return;
    }
    static START: OnceLock<Instant> = OnceLock::new();
    let now = START.get_or_init(Instant::now).elapsed().as_millis() as u64;
    let last = PUMP_LAST_MS.load(Ordering::Relaxed);
    let thread = PUMP_THREAD.load(Ordering::Relaxed);
    if thread == 0 {
        if now.saturating_sub(last) >= PUMP_SEARCH_MS {
            PUMP_LAST_MS.store(now, Ordering::Relaxed);
            let mut found = 0u32;
            unsafe {
                let _ = EnumWindows(Some(find_game_thread), LPARAM(&mut found as *mut u32 as isize));
            }
            PUMP_THREAD.store(found, Ordering::Relaxed);
            if found != 0 {
                crate::filelog::write(&format!("ticking on thread {found}; polled from thread {}", unsafe { GetCurrentThreadId() }));
            }
        }
        return;
    }
    if unsafe { GetCurrentThreadId() } != thread || now.saturating_sub(last) < PUMP_INTERVAL_MS {
        return;
    }
    PUMP_LAST_MS.store(now, Ordering::Relaxed);
    if let Some(s) = SHARED.get() {
        run_tick(s);
    }
}

/// Stops all hooks, waits until no thread is inside the agent's tick, then frees them, so
/// the module can be unloaded safely. In that order: a tick still running returns through
/// its trampoline, which freeing the hook would pull out from under it.
pub fn remove() {
    PUMP_TICKS.store(false, Ordering::SeqCst);
    crate::focus::disable();
    for t in TARGETS.iter() {
        let code = t.swap(0, Ordering::SeqCst) as *mut std::ffi::c_void;
        if !code.is_null() {
            unsafe {
                let _ = MinHook::disable_hook(code);
            }
        }
    }
    if let Some(s) = SHARED.get() {
        let deadline = Instant::now() + Duration::from_secs(2);
        while s.in_tick.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // Let any thread still in a thunk (between the tick and its jump) or between a
    // patched jump and the thunk finish.
    std::thread::sleep(Duration::from_millis(50));
    // Frees every hook and trampoline.
    MinHook::uninitialize();
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) static ENTRIES: AtomicUsize = AtomicUsize::new(0);

    /// Integer and float register arguments plus stack arguments, mixed so that each
    /// of rcx/rdx/r8/r9 and xmm0-3 carries a value.
    #[inline(never)]
    extern "C" fn sample(a: u64, b: f64, c: u64, d: f32, e: u64, f: f64) -> f64 {
        a as f64 * 1000.0 + b * 100.0 + c as f64 * 10.0 + d as f64 + e as f64 * 0.1 + f * 0.01
    }

    #[test]
    fn thunk_runs_entry_and_preserves_arguments() {
        let f: extern "C" fn(u64, f64, u64, f32, u64, f64) -> f64 = std::hint::black_box(sample);
        let expected = f(1, 2.0, 3, 4.0, 5, 6.0);
        unsafe { hook(SLOTS - 1, f as usize).expect("hook") };
        let before = ENTRIES.load(Ordering::SeqCst);
        let got = f(1, 2.0, 3, 4.0, 5, 6.0);
        assert_eq!(ENTRIES.load(Ordering::SeqCst), before + 1);
        assert_eq!(got, expected);
        unsafe {
            let _ = MinHook::disable_hook(f as *mut std::ffi::c_void);
        }
    }
}
