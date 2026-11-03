//! Engine helpers shared by every Unity game (IL2CPP or Mono).
//!
//! Hot-path reads (positions, rotations, frame count) use engine internal calls, which
//! allocate nothing. Anything else goes through managed methods via
//! [`Runtime::invoke`], which catches managed exceptions.

use std::ffi::c_void;

use crate::{Class, Error, Field, ManagedObject, Method, Physics, Result, Runtime};

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternion {
    /// Rotates Unity's up axis (+Y) by this rotation.
    pub fn up(self) -> Vector3 {
        let Quaternion { x, y, z, w } = self;
        Vector3 {
            x: 2.0 * (x * y - w * z),
            y: 1.0 - 2.0 * (x * x + z * z),
            z: 2.0 * (y * z + w * x),
        }
    }

    /// Rotates Unity's forward axis (+Z) by this rotation.
    pub fn forward(self) -> Vector3 {
        let Quaternion { x, y, z, w } = self;
        Vector3 {
            x: 2.0 * (x * z + w * y),
            y: 2.0 * (y * z - w * x),
            z: 1.0 - 2.0 * (x * x + y * y),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn to_srgb8(self) -> [u8; 3] {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [c(self.r), c(self.g), c(self.b)]
    }
}

/// Upper bound on route corners read back (long routes have a few hundred).
const MAX_PATH_CORNERS: usize = 4096;

type GetVec3Injected = unsafe extern "C" fn(*mut ManagedObject, *mut Vector3);
type GetQuatInjected = unsafe extern "C" fn(*mut ManagedObject, *mut Quaternion);
type GetFrameCount = unsafe extern "C" fn() -> i32;

/// Resolved Unity engine members. Build once per runtime with [`Unity::new`].
pub struct Unity {
    cached_ptr: Field,
    frame_count: Option<GetFrameCount>,
    get_position_injected: Option<GetVec3Injected>,
    get_rotation_injected: Option<GetQuatInjected>,
    transform_get_position: Method,
    transform_get_rotation: Method,
    component_get_transform: Method,
    component_get_game_object: Method,
    game_object_get_transform: Method,
    game_object_get_component: Method,
    time_get_frame_count: Method,
    // Used by world exports only; optional so a missing overload never blocks startup.
    object_get_name: Option<Method>,
    find_objects_of_type: Option<Method>,
    game_object_active_in_hierarchy: Option<Method>,
    transform_get_parent: Option<Method>,
    camera: Option<CameraMembers>,
    nav: Option<NavMembers>,
    physics: Option<Physics>,
}

/// `UnityEngine.AI` members for [`Unity::find_path`].
struct NavMembers {
    path_class: Class,
    sample_position: Method,
    calculate_path: Method,
    corners: Method,
    status: Method,
}

/// `UnityEngine.AI.NavMeshHit` (position first).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NavMeshHit {
    position: Vector3,
    normal: Vector3,
    distance: f32,
    mask: i32,
    hit: i32,
}

/// `NavMesh.AllAreas`.
const ALL_AREAS: i32 = -1;

/// A walking route on the game's navigation mesh.
#[derive(Debug, Clone)]
pub struct NavPath {
    pub corners: Vec<Vector3>,
    /// False when the destination can't be reached and the route ends as close as it gets.
    pub complete: bool,
}

/// `UnityEngine.Camera` members for [`Unity::main_camera`].
struct CameraMembers {
    main: Method,
    fov: Method,
    aspect: Method,
}

/// The main camera's pose and lens.
#[derive(Debug, Clone, Copy)]
pub struct CameraPose {
    pub position: Vector3,
    pub rotation: Quaternion,
    /// Vertical field of view, degrees.
    pub fov_y: f32,
    pub aspect: f32,
}

impl Unity {
    pub fn new(rt: &Runtime) -> Result<Self> {
        let object = rt.class("UnityEngine", "Object")?;
        let transform = rt.class("UnityEngine", "Transform")?;
        let component = rt.class("UnityEngine", "Component")?;
        let game_object = rt.class("UnityEngine", "GameObject")?;
        let time = rt.class("UnityEngine", "Time")?;
        unsafe {
            Ok(Self {
                cached_ptr: rt.field(object, "m_CachedPtr")?,
                frame_count: rt.icall("UnityEngine.Time::get_frameCount()").ok(),
                get_position_injected: rt.icall("UnityEngine.Transform::get_position_Injected(UnityEngine.Vector3&)").ok(),
                get_rotation_injected: rt.icall("UnityEngine.Transform::get_rotation_Injected(UnityEngine.Quaternion&)").ok(),
                transform_get_position: rt.method(transform, "get_position", 0)?,
                transform_get_rotation: rt.method(transform, "get_rotation", 0)?,
                component_get_transform: rt.method(component, "get_transform", 0)?,
                component_get_game_object: rt.method(component, "get_gameObject", 0)?,
                game_object_get_transform: rt.method(game_object, "get_transform", 0)?,
                game_object_get_component: rt.method(game_object, "GetComponent", 1)?,
                time_get_frame_count: rt.method(time, "get_frameCount", 0)?,
                object_get_name: rt.method(object, "get_name", 0).ok(),
                find_objects_of_type: rt.method_with_params(object, "FindObjectsOfType", &["System.Type", "System.Boolean"]).ok(),
                game_object_active_in_hierarchy: rt.method(game_object, "get_activeInHierarchy", 0).ok(),
                transform_get_parent: rt.method(transform, "get_parent", 0).ok(),
                nav: (|| {
                    let nav = rt.class("UnityEngine.AI", "NavMesh").ok()?;
                    let path = rt.class("UnityEngine.AI", "NavMeshPath").ok()?;
                    Some(NavMembers {
                        path_class: path,
                        sample_position: rt
                            .method_with_params(nav, "SamplePosition", &["UnityEngine.Vector3", "UnityEngine.AI.NavMeshHit&", "System.Single", "System.Int32"])
                            .ok()?,
                        calculate_path: rt
                            .method_with_params(nav, "CalculatePath", &["UnityEngine.Vector3", "UnityEngine.Vector3", "System.Int32", "UnityEngine.AI.NavMeshPath"])
                            .ok()?,
                        corners: rt.method(path, "get_corners", 0).ok()?,
                        status: rt.method(path, "get_status", 0).ok()?,
                    })
                })(),
                physics: Physics::new(rt).ok(),
                camera: rt.class("UnityEngine", "Camera").ok().and_then(|c| {
                    Some(CameraMembers {
                        main: rt.method(c, "get_main", 0).ok()?,
                        fov: rt.method(c, "get_fieldOfView", 0).ok()?,
                        aspect: rt.method(c, "get_aspect", 0).ok()?,
                    })
                }),
            })
        }
    }

    /// Whether a `UnityEngine.Object` reference still points at a live native object.
    /// Destroyed objects keep their managed wrapper but lose the native pointer; calling
    /// engine methods on them throws, so check before every use.
    ///
    /// # Safety
    /// `obj` must be null or a managed object deriving from `UnityEngine.Object`.
    pub unsafe fn is_alive(&self, obj: *mut ManagedObject) -> bool {
        !obj.is_null() && !self.cached_ptr.get::<*mut c_void>(obj).is_null()
    }

    /// Current frame number. Main thread only.
    pub fn frame_count(&self, rt: &Runtime) -> Result<i32> {
        unsafe {
            match self.frame_count {
                Some(f) => Ok(f()),
                None => rt.unbox(rt.invoke(&self.time_get_frame_count, std::ptr::null_mut(), &mut [])?),
            }
        }
    }

    /// # Safety
    /// `transform` must be a `UnityEngine.Transform` (checked for liveness here). Main thread only.
    pub unsafe fn position(&self, rt: &Runtime, transform: *mut ManagedObject) -> Result<Vector3> {
        if !self.is_alive(transform) {
            return Err(Error::Null);
        }
        match self.get_position_injected {
            Some(f) => {
                let mut v = Vector3::default();
                f(transform, &mut v);
                Ok(v)
            }
            None => rt.unbox(rt.invoke(&self.transform_get_position, transform, &mut [])?),
        }
    }

    /// # Safety
    /// Same as [`Unity::position`].
    pub unsafe fn rotation(&self, rt: &Runtime, transform: *mut ManagedObject) -> Result<Quaternion> {
        if !self.is_alive(transform) {
            return Err(Error::Null);
        }
        match self.get_rotation_injected {
            Some(f) => {
                let mut q = Quaternion::default();
                f(transform, &mut q);
                Ok(q)
            }
            None => rt.unbox(rt.invoke(&self.transform_get_rotation, transform, &mut [])?),
        }
    }

    /// `component.transform`.
    ///
    /// # Safety
    /// `component` must be a `UnityEngine.Component`. Main thread only.
    pub unsafe fn component_transform(&self, rt: &Runtime, component: *mut ManagedObject) -> Result<*mut ManagedObject> {
        if !self.is_alive(component) {
            return Err(Error::Null);
        }
        non_null(rt.invoke(&self.component_get_transform, component, &mut [])?)
    }

    /// Physics queries, if the game ships the physics module.
    pub fn physics(&self) -> Option<&Physics> {
        self.physics.as_ref()
    }

    /// Whether the game has a navigation mesh API to route on.
    pub fn has_navigation(&self) -> bool {
        self.nav.is_some()
    }

    /// The nearest point on the navigation mesh within `max_distance` of `at`.
    unsafe fn nav_point(&self, rt: &Runtime, nav: &NavMembers, at: Vector3, max_distance: f32) -> Option<Vector3> {
        let (mut p, mut hit, mut dist, mut areas) = (at, NavMeshHit::default(), max_distance, ALL_AREAS);
        let mut args = [
            &mut p as *mut Vector3 as *mut c_void,
            &mut hit as *mut NavMeshHit as *mut c_void,
            &mut dist as *mut f32 as *mut c_void,
            &mut areas as *mut i32 as *mut c_void,
        ];
        let found: bool = rt.unbox(rt.invoke(&nav.sample_position, std::ptr::null_mut(), &mut args).ok()?).ok()?;
        found.then_some(hit.position)
    }

    /// A walking route from `from` to `to` on the game's navigation mesh (the one its AI
    /// uses). Both ends are first moved onto the mesh, within `snap` world units.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn find_path(&self, rt: &Runtime, from: Vector3, to: Vector3, snap: f32) -> Result<NavPath> {
        let nav = self.nav.as_ref().ok_or_else(|| Error::MemberNotFound("UnityEngine.AI.NavMesh".into()))?;
        let start = self.nav_point(rt, nav, from, snap).ok_or_else(|| Error::MemberNotFound("navigation mesh near the player".into()))?;
        let end = self.nav_point(rt, nav, to, snap).ok_or_else(|| Error::MemberNotFound("navigation mesh near the destination".into()))?;
        let path = rt.new_object(nav.path_class)?;
        let (mut a, mut b, mut areas) = (start, end, ALL_AREAS);
        let mut args = [
            &mut a as *mut Vector3 as *mut c_void,
            &mut b as *mut Vector3 as *mut c_void,
            &mut areas as *mut i32 as *mut c_void,
            path as *mut c_void,
        ];
        let found: bool = rt.unbox(rt.invoke(&nav.calculate_path, std::ptr::null_mut(), &mut args)?)?;
        // `NavMeshPathStatus`: 0 complete, 1 partial, 2 invalid.
        let status: i32 = rt.unbox(rt.invoke(&nav.status, path, &mut [])?)?;
        if !found || status == 2 {
            return Err(Error::MemberNotFound(format!("path between the two points (status {status})")));
        }
        let array = rt.invoke(&nav.corners, path, &mut [])?;
        Ok(NavPath { corners: crate::read_value_array::<Vector3>(array, MAX_PATH_CORNERS), complete: status == 0 })
    }

    /// `Camera.main`'s pose and lens, if there is a main camera.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn main_camera(&self, rt: &Runtime) -> Option<CameraPose> {
        let c = self.camera.as_ref()?;
        let camera = rt.invoke(&c.main, std::ptr::null_mut(), &mut []).ok()?;
        if !self.is_alive(camera) {
            return None;
        }
        let transform = self.component_transform(rt, camera).ok()?;
        Some(CameraPose {
            position: self.position(rt, transform).ok()?,
            rotation: self.rotation(rt, transform).ok()?,
            fov_y: rt.unbox(rt.invoke(&c.fov, camera, &mut []).ok()?).ok()?,
            aspect: rt.unbox(rt.invoke(&c.aspect, camera, &mut []).ok()?).ok()?,
        })
    }

    /// Whether `transform` is `ancestor` or below it (walks at most `max_depth` parents).
    ///
    /// # Safety
    /// Both must be `UnityEngine.Transform`s. Main thread only.
    pub unsafe fn is_within(&self, rt: &Runtime, transform: *mut ManagedObject, ancestors: &[*mut ManagedObject], max_depth: usize) -> Result<bool> {
        let m = self.transform_get_parent.as_ref().ok_or_else(|| Error::MemberNotFound("Transform::get_parent".into()))?;
        let mut current = transform;
        for _ in 0..=max_depth {
            if !self.is_alive(current) {
                return Ok(false);
            }
            if ancestors.contains(&current) {
                return Ok(true);
            }
            current = rt.invoke(m, current, &mut [])?;
        }
        Ok(false)
    }

    /// `component.gameObject`.
    ///
    /// # Safety
    /// `component` must be a `UnityEngine.Component`. Main thread only.
    pub unsafe fn component_game_object(&self, rt: &Runtime, component: *mut ManagedObject) -> Result<*mut ManagedObject> {
        if !self.is_alive(component) {
            return Err(Error::Null);
        }
        non_null(rt.invoke(&self.component_get_game_object, component, &mut [])?)
    }

    /// `gameObject.transform`.
    ///
    /// # Safety
    /// `game_object` must be a `UnityEngine.GameObject`. Main thread only.
    pub unsafe fn game_object_transform(&self, rt: &Runtime, game_object: *mut ManagedObject) -> Result<*mut ManagedObject> {
        if !self.is_alive(game_object) {
            return Err(Error::Null);
        }
        non_null(rt.invoke(&self.game_object_get_transform, game_object, &mut [])?)
    }

    /// `gameObject.GetComponent(typeof(class))`; `Ok(None)` when absent.
    ///
    /// # Safety
    /// `game_object` must be a `UnityEngine.GameObject`. Main thread only.
    pub unsafe fn get_component(&self, rt: &Runtime, game_object: *mut ManagedObject, class: Class) -> Result<Option<*mut ManagedObject>> {
        if !self.is_alive(game_object) {
            return Err(Error::Null);
        }
        let ty = rt.type_object(class);
        let mut args = [ty as *mut c_void];
        let c = rt.invoke(&self.game_object_get_component, game_object, &mut args)?;
        Ok(self.is_alive(c).then_some(c))
    }

    /// `Object.FindObjectsOfType(typeof(class), includeInactive)`: every loaded scene
    /// object of that type. Slow; meant for one-off scans, never per frame.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn find_objects(&self, rt: &Runtime, class: Class, include_inactive: bool) -> Result<Vec<*mut ManagedObject>> {
        let m = self.find_objects_of_type.as_ref().ok_or_else(|| Error::MemberNotFound("Object::FindObjectsOfType(Type, bool)".into()))?;
        let mut inactive = include_inactive;
        let mut args = [rt.type_object(class) as *mut c_void, &mut inactive as *mut bool as *mut c_void];
        let array = rt.invoke(m, std::ptr::null_mut(), &mut args)?;
        Ok(crate::read_object_array(array, usize::MAX).into_iter().filter(|&o| self.is_alive(o)).collect())
    }

    /// `obj.name`.
    ///
    /// # Safety
    /// `obj` must be a `UnityEngine.Object`. Main thread only.
    pub unsafe fn object_name(&self, rt: &Runtime, obj: *mut ManagedObject) -> Option<String> {
        if !self.is_alive(obj) {
            return None;
        }
        crate::read_string(rt.invoke(self.object_get_name.as_ref()?, obj, &mut []).ok()?)
    }

    /// `gameObject.activeInHierarchy`.
    ///
    /// # Safety
    /// `game_object` must be a `UnityEngine.GameObject`. Main thread only.
    pub unsafe fn active_in_hierarchy(&self, rt: &Runtime, game_object: *mut ManagedObject) -> Option<bool> {
        if !self.is_alive(game_object) {
            return None;
        }
        rt.unbox(rt.invoke(self.game_object_active_in_hierarchy.as_ref()?, game_object, &mut []).ok()?).ok()
    }
}

