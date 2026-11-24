//! Agent state shared between the game's main thread (frame hook) and the pipe thread.
//!
//! The main thread never blocks: it uses `try_lock` and non-blocking channel sends, and
//! does no work at all while no app is connected.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use game_maps_protocol::{AgentMessage, GamePin, Vec3, Capabilities, Hotkey, HotkeyAction, LogLevel, SnapshotUpdate, TeleportTarget, WorldLayer, WorldSnapshot, PROTOCOL_VERSION};
use tokio::sync::mpsc;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, GetForegroundWindow, GetWindowThreadProcessId, ASFW_ANY};

use crate::diff::{creatures_changed, live_changed, markers_changed, world_changed};
use crate::hotkeys::{Hotkeys, KeyState};
use crate::engine::Engine;
use crate::provider::{to_unity, Provider};
use crate::walk::World;

const DEFAULT_MAX_HZ: u32 = 30;
const HEARTBEAT: Duration = Duration::from_secs(1);
const PENDING_LOG_CAP: usize = 200;
/// Main-thread time a route check may take in one frame.
const ROUTE_CHECK_BUDGET: Duration = Duration::from_millis(3);
/// Upper bounds on points per check/probe request (a long cave route is a few hundred).
const MAX_CHECK_POINTS: usize = 4000;
const MAX_PROBE_POINTS: usize = 64;

pub enum Command {
    SetHotkeys(Vec<Hotkey>),
    SetStreamRate(u32),
    MapOpened,
    Teleport { request_id: u32, target: TeleportTarget },
    ExportWorld { request_id: u32 },
    FindPath { request_id: u32, to: Vec3 },
    CheckRoute { request_id: u32, points: Vec<Vec3> },
    ProbeGround { request_id: u32, points: Vec<Vec3> },
    CompanionOrders { request_id: u32, companion: String },
    PlaceInfo { request_id: u32, position: Vec3 },
    CompanionOrder { request_id: u32, companion: String, order: String },
    ReadWorldMap { request_id: u32, dir: String, world: String, terrain: bool },
    SetGamePins(Vec<GamePin>),
    GamePing(Vec3),
    SetGameOptions(std::collections::BTreeMap<String, bool>),
}

impl Command {
    /// Whether the app waits for an answer.
    fn is_request(&self) -> bool {
        match self {
            Self::Teleport { .. }
            | Self::ExportWorld { .. }
            | Self::FindPath { .. }
            | Self::CheckRoute { .. }
            | Self::ProbeGround { .. }
            | Self::CompanionOrders { .. }
            | Self::PlaceInfo { .. }
            | Self::CompanionOrder { .. }
            | Self::ReadWorldMap { .. } => true,
            Self::SetHotkeys(_) | Self::SetStreamRate(_) | Self::MapOpened | Self::SetGamePins(_) | Self::GamePing(_) | Self::SetGameOptions(_) => false,
        }
    }
}

/// A message for the app, and the connection it's for (see [`Shared::session`]): what was
/// meant for an app that has gone isn't sent to the next one, whose requests are numbered
/// from one again.
pub struct Outgoing {
    pub session: u64,
    pub message: AgentMessage,
}

/// Room kept in the outbox for the answer to a request. Taken before the request is carried
/// out: with the outbox full, nothing is done, rather than done and never answered.
struct Reply {
    session: u64,
    room: mpsc::OwnedPermit<Outgoing>,
}

impl Reply {
    fn send(self, message: AgentMessage) {
        self.room.send(Outgoing { session: self.session, message });
    }
}

/// Answers a request, if it's one that's answered.
fn answer(reply: Option<Reply>, message: AgentMessage) {
    if let Some(reply) = reply {
        reply.send(message);
    }
}

struct MainState<E: Engine> {
    provider: Option<Box<dyn Provider<E>>>,
    hotkeys: Hotkeys,
    last_frame: Option<i64>,
    frame: u64,
    min_interval: Duration,
    last_sample_at: Option<Instant>,
    last_sent_at: Option<Instant>,
    last_sent: Option<WorldSnapshot>,
    /// The connection that has been sent a whole snapshot (see [`Shared::session`]). Any
    /// other has nothing to add updates to yet, and is sent one first.
    full_sent_to: Option<u64>,
    /// Running world export: request id, start time and where its answer goes.
    export: Option<(u32, Instant, Option<Reply>)>,
    /// What's solid for walk checks, on the surface and underground (resolved on first use).
    solid: Option<[E::Solid; 2]>,
    /// An app was connected on the last tick.
    app_here: bool,
}

