//! Wire protocol between the overlay app and the agent injected into a game.
//!
//! Transport: a byte stream (named pipe) carrying newline-delimited JSON. Each line
//! is one [`AgentMessage`] (agent → app) or [`AppMessage`] (app → agent). JSON
//! keeps it debuggable; at ≤30 small messages per second the encoding cost is
//! negligible next to the frame budget.
//!
//! Coordinates are always the game's raw world coordinates. Mapping onto the map
//! happens in the app, so the agent stays free of per-map knowledge.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Bumped on any incompatible change. Both sides refuse to talk across versions.
pub const PROTOCOL_VERSION: u32 = 2;

/// Name of the pipe an agent serves for a given game process id.
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\game-maps-agent-{pid}")
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn distance_sq(self, other: Vec3) -> f32 {
        let (dx, dy, dz) = (self.x - other.x, self.y - other.y, self.z - other.z);
        dx * dx + dy * dy + dz * dz
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// Which world layer the player is in. Games without layers always report `Surface`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldLayer {
    #[default]
    Surface,
    Caves,
    /// Areas that aren't on any map (menus, cutscenes, end-game locations).
    Other,
}

/// Feature flags an agent reports for the attached game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub live_location: bool,
    pub teleport: bool,
    pub other_players: bool,
    pub hotkeys: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalPlayer {
    pub position: Vec3,
    /// Camera facing direction (unit-ish vector), if the game exposes it.
    pub forward: Option<Vec3>,
    pub layer: WorldLayer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemotePlayer {
    /// Stable id for the session (network entity id or similar).
    pub id: String,
    pub name: String,
    /// sRGB color the game assigns the player, if any.
    pub color: Option<[u8; 3]>,
    pub position: Vec3,
    pub forward: Option<Vec3>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// An AI companion the game tracks (e.g. Kelvin in Sons of the Forest).
    Companion,
    /// Any other marker the game's own map/GPS shows (placed locators, vehicles, …).
    Locator,
}

/// A live marker from the game's own map or GPS, so the overlay shows everything the
/// in-game map does, including things that move or are placed during play.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Stable for the marker's lifetime.
    pub id: String,
    pub kind: MarkerKind,
    /// Display name, when the game provides or implies one.
    pub label: Option<String>,
    /// Game icon name (matches `games/<id>/icons/game/<icon>.png`).
    pub icon: Option<String>,
    pub position: Vec3,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorldSnapshot {
    /// Game frame number the snapshot was taken on.
    pub frame: u64,
    /// `None` while not in a world (main menu, loading).
    pub local: Option<LocalPlayer>,
    pub others: Vec<RemotePlayer>,
    #[serde(default)]
    pub markers: Vec<Marker>,
}

/// What a hotkey does. Kept as an enum so new actions stay type-checked on both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    ToggleMap,
    /// Zoom the mini-map in or out (the full map handles its own keys while focused).
    ZoomIn,
    ZoomOut,
}

/// A key binding using Windows virtual-key codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub action: HotkeyAction,
    pub vk: u16,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Where to teleport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeleportTarget {
    /// World position. When `snap_to_ground` is set, the agent resolves the height
    /// itself (terrain/raycast), so the map can teleport to pins without a known Y.
    Position { position: Vec3, snap_to_ground: bool },
    /// Another player's or a companion marker's current position.
    Player { id: String },
}

/// One object found by a world export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportEntry {
    /// Provider-defined kind, e.g. `pickup` or `gps_locator`.
    pub kind: String,
    pub name: String,
    pub position: Vec3,
    /// Whether the object was active in the scene when exported.
    pub active: bool,
    /// Provider-defined details (item id, item type, terrain height, …).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, String>,
}

/// Floor heights of one cave, sampled on a regular grid seen from above (dev tooling
/// renders them into the underground map layer).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CaveScan {
    /// Game-defined id of the cave (an area bit in Sons of the Forest).
    pub area: i32,
    /// World position of the grid's first cell corner (east, north axes).
    pub origin_x: f32,
    pub origin_z: f32,
    /// Cell edge length in world units.
    pub cell: f32,
    pub width: u32,
    pub height: u32,
    /// Floors found per cell, highest first, as `layers` planes of `width * height`
    /// little-endian i16 decimeters (`i16::MIN` = none), base64-encoded.
    pub layers: u32,
    pub floors: String,
    /// Free height above each floor in decimeters (same layout), capped by the scan.
    pub clearance: String,
}

