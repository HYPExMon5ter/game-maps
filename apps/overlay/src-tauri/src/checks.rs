//! Setup problems that stop the map from working, shown in the launcher:
//! - the game runs as administrator but the app doesn't (the agent can't be loaded);
//! - the game uses exclusive fullscreen (no window can be drawn over it).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::link::AgentLink;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct SetupChecks {
    /// The game is elevated and the app is not.
    pub needs_admin: bool,
    /// Last seen while the game was in front: it uses exclusive fullscreen.
    pub exclusive_fullscreen: bool,
    /// The game has updated since its map was made: pins may have moved.
    pub map_outdated: bool,
}

/// Fullscreen state of the current game process, sampled while it is in front
/// (Windows only reports it for the foreground app).
#[derive(Default)]
pub struct FullscreenWatch(Mutex<Option<(u32, bool)>>);

pub fn current(app: &AppHandle) -> SetupChecks {
    let Some(pid) = app.state::<Arc<AgentLink>>().status().pid else { return SetupChecks::default() };
    let exclusive_fullscreen = matches!(*app.state::<FullscreenWatch>().0.lock().unwrap(), Some((p, true)) if p == pid);
    let needs_admin = is_elevated(pid).unwrap_or(false) && !is_elevated(std::process::id()).unwrap_or(false);
    SetupChecks { needs_admin, exclusive_fullscreen, map_outdated: map_outdated(app, pid) }
}

fn map_outdated(app: &AppHandle, pid: u32) -> bool {
    let link = app.state::<Arc<AgentLink>>();
    let Some(id) = link.status().game_id else { return false };
    let Some(game) = app.state::<crate::Registry>().0.iter().find(|g| g.id == id).cloned() else { return false };
    let (Some(app_id), Some(made_from)) = (game.steam_app_id, app.state::<crate::mappacks::MapPacks>().installed_build(&id)) else { return false };
    crate::injector::exe_path(pid).and_then(|exe| crate::games::steam_build(&exe, app_id)).is_some_and(|now| now != made_from)
}

pub fn start_fullscreen_watch(app: AppHandle) {
    std::thread::Builder::new()
        .name("fullscreen-watch".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(pid) = app.state::<Arc<AgentLink>>().status().pid else { continue };
            if foreground_pid() == Some(pid) {
                let exclusive = unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN);
                *app.state::<FullscreenWatch>().0.lock().unwrap() = Some((pid, exclusive));
            }
        })
        .expect("spawn fullscreen watcher");
}

fn foreground_pid() -> Option<u32> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

fn is_elevated(pid: u32) -> Option<bool> {
    unsafe {
        let process = if pid == std::process::id() {
            GetCurrentProcess()
        } else {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?
        };
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        if pid != std::process::id() {
            let _ = windows::Win32::Foundation::CloseHandle(process);
        }
        opened.ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok.ok()?;
        Some(elevation.TokenIsElevated != 0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_own_elevation() {
        assert!(super::is_elevated(std::process::id()).is_some());
    }
}