/// The main-thread side, for whichever engine the game runs on.
trait Ticker: Send + Sync {
    fn tick(&self, shared: &Shared);
}

struct Main<E: Engine> {
    engine: E,
    state: Mutex<MainState<E>>,
}

pub struct Shared {
    main: Box<dyn Ticker>,
    /// What the app asked for, with the connection it asked on.
    inbox: Mutex<Vec<(u64, Command)>>,
    outbox: mpsc::Sender<Outgoing>,
    pending_logs: Mutex<VecDeque<AgentMessage>>,
    pub connected: AtomicBool,
    /// Changes whenever an app connects or goes: what one connection asked for, and the
    /// answers to it, are of no use to another.
    session: AtomicU64,
    resend: AtomicBool,
    pub stopping: AtomicBool,
    pub in_tick: AtomicU32,
    game: Option<&'static str>,
    capabilities: Capabilities,
}

fn split<T>(result: Result<T, String>) -> (Option<T>, Option<String>) {
    match result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    }
}

fn interval_for(max_hz: u32) -> Duration {
    Duration::from_secs_f64(1.0 / max_hz.clamp(1, 120) as f64)
}

impl Shared {
    pub fn new<E: Engine>(engine: E, provider: Option<Box<dyn Provider<E>>>, outbox: mpsc::Sender<Outgoing>) -> Self {
        let game = provider.as_ref().map(|p| p.game_id());
        let capabilities = provider.as_ref().map(|p| p.capabilities()).unwrap_or(Capabilities { hotkeys: true, ..Default::default() });
        let state = Mutex::new(MainState {
                provider,
                hotkeys: Hotkeys::default(),
                last_frame: None,
                frame: 0,
                min_interval: interval_for(DEFAULT_MAX_HZ),
                last_sample_at: None,
                last_sent_at: None,
                last_sent: None,
                full_sent_to: None,
                export: None,
                solid: None,
                app_here: false,
            });
        Self {
            main: Box::new(Main { engine, state }),
            inbox: Mutex::new(Vec::new()),
            outbox,
            pending_logs: Mutex::new(VecDeque::new()),
            connected: AtomicBool::new(false),
            session: AtomicU64::new(0),
            resend: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            in_tick: AtomicU32::new(0),
            game,
            capabilities,
        }
    }

    pub fn hello(&self) -> AgentMessage {
        AgentMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            game: self.game.map(str::to_owned),
            capabilities: self.capabilities,
            module: crate::module_file_name(),
        }
    }

    /// The connection there is now (see [`Outgoing`]).
    pub fn session(&self) -> u64 {
        self.session.load(Ordering::SeqCst)
    }

    /// Called by the pipe thread with what the connected app asks for.
    pub fn push_command(&self, cmd: Command) {
        self.inbox.lock().unwrap_or_else(|e| e.into_inner()).push((self.session(), cmd));
    }

    /// Called by the pipe thread when an app connects/disconnects.
    pub fn set_connected(&self, connected: bool) {
        // First: nothing may be sent or carried out for the connection there was.
        self.session.fetch_add(1, Ordering::SeqCst);
        // What the last app's ticks saw of the game's text input says nothing to this one.
        crate::focus::set_typing(false);
        self.connected.store(connected, Ordering::SeqCst);
        if connected {
            self.resend.store(true, Ordering::SeqCst);
            let pending: Vec<_> = self.pending_logs.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
            for msg in pending {
                self.send(msg);
            }
        }
    }

    /// May run on the game's main thread: never waits.
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        let msg = AgentMessage::Log { level, message: message.into() };
        if self.connected.load(Ordering::Relaxed) {
            self.send(msg);
            return;
        }
        // Being emptied for an app that just connected: one line less, rather than wait.
        let Ok(mut pending) = self.pending_logs.try_lock() else { return };
        if pending.len() == PENDING_LOG_CAP {
            pending.pop_front();
        }
        pending.push_back(msg);
    }

    /// Queues `message` for the connected app, unless the outbox is full: never waits.
    fn send(&self, message: AgentMessage) {
        let _ = self.outbox.try_send(Outgoing { session: self.session(), message });
    }

    /// Room in the outbox for one message to the app connected as `session`, if there is any.
    fn room_for(&self, session: u64) -> Option<Reply> {
        self.outbox.clone().try_reserve_owned().ok().map(|room| Reply { session, room })
    }

    /// Runs once per game frame on the main thread (from the frame hook).
    pub fn tick(&self) {
        if self.stopping.load(Ordering::Relaxed) {
            return;
        }
        self.main.tick(self);
    }
}

