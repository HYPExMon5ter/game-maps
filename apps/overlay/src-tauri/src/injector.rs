//! Finding game processes and loading the agent DLL into them.
//!
//! Injection uses the standard `LoadLibraryW` remote-thread technique. The agent is
//! first copied to a content-addressed path, so the original can be rebuilt or
//! updated while a copy is loaded in a running game.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{s, w};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW, MODULEENTRY32W, PROCESSENTRY32W,
    TH32CS_SNAPMODULE, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeProcess, GetExitCodeThread, OpenProcess, WaitForSingleObject, LPTHREAD_START_ROUTINE,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};
use windows::core::BOOL;

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("agent DLL not found at {0}")]
    MissingAgent(PathBuf),
    #[error("could not stage agent DLL: {0}")]
    Stage(std::io::Error),
    #[error("cannot open the game process (is it running as administrator?): {0}")]
    OpenProcess(windows::core::Error),
    #[error("injection failed: {0}")]
    Win32(windows::core::Error),
    #[error("the game did not load the agent")]
    LoadFailed,
}

/// Returns the pid of the first running process whose executable name matches.
pub fn find_process(exe_name: &str) -> Option<u32> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = None;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case(exe_name) {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

/// Whether process `pid` has a module (DLL) with this file name loaded. `None` if the
/// module list can't be read.
pub fn module_loaded(pid: u32, file_name: &str) -> Option<bool> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid).ok()?;
        let _snap = HandleGuard(snap);
        let mut entry = MODULEENTRY32W { dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32, ..Default::default() };
        if Module32FirstW(snap, &mut entry).is_err() {
            return None;
        }
        loop {
            let len = entry.szModule.iter().position(|&c| c == 0).unwrap_or(entry.szModule.len());
            if String::from_utf16_lossy(&entry.szModule[..len]).eq_ignore_ascii_case(file_name) {
                return Some(true);
            }
            if Module32NextW(snap, &mut entry).is_err() {
                return Some(false);
            }
        }
    }
}

/// Whether a process is still running.
pub fn is_alive(pid: u32) -> bool {
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let _h = HandleGuard(h);
        let mut code = 0u32;
        GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE
    }
}

/// The game's largest visible top-level window, used to place the overlay and to give
/// focus back when the map closes.
pub fn main_window(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        best: Option<(HWND, i64)>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let s = &mut *(lparam.0 as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == s.pid && IsWindowVisible(hwnd).as_bool() {
            let mut r = Default::default();
            if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut r).is_ok() {
                let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                if s.best.is_none_or(|(_, a)| area > a) {
                    s.best = Some((hwnd, area));
                }
            }
        }
        BOOL(1)
    }
    let mut search = Search { pid, best: None };
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
    }
    search.best.map(|(h, _)| h)
}

/// Copies the agent to `<staging_dir>/game_maps_agent-<hash>.dll` and returns that path.
pub fn stage_agent(agent: &Path, staging_dir: &Path) -> Result<PathBuf, InjectError> {
    let bytes = std::fs::read(agent).map_err(|_| InjectError::MissingAgent(agent.to_owned()))?;
    let hash = fnv1a64(&bytes);
    std::fs::create_dir_all(staging_dir).map_err(InjectError::Stage)?;
    let staged = staging_dir.join(format!("game_maps_agent-{hash:016x}.dll"));
    if !staged.exists() {
        let tmp = staged.with_extension("tmp");
        std::fs::write(&tmp, &bytes).map_err(InjectError::Stage)?;
        std::fs::rename(&tmp, &staged).map_err(InjectError::Stage)?;
    }
    remove_stale_agents(staging_dir, &staged);
    Ok(staged)
}

/// Deletes older staged agents. Copies still loaded in a running game are locked by
/// Windows and simply skipped until a later run.
fn remove_stale_agents(staging_dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(staging_dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if path != keep && name.starts_with("game_maps_agent-") && name.ends_with(".dll") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Loads `dll` into process `pid` and waits for `LoadLibraryW` to return.
pub fn inject(pid: u32, dll: &Path) -> Result<(), InjectError> {
    let path: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
    let bytes = path.len() * 2;
    unsafe {
        let process = OpenProcess(
            PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ,
            false,
            pid,
        )
        .map_err(InjectError::OpenProcess)?;
        let _process = HandleGuard(process);

        let remote = VirtualAllocEx(process, None, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if remote.is_null() {
            return Err(InjectError::Win32(windows::core::Error::from_thread()));
        }
        let result = (|| {
            WriteProcessMemory(process, remote, path.as_ptr() as *const c_void, bytes, None).map_err(InjectError::Win32)?;
            // kernel32 is mapped at the same address in every process of a boot session.
            let kernel32 = GetModuleHandleW(w!("kernel32.dll")).map_err(InjectError::Win32)?;
            let load_library = GetProcAddress(kernel32, s!("LoadLibraryW")).ok_or(InjectError::LoadFailed)?;
            let start: LPTHREAD_START_ROUTINE = Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut c_void) -> u32,
            >(load_library));
            let thread = CreateRemoteThread(process, None, 0, start, Some(remote), 0, None).map_err(InjectError::Win32)?;
            let _thread = HandleGuard(thread);
            WaitForSingleObject(thread, 15_000);
            let mut code = 0u32;
            GetExitCodeThread(thread, &mut code).map_err(InjectError::Win32)?;
            // Exit code is the low 32 bits of the loaded module handle; 0 means failure.
            if code == 0 {
                return Err(InjectError::LoadFailed);
            }
            Ok(())
        })();
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        result
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_own_process() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        assert!(find_process(&name).is_some());
        assert!(find_process("definitely-not-running-xyz.exe").is_none());
    }

    #[test]
    fn finds_own_modules() {
        let pid = std::process::id();
        assert_eq!(module_loaded(pid, "KERNEL32.DLL"), Some(true));
        assert_eq!(module_loaded(pid, "definitely-not-loaded.dll"), Some(false));
    }

    #[test]
    fn staging_is_content_addressed_and_idempotent() {
        let dir = std::env::temp_dir().join(format!("gm-stage-test-{}", std::process::id()));
        let src = dir.join("agent.dll");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&src, b"abc").unwrap();
        let a = stage_agent(&src, &dir.join("staged")).unwrap();
        let b = stage_agent(&src, &dir.join("staged")).unwrap();
        assert_eq!(a, b);
        std::fs::write(&src, b"abcd").unwrap();
        let c = stage_agent(&src, &dir.join("staged")).unwrap();
        assert_ne!(a, c);
        assert!(!a.exists(), "stale staged agent is cleaned up");
        assert!(c.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
