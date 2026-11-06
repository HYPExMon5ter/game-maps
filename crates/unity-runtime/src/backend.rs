//! The raw runtime operations each scripting backend (IL2CPP, Mono) provides.
//!
//! [`crate::Runtime`] builds its safe-ish, cached API on top of these. Pointers are
//! opaque runtime handles (`Il2CppClass*` / `MonoClass*` and so on); only the backend
//! that produced a handle may interpret it.

use std::ffi::c_void;

pub(crate) type Ptr = *mut c_void;

/// `FIELD_ATTRIBUTE_STATIC` from ECMA-335.
pub(crate) const FIELD_ATTRIBUTE_STATIC: u32 = 0x0010;
/// `METHOD_ATTRIBUTE_STATIC` from ECMA-335.
pub(crate) const METHOD_ATTRIBUTE_STATIC: u32 = 0x0010;

pub(crate) trait Backend: Send + Sync {
    /// Registers the calling thread with the runtime; returns a handle for `detach_thread`.
    unsafe fn attach_thread(&self) -> Ptr;
    unsafe fn detach_thread(&self, thread: Ptr);

    /// Images of every loaded assembly.
    unsafe fn images(&self) -> Vec<Ptr>;
    /// A top-level class in `image`, or null.
    unsafe fn class_from_name(&self, image: Ptr, namespace: &std::ffi::CStr, name: &std::ffi::CStr) -> Ptr;
    /// A nested type of `class` by simple name, or null.
    unsafe fn nested_class(&self, class: Ptr, name: &str) -> Ptr;
    unsafe fn class_name(&self, class: Ptr) -> String;
    unsafe fn class_parent(&self, class: Ptr) -> Ptr;
    unsafe fn is_assignable_from(&self, base: Ptr, derived: Ptr) -> bool;
    /// The `System.Type` object for a class.
    unsafe fn type_object(&self, class: Ptr) -> Ptr;

    /// A method declared on `class` itself (not its bases), or null.
    unsafe fn method_from_name(&self, class: Ptr, name: &std::ffi::CStr, args: i32) -> Ptr;
    /// Every method declared on `class` itself.
    unsafe fn methods(&self, class: Ptr) -> Vec<Ptr>;
    unsafe fn method_name(&self, method: Ptr) -> String;
    /// Full type names of the parameters, e.g. `UnityEngine.Vector3`.
    unsafe fn method_param_types(&self, method: Ptr) -> Vec<String>;
    /// `(flags, implementation flags)`.
    unsafe fn method_flags(&self, method: Ptr) -> (u32, u32);
    /// Native code for the method, compiling it first if the backend needs to.
    unsafe fn method_code(&self, method: Ptr) -> *const c_void;

    /// A field of `class` or one of its bases, or null.
    unsafe fn field_from_name(&self, class: Ptr, name: &std::ffi::CStr) -> Ptr;
    /// Full type name of a field, e.g. `System.Int32`.
    unsafe fn field_type_name(&self, field: Ptr) -> String;
    /// Byte offset from the object start (including the header) for instance fields.
    unsafe fn field_offset(&self, field: Ptr) -> usize;
    unsafe fn field_flags(&self, field: Ptr) -> u32;
    unsafe fn field_static_get(&self, field: Ptr, out: *mut c_void);

    /// An engine internal call by its IL2CPP-style name, e.g.
    /// `UnityEngine.Time::get_frameCount()`, or null if the backend can't resolve by name.
    unsafe fn icall_by_name(&self, name: &str) -> *const c_void;
    /// The internal call implementing `method`, or null if the backend can't resolve
    /// by method (or `method` isn't an internal call).
    unsafe fn icall_for_method(&self, method: Ptr) -> *const c_void;

    /// Invokes a method; a thrown exception is reported through `exc`.
    unsafe fn invoke(&self, method: Ptr, instance: Ptr, args: *mut Ptr, exc: *mut Ptr) -> Ptr;
    unsafe fn unbox(&self, boxed: Ptr) -> *mut c_void;
    unsafe fn object_class(&self, obj: Ptr) -> Ptr;
    unsafe fn new_string(&self, s: &std::ffi::CStr) -> Ptr;
    /// A new instance of `class`, initialized by its parameterless constructor.
    unsafe fn new_object(&self, class: Ptr) -> Ptr;
    /// A new boxed instance of `class`, zeroed, with no constructor run.
    unsafe fn new_value(&self, class: Ptr) -> Ptr;
    unsafe fn new_array(&self, element: Ptr, len: usize) -> Ptr;
    unsafe fn array_element_size(&self, array_class: Ptr) -> i32;
}

/// Splits an IL2CPP-style icall name `Namespace.Class::method(Type1,Type2)` into
/// `(namespace, class, method, parameter count)`.
pub(crate) fn parse_icall_name(name: &str) -> Option<(&str, &str, &str, usize)> {
    let (ty, rest) = name.split_once("::")?;
    let (method, params) = rest.split_once('(')?;
    let params = params.strip_suffix(')')?;
    let count = if params.trim().is_empty() { 0 } else { params.split(',').count() };
    let (namespace, class) = ty.rsplit_once('.').unwrap_or(("", ty));
    Some((namespace, class, method, count))
}

pub(crate) unsafe fn cstr(p: *const std::ffi::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::parse_icall_name;

    #[test]
    fn parses_icall_names() {
        assert_eq!(parse_icall_name("UnityEngine.Time::get_frameCount()"), Some(("UnityEngine", "Time", "get_frameCount", 0)));
        assert_eq!(
            parse_icall_name("UnityEngine.Transform::get_position_Injected(UnityEngine.Vector3&)"),
            Some(("UnityEngine", "Transform", "get_position_Injected", 1))
        );
        assert_eq!(parse_icall_name("Foo::Bar(int,int)"), Some(("", "Foo", "Bar", 2)));
        assert_eq!(parse_icall_name("nonsense"), None);
    }
}
