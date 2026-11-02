//! Per-frame main-thread callback via detours on engine/game methods that Unity calls
//! every frame. Running on the main thread is what makes Unity API calls safe.
//!
//! Several methods are hooked because none runs in every situation: UI event systems
//! may be disabled in gameplay, render-loop entry points only exist in scriptable
//! render pipelines, and game-specific `Update`s only run in the world. The agent's
//! tick dedupes by frame number, so extra hooks cost one cheap check each.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use il2cpp::Runtime;
use minhook::MinHook;

use crate::SHARED;

/// Native signature shape of a hooked method (IL2CPP appends a `MethodInfo*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Instance method without parameters: `(this, method)`.
    Instance0,
    /// Static method with three pointer-sized parameters: `(a, b, c, method)`.
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

type Instance0Fn = unsafe extern "C" fn(*mut c_void, *const c_void);
type Static3Fn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const c_void);

const SLOTS: usize = 6;
static TARGETS: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static ORIGINALS: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];

#[inline(always)]
fn enter() {
    if let Some(s) = SHARED.get() {
        s.in_tick.fetch_add(1, Ordering::SeqCst);
        if catch_unwind(AssertUnwindSafe(|| s.tick())).is_err() {
            // A poisoned main-state lock disables further ticks; the game keeps running.
            s.log(game_maps_protocol::LogLevel::Error, "agent tick panicked; live features disabled");
        }
    }
}

#[inline(always)]
fn leave() {
    if let Some(s) = SHARED.get() {
        s.in_tick.fetch_sub(1, Ordering::SeqCst);
    }
}

macro_rules! detours {
    ($($slot:literal => $inst:ident, $stat:ident;)*) => {
        $(
            unsafe extern "C" fn $inst(this: *mut c_void, method: *const c_void) {
                enter();
                let original: Instance0Fn = std::mem::transmute(ORIGINALS[$slot].load(Ordering::SeqCst));
                original(this, method);
                leave();
            }
            unsafe extern "C" fn $stat(a: *mut c_void, b: *mut c_void, c: *mut c_void, method: *const c_void) {
                enter();
                let original: Static3Fn = std::mem::transmute(ORIGINALS[$slot].load(Ordering::SeqCst));
                original(a, b, c, method);
                leave();
            }
        )*
        fn detour_for(slot: usize, shape: Shape) -> *mut c_void {
            match (slot, shape) {
                $(
                    ($slot, Shape::Instance0) => $inst as *mut c_void,
                    ($slot, Shape::Static3) => $stat as *mut c_void,
                )*
                _ => unreachable!("slot out of range"),
            }
        }
    };
}

detours! {
    0 => inst0, stat0;
    1 => inst1, stat1;
    2 => inst2, stat2;
    3 => inst3, stat3;
    4 => inst4, stat4;
    5 => inst5, stat5;
}

fn param_count(shape: Shape) -> i32 {
    match shape {
        Shape::Instance0 => 0,
        Shape::Static3 => 3,
    }
}

/// Installs every resolvable target. Returns the installed method names; errors for
/// targets that couldn't be hooked are returned alongside.
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
        unsafe {
            match MinHook::create_hook(code as *mut c_void, detour_for(slot, t.shape)) {
                Ok(original) => {
                    ORIGINALS[slot].store(original as usize, Ordering::SeqCst);
                    if let Err(e) = MinHook::enable_hook(code as *mut c_void) {
                        let _ = MinHook::remove_hook(code as *mut c_void);
                        errors.push(format!("{label}: enable failed {e:?}"));
                        continue;
                    }
                    TARGETS[slot].store(code, Ordering::SeqCst);
                    slot += 1;
                    installed.push(label);
                }
                Err(e) => errors.push(format!("{label}: hook failed {e:?}")),
            }
        }
    }
    (installed, errors)
}

type Static0Fn = unsafe extern "C" fn(*const c_void);
static QUIT_TARGET: AtomicUsize = AtomicUsize::new(0);
static QUIT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Unity calls this once when the application is quitting (menu quit, Alt+F4, window
/// close). From then on the agent does no more work on the main thread, so it never
/// touches objects or runtime state that are being torn down.
unsafe extern "C" fn on_quit(method: *const c_void) {
    if let Some(s) = SHARED.get() {
        s.stopping.store(true, Ordering::SeqCst);
    }
    let original: Static0Fn = std::mem::transmute(QUIT_ORIGINAL.load(Ordering::SeqCst));
    original(method);
}

/// Hooks `UnityEngine.Application.Internal_ApplicationQuit` (see [`on_quit`]).
pub fn install_quit_hook(rt: &Runtime) -> Result<(), String> {
    let m = rt
        .class("UnityEngine", "Application")
        .and_then(|c| rt.method(c, "Internal_ApplicationQuit", 0))
        .map_err(|e| e.to_string())?;
    let code = rt.method_pointer(&m) as usize;
    if code == 0 || !m.is_static {
        return Err("Application.Internal_ApplicationQuit has no static code".into());
    }
    unsafe {
        let original = MinHook::create_hook(code as *mut c_void, on_quit as *mut c_void).map_err(|e| format!("{e:?}"))?;
        QUIT_ORIGINAL.store(original as usize, Ordering::SeqCst);
        MinHook::enable_hook(code as *mut c_void).map_err(|e| format!("{e:?}"))?;
    }
    QUIT_TARGET.store(code, Ordering::SeqCst);
    Ok(())
}

/// Removes all hooks and waits until no thread is inside a detour, so the module can
/// be unloaded safely.
pub fn remove() {
    let mut any = false;
    for t in TARGETS.iter().chain([&QUIT_TARGET]) {
        let code = t.swap(0, Ordering::SeqCst) as *mut c_void;
        if !code.is_null() {
            any = true;
            unsafe {
                let _ = MinHook::disable_hook(code);
                let _ = MinHook::remove_hook(code);
            }
        }
    }
    if !any {
        return;
    }
    if let Some(s) = SHARED.get() {
        let deadline = Instant::now() + Duration::from_secs(2);
        while s.in_tick.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // Let any thread that was between a patched jump and our counter finish.
    std::thread::sleep(Duration::from_millis(50));
    MinHook::uninitialize();
}
