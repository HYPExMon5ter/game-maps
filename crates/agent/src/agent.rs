//! Agent state shared between the game's main thread (frame hook) and the pipe thread.
//!
//! The main thread never blocks: it uses `try_lock` and non-blocking channel sends, and
//! does no work at all while no app is connected.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use game_maps_protocol::{AgentMessage, Capabilities, Hotkey, LogLevel, TeleportTarget, WorldSnapshot, PROTOCOL_VERSION};
use il2cpp::{Runtime, Unity};
use tokio::sync::mpsc;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, GetForegroundWindow, GetWindowThreadProcessId, ASFW_ANY};

use crate::diff::has_meaningful_change;
use crate::hotkeys::{Hotkeys, KeyState};
use crate::provider::{Cx, Provider};

const DEFAULT_MAX_HZ: u32 = 30;
const HEARTBEAT: Duration = Duration::from_secs(1);
const PENDING_LOG_CAP: usize = 200;

pub enum Command {
    SetHotkeys(Vec<Hotkey>),
    SetStreamRate(u32),
    Teleport { request_id: u32, target: TeleportTarget },
    ExportWorld { request_id: u32 },
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
}

pub struct Shared {
    pub rt: Runtime,
    unity: Unity,
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

    /// Runs once per game frame on the main thread (from the frame hook).
    pub fn tick(&self) {
        if self.stopping.load(Ordering::Relaxed) {
            return;
        }
        let Ok(mut st) = self.main.try_lock() else { return };

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
                Command::SetHotkeys(h) => st.hotkeys.set(h),
                Command::SetStreamRate(hz) => st.min_interval = interval_for(hz),
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
                unsafe {
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
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
        let snap = unsafe { provider.snapshot(&cx, st.frame) };
        let force = self.resend.swap(false, Ordering::SeqCst) || st.last_sent_at.is_none_or(|t| now - t >= HEARTBEAT);
        let changed = st.last_sent.as_ref().is_none_or(|prev| has_meaningful_change(prev, &snap));
        if (force || changed) && self.outbox.try_send(AgentMessage::Snapshot(snap.clone())).is_ok() {
            st.last_sent_at = Some(now);
            st.last_sent = Some(snap);
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