impl<E: Engine> Main<E> {
    /// Runs `f` with the game's physics, treating as solid what the provider says is solid
    /// where the player is. Main thread only.
    fn with_world<T>(&self, s: &Shared, st: &mut MainState<E>, cx: &E::Cx<'_>, mut f: impl FnMut(&dyn World) -> T) -> Result<T, String> {
        let local = st.last_sent.as_ref().and_then(|s| s.local.as_ref()).ok_or("not in a world")?;
        let underground = local.layer == WorldLayer::Caves;
        let solid = match st.solid {
            Some(m) => m,
            None => {
                let m = unsafe {
                    match st.provider.as_mut() {
                        Some(p) => [p.solid(cx, false)?, p.solid(cx, true)?],
                        None => [E::default_solid(cx)?; 2],
                    }
                };
                s.log(LogLevel::Info, format!("walk checks: {}", unsafe { self.engine.describe_solid(m) }));
                st.solid = Some(m);
                m
            }
        };
        unsafe { self.engine.with_world(solid[underground as usize], &mut f) }
    }
}

impl<E: Engine> Ticker for Main<E> {
    fn tick(&self, s: &Shared) {
        let mut st = match self.state.try_lock() {
            Ok(st) => st,
            // A tick panicked with the lock held (see `hook::run_tick`): carry on.
            Err(std::sync::TryLockError::Poisoned(e)) => {
                self.state.clear_poison();
                e.into_inner()
            }
            Err(std::sync::TryLockError::WouldBlock) => return,
        };

        // The hooked method may be shared by several components (identical code
        // folding) or run several times per frame; only do work once per frame.
        let fc = self.engine.frame_count();
        if fc.is_some() && fc == st.last_frame {
            return;
        }
        st.last_frame = fc;
        st.frame += 1;

        // The pipe thread is adding to it right now: next frame, rather than wait.
        let commands = match s.inbox.try_lock() {
            Ok(mut inbox) => std::mem::take(&mut *inbox),
            Err(std::sync::TryLockError::Poisoned(e)) => std::mem::take(&mut *e.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => Vec::new(),
        };
        let cx = self.engine.cx();
        let st = &mut *st;

        for (asked_on, cmd) in commands {
            // From a connection that has gone: nobody is waiting for it any more. Looked at
            // for each one: the app can go while the one before is being carried out.
            if asked_on != s.session() {
                continue;
            }
            let reply = if cmd.is_request() {
                // Nowhere to put the answer: not done, so "the game did not respond" is true.
                // The answer is for the connection that asked, whoever is there by then.
                let Some(reply) = s.room_for(asked_on) else { continue };
                Some(reply)
            } else {
                None
            };
            match cmd {
                Command::SetHotkeys(h) => {
                    crate::focus::set_hidden_hotkey(h.iter().find(|k| k.action == HotkeyAction::ToggleMap));
                    st.hotkeys.set(h);
                }
                Command::SetStreamRate(hz) => st.min_interval = interval_for(hz),
                Command::MapOpened => {
                    if let Some(p) = st.provider.as_mut() {
                        p.map_opened();
                    }
                }
                Command::Teleport { request_id, target } => {
                    let result = match st.provider.as_mut() {
                        Some(p) => unsafe { p.teleport(&cx, &target) },
                        None => Err("this game has no teleport support".into()),
                    };
                    if let Err(e) = &result {
                        s.log(LogLevel::Warn, format!("teleport failed: {e}"));
                    }
                    answer(reply, AgentMessage::TeleportResult { request_id, ok: result.is_ok(), error: result.err() });
                }
                Command::FindPath { request_id, to } => {
                    // The latest snapshot is at most a heartbeat old: close enough for a route.
                    let from = st.last_sent.as_ref().and_then(|s| s.local.as_ref()).map(|l| l.position);
                    let result = match (from, st.provider.as_mut()) {
                        (Some(from), Some(p)) => unsafe { p.find_path(&cx, from, to) }.map_err(|e| format!("no route: {e}")),
                        (None, _) => Err("not in a world".into()),
                        (_, None) => Err("routes are not supported for this game".into()),
                    };
                    let (path, error) = match result {
                        Ok(p) => (Some(p), None),
                        Err(e) => (None, Some(e)),
                    };
                    answer(reply, AgentMessage::PathResult { request_id, path, error });
                }
                Command::CheckRoute { request_id, points } => {
                    let started = Instant::now();
                    let result = self.with_world(s, st, &cx, |world| {
                        let points: Vec<_> = points.iter().take(MAX_CHECK_POINTS).map(|&p| to_unity(p)).collect();
                        crate::walk::check_route(world, &points, ROUTE_CHECK_BUDGET)
                    });
                    if let Ok((check, blocks)) = &result {
                        if !blocks.is_empty() {
                            let at: Vec<String> = blocks.iter().map(|b| format!("{:?} at ({:.1}, {:.1}, {:.1})", b.blocker, b.at.x, b.at.y, b.at.z)).collect();
                            let ms = started.elapsed().as_secs_f32() * 1000.0;
                            s.log(LogLevel::Info, format!("route check: {} of {} legs in {ms:.1} ms; blocked: {}", check.checked, points.len().saturating_sub(1), at.join(", ")));
                        }
                    }
                    let (check, error) = split(result.map(|(check, _)| check));
                    answer(reply, AgentMessage::RouteCheckResult { request_id, check, error });
                }
                Command::ProbeGround { request_id, points } => {
                    let camera = unsafe { self.engine.camera() }.map(|c| to_unity(c.position));
                    let result = self.with_world(s, st, &cx, |world| {
                        let points: Vec<_> = points.iter().take(MAX_PROBE_POINTS).map(|&p| to_unity(p)).collect();
                        crate::walk::probe_ground(world, &points, camera)
                    });
                    let (points, error) = split(result);
                    answer(reply, AgentMessage::GroundProbeResult { request_id, points, error });
                }
                Command::PlaceInfo { request_id, position } => {
                    let result = match st.provider.as_mut() {
                        Some(p) => unsafe { p.place_info(&cx, position) },
                        None => Err("this game has nothing to say about places".into()),
                    };
                    let (place, error) = split(result);
                    answer(reply, AgentMessage::PlaceInfoResult { request_id, place, error });
                }
                Command::CompanionOrders { request_id, companion } => {
                    let result = match st.provider.as_mut() {
                        Some(p) => unsafe { p.companion_orders(&cx, &companion) },
                        None => Err("this game has no companions".into()),
                    };
                    let (orders, error) = split(result);
                    answer(reply, AgentMessage::CompanionOrdersResult { request_id, orders, error });
                }
                Command::CompanionOrder { request_id, companion, order } => {
                    let result = match st.provider.as_mut() {
                        Some(p) => unsafe { p.companion_order(&cx, &companion, &order) },
                        None => Err("this game has no companions".into()),
                    };
                    match &result {
                        Ok(()) => s.log(LogLevel::Info, format!("{companion}: order {order} given")),
                        Err(e) => s.log(LogLevel::Warn, format!("{companion}: order {order} failed: {e}")),
                    }
                    answer(reply, AgentMessage::CompanionOrderResult { request_id, ok: result.is_ok(), error: result.err() });
                }
                Command::SetGamePins(pins) => {
                    if let Some(p) = st.provider.as_mut() {
                        unsafe { p.set_game_pins(&cx, pins) };
                    }
                }
                Command::GamePing(position) => {
                    let result = st.provider.as_mut().map_or(Err("this game has no pings".to_owned()), |p| unsafe { p.game_ping(&cx, position) });
                    if let Err(e) = result {
                        s.log(LogLevel::Warn, format!("game ping failed: {e}"));
                    }
                }
                Command::SetGameOptions(options) => {
                    if let Some(p) = st.provider.as_mut() {
                        p.set_options(&options);
                    }
                }
                Command::ReadWorldMap { request_id, dir, world, terrain } => {
                    let capture = match st.provider.as_mut() {
                        Some(p) => unsafe { p.read_world_map(&cx, &world, terrain) },
                        None => Err("this game has no world maps".into()),
                    };
                    match capture {
                        Ok(capture) => crate::worldmap::save(capture, dir, move |result| {
                            let (map, error) = split(result);
                            answer(reply, AgentMessage::WorldMapResult { request_id, map, error });
                        }),
                        Err(e) => answer(reply, AgentMessage::WorldMapResult { request_id, map: None, error: Some(e) }),
                    }
                }
                Command::ExportWorld { request_id } => {
                    if st.export.is_some() {
                        answer(reply, AgentMessage::ExportResult { request_id, export: None, error: Some("an export is already running".into()) });
                        continue;
                    }
                    let started = match st.provider.as_mut() {
                        Some(p) => unsafe { p.begin_export(&cx) },
                        None => Err("this game has no export support".into()),
                    };
                    match started {
                        Ok(()) => st.export = Some((request_id, Instant::now(), reply)),
                        Err(e) => answer(reply, AgentMessage::ExportResult { request_id, export: None, error: Some(e) }),
                    }
                }
            }
        }

        if let Some(p) = st.provider.as_mut() {
            unsafe { p.every_frame(&cx) };
        }
        if st.export.is_some() {
            if let Some(result) = st.provider.as_mut().and_then(|p| unsafe { p.poll_export(&cx) }) {
                let (request_id, started, reply) = st.export.take().expect("an export is running");
                match result {
                    Ok(export) => {
                        s.log(
                            LogLevel::Info,
                            format!("world export: {} entries, {} caves in {:.1} s", export.entries.len(), export.caves.len(), started.elapsed().as_secs_f32()),
                        );
                        answer(reply, AgentMessage::ExportResult { request_id, export: Some(export), error: None });
                    }
                    Err(e) => answer(reply, AgentMessage::ExportResult { request_id, export: None, error: Some(e) }),
                }
            }
        }

        if let Some(p) = st.provider.as_mut() {
            for note in p.take_notes() {
                s.log(LogLevel::Info, note);
            }
        }

        if !s.connected.load(Ordering::Relaxed) {
            if std::mem::take(&mut st.app_here) {
                crate::focus::set_typing(false);
                if let Some(p) = st.provider.as_mut() {
                    unsafe { p.app_left(&cx) };
                }
            }
            return;
        }
        st.app_here = true;

        // Hotkeys: only while the game window has focus and isn't capturing text. While it
        // is, the map key isn't kept from the game either: it's a letter the player types.
        let typing = st.provider.as_mut().is_some_and(|p| unsafe { p.input_blocked(&cx) });
        crate::focus::set_typing(typing);
        let blocked = typing || !game_has_focus();
        if blocked {
            st.hotkeys.suppress();
            // Clear the "pressed since last check" bits, so typing the hotkey in another
            // app doesn't fire it when the game gets focus back.
            for vk in st.hotkeys.keys() {
                unsafe { GetAsyncKeyState(vk as i32) };
            }
        } else {
            let key = |vk: u16| {
                let s = unsafe { GetAsyncKeyState(vk as i32) } as u16;
                KeyState { down: s & 0x8000 != 0, tapped: s & 0x0001 != 0 }
            };
            for action in st.hotkeys.poll(key) {
                // The game owns the foreground right now; hand that right to the overlay
                // so it can take focus when it opens.
                if action == HotkeyAction::ToggleMap {
                    unsafe {
                        let _ = AllowSetForegroundWindow(ASFW_ANY);
                    }
                }
                s.send(AgentMessage::HotkeyPressed { action });
            }
        }

        // World snapshot, rate-limited, sent only on meaningful change.
        let Some(provider) = st.provider.as_mut() else { return };
        let now = Instant::now();
        if st.last_sample_at.is_some_and(|t| now - t < st.min_interval) {
            return;
        }
        st.last_sample_at = Some(now);
        // Taken once per connection, before the provider is read: what it drops on
        // reconnecting is read again for this snapshot.
        if s.resend.swap(false, Ordering::SeqCst) {
            provider.reconnected();
        }
        // The connection this sample is for, read after the flag: an app connecting right
        // now is either this one, or gets its own sample next.
        let session = s.session();
        let mut snap = unsafe { provider.snapshot(&cx, st.frame) };
        if let Some(local) = snap.local.as_mut() {
            local.camera = unsafe { self.engine.camera() };
        }
        let heartbeat = st.last_sent_at.is_none_or(|t| now - t >= HEARTBEAT);
        // Whether the app has (or is about to get) a whole snapshot of the world there is
        // now, and so knows which world what's read next belongs to.
        let mut app_has_world = snap.world.is_some();
        match st.last_sent.as_mut() {
            Some(prev) if st.full_sent_to == Some(session) && !world_changed(prev, &snap) => {
                let live = live_changed(prev, &snap);
                // Lists that didn't change stay as sent: they're compared with what the app
                // has, so small moves add up instead of going unnoticed.
                let markers = markers_changed(prev, &snap).then(|| std::mem::take(&mut snap.markers));
                let creatures = creatures_changed(prev, &snap).then(|| std::mem::take(&mut snap.creatures));
                if live || heartbeat || markers.is_some() || creatures.is_some() {
                    let update = SnapshotUpdate {
                        frame: snap.frame,
                        local: snap.local,
                        others: snap.others,
                        markers,
                        creatures,
                        companions: snap.companions,
                        environment: snap.environment,
                        dropped_bag: snap.dropped_bag,
                        dying: snap.dying,
                        indoors: snap.indoors,
                        progress: snap.progress,
                        visited: snap.visited,
                    };
                    if s.outbox.try_send(Outgoing { session, message: AgentMessage::SnapshotUpdate(Box::new(update.clone())) }).is_ok() {
                        st.last_sent_at = Some(now);
                        update.apply(prev);
                    }
                }
            }
            // Tried again with every sample until it's on its way: the app drops updates
            // while it has no whole snapshot to add them to.
            _ => {
                if s.outbox.try_send(Outgoing { session, message: AgentMessage::Snapshot(Box::new(snap.clone())) }).is_ok() {
                    st.last_sent_at = Some(now);
                    st.last_sent = Some(snap);
                    st.full_sent_to = Some(session);
                } else {
                    app_has_world = false;
                }
            }
        }

        // What's around the player, after the snapshot that says which world it's in: the
        // app files it under the world it last heard of. With room for it kept first, since
        // the provider counts an area as told once it has been read.
        if app_has_world {
            if let Some(room) = s.room_for(session) {
                if let Some(zone) = unsafe { provider.next_zone_pins(&cx) } {
                    room.send(AgentMessage::ZonePins(zone));
                }
            }
        }
    }
}




fn game_has_focus() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use game_maps_protocol::{CameraView, GameWorld, LocalPlayer, PathInfo, WorldKind, ZonePins};

