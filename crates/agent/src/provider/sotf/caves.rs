//! Cave systems (`Sons.Areas.CaveSystem`): which cave contains a point, switching the
//! game in or out of a cave, and a cave's extents.
//!
//! Cave interiors only get collision and lighting once the game considers the player
//! inside (normally via entrance triggers). Teleports skip those triggers, so they
//! force-enter the destination cave (and force-exit others) first.
//!
//! Cave system objects are looked up again for every operation instead of being kept
//! between frames: scene changes (e.g. quitting to the main menu) destroy them, and a
//! stale pointer could otherwise be read after the GC reclaimed it.

use std::ffi::c_void;

use unity_runtime::{Class, Field, ListLayout, ManagedObject, Method, Runtime, Unity, Vector3};

/// Axis-aligned bounds (min, max).
pub type Aabb = (Vector3, Vector3);

pub struct Caves {
    class: Class,
    inside_extents: Method,
    forced_enter: Method,
    forced_exit: Method,
    /// `GetAreaMask`: the `Sons.Areas.AreaMask` bit of a cave system.
    area_mask: Method,
    /// `CaveEntranceManager.get_CurrentAreaMask`: the caves the player is in.
    current_areas: Method,
    area: Field,
    extents: Option<Field>,
    collider: Option<Class>,
    collider_bounds: Option<Method>,
}

impl Caves {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let class = rt.class("Sons.Areas", "CaveSystem").ok()?;
        let collider = rt.class("UnityEngine", "Collider").ok();
        Some(Self {
            class,
            inside_extents: rt.method_with_params(class, "IsInsideExtents", &["UnityEngine.Vector3"]).ok()?,
            forced_enter: rt.method(class, "DoForcedEnter", 0).ok()?,
            forced_exit: rt.method(class, "DoForcedExit", 0).ok()?,
            area_mask: rt.method(class, "GetAreaMask", 0).ok()?,
            current_areas: rt.method(rt.class("Sons.Areas", "CaveEntranceManager").ok()?, "get_CurrentAreaMask", 0).ok()?,
            area: rt.field(class, "_areaId").ok()?,
            extents: rt.field(class, "_extents").ok(),
            collider,
            collider_bounds: collider.and_then(|c| rt.method(c, "get_bounds", 0).ok()),
        })
    }

    /// Cave systems in the loaded scenes. Valid for the current frame only.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn systems(&self, rt: &Runtime, unity: &Unity) -> Vec<*mut ManagedObject> {
        unity.find_objects(rt, self.class, true).unwrap_or_default()
    }

    /// The cave system among `systems` whose extents contain `p`.
    ///
    /// # Safety
    /// Main thread only; `systems` from [`Caves::systems`] this frame.
    pub unsafe fn containing(&self, rt: &Runtime, systems: &[*mut ManagedObject], p: Vector3) -> Option<*mut ManagedObject> {
        systems.iter().copied().find(|&s| {
            let mut point = p;
            let mut args = [&mut point as *mut Vector3 as *mut c_void];
            rt.invoke(&self.inside_extents, s, &mut args).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(false)
        })
    }

    /// Area id (`Sons.Areas.AreaMask` bit) of a cave system.
    ///
    /// # Safety
    /// `system` must be a live cave system.
    pub unsafe fn area(&self, system: *mut ManagedObject) -> i32 {
        self.area.get(system)
    }

    /// Makes the game treat the player as inside `target` (or outside every cave when
    /// `None`): force-exits every other active cave, then force-enters `target`.
    ///
    /// # Safety
    /// Main thread only; `systems` from [`Caves::systems`] this frame.
    pub unsafe fn activate(&self, rt: &Runtime, systems: &[*mut ManagedObject], target: Option<*mut ManagedObject>) -> Result<(), String> {
        let mask = |result: Result<*mut ManagedObject, unity_runtime::Error>| result.and_then(|b| rt.unbox::<i32>(b)).unwrap_or(0);
        let current = mask(rt.invoke(&self.current_areas, std::ptr::null_mut(), &mut []));
        let is_in = |s: *mut ManagedObject| current & mask(rt.invoke(&self.area_mask, s, &mut [])) != 0;
        for &s in systems {
            if Some(s) != target && is_in(s) {
                rt.invoke(&self.forced_exit, s, &mut []).map_err(|e| format!("leaving cave failed: {e}"))?;
            }
        }
        if let Some(s) = target {
            if !is_in(s) {
                rt.invoke(&self.forced_enter, s, &mut []).map_err(|e| format!("entering cave failed: {e}"))?;
            }
        }
        Ok(())
    }

    /// Union of the cave's extent volumes, when they are colliders.
    ///
    /// # Safety
    /// Main thread only; `system` must be a live cave system.
    pub unsafe fn extent_bounds(&self, rt: &Runtime, unity: &Unity, system: *mut ManagedObject) -> Option<Aabb> {
        let (field, collider, get_bounds) = (self.extents?, self.collider?, self.collider_bounds.as_ref()?);
        let list: *mut ManagedObject = field.get(system);
        let layout = ListLayout::of(rt, list).ok()?;
        let mut bounds: Option<Aabb> = None;
        for e in layout.elements(list, 4096) {
            if !unity.is_alive(e) || !rt.class_of(e).is_some_and(|c| rt.is_assignable(collider, c)) {
                continue;
            }
            // `UnityEngine.Bounds` is (center, extents).
            let Ok([cx, cy, cz, ex, ey, ez]) = rt.invoke(get_bounds, e, &mut []).and_then(|b| rt.unbox::<[f32; 6]>(b)) else { continue };
            let (lo, hi) = (Vector3 { x: cx - ex, y: cy - ey, z: cz - ez }, Vector3 { x: cx + ex, y: cy + ey, z: cz + ez });
            bounds = Some(match bounds {
                None => (lo, hi),
                Some((a, b)) => (
                    Vector3 { x: a.x.min(lo.x), y: a.y.min(lo.y), z: a.z.min(lo.z) },
                    Vector3 { x: b.x.max(hi.x), y: b.y.max(hi.y), z: b.z.max(hi.z) },
                ),
            });
        }
        bounds
    }
}
