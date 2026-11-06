//! Connection to the in-game agent: detect a supported game, inject the agent if it
//! isn't loaded yet, then relay messages between the agent and the UI.
//!
//! Games set to automatic hooking are connected as soon as they start; manual games
//! wait for [`AgentLink::request_connect`]. [`AgentLink::disconnect`] unloads the agent
//! and keeps it out until the user connects again or the game restarts.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use game_maps_protocol::{
    decode_line, encode_line, pipe_name, AgentMessage, AppMessage, Capabilities, CompanionOrderOption, Hotkey, HotkeyAction, LogLevel, TeleportTarget,
    GamePin, GroundPoint, PathInfo, RouteCheck, Vec3, WorldExport, WorldLayer, WorldMap, WorldSnapshot, PROTOCOL_VERSION,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::analytics::{self, Feature, HookFailure};
use crate::games::GameEntry;
use crate::injector::{self, InjectError};
use crate::map_window;
use crate::overlay;
use crate::settings::{self, HookMode};
use crate::worldmaps;

pub const SNAPSHOT_HZ: u32 = 30;
/// Sampling while nothing is on screen: the agent samples on the game's main thread.
pub const HIDDEN_SNAPSHOT_HZ: u32 = 2;
/// How often running games are looked for while none is: walking the process list is the
/// one thing the app does all the time.
const GAME_SCAN_INTERVAL: Duration = Duration::from_secs(3);
/// How long a write to the agent may take before the session counts as lost.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const TELEPORT_TIMEOUT: Duration = Duration::from_secs(5);
const PATH_TIMEOUT: Duration = Duration::from_secs(3);
/// Drawing a world's terrain takes a moment, on top of the save.
const WORLD_MAP_TIMEOUT: Duration = Duration::from_secs(30);
/// Under the UI's 1.5 s between routes: a window asking again still gets a fresh one.
const ROUTE_SHARED_FOR: Duration = Duration::from_millis(1200);
/// Route checks and ground probes take a frame or two in the game.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// How often the UI gets a snapshot while the map is hidden.
const HIDDEN_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(500);
/// Places remembered for "teleport back" (most recent last).
const TELEPORT_HISTORY: usize = 20;
/// A world export scans every loaded object and every cave on the game's main thread.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(600);
/// How long to keep trying to reach a freshly injected agent (it waits for the game's
/// scripting runtime, which can take a while during game startup).
const CONNECT_AFTER_INJECT: Duration = Duration::from_secs(180);
/// The agent sends something at least this often while it's working.
const LIVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Silence this long means the game hung (or a load is taking very long): stop showing
/// live data that's out of date, and connect again.
const STALL_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the game's window must have been up before injecting.
const GAME_SETTLE: Duration = Duration::from_secs(3);
/// Longest wait between attempts to get an agent going again.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
/// A session this long counts as a success: the next failure starts over.
const STABLE_SESSION: Duration = Duration::from_secs(30);
/// How long an outdated agent may take to unhook and unload after being told to detach.
const AGENT_UNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
/// A connected agent ticks from the menus on: this long without data means it can't.
const NO_DATA_TIMEOUT: Duration = Duration::from_secs(120);

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;

/// Whether the UI should hear about this snapshot. Hidden map: a slow trickle keeps
/// background features (marking collectibles found) working without busy-updating it.
fn ui_wants_update(app: &AppHandle, inner: &mut Inner) -> bool {
    let wanted = overlay::is_visible(app) || map_window::is_open(app) || inner.hidden_sent_at.is_none_or(|t| t.elapsed() >= HIDDEN_SNAPSHOT_INTERVAL);
    if wanted {
        inner.hidden_sent_at = Some(Instant::now());
    }
    wanted
}

