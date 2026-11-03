//! `UnityEngine.Physics` queries (rays and sphere sweeps) against the game's colliders,
//! for checking what the world looks like right now.

use std::ffi::c_void;

use crate::{Class, Error, ManagedArray, Method, Result, Runtime, Vector3};

/// `UnityEngine.QueryTriggerInteraction.Ignore`: triggers aren't solid.
const IGNORE_TRIGGERS: i32 = 1;
/// Size of a `RaycastHit` buffer on the stack (the struct is 44 bytes).
const HIT_BYTES: usize = 64;
/// The built-in "Ignore Raycast" layer.
pub const IGNORE_RAYCAST_LAYER: i32 = 2;

/// Where a ray or sweep hit.
#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub point: Vector3,
    pub normal: Vector3,
    pub distance: f32,
}

/// Resolved `UnityEngine.Physics` members.
pub struct Physics {
    raycast: Method,
    raycast_all: Method,
    sphere_cast: Method,
    sync_transforms: Option<Method>,
    hit_class: Class,
    layer_to_name: Option<Method>,
    name_to_layer: Option<Method>,
    // `RaycastHit` field offsets in the unboxed struct.
    point: usize,
    normal: usize,
    distance: usize,
}

impl Physics {
    pub fn new(rt: &Runtime) -> Result<Self> {
        let physics = rt.class("UnityEngine", "Physics")?;
        let hit_class = rt.class("UnityEngine", "RaycastHit")?;
        let ray = |name: &str, hits: &str| {
            rt.method_with_params(
                physics,
                name,
                &["UnityEngine.Vector3", "UnityEngine.Vector3", hits, "System.Single", "System.Int32", "UnityEngine.QueryTriggerInteraction"],
            )
        };
        let layer_mask = rt.class("UnityEngine", "LayerMask").ok();
        // Field offsets include the object header of a boxed value; hits are read unboxed.
        let header = crate::layout::OBJECT_HEADER;
        let offset = |name: &str| rt.field(hit_class, name).map(|f| f.offset - header);
        Ok(Self {
            raycast: ray("Raycast", "UnityEngine.RaycastHit&")?,
            raycast_all: ray("RaycastNonAlloc", "UnityEngine.RaycastHit[]")?,
            sphere_cast: rt.method_with_params(
                physics,
                "SphereCast",
                &[
                    "UnityEngine.Vector3",
                    "System.Single",
                    "UnityEngine.Vector3",
                    "UnityEngine.RaycastHit&",
                    "System.Single",
                    "System.Int32",
                    "UnityEngine.QueryTriggerInteraction",
                ],
            )?,
            sync_transforms: rt.method(physics, "SyncTransforms", 0).ok(),
            hit_class,
            layer_to_name: layer_mask.and_then(|c| rt.method_with_params(c, "LayerToName", &["System.Int32"]).ok()),
            name_to_layer: layer_mask.and_then(|c| rt.method_with_params(c, "NameToLayer", &["System.String"]).ok()),
            point: offset("m_Point")?,
            normal: offset("m_Normal")?,
            distance: offset("m_Distance")?,
        })
    }

    /// The layer index named `name`, if the game defines one.
    ///
    /// # Safety
    /// Must run on an attached thread.
    pub unsafe fn layer(&self, rt: &Runtime, name: &str) -> Option<i32> {
        let m = self.name_to_layer.as_ref()?;
        let mut args = [rt.new_string(name) as *mut c_void];
        rt.unbox::<i32>(rt.invoke(m, std::ptr::null_mut(), &mut args).ok()?).ok().filter(|l| (0..32).contains(l))
    }

    /// Names of the game's layers, as `(index, name)` (diagnostics).
    ///
    /// # Safety
    /// Must run on an attached thread.
    pub unsafe fn layer_names(&self, rt: &Runtime) -> Vec<(i32, String)> {
        let Some(m) = self.layer_to_name.as_ref() else { return Vec::new() };
        (0..32)
            .filter_map(|mut i| {
                let mut args = [&mut i as *mut i32 as *mut c_void];
                let name = crate::read_string(rt.invoke(m, std::ptr::null_mut(), &mut args).ok()?)?;
                (!name.is_empty()).then_some((i, name))
            })
            .collect()
    }

    /// A layer mask of everything except the named layers (missing names are skipped)
    /// and "Ignore Raycast".
    ///
    /// # Safety
    /// Must run on an attached thread.
    pub unsafe fn mask_without(&self, rt: &Runtime, names: &[&str]) -> i32 {
        let excluded = names.iter().filter_map(|n| self.layer(rt, n)).fold(1 << IGNORE_RAYCAST_LAYER, |m, l| m | (1 << l));
        !excluded
    }

