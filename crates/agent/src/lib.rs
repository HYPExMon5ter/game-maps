//! Game Maps agent: loaded into a game process by the overlay app.
//!
//! Lifecycle: `DllMain` starts a worker thread → wait for the game's engine (a Unity
//! scripting runtime, or Unreal's object tables) → pick a game provider → hook a
//! per-frame point on the game's main thread → serve the app over a named pipe → on
//! `Detach`, unhook and unload itself. The game folder is never modified.

mod agent;
mod diff;
mod engine;
mod filelog;
mod focus;
mod hook;
mod hotkeys;
mod pipe;
mod provider;
mod unreal;
mod walk;
mod worldmap;

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use game_maps_protocol::{AgentMessage, LogLevel};
use tokio::sync::mpsc::Sender;
use unity_runtime::{Runtime, Unity};
use unreal_runtime::Unreal;
use windows::core::{BOOL, HSTRING};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HMODULE, HWND, LPARAM};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread, GetModuleFileNameW};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowThreadProcessId};

use agent::Shared;
use engine::UnityEngine;
use unreal::UnrealEngine;

pub(crate) static SHARED: OnceLock<Shared> = OnceLock::new();
static MODULE: AtomicUsize = AtomicUsize::new(0);

/// File name of this DLL (the app stages agents under content-hashed names).
pub(crate) fn module_file_name() -> Option<String> {
    let module = HMODULE(MODULE.load(Ordering::Relaxed) as *mut c_void);
    let mut buf = [0u16; 1024];
    let len = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
    if len == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len]);
    path.rsplit(['\\', '/']).next().map(str::to_owned)
}

/// How long to wait for the game's scripting runtime when injected very early.
/// Long enough for a frame, even at a low frame rate.
const LAST_TICK: Duration = Duration::from_millis(250);
const RUNTIME_WAIT: Duration = Duration::from_secs(300);

#[no_mangle]
extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            let _ = DisableThreadLibraryCalls(module);
        }
        let module = module.0 as usize;
        MODULE.store(module, Ordering::Relaxed);
        // Never do real work under the loader lock.
        std::thread::spawn(move || {
            run();
            if focus::thread_in_get_message() {
                filelog::write("a thread is waiting in GetMessage; staying loaded");
                return;
            }
            unload(HMODULE(module as *mut c_void));
        });
    }
    1
}

fn run() {
    // One agent per process: a second injected copy exits immediately.
    let mutex_name = HSTRING::from(format!(r"Local\game-maps-agent-{}", std::process::id()));
    let _mutex = match unsafe { CreateMutexW(None, false, &mutex_name) } {
        Ok(h) if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS => HandleGuard(h),
        Ok(h) => {
            drop(HandleGuard(h));
            filelog::write("another agent is already loaded; exiting");
            return;
        }
        Err(e) => {
            filelog::write(&format!("CreateMutexW failed: {e}"));
            return;
        }
    };
    filelog::write(concat!("agent ", env!("CARGO_PKG_VERSION"), " starting"));

    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let shared = match wait_for_engine() {
        Some(Found::Unity(rt)) => start_unity(rt, tx),
        Some(Found::Unreal(ue)) => Some(start_unreal(*ue, tx)),
        None => None,
    };
    let Some(shared) = shared else { return };
    if let Err(e) = focus::install_minimize_guard() {
        shared.log(LogLevel::Warn, format!("minimize guard unavailable: {e}"));
    }
    if let Err(e) = focus::install_input_filters() {
        shared.log(LogLevel::Warn, format!("input filters unavailable: {e}"));
    }

    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime.block_on(pipe::serve(shared, rx)),
        Err(e) => filelog::write(&format!("tokio runtime failed: {e}")),
    }
    filelog::write("detaching");
    worldmap::wait_for_saves();
    // A tick to undo what was only for the app, before the hooks go.
    std::thread::sleep(LAST_TICK);
    shared.stopping.store(true, std::sync::atomic::Ordering::SeqCst);
    hook::remove();
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

enum Found {
    Unity(Runtime),
    Unreal(Box<Unreal>),
}

