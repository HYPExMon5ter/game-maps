//! Game providers: per-game knowledge of where the player, other players, and the
//! teleport routine live. Everything game-specific in the agent goes here.

mod sotf;
mod stranded_deep;
mod sunkenland;
mod valheim;

use game_maps_protocol::{Capabilities, CompanionOrderOption, GamePin, PathInfo, PlaceInfo, ZonePins, TeleportTarget, Vec3, WorldExport, WorldSnapshot};

/// How far (world units) route ends may be from Unity's NavMesh.
const NAVMESH_SNAP: f32 = 25.0;
use unity_runtime::{Physics, Runtime, Unity, Vector3};

use crate::hook::HookTarget;
use crate::worldmap::WorldMapCapture;

/// Per-call context for main-thread work.
pub struct Cx<'a> {
    pub rt: &'a Runtime,
    pub unity: &'a Unity,
}

pub trait Provider: Send {
    fn game_id(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;

    /// Game methods that run every frame, hooked in addition to the engine-level ones.
    fn frame_hooks(&self) -> &'static [HookTarget] {
        &[]
    }

    /// Drains pending human-readable diagnostics (which members resolved, first-scan
    /// results, …). Forwarded to the app's log.
    fn take_notes(&mut self) -> Vec<String>;

    /// The app connected. Snapshots stop while it's away, so objects held from before may
    /// be gone: the world can change meanwhile.
    fn reconnected(&mut self) {}

    /// The full map opened: searches too slow to repeat while playing can run now.
    fn map_opened(&mut self) {}

    /// Reads current world state. Main thread only; must be cheap.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn snapshot(&mut self, cx: &Cx, frame: u64) -> WorldSnapshot;

    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn teleport(&mut self, cx: &Cx, target: &TeleportTarget) -> Result<(), String>;

    /// Starts exporting the loaded world (items, points of interest, cave layouts) for
    /// dev tooling. The export may span many frames; see [`Provider::poll_export`].
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn begin_export(&mut self, _cx: &Cx) -> Result<(), String> {
        Err("world export is not supported for this game".into())
    }

    /// Advances a running export; `Some` once it has finished. Called every frame.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn poll_export(&mut self, _cx: &Cx) -> Option<Result<WorldExport, String>> {
        None
    }

    /// A walking route from `from` to `to`. The default uses Unity's NavMesh; games with
    /// their own pathfinding override it.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn find_path(&mut self, cx: &Cx, from: Vec3, to: Vec3) -> Result<PathInfo, String> {
        cx.unity
            .find_path(cx.rt, to_unity(from), to_unity(to), NAVMESH_SNAP)
            .map(|p| PathInfo { corners: p.corners.into_iter().map(to_proto).collect(), complete: p.complete })
            .map_err(|e| e.to_string())
    }

    /// Layers that are solid ground and walls for route checks and ground probes, on the
    /// surface or underground.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn solid_mask(&mut self, cx: &Cx, physics: &Physics, _underground: bool) -> i32 {
        default_solid_mask(cx, physics)
    }

    /// What `companion` can be told to do right now.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn companion_orders(&mut self, _cx: &Cx, _companion: &str) -> Result<Vec<CompanionOrderOption>, String> {
        Err("this game has no companion orders".into())
    }

    /// What the game says about the place at `at`, for a pin the player opened.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn place_info(&mut self, _cx: &Cx, _at: Vec3) -> Result<PlaceInfo, String> {
        Err("this game has nothing to say about places".into())
    }

    /// Tells `companion` to do `order` (an id from [`Provider::companion_orders`]).
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn companion_order(&mut self, _cx: &Cx, _companion: &str, _order: &str) -> Result<(), String> {
        Err("this game has no companion orders".into())
    }

    /// Reads the map of the loaded world, for games whose worlds are generated, so no
    /// map pack can have them. With `terrain` it includes the picture of the land. An
    /// error may only mean the game hasn't finished building the map: the app asks again.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn read_world_map(&mut self, _cx: &Cx, _terrain: bool) -> Result<WorldMapCapture, String> {
        Err("this game has no world maps".into())
    }

    /// Brings the game's own map in line with the player's pins (see
    /// `Capabilities::game_map`). It may have to wait for the map to be ready.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn set_game_pins(&mut self, _cx: &Cx, _pins: Vec<GamePin>) {}

    /// Pings `position` the way the game does, for everyone in the world.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn game_ping(&mut self, _cx: &Cx, _position: Vec3) -> Result<(), String> {
        Err("this game has no pings".into())
    }

    /// The app went away: undo what only makes sense while it's there.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn app_left(&mut self, _cx: &Cx) {}

    /// The player's options for this game (see `AppMessage::SetGameOptions`).
    fn set_options(&mut self, _options: &std::collections::BTreeMap<String, bool>) {}

    /// Called with each snapshot, for games that remember what's around the player: what
    /// changed in the next area, if anything. Must stay cheap however much is around.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn next_zone_pins(&mut self, _cx: &Cx) -> Option<ZonePins> {
        None
    }

    /// Whether the game is capturing text input (chat), so hotkeys must not fire.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    unsafe fn input_blocked(&mut self, _cx: &Cx) -> bool {
        false
    }
}

/// Picks the provider for the running game, if one exists. Metadata lookups only;
/// safe from any attached thread.
pub fn detect(rt: &Runtime) -> Option<Box<dyn Provider>> {
    sotf::SonsOfTheForest::detect(rt)
        .map(|p| Box::new(p) as Box<dyn Provider>)
        .or_else(|| sunkenland::Sunkenland::detect(rt).map(|p| Box::new(p) as Box<dyn Provider>))
        .or_else(|| valheim::Valheim::detect(rt).map(|p| Box::new(p) as Box<dyn Provider>))
        .or_else(|| stranded_deep::StrandedDeep::detect(rt).map(|p| Box::new(p) as Box<dyn Provider>))
}

/// Layers games commonly use for things that aren't the world's solid geometry.
const NON_SOLID_LAYERS: &[&str] = &[
    "Water", "UI", "TransparentFX", "Player", "LocalPlayer", "Character", "Characters", "Ragdoll", "Held", "Inventory", "PickUp", "Pickup", "Camera", "HUD",
];

/// Everything but layers commonly used for players, characters, items, water and UI.
///
/// # Safety
/// Must run on an attached thread.
pub(crate) unsafe fn default_solid_mask(cx: &Cx, physics: &Physics) -> i32 {
    physics.mask_without(cx.rt, NON_SOLID_LAYERS)
}

pub(crate) fn to_proto(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub(crate) fn to_unity(v: Vec3) -> Vector3 {
    Vector3 { x: v.x, y: v.y, z: v.z }
}