    use super::*;

    /// An engine with nothing in it: the agent's own bookkeeping is what's tested.
    struct NoEngine;

    impl Engine for NoEngine {
        type Cx<'a> = ();
        type Solid = ();

        fn cx(&self) {}

        fn frame_count(&self) -> Option<i64> {
            None
        }

        unsafe fn camera(&self) -> Option<CameraView> {
            None
        }

        unsafe fn find_path(_cx: &(), _from: Vec3, _to: Vec3) -> Result<PathInfo, String> {
            Err("no navigation".into())
        }

        unsafe fn default_solid(_cx: &()) -> Result<(), String> {
            Ok(())
        }

        unsafe fn with_world<T>(&self, _solid: (), _f: &mut dyn FnMut(&dyn World) -> T) -> Result<T, String> {
            Err("no physics".into())
        }
    }

    /// What the game was asked to do.
    #[derive(Default)]
    struct Seen {
        teleports: u32,
        reconnects: u32,
        zones_read: u32,
    }

    struct Game(Arc<Mutex<Seen>>);

    impl Provider<NoEngine> for Game {
        fn game_id(&self) -> &'static str {
            "test"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn take_notes(&mut self) -> Vec<String> {
            Vec::new()
        }

        fn reconnected(&mut self) {
            self.0.lock().unwrap().reconnects += 1;
        }

