//! Keeps the app usable when something goes wrong:
//! - panics are written to the log;
//! - corrupt save files are moved aside and the last good copy restored, so a damaged
//!   file can't stop the app from starting;
//! - when a window's web view crashes it reloads, and when the whole WebView2 browser
//!   process dies (every window goes black) the app restarts, a limited number of times.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, Runtime, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
};
use webview2_com::ProcessFailedEventHandler;

use crate::analytics::{self, Problem};

/// Save files the app can't start properly without.
const SAVE_FILES: &[&str] = &["user-data.json", "app-settings.json"];
const RESTART_LOG: &str = "restarts.json";
/// At most this many automatic restarts per `RESTART_WINDOW`: past that, restarting isn't
/// helping and would only flash windows at the user.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(600);

pub fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default(info);
    }));
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn is_valid_json(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).is_ok())
}

/// Checks one save file: a good file is backed up, a bad one is moved aside and replaced
/// by its backup if there is one (else the app starts with defaults).
fn protect_file(path: &Path) {
    let backup = path.with_extension("json.bak");
    if !path.exists() {
        return;
    }
    if is_valid_json(path) {
        if let Err(e) = std::fs::copy(path, &backup) {
            log::warn!("could not back up {}: {e}", path.display());
        }
        return;
    }
    let aside = path.with_extension(format!("json.corrupt-{}", now_secs()));
    log::error!("{} is corrupt; moving it to {}", path.display(), aside.display());
    if let Err(e) = std::fs::rename(path, &aside) {
        log::error!("could not move {} aside: {e}", path.display());
        return;
    }
    if is_valid_json(&backup) {
        match std::fs::copy(&backup, path) {
            Ok(_) => log::warn!("restored {} from its backup", path.display()),
            Err(e) => log::error!("could not restore {}: {e}", path.display()),
        }
    }
}

/// Run before anything reads the save files.
pub fn protect_save_files(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    for name in SAVE_FILES {
        protect_file(&dir.join(name));
    }
}

/// Times of recent restarts (seconds), with this one added if another is allowed.
fn allow_restart(previous: &[u64], now: u64) -> Option<Vec<u64>> {
    let mut recent: Vec<u64> = previous.iter().copied().filter(|&t| now.saturating_sub(t) < RESTART_WINDOW.as_secs()).collect();
    if recent.len() >= MAX_RESTARTS {
        return None;
    }
    recent.push(now);
    Some(recent)
}

/// Restarts the app, unless it has been restarting too often.
fn restart<R: Runtime>(app: &AppHandle<R>) {
    // Every window reports the same failure.
    static RESTARTING: AtomicBool = AtomicBool::new(false);
    if RESTARTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(dir) = app.path().app_data_dir() else { return };
    let file = dir.join(RESTART_LOG);
    let previous: Vec<u64> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let Some(recent) = allow_restart(&previous, now_secs()) else {
        log::error!("the web view keeps failing; not restarting again (more than {MAX_RESTARTS} times in {} minutes)", RESTART_WINDOW.as_secs() / 60);
        // Every window is dead at this point: this is the only way left to say so.
        app.dialog().message("Game Maps keeps crashing and stopped restarting itself. Close it from the tray and start it again. If this keeps happening, please report it with the log files.").title("Game Maps").show(|_| {});
        return;
    };
    if let Ok(json) = serde_json::to_vec(&recent) {
        let _ = std::fs::write(&file, json);
    }
    log::warn!("restarting after a web view failure");
    let app = app.clone();
    // Not from inside the web view's own event.
    std::thread::spawn(move || app.restart());
}

/// Watches a window's web view for crashes.
pub fn watch_webview(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let label = window.label().to_owned();
    let result = window.with_webview(move |webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else { return };
        let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(args) = args {
                let _ = args.ProcessFailedKind(&mut kind);
            }
            log::error!("web view process failed in {label}: kind {}", kind.0);
            analytics::problem(&app, Problem::WebviewCrash);
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                restart(&app);
            } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE {
                if let Some(view) = sender {
                    let _ = view.Reload();
                }
            }
            Ok(())
        }));
        let mut token = 0;
        if let Err(e) = core.add_ProcessFailed(&handler, &mut token) {
            log::warn!("cannot watch the web view: {e}");
        }
    });
    if let Err(e) = result {
        log::warn!("cannot watch {}: {e}", window.label());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_restarts_within_the_window() {
        let now = 10_000;
        assert_eq!(allow_restart(&[], now), Some(vec![now]));
        assert_eq!(allow_restart(&[now - 100, now - 50], now), Some(vec![now - 100, now - 50, now]));
        assert_eq!(allow_restart(&[now - 100, now - 50, now - 10], now), None);
        // Old restarts no longer count.
        assert_eq!(allow_restart(&[now - 5000, now - 4000, now - 10], now), Some(vec![now - 10, now]));
    }

    #[test]
    fn replaces_a_corrupt_file_with_its_backup() {
        let dir = std::env::temp_dir().join(format!("game-maps-recovery-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("user-data.json");

        std::fs::write(&file, br#"{"a":1}"#).unwrap();
        protect_file(&file);
        assert!(dir.join("user-data.json.bak").exists());

        std::fs::write(&file, b"{broken").unwrap();
        protect_file(&file);
        assert_eq!(std::fs::read(&file).unwrap(), br#"{"a":1}"#);
        assert!(std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("corrupt")));

        // No backup to fall back on: the file goes and the app starts with defaults.
        std::fs::remove_file(dir.join("user-data.json.bak")).unwrap();
        std::fs::write(&file, b"{broken").unwrap();
        protect_file(&file);
        assert!(!file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