/// Waits for the game's engine to be ready to use.
fn wait_for_engine() -> Option<Found> {
    let deadline = Instant::now() + RUNTIME_WAIT;
    let mut waiting_for = "";
    loop {
        match Runtime::load() {
            Ok(rt) => {
                filelog::write(&format!("{} runtime found", rt.kind()));
                return Some(Found::Unity(rt));
            }
            Err(unity_runtime::Error::DomainNotReady) => {}
            Err(unity_runtime::Error::NoRuntime) if has_window_of_class("UnrealWindow") => match Unreal::load() {
                Ok(ue) => {
                    filelog::write(&format!("Unreal engine found: {} objects, layout {:?}", ue.object_count(), ue.layout()));
                    return Some(Found::Unreal(Box::new(ue)));
                }
                Err(unreal_runtime::Error::NoEngine(what)) => {
                    if what != waiting_for {
                        waiting_for = what;
                        filelog::write(&format!("waiting for the Unreal engine: {what} not found yet"));
                    }
                }
                Err(e) => {
                    filelog::write(&format!("Unreal engine not usable: {e}"));
                    return None;
                }
            },
            Err(unity_runtime::Error::NoRuntime) => {}
            Err(e) => {
                filelog::write(&format!("Unity runtime not usable: {e}"));
                return None;
            }
        }
        if Instant::now() >= deadline {
            filelog::write("no supported engine found; exiting");
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Resolves members and installs the frame hooks of a Unity game.
fn start_unity(rt: Runtime, tx: Sender<AgentMessage>) -> Option<&'static Shared> {
    // Attached only while resolving members and installing hooks (Mono's class loading
    // and JIT need an attached thread). This thread then just serves the pipe and never
    // touches managed objects; staying attached could make the runtime wait for it when
    // the game shuts down. Hooks firing before the agent is set up do nothing.
    let thread = rt.attach_thread();
    let unity = match Unity::new(&rt) {
        Ok(u) => u,
        Err(e) => {
            filelog::write(&format!("not a supported Unity runtime: {e}"));
            return None;
        }
    };
    let provider = provider::detect(&rt);
    let mut targets = hook::ENGINE_TARGETS.to_vec();
    if let Some(p) = &provider {
        targets.extend_from_slice(p.frame_hooks());
    }
    let (installed, errors) = hook::install(&rt, &targets);
    let input_guarded = focus::install_input_guard(&rt);
    let quit_hook = hook::install_quit_hook(&rt);
    let navigation = unity.has_navigation();
    drop(thread);
    let shared = SHARED.get_or_init(|| Shared::new(UnityEngine { rt, unity }, provider, tx));
    for e in &errors {
        filelog::write(&format!("frame hook skipped: {e}"));
        shared.log(LogLevel::Debug, format!("frame hook skipped: {e}"));
    }
    if let Err(e) = quit_hook {
        shared.log(LogLevel::Warn, format!("quit hook unavailable: {e}"));
    }
    shared.log(LogLevel::Info, format!("input guard: {input_guarded} Input functions"));
    shared.log(LogLevel::Info, format!("navigation (routes): {navigation}"));
    if installed.is_empty() {
        hook::enable_pump_ticks("UnityWndClass");
        filelog::write("no frame hook could be installed; ticking from the message pump");
        shared.log(LogLevel::Warn, "no frame hook could be installed; ticking from the message pump");
    } else {
        shared.log(LogLevel::Info, format!("frame hooks: {}", installed.join(", ")));
    }
    Some(shared)
}

/// Sets up an Unreal game: it ticks from the message pump (see `hook`).
fn start_unreal(ue: Unreal, tx: Sender<AgentMessage>) -> &'static Shared {
    #[cfg(debug_assertions)]
    unreal::dump_types(&ue);
    let engine = UnrealEngine::new(ue);
    let provider = provider::detect_unreal(&engine);
    let shared = SHARED.get_or_init(|| Shared::new(engine, provider, tx));
    hook::enable_pump_ticks("UnrealWindow");
    shared.log(LogLevel::Info, "Unreal engine: ticking from the message pump");
    shared
}

unsafe extern "system" fn window_of_class(hwnd: HWND, data: LPARAM) -> BOOL {
    let (class, found) = &mut *(data.0 as *mut (&str, bool));
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let mut name = [0u16; 64];
    let len = GetClassNameW(hwnd, &mut name).max(0) as usize;
    if pid == GetCurrentProcessId() && String::from_utf16_lossy(&name[..len]) == **class {
        *found = true;
        return BOOL(0);
    }
    BOOL(1)
}

/// Whether this process has a window of class `class`.
fn has_window_of_class(class: &str) -> bool {
    let mut data = (class, false);
    unsafe {
        let _ = EnumWindows(Some(window_of_class), LPARAM(&mut data as *mut _ as isize));
    }
    data.1
}

fn unload(module: HMODULE) -> ! {
    unsafe { FreeLibraryAndExitThread(module, 0) }
}