        unsafe fn snapshot(&mut self, _cx: &(), frame: u64) -> WorldSnapshot {
            WorldSnapshot {
                frame,
                local: Some(LocalPlayer { position: Vec3::new(1.0, 2.0, 3.0), forward: None, layer: WorldLayer::Surface, camera: None }),
                world: Some(GameWorld { id: "world-1".into(), name: None, kind: WorldKind::SinglePlayer }),
                ..Default::default()
            }
        }

        unsafe fn teleport(&mut self, _cx: &(), _target: &TeleportTarget) -> Result<(), String> {
            self.0.lock().unwrap().teleports += 1;
            Ok(())
        }

        unsafe fn next_zone_pins(&mut self, _cx: &()) -> Option<ZonePins> {
            self.0.lock().unwrap().zones_read += 1;
            Some(ZonePins { x: 0, y: 0, pins: Vec::new() })
        }
    }

    /// An agent whose outbox holds `room` messages, with an app connected.
    fn connected(room: usize) -> (Shared, mpsc::Receiver<Outgoing>, Arc<Mutex<Seen>>) {
        let (tx, rx) = mpsc::channel(room);
        let seen = Arc::new(Mutex::new(Seen::default()));
        let shared = Shared::new(NoEngine, Some(Box::new(Game(seen.clone()))), tx);
        shared.set_connected(true);
        (shared, rx, seen)
    }

