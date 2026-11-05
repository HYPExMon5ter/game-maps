//! App updates through the Tauri updater (signed installers on GitHub Releases).
//!
//! The app checks at startup and every 30 minutes. With automatic updates on, a found
//! update installs once no game is running, so a session is never interrupted. With
//! them off, the user is asked (once per version, also only while no game runs) and
//! the launcher keeps offering it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::analytics::{self, Problem};
use crate::link::{AgentLink, LinkState};
use crate::settings;

pub const EVENT: &str = "update-state";
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
const TICK: Duration = Duration::from_secs(60);
/// A stalled server mustn't hold up checks and installs for good.
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateState {
    /// Development builds don't update themselves.
    Disabled,
    Idle,
    Checking,
    UpToDate,
    Available { version: String, notes: Option<String> },
    Downloading { received: u64, total: Option<u64> },
    Installing,
    Error { message: String },
}

pub struct Updates {
    state: Mutex<UpdateState>,
    pending: Mutex<Option<Update>>,
    /// Serializes check/install.
    busy: tokio::sync::Mutex<()>,
}

impl Default for Updates {
    fn default() -> Self {
        let state = if cfg!(debug_assertions) { UpdateState::Disabled } else { UpdateState::Idle };
        Self { state: Mutex::new(state), pending: Mutex::new(None), busy: tokio::sync::Mutex::new(()) }
    }
}

impl Updates {
    pub fn state(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    fn set(&self, app: &AppHandle, state: UpdateState) {
        *self.state.lock().unwrap() = state.clone();
        let _ = app.emit(EVENT, state);
    }

    fn pending_version(&self) -> Option<String> {
        self.pending.lock().unwrap().as_ref().map(|u| u.version.clone())
    }

    /// Looks for a newer version. `manual`: the user asked, so show progress and
    /// errors; background checks stay quiet unless they find something.
    pub async fn check(&self, app: &AppHandle, manual: bool) -> Result<(), String> {
        if cfg!(debug_assertions) {
            return Err("updates are disabled in development builds".into());
        }
        let _busy = self.busy.lock().await;
        if manual {
            self.set(app, UpdateState::Checking);
        }
        let result = async { tokio::time::timeout(CHECK_TIMEOUT, app.updater().map_err(|e| e.to_string())?.check()).await.map_err(|_| "timed out".to_string())?.map_err(|e| e.to_string()) }.await;
        match result {
            Ok(Some(update)) => {
                log::info!("update available: {}", update.version);
                self.set(app, UpdateState::Available { version: update.version.clone(), notes: update.body.clone() });
                *self.pending.lock().unwrap() = Some(update);
                Ok(())
            }
            Ok(None) => {
                *self.pending.lock().unwrap() = None;
                self.set(app, UpdateState::UpToDate);
                Ok(())
            }
            Err(e) => {
                log::info!("update check failed: {e}");
                if manual {
                    self.set(app, UpdateState::Error { message: "Could not check for updates. Are you online?".into() });
                }
                Err(e)
            }
        }
    }

    /// Downloads and installs the pending update, then restarts the app.
    pub async fn install(&self, app: &AppHandle) -> Result<(), String> {
        let _busy = self.busy.lock().await;
        let update = self.pending.lock().unwrap().take().ok_or("no update to install")?;
        let mut received = 0u64;
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        self.set(app, UpdateState::Downloading { received: 0, total: None });
        let result = update
            .download_and_install(
                |chunk, total| {
                    received += chunk as u64;
                    if last_emit.elapsed() > Duration::from_millis(150) {
                        last_emit = Instant::now();
                        self.set(app, UpdateState::Downloading { received, total });
                    }
                },
                || self.set(app, UpdateState::Installing),
            )
            .await;
        match result {
            Ok(()) => {
                log::info!("update {} installed; restarting", update.version);
                app.restart();
            }
            Err(e) => {
                log::warn!("update install failed: {e}");
                analytics::problem(app, Problem::UpdateInstall);
                self.set(app, UpdateState::Error { message: format!("The update failed to install: {e}") });
                *self.pending.lock().unwrap() = Some(update);
                Err(e.to_string())
            }
        }
    }
}

/// Periodic checks; installs found updates automatically while no game is running.
pub fn start_background(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut last_check: Option<Instant> = None;
        // One automatic install attempt per check; after a failure the user can retry.
        let mut auto_tried = false;
        // Ask about each version once; the launcher keeps offering it after that.
        let mut prompted: Option<String> = None;
        loop {
            let updates = app.state::<Updates>();
            if last_check.is_none_or(|t| t.elapsed() >= CHECK_INTERVAL) {
                last_check = Some(Instant::now());
                auto_tried = false;
                let _ = updates.check(&app, false).await;
            }
            let game_running = app.state::<std::sync::Arc<AgentLink>>().status().state != LinkState::WaitingForGame;
            if let (Some(version), false) = (updates.pending_version(), game_running) {
                if settings::get(&app).auto_update {
                    if !auto_tried {
                        auto_tried = true;
                        let _ = updates.install(&app).await;
                    }
                } else if prompted.as_ref() != Some(&version) {
                    prompted = Some(version.clone());
                    prompt(&app, &version);
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Asks whether to install `version` now (automatic updates are off).
fn prompt(app: &AppHandle, version: &str) {
    let handle = app.clone();
    app.dialog()
        .message(format!("Game Maps {version} is available. Install it now? Game Maps restarts when it's done."))
        .title("Update available")
        .buttons(MessageDialogButtons::OkCancelCustom("Update now".into(), "Later".into()))
        .show(move |update_now| {
            if update_now {
                tauri::async_runtime::spawn(async move {
                    let _ = handle.state::<Updates>().install(&handle).await;
                });
            }
        });
}
