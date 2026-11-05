//! Agent state shared between the game's main thread (frame hook) and the pipe thread.
//!
//! The main thread never blocks: it uses `try_lock` and non-blocking channel sends, and
//! does no work at all while no app is connected.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use game_maps_protocol::{AgentMessage, CameraView, Vec3, Capabilities, Hotkey, HotkeyAction, LogLevel, SnapshotUpdate, TeleportTarget, WorldLayer, WorldSnapshot, PROTOCOL_VERSION};
use unity_runtime::{Runtime, Unity};
use tokio::sync::mpsc;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, GetForegroundWindow, GetWindowThreadProcessId, ASFW_ANY};

use crate::diff::{creatures_changed, live_changed, markers_changed, world_changed};
use crate::hotkeys::{Hotkeys, KeyState};
use crate::provider::{default_solid_mask, to_proto, to_unity, Cx, Provider};

const DEFAULT_MAX_HZ: u32 = 30;
const HEARTBEAT: Duration = Duration::from_secs(1);
const PENDING_LOG_CAP: usize = 200;
/// Main-thread time a route check may take in one frame.
const ROUTE_CHECK_BUDGET: Duration = Duration::from_millis(3);
/// Upper bounds on points per check/probe request (a long cave route is a few hundred).
const MAX_CHECK_POINTS: usize = 4000;
const MAX_PROBE_POINTS: usize = 64;
/// Hits kept per ground probe ray.
const PROBE_HITS: usize = 8;

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
    CompanionOrder { request_id: u32, companion: String, order: String },
}

struct MainState {
    provider: Option<Box<dyn Provider>>,
    hotkeys: Hotkeys,
    last_frame: Option<i32>,
    frame: u64,
    min_interval: Duration,
    last_sample_at: Option<Instant>,
    last_sent_at: Option<Instant>,
    last_sent: Option<WorldSnapshot>,
    /// Running world export: request id and start time.
    export: Option<(u32, Instant)>,
    /// Solid layers for walk checks, on the surface and underground (resolved on first use).
    solid_masks: Option<[i32; 2]>,
}

pub struct Shared {
    pub rt: Runtime,
    pub unity: Unity,
    main: Mutex<MainState>,
    inbox: Mutex<Vec<Command>>,
    outbox: mpsc::Sender<AgentMessage>,
    pending_logs: Mutex<VecDeque<AgentMessage>>,
    pub connected: AtomicBool,
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
    pub fn new(rt: Runtime, unity: Unity, provider: Option<Box<dyn Provider>>, outbox: mpsc::Sender<AgentMessage>) -> Self {
        let game = provider.as_ref().map(|p| p.game_id());
        let capabilities = provider.as_ref().map(|p| p.capabilities()).unwrap_or(Capabilities { hotkeys: true, ..Default::default() });
        Self {
            rt,
            unity,
            main: Mutex::new(MainState {
                provider,
                hotkeys: Hotkeys::default(),
                last_frame: None,
                frame: 0,
                min_interval: interval_for(DEFAULT_MAX_HZ),
                last_sample_at: None,
                last_sent_at: None,
                last_sent: None,
                export: None,
                solid_masks: None,
            }),
            inbox: Mutex::new(Vec::new()),
            outbox,
            pending_logs: Mutex::new(VecDeque::new()),
            connected: AtomicBool::new(false),
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

    pub fn push_command(&self, cmd: Command) {
        self.inbox.lock().unwrap().push(cmd);
    }

    /// Called by the pipe thread when an app connects/disconnects.
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::SeqCst);
        if connected {
            self.resend.store(true, Ordering::SeqCst);
            let pending: Vec<_> = self.pending_logs.lock().unwrap().drain(..).collect();
            for msg in pending {
                let _ = self.outbox.try_send(msg);
            }
        }
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        let msg = AgentMessage::Log { level, message: message.into() };
        if self.connected.load(Ordering::Relaxed) {
            let _ = self.outbox.try_send(msg); // channel full: drop rather than block
            return;
        }
        let mut pending = self.pending_logs.lock().unwrap();
        if pending.len() == PENDING_LOG_CAP {
            pending.pop_front();
        }
        pending.push_back(msg);
    }

    fn send(&self, msg: AgentMessage) {
        let _ = self.outbox.try_send(msg);
    }