/// Snapshot of the world's static content (items, points of interest), used by dev
/// tooling to build a game's pin data.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WorldExport {
    pub game: String,
    pub entries: Vec<ExportEntry>,
    /// Diagnostics: counts per scanned type, members that failed to resolve, …
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caves: Vec<CaveScan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Hello {
        protocol_version: u32,
        agent_version: String,
        /// Game id (matches `games/<id>/game.json`), if a provider recognized the game.
        game: Option<String>,
        capabilities: Capabilities,
        /// File name the agent was loaded from; lets the app detect and replace an
        /// outdated agent that is still loaded in a running game.
        #[serde(default)]
        module: Option<String>,
    },
    Snapshot(WorldSnapshot),
    HotkeyPressed { action: HotkeyAction },
    TeleportResult { request_id: u32, ok: bool, error: Option<String> },
    Log { level: LogLevel, message: String },
    ExportResult { request_id: u32, export: Option<WorldExport>, error: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppMessage {
    Hello { protocol_version: u32 },
    SetHotkeys { hotkeys: Vec<Hotkey> },
    /// Upper bound on snapshot rate. The agent only sends when something changed.
    SetStreamRate { max_hz: u32 },
    Teleport { request_id: u32, target: TeleportTarget },
    /// Scan the loaded world for items and points of interest (slow; dev tooling).
    ExportWorld { request_id: u32 },
    /// Unhook and unload the agent from the game process.
    Detach,
}

/// Encodes one message as a single JSON line (with trailing `\n`).
pub fn encode_line<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut buf = serde_json::to_vec(msg).expect("protocol messages always serialize");
    buf.push(b'\n');
    buf
}

/// Decodes one JSON line (trailing whitespace allowed).
pub fn decode_line<'a, T: Deserialize<'a>>(line: &'a [u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(line.trim_ascii_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_messages_round_trip() {
        let msgs = vec![
            AgentMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                agent_version: "0.1.0".into(),
                game: Some("sons-of-the-forest".into()),
                capabilities: Capabilities { live_location: true, teleport: true, other_players: true, hotkeys: true },
                module: Some("game_maps_agent-0123.dll".into()),
            },
            AgentMessage::Snapshot(WorldSnapshot {
                frame: 42,
                local: Some(LocalPlayer {
                    position: Vec3::new(1.0, 2.0, 3.0),
                    forward: Some(Vec3::new(0.0, 0.0, 1.0)),
                    layer: WorldLayer::Caves,
                }),
                others: vec![RemotePlayer {
                    id: "7".into(),
                    name: "Kelvin".into(),
                    color: Some([255, 0, 128]),
                    position: Vec3::new(-5.0, 0.5, 10.0),
                    forward: None,
                }],
                markers: vec![Marker {
                    id: "m1".into(),
                    kind: MarkerKind::Companion,
                    label: Some("Kelvin".into()),
                    icon: Some("RobbyLogo".into()),
                    position: Vec3::new(3.0, 4.0, 5.0),
                }],
            }),
            AgentMessage::HotkeyPressed { action: HotkeyAction::ToggleMap },
            AgentMessage::HotkeyPressed { action: HotkeyAction::ZoomOut },
            AgentMessage::TeleportResult { request_id: 3, ok: false, error: Some("not in world".into()) },
            AgentMessage::Log { level: LogLevel::Warn, message: "x".into() },
            AgentMessage::ExportResult {
                request_id: 4,
                export: Some(WorldExport {
                    game: "g".into(),
                    entries: vec![ExportEntry {
                        kind: "pickup".into(),
                        name: "Rope".into(),
                        position: Vec3::new(1.0, 2.0, 3.0),
                        active: true,
                        props: BTreeMap::from([("item_id".into(), "403".into())]),
                    }],
                    notes: vec![],
                    caves: vec![],
                }),
                error: None,
            },
        ];
        for msg in msgs {
            let line = encode_line(&msg);
            assert_eq!(line.last(), Some(&b'\n'));
            assert!(!line[..line.len() - 1].contains(&b'\n'));
            let back: AgentMessage = decode_line(&line).unwrap();
            assert_eq!(back, msg);
        }
    }

    #[test]
    fn app_messages_round_trip() {
        let msgs = vec![
            AppMessage::Hello { protocol_version: PROTOCOL_VERSION },
            AppMessage::SetHotkeys {
                hotkeys: vec![
                    Hotkey { action: HotkeyAction::ToggleMap, vk: 0x4D, ctrl: false, shift: false, alt: false },
                    Hotkey { action: HotkeyAction::ZoomIn, vk: 0x6B, ctrl: false, shift: false, alt: false },
                ],
            },
            AppMessage::SetStreamRate { max_hz: 30 },
            AppMessage::Teleport {
                request_id: 1,
                target: TeleportTarget::Position { position: Vec3::new(1.0, 0.0, 2.0), snap_to_ground: true },
            },
            AppMessage::Teleport { request_id: 2, target: TeleportTarget::Player { id: "7".into() } },
            AppMessage::ExportWorld { request_id: 5 },
            AppMessage::Detach,
        ];
        for msg in msgs {
            let back: AppMessage = decode_line(&encode_line(&msg)).unwrap();
            assert_eq!(back, msg);
        }
    }

    #[test]
    fn wire_format_is_tagged_snake_case() {
        let line = encode_line(&AppMessage::SetStreamRate { max_hz: 20 });
        assert_eq!(std::str::from_utf8(&line).unwrap(), "{\"type\":\"set_stream_rate\",\"max_hz\":20}\n");
    }

    #[test]
    fn pipe_name_is_per_process() {
        assert_eq!(pipe_name(1234), r"\\.\pipe\game-maps-agent-1234");
    }
}
