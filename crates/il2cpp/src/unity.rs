//! Engine helpers shared by every Unity IL2CPP game.
//!
//! Hot-path reads (positions, rotations, frame count) use engine internal calls, which
//! allocate nothing. Anything else goes through managed methods via
//! [`Runtime::invoke`], which catches managed exceptions.

use std::ffi::c_void;

use crate::{Class, Error, Field, Il2CppObject, Method, Result, Runtime};

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

type GetVec3Injected = unsafe extern "C" fn(*mut Il2CppObject, *mut Vector3);
type GetQuatInjected = unsafe extern "C" fn(*mut Il2CppObject, *mut Quaternion);
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
            })
        }
    }

    /// Whether a `UnityEngine.Object` reference still points at a live native object.
    /// Destroyed objects keep their managed wrapper but lose the native pointer; calling
    /// engine methods on them throws, so check before every use.
    ///
    /// # Safety
    /// `obj` must be null or a managed object deriving from `UnityEngine.Object`.
    pub unsafe fn is_alive(&self, obj: *mut Il2CppObject) -> bool {
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
    pub unsafe fn position(&self, rt: &Runtime, transform: *mut Il2CppObject) -> Result<Vector3> {
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
    pub unsafe fn rotation(&self, rt: &Runtime, transform: *mut Il2CppObject) -> Result<Quaternion> {
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
    pub unsafe fn component_transform(&self, rt: &Runtime, component: *mut Il2CppObject) -> Result<*mut Il2CppObject> {
        if !self.is_alive(component) {
            return Err(Error::Null);
        }
        non_null(rt.invoke(&self.component_get_transform, component, &mut [])?)
    }

    /// `component.gameObject`.
    ///
    /// # Safety
    /// `component` must be a `UnityEngine.Component`. Main thread only.
    pub unsafe fn component_game_object(&self, rt: &Runtime, component: *mut Il2CppObject) -> Result<*mut Il2CppObject> {
        if !self.is_alive(component) {
            return Err(Error::Null);
        }
        non_null(rt.invoke(&self.component_get_game_object, component, &mut [])?)
    }

    /// `gameObject.transform`.
    ///
    /// # Safety
    /// `game_object` must be a `UnityEngine.GameObject`. Main thread only.
    pub unsafe fn game_object_transform(&self, rt: &Runtime, game_object: *mut Il2CppObject) -> Result<*mut Il2CppObject> {
        if !self.is_alive(game_object) {
            return Err(Error::Null);
        }
        non_null(rt.invoke(&self.game_object_get_transform, game_object, &mut [])?)
    }

    /// `gameObject.GetComponent(typeof(class))`; `Ok(None)` when absent.
    ///
    /// # Safety
    /// `game_object` must be a `UnityEngine.GameObject`. Main thread only.
    pub unsafe fn get_component(&self, rt: &Runtime, game_object: *mut Il2CppObject, class: Class) -> Result<Option<*mut Il2CppObject>> {
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
    pub unsafe fn find_objects(&self, rt: &Runtime, class: Class, include_inactive: bool) -> Result<Vec<*mut Il2CppObject>> {
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
    pub unsafe fn object_name(&self, rt: &Runtime, obj: *mut Il2CppObject) -> Option<String> {
        if !self.is_alive(obj) {
            return None;
        }
        crate::read_string(rt.invoke(self.object_get_name.as_ref()?, obj, &mut []).ok()?)
    }

    /// `gameObject.activeInHierarchy`.
    ///
    /// # Safety
    /// `game_object` must be a `UnityEngine.GameObject`. Main thread only.
    pub unsafe fn active_in_hierarchy(&self, rt: &Runtime, game_object: *mut Il2CppObject) -> Option<bool> {
        if !self.is_alive(game_object) {
            return None;
        }
        rt.unbox(rt.invoke(self.game_object_active_in_hierarchy.as_ref()?, game_object, &mut []).ok()?).ok()
    }
}

fn non_null(p: *mut Il2CppObject) -> Result<*mut Il2CppObject> {
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
    fn color_to_srgb8_clamps() {
        assert_eq!(Color { r: 1.5, g: 0.5, b: -1.0, a: 1.0 }.to_srgb8(), [255, 128, 0]);
    }
}