    /// Runs `f` with the game's physics, using the solid layers for the layer the player
    /// is in. Main thread only.
    fn with_world<T>(&self, st: &mut MainState, cx: &Cx, f: impl FnOnce(&crate::walk::PhysicsWorld) -> T) -> Result<T, String> {
        let physics = self.unity.physics().ok_or("this game has no physics queries")?;
        let local = st.last_sent.as_ref().and_then(|s| s.local.as_ref()).ok_or("not in a world")?;
        let underground = local.layer == WorldLayer::Caves;
        let masks = match st.solid_masks {
            Some(m) => m,
            None => {
                let m = match st.provider.as_mut() {
                    Some(p) => unsafe { [p.solid_mask(cx, physics, false), p.solid_mask(cx, physics, true)] },
                    None => unsafe { [default_solid_mask(cx, physics); 2] },
                };
                let layers = unsafe { physics.layer_names(cx.rt) };
                self.log(LogLevel::Info, format!("walk checks: solid masks {:#010x}/{:#010x}, layers {layers:?}", m[0] as u32, m[1] as u32));
                st.solid_masks = Some(m);
                m
            }
        };
        let hits = unsafe { physics.hit_buffer(cx.rt, PROBE_HITS) }.map_err(|e| e.to_string())?;
        let world = crate::walk::PhysicsWorld { rt: cx.rt, physics, mask: masks[underground as usize], hits };
        Ok(f(&world))
    }