/// Wait before the next attempt after `failures` failed ones: 5 s, doubling up to a minute.
fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs(5u64.saturating_mul(1u64 << failures.min(10))).min(MAX_RETRY_DELAY)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionEnd {
    Disconnected,
    /// An outdated agent was told to unload; the current build should be injected.
    AgentReplaced,
    /// The agent went silent.
    Stalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    WaitingForGame,
    /// The game is running but not connected: hooking is manual, or the user disconnected.
    Ready,
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
    /// Where the player was before each teleport this session (most recent last).
    teleported_from: Vec<(Vec3, WorldLayer)>,
    /// Last snapshot sent to the UI while the map was hidden.
    hidden_sent_at: Option<Instant>,
    /// Last message from the agent.
    last_message: Option<Instant>,
    /// The world last reported, for telling when the player is in another one.
    world_id: Option<String>,
    /// A whole snapshot was held back from the UI, so it can't take updates yet.
    ui_behind: bool,
    /// The last route found: every window showing the map asks for it.
    route: Option<(Vec3, Instant, PathInfo)>,
}

/// Per-process hooking decisions.
#[derive(Default)]
struct Control {
    /// Processes the agent was injected into (never injected twice).
    injected: HashSet<u32>,
    /// Processes the user disconnected from.
    detached: HashSet<u32>,
    connect_requested: Option<u32>,
    /// A game process and when its window was first seen.
    window_seen: Option<(u32, Instant)>,
    /// Processes already counted as connected: reconnects aren't new sessions.
    counted_connected: HashSet<u32>,
    /// Failures already counted: attempts repeat every minute while one lasts.
    counted_failures: HashSet<(u32, HookFailure)>,
}

enum ConnectError {
    /// No agent in the game, and injecting wasn't allowed.
    NotLoaded,
    /// The game is still starting: too early to inject.
    Starting,
    /// What to show the player, and what to count (`None`: nothing went wrong with the app).
    Failed(String, Option<HookFailure>),
}

impl From<InjectError> for ConnectError {
    fn from(e: InjectError) -> Self {
        let failure = match e {
            InjectError::MissingAgent(_) | InjectError::Stage(_) => HookFailure::AgentFiles,
            InjectError::OpenProcess(_) => HookFailure::ProcessAccess,
            InjectError::Win32(_) | InjectError::LoadFailed => HookFailure::Injection,
        };
        Self::Failed(e.to_string(), Some(failure))
    }
}

pub struct AgentLink {
    inner: Mutex<Inner>,
    control: Mutex<Control>,
    /// The agent build staged for injecting.
    staged: Mutex<Option<PathBuf>>,
    /// Wakes the link loop early (connect requests, hook mode changes).
    wake: Notify,
    teleports: Pending<()>,
    exports: Pending<WorldExport>,
    world_maps: Pending<WorldMap>,
    paths: Pending<PathInfo>,
    route_checks: Pending<RouteCheck>,
    ground_probes: Pending<Vec<GroundPoint>>,
    companion_orders: Pending<Vec<CompanionOrderOption>>,
    companion_results: Pending<()>,
    next_request: AtomicU32,
}

/// Requests waiting for the agent's answer, by request id.
struct Pending<T>(Mutex<HashMap<u32, oneshot::Sender<Result<T, String>>>>);

impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

impl<T> Pending<T> {
    /// Hands the agent's answer to whoever is waiting for it (unless they gave up).
    fn resolve(&self, request_id: u32, result: Option<T>, error: Option<String>, fallback: &str) {
        if let Some(tx) = self.0.lock().unwrap().remove(&request_id) {
            let _ = tx.send(result.ok_or_else(|| error.unwrap_or_else(|| fallback.into())));
        }
    }

    fn fail_all(&self, error: &str) {
        for (_, tx) in self.0.lock().unwrap().drain() {
            let _ = tx.send(Err(error.into()));
        }
    }
}

