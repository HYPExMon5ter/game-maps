//! Game providers: per-game knowledge of where the player, other players, and the
//! teleport routine live. Everything game-specific in the agent goes here.

mod sotf;

use game_maps_protocol::{Capabilities, TeleportTarget, Vec3, WorldExport, WorldSnapshot};
use il2cpp::{Runtime, Unity, Vector3};

use crate::hook::HookTarget;

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
    sotf::SonsOfTheForest::detect(rt).map(|p| Box::new(p) as Box<dyn Provider>)
}

pub(crate) fn to_proto(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub(crate) fn to_unity(v: Vec3) -> Vector3 {
    Vector3 { x: v.x, y: v.y, z: v.z }
}
