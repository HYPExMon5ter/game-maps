//! Game Maps agent: loaded into a game process by the overlay app.
//!
//! Lifecycle: `DllMain` starts a worker thread → wait for the IL2CPP runtime → pick a
//! game provider → hook one per-frame engine method → serve the app over a named pipe
//! → on `Detach`, unhook and unload itself. The game folder is never modified.

mod agent;
mod diff;
mod filelog;
mod hook;
mod hotkeys;
mod pipe;
mod provider;

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use game_maps_protocol::LogLevel;
use il2cpp::{Runtime, Unity};
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

    // Metadata lookups below don't allocate managed objects, so they're safe before
    // this thread is attached to the runtime.
    let Some(rt) = wait_for_runtime() else {
        filelog::write("IL2CPP runtime not found; exiting");
        return;
    };
    let unity = match Unity::new(&rt) {
        Ok(u) => u,
        Err(e) => {
            filelog::write(&format!("not a supported Unity runtime: {e}"));
            return;
        }
    };
    let provider = provider::detect(&rt);
    let mut targets = hook::ENGINE_TARGETS.to_vec();
    if let Some(p) = &provider {
        targets.extend_from_slice(p.frame_hooks());
    }

    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let shared = SHARED.get_or_init(|| Shared::new(rt, unity, provider, tx));
    // Attached only while installing hooks. This thread then just serves the pipe and
    // never touches managed objects; staying attached could make the runtime wait for
    // it when the game shuts down.
    let (installed, errors) = {
        let _thread = shared.rt.attach_thread();
        hook::install(&shared.rt, &targets)
    };
    for e in &errors {
        filelog::write(&format!("frame hook skipped: {e}"));
        shared.log(LogLevel::Debug, format!("frame hook skipped: {e}"));
    }
    if let Err(e) = hook::install_quit_hook(&shared.rt) {
        shared.log(LogLevel::Warn, format!("quit hook unavailable: {e}"));
    }
    if installed.is_empty() {
        shared.log(LogLevel::Error, "no frame hook could be installed; live features unavailable");
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
            Err(il2cpp::Error::DomainNotReady | il2cpp::Error::NotIl2Cpp) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(_) => return None,
        }
    }
}

fn unload(module: HMODULE) -> ! {
    unsafe { FreeLibraryAndExitThread(module, 0) }
}
