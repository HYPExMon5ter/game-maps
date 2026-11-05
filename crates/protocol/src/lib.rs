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
pub const PROTOCOL_VERSION: u32 = 4;

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
    /// The game's main camera, for drawing waypoints over the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraView>,
}

/// Where the game's camera is and what it sees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameraView {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    /// Vertical field of view, degrees.
    pub fov_y: f32,
    /// Width / height of the camera's image.
    pub aspect: f32,
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
    /// Creatures the game has active around the player (its own loading distance).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creatures: Vec<Creature>,
    /// The multiplayer session the player is in; `None` in single player or when the
    /// game doesn't expose one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<MultiplayerSession>,
    /// The world or save that's loaded; `None` in menus or when the game doesn't expose it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<GameWorld>,
    /// AI companions the game reports details for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companions: Vec<CompanionStatus>,
    /// Time, season and weather, for games that expose them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    /// Where the player's things are after dying, while they're on the ground, in games
    /// that drop them somewhere of their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_bag: Option<Vec3>,
    /// The player is dying or dead right now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dying: bool,
}

/// What changed since the last [`WorldSnapshot`]. Sent in its place: looking around would
/// otherwise send every marker 30 times a second. The two big lists only come when they
/// changed; the rest is small.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotUpdate {
    pub frame: u64,
    pub local: Option<LocalPlayer>,
    pub others: Vec<RemotePlayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markers: Option<Vec<Marker>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creatures: Option<Vec<Creature>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companions: Vec<CompanionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_bag: Option<Vec3>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dying: bool,
}

impl SnapshotUpdate {
    /// Applies this to `snapshot`, leaving lists that didn't change as they were.
    pub fn apply(self, snapshot: &mut WorldSnapshot) {
        snapshot.frame = self.frame;
        snapshot.local = self.local;
        snapshot.others = self.others;
        snapshot.companions = self.companions;
        snapshot.environment = self.environment;
        snapshot.dropped_bag = self.dropped_bag;
        snapshot.dying = self.dying;
        if let Some(markers) = self.markers {
            snapshot.markers = markers;
        }
        if let Some(creatures) = self.creatures {
            snapshot.creatures = creatures;
        }
    }
}

/// One of a companion's stats, as the game names it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanionStat {
    pub name: String,
    pub value: f32,
    pub max: f32,
}

/// What's known about an AI companion right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanionStatus {
    /// Stable id for the companion in this game.
    pub id: String,
    pub name: String,
    pub position: Option<Vec3>,
    pub alive: bool,
    pub injured: bool,
    /// What they're doing, when the game says.
    pub activity: Option<String>,
    pub stats: Vec<CompanionStat>,
}

/// Something a companion can be told to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionOrderOption {
    /// Sent back with `AppMessage::CompanionOrder`; only the agent interprets it.
    pub id: String,
    /// Heading it's listed under.
    pub group: String,
    pub label: String,
}

/// Time, season and weather in the world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    /// Days survived, when the game counts them.
    pub day: Option<u32>,
    pub hour: u8,
    pub minute: u8,
    pub season: Option<String>,
    pub weather: Option<String>,
}

/// A world or save, so the app can keep found items, pins and explored areas apart
/// for each one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameWorld {
    /// Stable for this world across sessions (e.g. the save's id), different for other
    /// worlds of the same game. Unique within the game, not across games.
    pub id: String,
    /// The world's name as the game shows it, if it has one.
    pub name: Option<String>,
    pub kind: WorldKind,
}

/// How the player is in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldKind {
    /// Their own world, alone.
    SinglePlayer,
    /// Their own world, with others joining.
    Hosted,
    /// Someone else's world.
    Joined,
}

/// A multiplayer session, as every player in it sees it: lets copies of the app in the
/// same game find each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiplayerSession {
    /// The same for everyone in the session (e.g. the Steam lobby), different elsewhere.
    pub id: String,
    /// This player's name as the others see it.
    pub player_name: Option<String>,
}

/// What a creature is to the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreatureKind {
    /// Attacks players (cannibals, mutants).
    Hostile,
    /// Wildlife.
    Animal,
    /// On the player's side (companions).
    Friendly,
}

/// A creature near the player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Creature {
    /// Stable while the creature is around.
    pub id: String,
    pub kind: CreatureKind,
    /// What it is, as the game names it (e.g. "Fingers", "Deer").
    pub name: String,
    pub position: Vec3,
}

/// What a hotkey does. Kept as an enum so new actions stay type-checked on both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    ToggleMap,
    /// Zoom the mini-map in or out (the full map handles its own keys while focused).
    ZoomIn,
    ZoomOut,
    /// Return to where the player was before their last teleport.
    TeleportBack,
    /// Drop a pin where the player stands.
    QuickMark,
    /// Track the closest thing not found yet.
    NearestUnfound,
    /// Accept the latest track request from another player.
    AcceptRequest,
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
    /// `layer` is what's at that position: a surface spot can be within a cave's bounds,
    /// and only the app knows which it is. Snapping always means the surface.
    Position {
        position: Vec3,
        snap_to_ground: bool,
        #[serde(default)]
        layer: WorldLayer,
    },
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
    Snapshot(Box<WorldSnapshot>),
    /// Changes to the last [`AgentMessage::Snapshot`].
    SnapshotUpdate(Box<SnapshotUpdate>),
    HotkeyPressed { action: HotkeyAction },
    TeleportResult { request_id: u32, ok: bool, error: Option<String> },
    Log { level: LogLevel, message: String },
    ExportResult { request_id: u32, export: Option<WorldExport>, error: Option<String> },
    PathResult { request_id: u32, path: Option<PathInfo>, error: Option<String> },
    RouteCheckResult { request_id: u32, check: Option<RouteCheck>, error: Option<String> },
    GroundProbeResult { request_id: u32, points: Option<Vec<GroundPoint>>, error: Option<String> },
    CompanionOrdersResult { request_id: u32, orders: Option<Vec<CompanionOrderOption>>, error: Option<String> },
    CompanionOrderResult { request_id: u32, ok: bool, error: Option<String> },
}