fn non_null(p: *mut ManagedObject) -> Result<*mut ManagedObject> {
    if p.is_null() {
        Err(Error::Null)
    } else {
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_rotation_faces_plus_z() {
        let f = Quaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }.forward();
        assert_eq!(f, Vector3 { x: 0.0, y: 0.0, z: 1.0 });
    }

    #[test]
    fn yaw_90_faces_plus_x() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let f = Quaternion { x: 0.0, y: h, z: 0.0, w: h }.forward();
        assert!((f.x - 1.0).abs() < 1e-6 && f.y.abs() < 1e-6 && f.z.abs() < 1e-6, "{f:?}");
    }

    #[test]
    fn identity_rotation_keeps_up() {
        assert_eq!(Quaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }.up(), Vector3 { x: 0.0, y: 1.0, z: 0.0 });
    }

    #[test]
    fn pitch_down_90_tilts_up_forward() {
        // 90 degrees about +X: forward (+Z) turns to -Y, up (+Y) turns to +Z.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let q = Quaternion { x: h, y: 0.0, z: 0.0, w: h };
        let (f, u) = (q.forward(), q.up());
        assert!(f.y < -0.999 && u.z > 0.999, "{f:?} {u:?}");
    }

    #[test]
    fn color_to_srgb8_clamps() {
        assert_eq!(Color { r: 1.5, g: 0.5, b: -1.0, a: 1.0 }.to_srgb8(), [255, 128, 0]);
    }
}