    /// A layer mask of just the named layers (missing names are skipped).
    ///
    /// # Safety
    /// Must run on an attached thread.
    pub unsafe fn mask_of(&self, rt: &Runtime, names: &[&str]) -> i32 {
        names.iter().filter_map(|n| self.layer(rt, n)).fold(0, |m, l| m | (1 << l))
    }

    /// Applies pending transform changes to the physics scene (after teleporting the
    /// player into a different area, say).
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn sync_transforms(&self, rt: &Runtime) {
        if let Some(m) = &self.sync_transforms {
            let _ = rt.invoke(m, std::ptr::null_mut(), &mut []);
        }
    }

    /// First solid hit along a ray.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn raycast(&self, rt: &Runtime, origin: Vector3, direction: Vector3, max_distance: f32, mask: i32) -> Option<RayHit> {
        let (mut origin, mut direction, mut dist, mut mask, mut triggers) = (origin, direction, max_distance, mask, IGNORE_TRIGGERS);
        let mut hit = [0u8; HIT_BYTES];
        let mut args = [
            &mut origin as *mut Vector3 as *mut c_void,
            &mut direction as *mut Vector3 as *mut c_void,
            hit.as_mut_ptr() as *mut c_void,
            &mut dist as *mut f32 as *mut c_void,
            &mut mask as *mut i32 as *mut c_void,
            &mut triggers as *mut i32 as *mut c_void,
        ];
        let found: bool = rt.unbox(rt.invoke(&self.raycast, std::ptr::null_mut(), &mut args).ok()?).ok()?;
        found.then(|| self.read_hit(hit.as_ptr()))
    }

    /// First solid hit when sweeping a sphere of `radius` from `origin`. Colliders the
    /// sphere already overlaps at `origin` aren't reported.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn sphere_cast(&self, rt: &Runtime, origin: Vector3, radius: f32, direction: Vector3, max_distance: f32, mask: i32) -> Option<RayHit> {
        let (mut origin, mut radius, mut direction, mut dist, mut mask, mut triggers) = (origin, radius, direction, max_distance, mask, IGNORE_TRIGGERS);
        let mut hit = [0u8; HIT_BYTES];
        let mut args = [
            &mut origin as *mut Vector3 as *mut c_void,
            &mut radius as *mut f32 as *mut c_void,
            &mut direction as *mut Vector3 as *mut c_void,
            hit.as_mut_ptr() as *mut c_void,
            &mut dist as *mut f32 as *mut c_void,
            &mut mask as *mut i32 as *mut c_void,
            &mut triggers as *mut i32 as *mut c_void,
        ];
        let found: bool = rt.unbox(rt.invoke(&self.sphere_cast, std::ptr::null_mut(), &mut args).ok()?).ok()?;
        found.then(|| self.read_hit(hit.as_ptr()))
    }

    /// A buffer for [`Physics::raycast_all`]. Only kept alive by references the GC can
    /// see, so don't keep it across frames.
    ///
    /// # Safety
    /// Must run on an attached thread.
    pub unsafe fn hit_buffer(&self, rt: &Runtime, len: usize) -> Result<ManagedArray> {
        rt.new_array(self.hit_class, len)
    }

    /// Every solid hit along a ray (unordered), up to the buffer's length.
    ///
    /// # Safety
    /// Main thread only; `hits` from [`Physics::hit_buffer`] this frame.
    pub unsafe fn raycast_all(&self, rt: &Runtime, origin: Vector3, direction: Vector3, max_distance: f32, mask: i32, hits: &ManagedArray) -> Result<Vec<RayHit>> {
        let (mut origin, mut direction, mut dist, mut mask, mut triggers) = (origin, direction, max_distance, mask, IGNORE_TRIGGERS);
        let mut args = [
            &mut origin as *mut Vector3 as *mut c_void,
            &mut direction as *mut Vector3 as *mut c_void,
            hits.object as *mut c_void,
            &mut dist as *mut f32 as *mut c_void,
            &mut mask as *mut i32 as *mut c_void,
            &mut triggers as *mut i32 as *mut c_void,
        ];
        let count: i32 = rt.unbox(rt.invoke(&self.raycast_all, std::ptr::null_mut(), &mut args)?)?;
        if count < 0 {
            return Err(Error::Null);
        }
        Ok((0..(count as usize).min(hits.len)).map(|i| self.read_hit(hits.element(i))).collect())
    }

    unsafe fn read_hit(&self, hit: *const u8) -> RayHit {
        RayHit {
            point: std::ptr::read_unaligned(hit.add(self.point) as *const Vector3),
            normal: std::ptr::read_unaligned(hit.add(self.normal) as *const Vector3),
            distance: std::ptr::read_unaligned(hit.add(self.distance) as *const f32),
        }
    }
}