    fn teleport(request_id: u32) -> Command {
        Command::Teleport { request_id, target: TeleportTarget::Position { position: Vec3::ZERO, snap_to_ground: false, layer: WorldLayer::Surface } }
    }

    fn drain(outbox: &mut mpsc::Receiver<Outgoing>) -> Vec<Outgoing> {
        std::iter::from_fn(|| outbox.try_recv().ok()).collect()
    }

    /// A tick that's due another sample of the world.
    fn tick_with_sample(shared: &Shared) {
        std::thread::sleep(interval_for(DEFAULT_MAX_HZ) + Duration::from_millis(5));
        shared.tick();
    }

    #[test]
    fn a_request_that_cannot_be_answered_is_not_carried_out() {
        let (shared, mut outbox, seen) = connected(1);
        shared.log(LogLevel::Info, "fills the outbox");
        shared.push_command(teleport(1));
        shared.tick();
        assert_eq!(seen.lock().unwrap().teleports, 0);

        drain(&mut outbox);
        shared.push_command(teleport(2));
        shared.tick();
        assert_eq!(seen.lock().unwrap().teleports, 1);
        assert!(drain(&mut outbox).iter().any(|m| matches!(m.message, AgentMessage::TeleportResult { request_id: 2, ok: true, .. })));
    }

