//! What the agent needs from a game engine, whichever it is: frames, the camera, physics
//! for route checks, and the context providers work in.
//!
//! Positions everywhere in the agent and the protocol are in Unity's convention: meters,
//! Y up, left-handed. Unity games use theirs as they are; Unreal positions are converted
//! where they're read (see `unreal::to_proto`), so the app and the route checks never
//! need to know which engine a game runs on.

use game_maps_protocol::{CameraView, PathInfo, Vec3};
use unity_runtime::{Runtime, Unity};

use crate::provider::{default_solid_mask, to_proto, to_unity, NAVMESH_SNAP};
use crate::walk::{PhysicsWorld, World};

/// Hits kept per ground probe ray.
const PROBE_HITS: usize = 8;

pub trait Engine: Send + Sync + Sized + 'static {
    /// What providers get for their main-thread work.
    type Cx<'a>;
    /// What route checks treat as solid.
    type Solid: Copy + Send + std::fmt::Debug;

    fn cx(&self) -> Self::Cx<'_>;

    /// The engine's frame number, so work runs once per frame however many hooks fire.
    fn frame_count(&self) -> Option<i64>;

    /// The camera the player sees through.
    ///
    /// # Safety
    /// Main thread only.
    unsafe fn camera(&self) -> Option<CameraView>;

    /// A walking route with the engine's own navigation, for games without their own.
    ///
    /// # Safety
    /// Main thread only.
    unsafe fn find_path(cx: &Self::Cx<'_>, from: Vec3, to: Vec3) -> Result<PathInfo, String>;

    /// What's solid for route checks when the game doesn't say.
    ///
    /// # Safety
    /// Main thread only.
    unsafe fn default_solid(cx: &Self::Cx<'_>) -> Result<Self::Solid, String>;

    /// Runs `f` with physics queries against what `solid` says is solid.
    ///
    /// # Safety
    /// Main thread only.
    unsafe fn with_world<T>(&self, solid: Self::Solid, f: &mut dyn FnMut(&dyn World) -> T) -> Result<T, String>;

    /// A line for the log about what's solid.
    ///
    /// # Safety
    /// Main thread only.
    unsafe fn describe_solid(&self, _solid: [Self::Solid; 2]) -> String {
        String::new()
    }
}

/// A Unity game: its scripting runtime and engine API.
pub struct UnityEngine {
    pub rt: Runtime,
    pub unity: Unity,
}

/// What Unity providers work with.
pub struct Cx<'a> {
    pub rt: &'a Runtime,
    pub unity: &'a Unity,
}

impl Engine for UnityEngine {
    type Cx<'a> = Cx<'a>;
    /// A layer mask.
    type Solid = i32;

    fn cx(&self) -> Cx<'_> {
        Cx { rt: &self.rt, unity: &self.unity }
    }

    fn frame_count(&self) -> Option<i64> {
        self.unity.frame_count(&self.rt).ok().map(i64::from)
    }

    unsafe fn camera(&self) -> Option<CameraView> {
        self.unity.main_camera(&self.rt).map(|c| CameraView {
            position: to_proto(c.position),
            forward: to_proto(c.rotation.forward()),
            up: to_proto(c.rotation.up()),
            fov_y: c.fov_y,
            aspect: c.aspect,
        })
    }

    unsafe fn find_path(cx: &Cx, from: Vec3, to: Vec3) -> Result<PathInfo, String> {
        cx.unity
            .find_path(cx.rt, to_unity(from), to_unity(to), NAVMESH_SNAP)
            .map(|p| PathInfo { corners: p.corners.into_iter().map(to_proto).collect(), complete: p.complete })
            .map_err(|e| e.to_string())
    }

    unsafe fn default_solid(cx: &Cx) -> Result<i32, String> {
        let physics = cx.unity.physics().ok_or("this game has no physics queries")?;
        Ok(default_solid_mask(cx, physics))
    }

    unsafe fn with_world<T>(&self, mask: i32, f: &mut dyn FnMut(&dyn World) -> T) -> Result<T, String> {
        let physics = self.unity.physics().ok_or("this game has no physics queries")?;
        let hits = physics.hit_buffer(&self.rt, PROBE_HITS).map_err(|e| e.to_string())?;
        let world = PhysicsWorld { rt: &self.rt, physics, mask, hits };
        Ok(f(&world))
    }

    unsafe fn describe_solid(&self, masks: [i32; 2]) -> String {
        let layers = self.unity.physics().map(|p| p.layer_names(&self.rt)).unwrap_or_default();
        format!("solid masks {:#010x}/{:#010x}, layers {layers:?}", masks[0] as u32, masks[1] as u32)
    }
}