#[derive(Clone)]
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
                teleported_from: Vec::new(),
                hidden_sent_at: None,
                last_message: None,
                world_id: None,
                ui_behind: false,
                route: None,
            }),
            control: Mutex::new(Control::default()),
            staged: Mutex::new(None),
            wake: Notify::new(),
            teleports: Pending::default(),
            exports: Pending::default(),
            world_maps: Pending::default(),
            paths: Pending::default(),
            route_checks: Pending::default(),
            ground_probes: Pending::default(),
            companion_orders: Pending::default(),
            companion_results: Pending::default(),
            next_request: AtomicU32::new(1),
        });
        let l = link.clone();
        // The link is what everything live depends on: if it ever panics, start it again.
        tauri::async_runtime::spawn(async move {
            loop {
                let (run, app, config) = (l.clone(), app.clone(), config.clone());
                match tauri::async_runtime::spawn(async move { run.run(app, config).await }).await {
                    Ok(()) => break,
                    Err(e) => log::error!("agent link stopped: {e}; restarting it"),
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        link
    }

    pub fn status(&self) -> LinkStatus {
        self.inner.lock().unwrap().status.clone()
    }

    pub fn last_snapshot(&self) -> Option<WorldSnapshot> {
        self.inner.lock().unwrap().snapshot.clone()
    }

    /// Connects to the running game now, injecting the agent if needed.
    pub fn request_connect(&self) -> Result<(), String> {
        let pid = self.status().pid.ok_or("the game is not running")?;
        let mut control = self.control.lock().unwrap();
        control.connect_requested = Some(pid);
        // Retry after a failed injection (`connect` won't load a copy that's still there).
        control.injected.remove(&pid);
        drop(control);
        self.wake.notify_one();
        Ok(())
    }

    /// Unloads the agent from the running game and stays disconnected.
    pub fn disconnect(&self) -> Result<(), String> {
        let pid = self.status().pid.ok_or("the game is not running")?;
        {
            let mut control = self.control.lock().unwrap();
            control.detached.insert(pid);
            control.injected.remove(&pid);
            control.connect_requested = None;
        }
        if let Some(tx) = &self.inner.lock().unwrap().sender {
            let _ = tx.send(AppMessage::Detach);
        }
        self.wake.notify_one();
        Ok(())
    }

    /// How often the agent should sample the game.
    pub fn set_stream_rate(&self, max_hz: u32) {
        if let Some(tx) = &self.inner.lock().unwrap().sender {
            let _ = tx.send(AppMessage::SetStreamRate { max_hz });
        }
    }

    pub fn map_opened(&self) {
        if let Some(tx) = &self.inner.lock().unwrap().sender {
            let _ = tx.send(AppMessage::MapOpened);
        }
    }

    /// Re-evaluates hooking right away (e.g. after a hook mode change).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    async fn idle(&self, duration: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(duration) => {}
            _ = self.wake.notified() => {}
        }
    }

    /// Sends new hotkeys to the agent if it is connected to `game_id`. New sessions
    /// read them from the settings.
    pub fn apply_hotkeys(&self, game_id: &str, hotkeys: Vec<Hotkey>) {
        let inner = self.inner.lock().unwrap();
        if let (Some(tx), Some(current)) = (&inner.sender, &inner.status.game_id) {
            if current == game_id {
                let _ = tx.send(AppMessage::SetHotkeys { hotkeys });
            }
        }
    }

    /// Whether the agent is connected and working (it can be connected with its frame
    /// hooks failing, or stuck). Without it, the map still opens as a plain map.
    pub fn is_live(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.status.state == LinkState::Connected && inner.last_message.is_some_and(|t| t.elapsed() < LIVE_TIMEOUT)
    }

    /// Teleports the player, remembering where they were for [`AgentLink::teleport_back`].
    pub async fn teleport(&self, app: &AppHandle, target: TeleportTarget) -> Result<(), String> {
        let from = self.inner.lock().unwrap().snapshot.as_ref().and_then(|s| s.local.as_ref()).map(|l| (l.position, l.layer));
        self.send_teleport(target).await?;
        analytics::count(app, Feature::Teleport);
        if let Some(from) = from {
            let mut inner = self.inner.lock().unwrap();
            if inner.teleported_from.len() == TELEPORT_HISTORY {
                inner.teleported_from.remove(0);
            }
            inner.teleported_from.push(from);
        }
        Ok(())
    }

    /// Returns the player to where they were before their last teleport. Each call goes
    /// one teleport further back.
    pub async fn teleport_back(&self) -> Result<(), String> {
        let from = self.inner.lock().unwrap().teleported_from.pop().ok_or("no teleport to go back from")?;
        let (position, layer) = from;
        let result = self.send_teleport(TeleportTarget::Position { position, snap_to_ground: false, layer }).await;
        if result.is_err() {
            self.inner.lock().unwrap().teleported_from.push(from);
        }
        result
    }

    /// How many teleports can be undone with [`AgentLink::teleport_back`].
    pub fn teleport_back_steps(&self) -> usize {
        self.inner.lock().unwrap().teleported_from.len()
    }

    async fn send_teleport(&self, target: TeleportTarget) -> Result<(), String> {
        if !self.inner.lock().unwrap().status.capabilities.is_some_and(|c| c.teleport) {
            return Err("teleport is not supported for this game".into());
        }
        self.request(&self.teleports, TELEPORT_TIMEOUT, |request_id| AppMessage::Teleport { request_id, target }).await
    }

    /// Asks the agent to scan the loaded world for items and points of interest.
    pub async fn export_world(&self) -> Result<WorldExport, String> {
        self.request(&self.exports, EXPORT_TIMEOUT, |request_id| AppMessage::ExportWorld { request_id }).await
    }

    /// The world the game last reported.
    pub fn world_id(&self) -> Option<String> {
        self.inner.lock().unwrap().world_id.clone()
    }

    /// Asks the agent to read the loaded world's map into `dir` (see `worldmaps`).
    pub async fn read_world_map(&self, dir: String, terrain: bool) -> Result<WorldMap, String> {
        self.request(&self.world_maps, WORLD_MAP_TIMEOUT, |request_id| AppMessage::ReadWorldMap { request_id, dir, terrain }).await
    }

    /// Puts the player's pins on the game's own map.
    pub fn set_game_pins(&self, pins: Vec<GamePin>) -> Result<(), String> {
        self.send(AppMessage::SetGamePins { pins })
    }

    /// The player's options for this game that the agent acts on.
    pub fn set_game_options(&self, options: std::collections::BTreeMap<String, bool>) -> Result<(), String> {
        self.send(AppMessage::SetGameOptions { options })
    }

    /// Pings a spot the way the game does, for everyone in the world.
    pub fn game_ping(&self, position: Vec3) -> Result<(), String> {
        self.send(AppMessage::GamePing { position })
    }

    fn send(&self, message: AppMessage) -> Result<(), String> {
        let inner = self.inner.lock().unwrap();
        let sender = inner.sender.as_ref().ok_or("not connected to the game")?;
        sender.send(message).map_err(|_| "connection closed".to_owned())
    }

    /// A walking route from the player to `to` on the game's navigation mesh.
    pub async fn find_path(&self, to: Vec3) -> Result<PathInfo, String> {
        // Finding a route blocks the game's main thread: two windows can share one.
        if let Some((_, _, path)) = self.inner.lock().unwrap().route.as_ref().filter(|(target, at, _)| *target == to && at.elapsed() < ROUTE_SHARED_FOR) {
            return Ok(path.clone());
        }
        let path = self.request(&self.paths, PATH_TIMEOUT, |request_id| AppMessage::FindPath { request_id, to }).await?;
        self.inner.lock().unwrap().route = Some((to, Instant::now(), path.clone()));
        Ok(path)
    }

    /// Which legs of a route (floor points about a meter apart) can't be walked now.
    pub async fn check_route(&self, points: Vec<Vec3>) -> Result<RouteCheck, String> {
        self.request(&self.route_checks, PROBE_TIMEOUT, |request_id| AppMessage::CheckRoute { request_id, points }).await
    }

    /// The floor under points near the player, and whether the camera sees them.
    pub async fn probe_ground(&self, points: Vec<Vec3>) -> Result<Vec<GroundPoint>, String> {
        self.request(&self.ground_probes, PROBE_TIMEOUT, |request_id| AppMessage::ProbeGround { request_id, points }).await
    }

    /// What `companion` can be told to do right now.
    pub async fn companion_orders(&self, companion: String) -> Result<Vec<CompanionOrderOption>, String> {
        self.request(&self.companion_orders, PROBE_TIMEOUT, |request_id| AppMessage::CompanionOrders { request_id, companion }).await
    }

    /// Tells `companion` to do `order`.
    pub async fn companion_order(&self, companion: String, order: String) -> Result<(), String> {
        self.request(&self.companion_results, PROBE_TIMEOUT, |request_id| AppMessage::CompanionOrder { request_id, companion, order }).await
    }

    /// Sends the message `make` builds for a new request id and waits for its answer.
    async fn request<T>(&self, pending: &Pending<T>, timeout: Duration, make: impl FnOnce(u32) -> AppMessage) -> Result<T, String> {
        let request_id = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let inner = self.inner.lock().unwrap();
            let sender = inner.sender.as_ref().ok_or("not connected to the game")?;
            pending.0.lock().unwrap().insert(request_id, tx);
            if sender.send(make(request_id)).is_err() {
                pending.0.lock().unwrap().remove(&request_id);
                return Err("connection closed".into());
            }
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("connection closed".into()),
            Err(_) => {
                pending.0.lock().unwrap().remove(&request_id);
                Err("the game did not respond".into())
            }
        }
    }

    /// Counts `failure` once per game process.
    fn count_failure(&self, app: &AppHandle, game: &GameEntry, pid: u32, failure: HookFailure) {
        if self.control.lock().unwrap().counted_failures.insert((pid, failure)) {
            analytics::hook_failed(app, &game.id, failure);
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
        // Failed attempts in a row.
        let mut failures: u32 = 0;
        loop {
            let names: Vec<&str> = config.games.iter().map(|g| g.process.as_str()).collect();
            let found = injector::find_first(&names).map(|(index, pid)| (&config.games[index], pid));
            let Some((game, pid)) = found else {
                analytics::game_closed(&app);
                *self.control.lock().unwrap() = Control::default();
                // Places from a game that's gone mean nothing in the next one.
                self.inner.lock().unwrap().teleported_from.clear();
                self.set_status(&app, LinkStatus { state: LinkState::WaitingForGame, game_id: None, pid: None, capabilities: None, message: None });
                self.idle(GAME_SCAN_INTERVAL).await;
                continue;
            };
            overlay::set_game_window(&app, pid);
            let status = |state: LinkState, message: Option<String>| LinkStatus {
                state,
                game_id: Some(game.id.clone()),
                pid: Some(pid),
                capabilities: None,
                message,
            };

            let (may_inject, detached, requested) = {
                let mut control = self.control.lock().unwrap();
                if control.connect_requested.take() == Some(pid) {
                    control.detached.remove(&pid);
                    (true, false, true)
                } else {
                    let detached = control.detached.contains(&pid);
                    (!detached && settings::get(&app).hook_mode(&game.id) == HookMode::Auto, detached, false)
                }
            };
            if detached {
                // Don't even reattach to an agent that is still unloading.
                self.set_status(&app, status(LinkState::Ready, Some("Disconnected".into())));
                self.idle(Duration::from_secs(2)).await;
                continue;
            }
            if may_inject {
                self.set_status(&app, status(LinkState::Connecting, None));
            }

            match self.connect(pid, &config, may_inject).await {
                Ok((pipe, expected_module)) => {
                    let started = Instant::now();
                    let end = self.session(&app, game, pid, pipe, &expected_module).await;
                    if end == SessionEnd::AgentReplaced {
                        // The outdated agent unloads itself; inject the current one next.
                        self.control.lock().unwrap().injected.remove(&pid);
                    }
                    let stable = started.elapsed() >= STABLE_SESSION;
                    if stable {
                        failures = 0;
                    }
                    if end == SessionEnd::Stalled {
                        self.set_status(&app, status(LinkState::Error, Some("The game stopped responding (it may be loading). Reconnecting…".into())));
                    }
                    // A session that ends right away is a failure too, not a reason to spin.
                    if end == SessionEnd::Stalled || (end == SessionEnd::Disconnected && !stable) {
                        self.idle(retry_delay(failures)).await;
                        failures = failures.saturating_add(1);
                    }
                }
                Err(ConnectError::NotLoaded) => {
                    self.set_status(&app, status(LinkState::Ready, None));
                    self.idle(Duration::from_secs(2)).await;
                    continue;
                }
                Err(ConnectError::Starting) => {
                    // Keeps a Connect click waiting too, in games connected by hand.
                    if requested {
                        self.control.lock().unwrap().connect_requested = Some(pid);
                    }
                    self.idle(Duration::from_secs(1)).await;
                    continue;
                }
                Err(ConnectError::Failed(message, failure)) => {
                    log::warn!("cannot connect to {}: {message}", game.id);
                    if let Some(failure) = failure {
                        self.count_failure(&app, game, pid, failure);
                    }
                    self.set_status(&app, status(LinkState::Error, Some(message)));
                    // Keep trying, less often the longer it fails. The agent may have died:
                    // inject it again (`connect` skips this if it's still loaded).
                    self.control.lock().unwrap().injected.remove(&pid);
                    self.idle(retry_delay(failures)).await;
                    failures = failures.saturating_add(1);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// The agent build to inject, staged once: hashing it on every poll is wasted work.
    fn staged_agent(&self, config: &LinkConfig) -> Result<PathBuf, InjectError> {
        let mut staged = self.staged.lock().unwrap();
        if let Some(dll) = staged.as_ref().filter(|p| p.exists()) {
            return Ok(dll.clone());
        }
        let dll = injector::stage_agent(&config.agent_dll, &config.staging_dir)?;
        *staged = Some(dll.clone());
        Ok(dll)
    }

    /// Whether the game has been up long enough to inject into. An agent injected the
    /// moment the process starts can hang the game before its window ever opens.
    fn game_settled(&self, pid: u32) -> bool {
        let mut control = self.control.lock().unwrap();
        if injector::main_window(pid).is_none() {
            control.window_seen = None;
            return false;
        }
        match control.window_seen {
            Some((seen_pid, at)) if seen_pid == pid => at.elapsed() >= GAME_SETTLE,
            _ => {
                control.window_seen = Some((pid, Instant::now()));
                false
            }
        }
    }

    /// Connects to the agent in `pid`, injecting it first if allowed and needed. Also
    /// returns the file name of the current agent build, to detect outdated agents.
    async fn connect(&self, pid: u32, config: &LinkConfig, may_inject: bool) -> Result<(NamedPipeClient, String), ConnectError> {
        let name = pipe_name(pid);
        let dll = self.staged_agent(config)?;
        let expected = dll.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut deadline = Instant::now() + Duration::from_secs(2);
        loop {
            // The player disconnected while this was waiting.
            if self.control.lock().unwrap().detached.contains(&pid) {
                return Err(ConnectError::NotLoaded);
            }
            match ClientOptions::new().open(&name) {
                Ok(pipe) => return Ok((pipe, expected)),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {}
                Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
                    if !may_inject {
                        return Err(ConnectError::NotLoaded);
                    }
                    let injected = self.control.lock().unwrap().injected.contains(&pid);
                    if !injected && !self.game_settled(pid) {
                        return Err(ConnectError::Starting);
                    }
                    let first = self.control.lock().unwrap().injected.insert(pid);
                    if first {
                        // A second load of a loaded DLL does nothing but keep it from ever
                        // unloading. It may be waiting for the game's runtime: give it time.
                        if injector::module_loaded(pid, &expected) == Some(true) {
                            log::info!("the agent is already in pid {pid}; waiting for it");
                        } else {
                            let dll = dll.clone();
                            log::info!("injecting {} into pid {pid}", dll.display());
                            tokio::task::spawn_blocking(move || injector::inject(pid, &dll))
                                .await
                                .map_err(|e| ConnectError::Failed(e.to_string(), Some(HookFailure::Injection)))??;
                        }
                        deadline = Instant::now() + CONNECT_AFTER_INJECT;
                    }
                }
                Err(e) => return Err(ConnectError::Failed(format!("cannot open agent pipe: {e}"), Some(HookFailure::Pipe))),
            }
            // First: a game that closed while this waited isn't a failed agent.
            if !injector::is_alive(pid) {
                return Err(ConnectError::Failed("the game exited".into(), None));
            }
            if Instant::now() > deadline {
                return Err(ConnectError::Failed("the game is running but the agent did not start; is it still loading?".into(), Some(HookFailure::AgentTimeout)));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn session(&self, app: &AppHandle, game: &GameEntry, pid: u32, pipe: NamedPipeClient, expected_module: &str) -> SessionEnd {
        let (read_half, mut write_half) = tokio::io::split(pipe);
        let mut lines = BufReader::new(read_half).lines();
        let (tx, mut rx) = mpsc::unbounded_channel::<AppMessage>();

        let hotkeys = settings::get(app).hotkeys(game);
        for msg in [
            AppMessage::Hello { protocol_version: PROTOCOL_VERSION },
            AppMessage::SetHotkeys { hotkeys },
            AppMessage::SetStreamRate { max_hz: overlay::stream_rate(app) },
        ] {
            let _ = tx.send(msg);
        }
        {
            let mut inner = self.inner.lock().unwrap();
            inner.sender = Some(tx);
            inner.last_message = Some(Instant::now());
        }

        let mut end = SessionEnd::Disconnected;
        let mut replaced_module = None;
        let started = Instant::now();
        let mut watchdog = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = watchdog.tick() => {
                    // Only once snapshots have come: an agent that can't tick at all (an
                    // unknown game version) is connected, just not live.
                    let inner = self.inner.lock().unwrap();
                    let silent = inner.snapshot.as_ref().and(inner.last_message).map(|t| t.elapsed());
                    // A game it doesn't recognize is counted as that, on hello.
                    let no_data = inner.snapshot.is_none() && inner.status.state == LinkState::Connected && inner.status.message.is_none();
                    drop(inner);
                    if no_data && started.elapsed() >= NO_DATA_TIMEOUT {
                        self.count_failure(app, game, pid, HookFailure::NoGameData);
                    }
                    if silent.is_some_and(|t| t >= STALL_TIMEOUT) {
                        log::warn!("no word from the agent in {}: reconnecting", game.id);
                        end = SessionEnd::Stalled;
                        break;
                    }
                }
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    if !matches!(tokio::time::timeout(WRITE_TIMEOUT, write_half.write_all(&encode_line(&msg))).await, Ok(Ok(()))) {
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
                            replaced_module = module;
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
        self.teleports.fail_all("connection closed");
        self.exports.fail_all("connection closed");
        self.world_maps.fail_all("connection closed");
        self.paths.fail_all("connection closed");
        self.route_checks.fail_all("connection closed");
        self.ground_probes.fail_all("connection closed");
        self.companion_orders.fail_all("connection closed");
        self.companion_results.fail_all("connection closed");
        let _ = app.emit("world-snapshot", WorldSnapshot::default());
        if end == SessionEnd::AgentReplaced {
            // Inject the new agent only once the old one has unhooked and unloaded: a copy
            // injected earlier sees the old one still loaded and exits.
            let deadline = Instant::now() + AGENT_UNLOAD_TIMEOUT;
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let gone = match &replaced_module {
                    Some(m) => injector::module_loaded(pid, m) != Some(true),
                    None => true,
                };
                if gone || Instant::now() > deadline || !injector::is_alive(pid) {
                    if !gone {
                        log::warn!("outdated agent {replaced_module:?} did not unload in time");
                    }
                    break;
                }
            }
        }
        end
    }

    fn handle(&self, app: &AppHandle, game: &GameEntry, pid: u32, msg: AgentMessage) {
        self.inner.lock().unwrap().last_message = Some(Instant::now());
        match msg {
            AgentMessage::Hello { protocol_version, agent_version, game: agent_game, capabilities, .. } => {
                let (state, message) = if protocol_version != PROTOCOL_VERSION {
                    self.count_failure(app, game, pid, HookFailure::ProtocolMismatch);
                    (LinkState::Error, Some(format!("agent protocol {protocol_version} is incompatible; restart the game to load the new agent")))
                } else if agent_game.as_deref() != Some(game.id.as_str()) {
                    self.count_failure(app, game, pid, HookFailure::GameNotRecognized);
                    (LinkState::Connected, Some("live features are not available for this game yet".into()))
                } else {
                    if self.control.lock().unwrap().counted_connected.insert(pid) {
                        analytics::game_connected(app, &game.id);
                    }
                    (LinkState::Connected, None)
                };
                log::info!("agent {agent_version} attached (game={agent_game:?}, caps={capabilities:?})");
                self.set_status(app, LinkStatus { state, game_id: Some(game.id.clone()), pid: Some(pid), capabilities: Some(capabilities), message });
            }
            AgentMessage::Snapshot(snap) => {
                let mut inner = self.inner.lock().unwrap();
                if let Some(world) = snap.world.as_ref().filter(|w| Some(&w.id) != inner.world_id.as_ref()) {
                    // "Teleport back" has nowhere to go in another world.
                    inner.world_id = Some(world.id.clone());
                    inner.teleported_from.clear();
                    if game.world_maps {
                        worldmaps::refresh_soon(app, &game.id, &world.id);
                    }
                }
                inner.snapshot = Some((*snap).clone());
                if ui_wants_update(app, &mut inner) {
                    inner.ui_behind = false;
                    drop(inner);
                    let _ = app.emit("world-snapshot", snap);
                } else {
                    inner.ui_behind = true;
                }
            }
            AgentMessage::SnapshotUpdate(update) => {
                let mut inner = self.inner.lock().unwrap();
                let Some(snapshot) = inner.snapshot.as_mut() else { return };
                (*update).clone().apply(snapshot);
                if ui_wants_update(app, &mut inner) {
                    // An update only means something on top of the last whole snapshot.
                    if std::mem::take(&mut inner.ui_behind) {
                        let whole = inner.snapshot.clone();
                        drop(inner);
                        let _ = app.emit("world-snapshot", whole);
                    } else {
                        drop(inner);
                        let _ = app.emit("world-update", update);
                    }
                }
            }
            AgentMessage::HotkeyPressed { action: HotkeyAction::ToggleMap } => overlay::toggle(app),
            AgentMessage::HotkeyPressed { action: HotkeyAction::ZoomIn } => overlay::zoom(app, 1),
            AgentMessage::HotkeyPressed { action: HotkeyAction::ZoomOut } => overlay::zoom(app, -1),
            // The map does these itself.
            AgentMessage::HotkeyPressed { action: action @ (HotkeyAction::QuickMark | HotkeyAction::NearestUnfound | HotkeyAction::AcceptRequest) } => {
                let _ = app.emit("hotkey-action", action);
            }
            AgentMessage::HotkeyPressed { action: HotkeyAction::TeleportBack } => {
                // Runs outside this handler: the result arrives through this same session.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let link = app.state::<Arc<AgentLink>>().inner().clone();
                    let result = link.teleport_back().await;
                    if let Err(e) = &result {
                        log::info!("teleport back: {e}");
                    }
                    let _ = app.emit("teleport-back", result.err());
                });
            }
            AgentMessage::TeleportResult { request_id, ok, error } => self.teleports.resolve(request_id, ok.then_some(()), error, "teleport failed"),
            AgentMessage::PathResult { request_id, path, error } => self.paths.resolve(request_id, path, error, "no route"),
            AgentMessage::RouteCheckResult { request_id, check, error } => self.route_checks.resolve(request_id, check, error, "route check failed"),
            AgentMessage::GroundProbeResult { request_id, points, error } => self.ground_probes.resolve(request_id, points, error, "ground probe failed"),
            AgentMessage::CompanionOrdersResult { request_id, orders, error } => self.companion_orders.resolve(request_id, orders, error, "no orders"),
            AgentMessage::CompanionOrderResult { request_id, ok, error } => self.companion_results.resolve(request_id, ok.then_some(()), error, "order failed"),
            AgentMessage::ExportResult { request_id, export, error } => self.exports.resolve(request_id, export, error, "export failed"),
            AgentMessage::ZonePins(zone) => {
                if let Some(world) = self.world_id() {
                    worldmaps::remember(app, &game.id, &world, zone);
                }
            }
            AgentMessage::WorldMapResult { request_id, map, error } => self.world_maps.resolve(request_id, map, error, "could not read the map"),
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