    #[test]
    fn what_a_connection_asked_for_dies_with_it() {
        let (shared, mut outbox, seen) = connected(8);
        shared.push_command(teleport(1));
        shared.set_connected(false);
        shared.set_connected(true);
        shared.tick();
        assert_eq!(seen.lock().unwrap().teleports, 0);

        // An answer already on its way isn't for the app that connects next.
        shared.push_command(teleport(1));
        shared.tick();
        shared.set_connected(false);
        shared.set_connected(true);
        let answers: Vec<Outgoing> = drain(&mut outbox).into_iter().filter(|m| matches!(m.message, AgentMessage::TeleportResult { .. })).collect();
        assert_eq!(answers.len(), 1);
        assert_ne!(answers[0].session, shared.session());
    }

    #[test]
    fn a_whole_snapshot_is_tried_until_it_is_on_its_way() {
        let (shared, mut outbox, seen) = connected(1);
        shared.log(LogLevel::Info, "fills the outbox");
        shared.tick();
        assert!(!drain(&mut outbox).iter().any(|m| matches!(m.message, AgentMessage::Snapshot(_))));
        // Nothing about the world before the app knows which world it is.
        assert_eq!(seen.lock().unwrap().zones_read, 0);

        tick_with_sample(&shared);
        assert!(drain(&mut outbox).iter().any(|m| matches!(m.message, AgentMessage::Snapshot(_))));
        // The provider heard of the app connecting once, not with every try.
        assert_eq!(seen.lock().unwrap().reconnects, 1);
    }

    #[test]
    fn an_area_is_only_read_with_room_to_send_it() {
        let (shared, mut outbox, seen) = connected(1);
        // The snapshot takes the only place in the outbox.
        shared.tick();
        assert!(matches!(drain(&mut outbox).as_slice(), [Outgoing { message: AgentMessage::Snapshot(_), .. }]));
        assert_eq!(seen.lock().unwrap().zones_read, 0);

        tick_with_sample(&shared);
        assert_eq!(seen.lock().unwrap().zones_read, 1);
        assert!(drain(&mut outbox).iter().any(|m| matches!(m.message, AgentMessage::ZonePins(_))));
    }
}
