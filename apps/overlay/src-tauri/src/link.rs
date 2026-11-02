//! Connection to the in-game agent: detect a supported game, inject the agent if it
//! isn't loaded yet, then relay messages between the agent and the UI.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use game_maps_protocol::{
    decode_line, encode_line, pipe_name, AgentMessage, AppMessage, Capabilities, Hotkey, HotkeyAction, LogLevel, TeleportTarget,
    WorldExport, WorldSnapshot, PROTOCOL_VERSION,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::{mpsc, oneshot};

use crate::games::GameEntry;
use crate::injector;
use crate::overlay;

const SNAPSHOT_HZ: u32 = 30;
const TELEPORT_TIMEOUT: Duration = Duration::from_secs(5);
/// A world export scans every loaded object and every cave on the game's main thread.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(600);
/// How long to keep trying to reach a freshly injected agent (it waits for the game's
/// scripting runtime, which can take a while during game startup).
const CONNECT_AFTER_INJECT: Duration = Duration::from_secs(180);

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionEnd {
    Disconnected,
    /// An outdated agent was told to unload; the current build should be injected.
    AgentReplaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    WaitingForGame,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkStatus {
    pub state: LinkState,
    pub game_id: Option<String>,
    pub pid: Option<u32>,
    pub capabilities: Option<Capabilities>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentLog {
    pub level: LogLevel,
    pub message: String,
}

struct Inner {
    status: LinkStatus,
    snapshot: Option<WorldSnapshot>,
    sender: Option<mpsc::UnboundedSender<AppMessage>>,
    hotkeys_override: Option<Vec<Hotkey>>,
}

pub struct AgentLink {
    inner: Mutex<Inner>,
    pending: Mutex<HashMap<u32, oneshot::Sender<Result<(), String>>>>,
    pending_exports: Mutex<HashMap<u32, oneshot::Sender<Result<WorldExport, String>>>>,
    next_request: AtomicU32,
}

pub struct LinkConfig {
    pub games: Vec<GameEntry>,
    pub agent_dll: PathBuf,
    pub staging_dir: PathBuf,
}

impl AgentLink {
    pub fn start(app: AppHandle, config: LinkConfig) -> Arc<Self> {
        let link = Arc::new(Self {
            inner: Mutex::new(Inner {
                status: LinkStatus { state: LinkState::WaitingForGame, game_id: None, pid: None, capabilities: None, message: None },
                snapshot: None,
                sender: None,
                hotkeys_override: None,
            }),
            pending: Mutex::new(HashMap::new()),
            pending_exports: Mutex::new(HashMap::new()),
            next_request: AtomicU32::new(1),
        });
        let l = link.clone();
        tauri::async_runtime::spawn(async move { l.run(app, config).await });
        link
    }

    pub fn status(&self) -> LinkStatus {
        self.inner.lock().unwrap().status.clone()
    }

    pub fn last_snapshot(&self) -> Option<WorldSnapshot> {
        self.inner.lock().unwrap().snapshot.clone()
    }

    pub fn set_hotkeys(&self, hotkeys: Vec<Hotkey>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(tx) = &inner.sender {
            let _ = tx.send(AppMessage::SetHotkeys { hotkeys: hotkeys.clone() });
        }
        inner.hotkeys_override = Some(hotkeys);
    }

    pub async fn teleport(&self, target: TeleportTarget) -> Result<(), String> {
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let inner = self.inner.lock().unwrap();
            let sender = inner.sender.as_ref().ok_or("not connected to the game")?;
            if !inner.status.capabilities.is_some_and(|c| c.teleport) {
                return Err("teleport is not supported for this game".into());
            }
            self.pending.lock().unwrap().insert(request_id, tx);
            sender.send(AppMessage::Teleport { request_id, target }).map_err(|_| "connection closed")?;
        }
        match tokio::time::timeout(TELEPORT_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("connection closed".into()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&request_id);
                Err("the game did not respond".into())
            }
        }
    }

    /// Asks the agent to scan the loaded world for items and points of interest.
    pub async fn export_world(&self) -> Result<WorldExport, String> {
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let inner = self.inner.lock().unwrap();
            let sender = inner.sender.as_ref().ok_or("not connected to the game")?;
            self.pending_exports.lock().unwrap().insert(request_id, tx);
            sender.send(AppMessage::ExportWorld { request_id }).map_err(|_| "connection closed")?;
        }
        match tokio::time::timeout(EXPORT_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("connection closed".into()),
            Err(_) => {
                self.pending_exports.lock().unwrap().remove(&request_id);
                Err("the game did not respond".into())
            }
        }
    }

    fn set_status(&self, app: &AppHandle, status: LinkStatus) {
        let changed = {
            let mut inner = self.inner.lock().unwrap();
            let changed = inner.status.state != status.state
                || inner.status.game_id != status.game_id
                || inner.status.message != status.message
                || inner.status.capabilities != status.capabilities;
            inner.status = status.clone();
            changed
        };
        if changed {
            log::info!("agent link: {:?} {:?}", status.state, status.message);
            let _ = app.emit("agent-status", status);
        }
    }

    async fn run(self: Arc<Self>, app: AppHandle, config: LinkConfig) {
        let mut injected: HashSet<u32> = HashSet::new();
        loop {
            let found = config.games.iter().find_map(|g| injector::find_process(&g.process).map(|pid| (g, pid)));
            let Some((game, pid)) = found else {
                injected.clear();
                self.set_status(&app, LinkStatus { state: LinkState::WaitingForGame, game_id: None, pid: None, capabilities: None, message: None });
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            };
            overlay::set_game_window(&app, pid);
            self.set_status(&app, LinkStatus {
                state: LinkState::Connecting,
                game_id: Some(game.id.clone()),
                pid: Some(pid),
                capabilities: None,
                message: None,
            });

            match self.connect(pid, &config, &mut injected).await {
                Ok((pipe, expected_module)) => {
                    if self.session(&app, game, pid, pipe, &expected_module).await == SessionEnd::AgentReplaced {
                        // The outdated agent unloads itself; inject the current one next.
                        injected.remove(&pid);
                    }
                }
                Err(message) => {
                    self.set_status(&app, LinkStatus {
                        state: LinkState::Error,
                        game_id: Some(game.id.clone()),
                        pid: Some(pid),
                        capabilities: None,
                        message: Some(message),
                    });
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Connects to the agent in `pid`, injecting it first if needed. Also returns the
    /// file name of the current agent build, to detect outdated loaded agents.
    async fn connect(&self, pid: u32, config: &LinkConfig, injected: &mut HashSet<u32>) -> Result<(NamedPipeClient, String), String> {
        let name = pipe_name(pid);
        let dll = injector::stage_agent(&config.agent_dll, &config.staging_dir).map_err(|e| e.to_string())?;
        let expected = dll.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match ClientOptions::new().open(&name) {
                Ok(pipe) => return Ok((pipe, expected)),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {}
                Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
                    if injected.insert(pid) {
                        let dll = dll.clone();
                        log::info!("injecting {} into pid {pid}", dll.display());
                        let res = tokio::task::spawn_blocking(move || injector::inject(pid, &dll))
                            .await
                            .map_err(|e| e.to_string())?;
                        res.map_err(|e| e.to_string())?;
                        deadline = Instant::now() + CONNECT_AFTER_INJECT;
                    }
                }
                Err(e) => return Err(format!("cannot open agent pipe: {e}")),
            }
            if Instant::now() > deadline {
                return Err("the game is running but the agent did not start; is it still loading?".into());
            }
            if !injector::is_alive(pid) {
                return Err("the game exited".into());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn session(&self, app: &AppHandle, game: &GameEntry, pid: u32, pipe: NamedPipeClient, expected_module: &str) -> SessionEnd {
        let (read_half, mut write_half) = tokio::io::split(pipe);
        let mut lines = BufReader::new(read_half).lines();
        let (tx, mut rx) = mpsc::unbounded_channel::<AppMessage>();

        let hotkeys = self.inner.lock().unwrap().hotkeys_override.clone().unwrap_or_else(|| game.hotkeys());
        for msg in [
            AppMessage::Hello { protocol_version: PROTOCOL_VERSION },
            AppMessage::SetHotkeys { hotkeys },
            AppMessage::SetStreamRate { max_hz: SNAPSHOT_HZ },
        ] {
            let _ = tx.send(msg);
        }
        self.inner.lock().unwrap().sender = Some(tx);

        let mut end = SessionEnd::Disconnected;
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    if write_half.write_all(&encode_line(&msg)).await.is_err() {
                        break;
                    }
                }
                line = lines.next_line() => {
                    let Ok(Some(line)) = line else { break };
                    match decode_line::<AgentMessage>(line.as_bytes()) {
                        Ok(AgentMessage::Hello { module, agent_version, .. }) if module.as_deref() != Some(expected_module) => {
                            log::info!("replacing outdated agent {agent_version} ({module:?}) with {expected_module}");
                            let _ = write_half.write_all(&encode_line(&AppMessage::Detach)).await;
                            end = SessionEnd::AgentReplaced;
                            break;
                        }
                        Ok(msg) => self.handle(app, game, pid, msg),
                        Err(e) => log::warn!("bad agent message: {e}"),
                    }
                }
            }
        }

        {
            let mut inner = self.inner.lock().unwrap();
            inner.sender = None;
            inner.snapshot = None;
        }
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err("connection closed".into()));
        }
        for (_, tx) in self.pending_exports.lock().unwrap().drain() {
            let _ = tx.send(Err("connection closed".into()));
        }
        let _ = app.emit("world-snapshot", WorldSnapshot::default());
        if end == SessionEnd::AgentReplaced {
            // Give the old agent time to unhook and unload before injecting again.
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        end
    }

    fn handle(&self, app: &AppHandle, game: &GameEntry, pid: u32, msg: AgentMessage) {
        match msg {
            AgentMessage::Hello { protocol_version, agent_version, game: agent_game, capabilities, .. } => {
                let (state, message) = if protocol_version != PROTOCOL_VERSION {
                    (LinkState::Error, Some(format!("agent protocol {protocol_version} is incompatible; restart the game to load the new agent")))
                } else if agent_game.as_deref() != Some(game.id.as_str()) {
                    (LinkState::Connected, Some("live features are not available for this game yet".into()))
                } else {
                    (LinkState::Connected, None)
                };
                log::info!("agent {agent_version} attached (game={agent_game:?}, caps={capabilities:?})");
                self.set_status(app, LinkStatus { state, game_id: Some(game.id.clone()), pid: Some(pid), capabilities: Some(capabilities), message });
            }
            AgentMessage::Snapshot(snap) => {
                self.inner.lock().unwrap().snapshot = Some(snap.clone());
                if overlay::is_visible(app) {
                    let _ = app.emit("world-snapshot", snap);
                }
            }
            AgentMessage::HotkeyPressed { action: HotkeyAction::ToggleMap } => overlay::toggle(app),
            AgentMessage::TeleportResult { request_id, ok, error } => {
                if let Some(tx) = self.pending.lock().unwrap().remove(&request_id) {
                    let _ = tx.send(if ok { Ok(()) } else { Err(error.unwrap_or_else(|| "teleport failed".into())) });
                }
            }
            AgentMessage::ExportResult { request_id, export, error } => {
                if let Some(tx) = self.pending_exports.lock().unwrap().remove(&request_id) {
                    let _ = tx.send(export.ok_or_else(|| error.unwrap_or_else(|| "export failed".into())));
                }
            }
            AgentMessage::Log { level, message } => {
                match level {
                    LogLevel::Debug => log::debug!("[agent] {message}"),
                    LogLevel::Info => log::info!("[agent] {message}"),
                    LogLevel::Warn => log::warn!("[agent] {message}"),
                    LogLevel::Error => log::error!("[agent] {message}"),
                }
                let _ = app.emit("agent-log", AgentLog { level, message });
            }
        }
    }
}