/// Which legs of a route can't be walked right now (see `AppMessage::CheckRoute`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteCheck {
    /// Indices `i` of blocked legs, from `points[i]` to `points[i + 1]`.
    pub blocked: Vec<u32>,
    /// Legs checked, from the start: long routes are checked within a time budget.
    pub checked: u32,
}

/// The ground under a probed point (see `AppMessage::ProbeGround`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GroundPoint {
    /// Height of the floor nearest the point, if there's one close by.
    pub y: Option<f32>,
    /// Whether the camera can see that floor point (nothing solid in between).
    pub visible: bool,
}

/// A walking route (see `AppMessage::FindPath`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathInfo {
    /// Corner points from the player to the destination.
    pub corners: Vec<Vec3>,
    /// False when the destination can't be reached; the route ends as close as it gets.
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppMessage {
    Hello { protocol_version: u32 },
    SetHotkeys { hotkeys: Vec<Hotkey> },
    /// Upper bound on snapshot rate. The agent only sends when something changed.
    SetStreamRate { max_hz: u32 },
    /// The full map opened: a moment when a slow refresh won't be felt in the game.
    MapOpened,
    Teleport { request_id: u32, target: TeleportTarget },
    /// Scan the loaded world for items and points of interest (slow; dev tooling).
    ExportWorld { request_id: u32 },
    /// A walking route from the player to `to` on the game's navigation mesh.
    FindPath { request_id: u32, to: Vec3 },
    /// Checks, against the world as it is now, that a player can walk from each point
    /// to the next (points on the floor, about a meter apart).
    CheckRoute { request_id: u32, points: Vec<Vec3> },
    /// Snaps points near the player to the floor and tells which the camera can see.
    ProbeGround { request_id: u32, points: Vec<Vec3> },
    /// What `companion` can be told to do right now.
    CompanionOrders { request_id: u32, companion: String },
    /// Tells `companion` to do `order` (an id from `CompanionOrders`).
    CompanionOrder { request_id: u32, companion: String, order: String },
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
            AgentMessage::Snapshot(Box::new(WorldSnapshot {
                frame: 42,
                local: Some(LocalPlayer {
                    position: Vec3::new(1.0, 2.0, 3.0),
                    forward: Some(Vec3::new(0.0, 0.0, 1.0)),
                    layer: WorldLayer::Caves,
                    camera: None,
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
                creatures: vec![Creature { id: "actor:1".into(), kind: CreatureKind::Hostile, name: "Fingers".into(), position: Vec3::new(6.0, 7.0, 8.0) }],
                session: Some(MultiplayerSession { id: "lobby:1".into(), player_name: Some("A".into()) }),
                world: Some(GameWorld { id: "sp:42".into(), name: Some("Island".into()), kind: WorldKind::SinglePlayer }),
                companions: vec![CompanionStatus {
                    id: "kelvin".into(),
                    name: "Kelvin".into(),
                    position: Some(Vec3::new(1.0, 2.0, 3.0)),
                    alive: true,
                    injured: false,
                    activity: Some("Logs".into()),
                    stats: vec![CompanionStat { name: "Health".into(), value: 90.0, max: 100.0 }],
                }],
                environment: Some(Environment { day: Some(3), hour: 14, minute: 5, season: Some("Summer".into()), weather: None }),
                dropped_bag: Some(Vec3::new(7.0, 8.0, 9.0)),
                dying: true,
            })),
            AgentMessage::HotkeyPressed { action: HotkeyAction::ToggleMap },
            AgentMessage::HotkeyPressed { action: HotkeyAction::QuickMark },
            AgentMessage::CompanionOrdersResult {
                request_id: 8,
                orders: Some(vec![CompanionOrderOption { id: "follow".into(), group: "Basics".into(), label: "Follow me".into() }]),
                error: None,
            },
            AgentMessage::CompanionOrderResult { request_id: 9, ok: false, error: Some("Kelvin isn't around".into()) },
            AgentMessage::HotkeyPressed { action: HotkeyAction::ZoomOut },
            AgentMessage::TeleportResult { request_id: 3, ok: false, error: Some("not in world".into()) },
            AgentMessage::Log { level: LogLevel::Warn, message: "x".into() },
            AgentMessage::RouteCheckResult { request_id: 6, check: Some(RouteCheck { blocked: vec![2, 3], checked: 9 }), error: None },
            AgentMessage::GroundProbeResult {
                request_id: 7,
                points: Some(vec![GroundPoint { y: Some(1.5), visible: true }, GroundPoint { y: None, visible: false }]),
                error: None,
            },
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
            AppMessage::MapOpened,
            AppMessage::Teleport {
                request_id: 1,
                target: TeleportTarget::Position { position: Vec3::new(1.0, 0.0, 2.0), snap_to_ground: true, layer: WorldLayer::Caves },
            },
            AppMessage::Teleport { request_id: 2, target: TeleportTarget::Player { id: "7".into() } },
            AppMessage::ExportWorld { request_id: 5 },
            AppMessage::CheckRoute { request_id: 6, points: vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(2.0, 2.0, 3.0)] },
            AppMessage::ProbeGround { request_id: 7, points: vec![Vec3::new(1.0, 2.0, 3.0)] },
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
