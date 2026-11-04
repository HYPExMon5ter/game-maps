//! Game Maps agent: loaded into a game process by the overlay app.
//!
//! Lifecycle: `DllMain` starts a worker thread → wait for the IL2CPP runtime → pick a
//! game provider → hook one per-frame engine method → serve the app over a named pipe
//! → on `Detach`, unhook and unload itself. The game folder is never modified.

mod agent;
mod diff;
mod filelog;
mod focus;
mod hook;
mod hotkeys;
mod pipe;
mod provider;
mod walk;

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use game_maps_protocol::LogLevel;
use unity_runtime::{Runtime, Unity};
use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HMODULE};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread, GetModuleFileNameW};
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::System::Threading::CreateMutexW;

use agent::Shared;

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

    let Some(rt) = wait_for_runtime() else {
        filelog::write("no Unity scripting runtime found; exiting");
        return;
    };
    filelog::write(&format!("{} runtime found", rt.kind()));
    let (tx, rx) = tokio::sync::mpsc::channel(256);

    // Attached only while resolving members and installing hooks (Mono's class loading
    // and JIT need an attached thread). This thread then just serves the pipe and never
    // touches managed objects; staying attached could make the runtime wait for it when
    // the game shuts down.
    let (shared, installed, errors, quit_hook, input_guarded) = {
        let thread = rt.attach_thread();
        let resolved = Unity::new(&rt).map(|unity| (unity, provider::detect(&rt)));
        drop(thread);
        let (unity, provider) = match resolved {
            Ok(r) => r,
            Err(e) => {
                filelog::write(&format!("not a supported Unity runtime: {e}"));
                return;
            }
        };
        let mut targets = hook::ENGINE_TARGETS.to_vec();
        if let Some(p) = &provider {
            targets.extend_from_slice(p.frame_hooks());
        }
        let shared = SHARED.get_or_init(|| Shared::new(rt, unity, provider, tx));
        let _thread = shared.rt.attach_thread();
        let (installed, errors) = hook::install(&shared.rt, &targets);
        let input_guarded = focus::install_input_guard(&shared.rt);
        (shared, installed, errors, hook::install_quit_hook(&shared.rt), input_guarded)
    };
    for e in &errors {
        filelog::write(&format!("frame hook skipped: {e}"));
        shared.log(LogLevel::Debug, format!("frame hook skipped: {e}"));
    }
    if let Err(e) = quit_hook {
        shared.log(LogLevel::Warn, format!("quit hook unavailable: {e}"));
    }
    if let Err(e) = focus::install_minimize_guard() {
        shared.log(LogLevel::Warn, format!("minimize guard unavailable: {e}"));
    }
    if let Err(e) = focus::install_input_filters() {
        shared.log(LogLevel::Warn, format!("input filters unavailable: {e}"));
    }
    shared.log(LogLevel::Info, format!("input guard: {input_guarded} Input functions"));
    shared.log(LogLevel::Info, format!("navigation (routes): {}", shared.unity.has_navigation()));
    if installed.is_empty() {
        hook::enable_pump_ticks();
        filelog::write("no frame hook could be installed; ticking from the message pump");
        shared.log(LogLevel::Warn, "no frame hook could be installed; ticking from the message pump");
    } else {
        shared.log(LogLevel::Info, format!("frame hooks: {}", installed.join(", ")));
    }

    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime.block_on(pipe::serve(shared, rx)),
        Err(e) => filelog::write(&format!("tokio runtime failed: {e}")),
    }
    filelog::write("detaching");
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

fn wait_for_runtime() -> Option<Runtime> {
    let deadline = Instant::now() + RUNTIME_WAIT;
    loop {
        match Runtime::load() {
            Ok(rt) => return Some(rt),
            Err(unity_runtime::Error::DomainNotReady | unity_runtime::Error::NoRuntime) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(_) => return None,
        }
    }
}

fn unload(module: HMODULE) -> ! {
    unsafe { FreeLibraryAndExitThread(module, 0) }
}