    /// Runs once per game frame on the main thread (from the frame hook).
    pub fn tick(&self) {
        if self.stopping.load(Ordering::Relaxed) {
            return;
        }
        let mut st = match self.main.try_lock() {
            Ok(st) => st,
            // A tick panicked with the lock held (see `hook::run_tick`): carry on.
            Err(std::sync::TryLockError::Poisoned(e)) => {
                self.main.clear_poison();
                e.into_inner()
            }
            Err(std::sync::TryLockError::WouldBlock) => return,
        };

        // The hooked method may be shared by several components (identical code
        // folding) or run several times per frame; only do work once per frame.
        let fc = self.unity.frame_count(&self.rt).ok();
        if fc.is_some() && fc == st.last_frame {
            return;
        }
        st.last_frame = fc;
        st.frame += 1;

        let commands = std::mem::take(&mut *self.inbox.lock().unwrap());
        let cx = Cx { rt: &self.rt, unity: &self.unity };
        let st = &mut *st;

        for cmd in commands {
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
                        self.log(LogLevel::Warn, format!("teleport failed: {e}"));
                    }
                    self.send(AgentMessage::TeleportResult { request_id, ok: result.is_ok(), error: result.err() });
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
                    self.send(AgentMessage::PathResult { request_id, path, error });
                }
                Command::CheckRoute { request_id, points } => {
                    let started = Instant::now();
                    let result = self.with_world(st, &cx, |world| {
                        let points: Vec<_> = points.iter().take(MAX_CHECK_POINTS).map(|&p| to_unity(p)).collect();
                        crate::walk::check_route(world, &points, ROUTE_CHECK_BUDGET)
                    });
                    if let Ok((check, blocks)) = &result {
                        if !blocks.is_empty() {
                            let at: Vec<String> = blocks.iter().map(|b| format!("{:?} at ({:.1}, {:.1}, {:.1})", b.blocker, b.at.x, b.at.y, b.at.z)).collect();
                            let ms = started.elapsed().as_secs_f32() * 1000.0;
                            self.log(LogLevel::Info, format!("route check: {} of {} legs in {ms:.1} ms; blocked: {}", check.checked, points.len().saturating_sub(1), at.join(", ")));
                        }
                    }
                    let (check, error) = split(result.map(|(check, _)| check));
                    self.send(AgentMessage::RouteCheckResult { request_id, check, error });
                }
                Command::ProbeGround { request_id, points } => {
                    let camera = unsafe { self.unity.main_camera(&self.rt) }.map(|c| c.position);
                    let result = self.with_world(st, &cx, |world| {
                        let points: Vec<_> = points.iter().take(MAX_PROBE_POINTS).map(|&p| to_unity(p)).collect();
                        crate::walk::probe_ground(world, &points, camera)
                    });
                    let (points, error) = split(result);
                    self.send(AgentMessage::GroundProbeResult { request_id, points, error });
                }
                Command::CompanionOrders { request_id, companion } => {
                    let result = match st.provider.as_mut() {
                        Some(p) => unsafe { p.companion_orders(&cx, &companion) },
                        None => Err("this game has no companions".into()),
                    };
                    let (orders, error) = split(result);
                    self.send(AgentMessage::CompanionOrdersResult { request_id, orders, error });
                }
                Command::CompanionOrder { request_id, companion, order } => {
                    let result = match st.provider.as_mut() {
                        Some(p) => unsafe { p.companion_order(&cx, &companion, &order) },
                        None => Err("this game has no companions".into()),
                    };
                    match &result {
                        Ok(()) => self.log(LogLevel::Info, format!("{companion}: order {order} given")),
                        Err(e) => self.log(LogLevel::Warn, format!("{companion}: order {order} failed: {e}")),
                    }
                    self.send(AgentMessage::CompanionOrderResult { request_id, ok: result.is_ok(), error: result.err() });
                }
                Command::ExportWorld { request_id } => {
                    if st.export.is_some() {
                        self.send(AgentMessage::ExportResult { request_id, export: None, error: Some("an export is already running".into()) });
                        continue;
                    }
                    let started = match st.provider.as_mut() {
                        Some(p) => unsafe { p.begin_export(&cx) },
                        None => Err("this game has no export support".into()),
                    };
                    match started {
                        Ok(()) => st.export = Some((request_id, Instant::now())),
                        Err(e) => self.send(AgentMessage::ExportResult { request_id, export: None, error: Some(e) }),
                    }
                }
            }
        }

        if let (Some((request_id, started)), Some(p)) = (st.export, st.provider.as_mut()) {
            if let Some(result) = unsafe { p.poll_export(&cx) } {
                st.export = None;
                match result {
                    Ok(export) => {
                        self.log(
                            LogLevel::Info,
                            format!("world export: {} entries, {} caves in {:.1} s", export.entries.len(), export.caves.len(), started.elapsed().as_secs_f32()),
                        );
                        self.send(AgentMessage::ExportResult { request_id, export: Some(export), error: None });
                    }
                    Err(e) => self.send(AgentMessage::ExportResult { request_id, export: None, error: Some(e) }),
                }
            }
        }

        if let Some(p) = st.provider.as_mut() {
            for note in p.take_notes() {
                self.log(LogLevel::Info, note);
            }
        }

        if !self.connected.load(Ordering::Relaxed) {
            return;
        }

        // Hotkeys: only while the game window has focus and isn't capturing text.
        let blocked = !game_has_focus() || st.provider.as_mut().is_some_and(|p| unsafe { p.input_blocked(&cx) });
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
                self.send(AgentMessage::HotkeyPressed { action });
            }
        }

        // World snapshot, rate-limited, sent only on meaningful change.
        let Some(provider) = st.provider.as_mut() else { return };
        let now = Instant::now();
        if st.last_sample_at.is_some_and(|t| now - t < st.min_interval) {
            return;
        }
        st.last_sample_at = Some(now);
        if self.resend.load(Ordering::SeqCst) {
            provider.reconnected();
        }
        let mut snap = unsafe { provider.snapshot(&cx, st.frame) };
        if let Some(local) = snap.local.as_mut() {
            local.camera = unsafe { self.unity.main_camera(&self.rt) }.map(|c| CameraView {
                position: to_proto(c.position),
                forward: to_proto(c.rotation.forward()),
                up: to_proto(c.rotation.up()),
                fov_y: c.fov_y,
                aspect: c.aspect,
            });
        }
        let resend = self.resend.swap(false, Ordering::SeqCst);
        let heartbeat = st.last_sent_at.is_none_or(|t| now - t >= HEARTBEAT);
        match st.last_sent.as_mut() {
            Some(prev) if !resend && !world_changed(prev, &snap) => {
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
                    };
                    if self.outbox.try_send(AgentMessage::SnapshotUpdate(Box::new(update.clone()))).is_ok() {
                        st.last_sent_at = Some(now);
                        update.apply(prev);
                    }
                }
            }
            _ => {
                if self.outbox.try_send(AgentMessage::Snapshot(Box::new(snap.clone()))).is_ok() {
                    st.last_sent_at = Some(now);
                    st.last_sent = Some(snap);
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
