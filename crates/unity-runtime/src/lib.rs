//! Name-based access to a running Unity scripting runtime (IL2CPP or Mono) from code
//! running inside the game process.
//!
//! Everything is looked up by namespace/type/member **name** through the runtime's own
//! exported API (`GameAssembly.dll` for IL2CPP, `mono-2.0-bdwgc.dll` for Mono), never by
//! hard-coded offsets or signatures. That keeps game integrations working across most
//! game updates, and lets one integration style serve both backends.
//!
//! Safety model: the runtime hands out raw pointers whose validity depends on game
//! state. Wrappers here are thin `Copy` handles; methods that dereference game memory
//! are `unsafe` and document what the caller must guarantee. Managed calls go through
//! the runtime's invoke function with an exception out-parameter, so managed exceptions
//! come back as [`Error::ManagedException`] instead of unwinding through Rust frames.

use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::sync::Mutex;

mod backend;
mod il2cpp;
mod mono;
mod physics;
mod unity;

use backend::{Backend, Ptr, FIELD_ATTRIBUTE_STATIC, METHOD_ATTRIBUTE_STATIC};
pub use physics::*;
pub use unity::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no IL2CPP or Mono runtime is loaded in this process")]
    NoRuntime,
    #[error("runtime export `{0}` is missing")]
    MissingExport(&'static str),
    #[error("scripting domain is not initialized yet")]
    DomainNotReady,
    #[error("class `{0}` not found")]
    ClassNotFound(String),
    #[error("member `{0}` not found")]
    MemberNotFound(String),
    #[error("internal call `{0}` not found")]
    IcallNotFound(String),
    #[error("managed exception thrown by `{0}`")]
    ManagedException(String),
    #[error("null object")]
    Null,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Opaque managed object (`Il2CppObject` / `MonoObject`).
#[repr(C)]
pub struct ManagedObject(c_void);

/// Layout constants shared by IL2CPP and Mono on 64-bit targets.
mod layout {
    /// Object header (class pointer and monitor) before an object's fields.
    pub const OBJECT_HEADER: usize = 0x10;
    /// String `length` (i32), followed by UTF-16 chars.
    pub const STRING_LENGTH: usize = 0x10;
    pub const STRING_CHARS: usize = 0x14;
    /// Array `max_length` (usize) and first element.
    pub const ARRAY_LENGTH: usize = 0x18;
    pub const ARRAY_DATA: usize = 0x20;
}

/// Which scripting backend the game uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Il2Cpp,
    Mono,
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BackendKind::Il2Cpp => "IL2CPP",
            BackendKind::Mono => "Mono",
        })
    }
}

/// Handle to the scripting runtime of the current process. Cheap to share; lookups are cached.
pub struct Runtime {
    backend: Box<dyn Backend>,
    kind: BackendKind,
    classes: Mutex<HashMap<(String, String), usize>>,
}

impl Runtime {
    /// Binds to the IL2CPP or Mono runtime in the current process.
    ///
    /// Returns [`Error::DomainNotReady`] if a runtime is loaded but hasn't finished
    /// initializing, and [`Error::NoRuntime`] if neither is loaded (yet); callers
    /// injected early should retry both.
    pub fn load() -> Result<Self> {
        let (backend, kind): (Box<dyn Backend>, _) = match il2cpp::Il2Cpp::load() {
            Ok(b) => (Box::new(b), BackendKind::Il2Cpp),
            Err(Error::NoRuntime) => (Box::new(mono::Mono::load()?), BackendKind::Mono),
            Err(e) => return Err(e),
        };
        Ok(Self { backend, kind, classes: Mutex::new(HashMap::new()) })
    }

    pub fn kind(&self) -> BackendKind {
        self.kind
    }

    /// Registers the calling thread with the runtime's GC. Required before a thread
    /// that wasn't created by the game touches managed objects. Detaches on drop.
    pub fn attach_thread(&self) -> ThreadGuard<'_> {
        let thread = unsafe { self.backend.attach_thread() };
        ThreadGuard { rt: self, thread }
    }

    /// Finds a class by namespace (may be empty) and name across all loaded assemblies.
    /// Nested types use `Outer/Inner` as `name`.
    pub fn class(&self, namespace: &str, name: &str) -> Result<Class> {
        let key = (namespace.to_owned(), name.to_owned());
        if let Some(&ptr) = self.classes.lock().unwrap().get(&key) {
            return Ok(Class(ptr as Ptr));
        }
        let full = if namespace.is_empty() { name.to_owned() } else { format!("{namespace}.{name}") };
        let not_found = || Error::ClassNotFound(full.clone());
        let ns = CString::new(namespace).map_err(|_| not_found())?;
        let found = unsafe {
            let images = self.backend.images();
            let top_level = |n: &str| -> Option<Ptr> {
                let c = CString::new(n).ok()?;
                images.iter().find_map(|&image| {
                    let class = self.backend.class_from_name(image, &ns, &c);
                    (!class.is_null()).then_some(class)
                })
            };
            // IL2CPP resolves `Outer/Inner` directly; otherwise walk the nesting.
            top_level(name).or_else(|| {
                let mut parts = name.split('/');
                let mut class = top_level(parts.next()?)?;
                for inner in parts {
                    class = self.backend.nested_class(class, inner);
                    if class.is_null() {
                        return None;
                    }
                }
                Some(class)
            })
        };
        let class = found.ok_or_else(not_found)?;
        self.classes.lock().unwrap().insert(key, class as usize);
        Ok(Class(class))
    }

    /// Looks up a method by name and parameter count (walks base classes).
    pub fn method(&self, class: Class, name: &str, args: i32) -> Result<Method> {
        let c = CString::new(name).map_err(|_| Error::MemberNotFound(name.into()))?;
        let mut current = class.0;
        unsafe {
            while !current.is_null() {
                let m = self.backend.method_from_name(current, &c, args);
                if !m.is_null() {
                    return Ok(self.wrap_method(m));
                }
                current = self.backend.class_parent(current);
            }
        }
        Err(Error::MemberNotFound(format!("{}::{name}({args})", self.class_name(class))))
    }

    /// Finds the overload of `name` (declared on `class` itself) whose parameter types
    /// are exactly `params`, given as full type names such as `UnityEngine.Vector3`.
    pub fn method_with_params(&self, class: Class, name: &str, params: &[&str]) -> Result<Method> {
        unsafe {
            for m in self.backend.methods(class.0) {
                if self.backend.method_name(m) == name && self.backend.method_param_types(m) == params {
                    return Ok(self.wrap_method(m));
                }
            }
        }
        Err(Error::MemberNotFound(format!("{}::{name}({})", self.class_name(class), params.join(", "))))
    }

    fn wrap_method(&self, m: Ptr) -> Method {
        let (flags, _) = unsafe { self.backend.method_flags(m) };
        let name = unsafe { self.backend.method_name(m) };
        Method { ptr: m, name, is_static: flags & METHOD_ATTRIBUTE_STATIC != 0 }
    }

    /// Looks up a field by name (walks base classes).
    pub fn field(&self, class: Class, name: &str) -> Result<Field> {
        let c = CString::new(name).map_err(|_| Error::MemberNotFound(name.into()))?;
        let f = unsafe { self.backend.field_from_name(class.0, &c) };
        if f.is_null() {
            return Err(Error::MemberNotFound(format!("{}::{name}", self.class_name(class))));
        }
        let (offset, flags) = unsafe { (self.backend.field_offset(f), self.backend.field_flags(f)) };
        Ok(Field { ptr: f, offset, is_static: flags & FIELD_ATTRIBUTE_STATIC != 0 })
    }

    /// Resolves an engine internal call, e.g. `UnityEngine.Time::get_frameCount()`.
    ///
    /// # Safety
    /// `F` must be a function pointer type matching the icall's native signature.
    pub unsafe fn icall<F: Copy>(&self, name: &str) -> Result<F> {
        assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*const c_void>());
        let mut p = self.backend.icall_by_name(name);
        if p.is_null() {
            if let Some((namespace, class, method, args)) = backend::parse_icall_name(name) {
                if let Ok(m) = self.class(namespace, class).and_then(|c| self.method(c, method, args as i32)) {
                    p = self.backend.icall_for_method(m.ptr);
                }
            }
        }
        if p.is_null() {
            return Err(Error::IcallNotFound(name.into()));
        }
        Ok(std::mem::transmute_copy::<*const c_void, F>(&p))
    }

    /// Reads a static field.
    ///
    /// # Safety
    /// `T` must match the field's storage type (object references are `*mut ManagedObject`).
    pub unsafe fn static_value<T: Copy + Default>(&self, field: Field) -> T {
        debug_assert!(field.is_static);
        let mut out = T::default();
        self.backend.field_static_get(field.ptr, &mut out as *mut T as *mut c_void);
        out
    }

    /// Reads a field whether it is static or an instance field (`instance` is ignored for
    /// static fields; a null `instance` yields `T::default()` for instance fields).
    ///
    /// # Safety
    /// Same as [`Runtime::static_value`] / [`Field::get`].
    pub unsafe fn read_field<T: Copy + Default>(&self, field: Field, instance: *mut ManagedObject) -> T {
        if field.is_static {
            self.static_value(field)
        } else if instance.is_null() {
            T::default()
        } else {
            field.get(instance)
        }
    }

    /// Invokes a managed method, catching managed exceptions.
    ///
    /// Returns the boxed result for value types, the object for reference types, or
    /// null for `void`.
    ///
    /// # Safety
    /// `instance` must be null for static methods or a live object of the declaring
    /// type; `args` must point to correctly typed values (value types by pointer,
    /// references as the object pointer itself). Must run on an attached thread, and
    /// on the main thread for most Unity APIs.
    pub unsafe fn invoke(&self, method: &Method, instance: *mut ManagedObject, args: &mut [*mut c_void]) -> Result<*mut ManagedObject> {
        let mut exc: Ptr = std::ptr::null_mut();
        let params = if args.is_empty() { std::ptr::null_mut() } else { args.as_mut_ptr() };
        let ret = self.backend.invoke(method.ptr, instance as Ptr, params, &mut exc);
        if !exc.is_null() {
            return Err(Error::ManagedException(method.name.clone()));
        }
        Ok(ret as *mut ManagedObject)
    }

    /// Invokes an instance method of a value type on a boxed value, e.g. a struct
    /// returned by [`Runtime::invoke`].
    ///
    /// # Safety
    /// Same as [`Runtime::invoke`]; `boxed` must be a boxed instance of the method's
    /// declaring type.
    pub unsafe fn invoke_boxed(&self, method: &Method, boxed: *mut ManagedObject, args: &mut [*mut c_void]) -> Result<*mut ManagedObject> {
        if boxed.is_null() {
            return Err(Error::Null);
        }
        // IL2CPP unboxes value-type receivers itself; Mono expects the unboxed data.
        let this = match self.kind {
            BackendKind::Il2Cpp => boxed,
            BackendKind::Mono => self.backend.unbox(boxed as Ptr) as *mut ManagedObject,
        };
        self.invoke(method, this, args)
    }

    /// Unboxes a value-type result from [`Runtime::invoke`].
    ///
    /// # Safety
    /// `boxed` must be a boxed `T`.
    pub unsafe fn unbox<T: Copy>(&self, boxed: *mut ManagedObject) -> Result<T> {
        if boxed.is_null() {
            return Err(Error::Null);
        }
        Ok(*(self.backend.unbox(boxed as Ptr) as *const T))
    }

    /// A boxed integer of any built-in integer type, read by its actual size and
    /// signedness (`None` for anything else).
    ///
    /// # Safety
    /// `boxed` must be null or a live managed object.
    pub unsafe fn unbox_integer(&self, boxed: *mut ManagedObject) -> Option<i128> {
        let class = self.class_of(boxed)?;
        let data = self.backend.unbox(boxed as Ptr);
        Some(match self.class_name(class).as_str() {
            "Byte" => *(data as *const u8) as i128,
            "SByte" => *(data as *const i8) as i128,
            "Int16" => *(data as *const i16) as i128,
            "UInt16" => *(data as *const u16) as i128,
            "Int32" => *(data as *const i32) as i128,
            "UInt32" => *(data as *const u32) as i128,
            "Int64" => *(data as *const i64) as i128,
            "UInt64" => *(data as *const u64) as i128,
            _ => return None,
        })
    }

    /// The text of a boxed `System.Guid` or a string (`None` for anything else).
    ///
    /// # Safety
    /// `boxed` must be null or a live managed object.
    pub unsafe fn guid_or_string(&self, boxed: *mut ManagedObject) -> Option<String> {
        let class = self.class_of(boxed)?;
        match self.class_name(class).as_str() {
            "String" => read_string(boxed),
            "Guid" => {
                let b = std::slice::from_raw_parts(self.backend.unbox(boxed as Ptr) as *const u8, 16);
                // System.Guid's first three fields are little-endian.
                let (a, c, d) = (u32::from_le_bytes([b[0], b[1], b[2], b[3]]), u16::from_le_bytes([b[4], b[5]]), u16::from_le_bytes([b[6], b[7]]));
                let rest: String = b[8..].iter().map(|x| format!("{x:02x}")).collect();
                Some(format!("{a:08x}-{c:04x}-{d:04x}-{}-{}", &rest[..4], &rest[4..]))
            }
            _ => None,
        }
    }

    /// Name of a boxed enum value (`System.Enum.ToString()`), e.g. as returned by
    /// [`Runtime::invoke`] for an enum-typed getter.
    ///
    /// # Safety
    /// `boxed` must be null or a boxed enum. Must run on an attached thread.
    pub unsafe fn enum_name(&self, boxed: *mut ManagedObject) -> Option<String> {
        if boxed.is_null() {
            return None;
        }
        let to_string = self.method(self.class("System", "Enum").ok()?, "ToString", 0).ok()?;
        read_string(self.invoke(&to_string, boxed, &mut []).ok()?)
    }

    /// Allocates a managed array `T[len]` where `element` is `T`. The array is only
    /// kept alive by references the GC can see (e.g. the calling thread's stack), so
    /// don't keep it across frames.
    ///
    /// # Safety
    /// Must run on an attached thread.
    pub unsafe fn new_array(&self, element: Class, len: usize) -> Result<ManagedArray> {
        let array = self.backend.new_array(element.0, len);
        if array.is_null() {
            return Err(Error::Null);
        }
        let class = self.backend.object_class(array);
        let stride = self.backend.array_element_size(class).max(0) as usize;
        Ok(ManagedArray { object: array as *mut ManagedObject, stride, len })
    }

    /// A new instance of `class` built by its parameterless constructor. Only kept alive
    /// by references the GC can see, so don't keep it across frames.
    ///
    /// # Safety
    /// Must run on an attached thread (the main thread for Unity types).
    pub unsafe fn new_object(&self, class: Class) -> Result<*mut ManagedObject> {
        let obj = self.backend.new_object(class.0);
        if obj.is_null() {
            return Err(Error::Null);
        }
        Ok(obj as *mut ManagedObject)
    }

    /// The `System.Type` object for a class, as needed by `GetComponent(Type)`.
    pub fn type_object(&self, class: Class) -> *mut ManagedObject {
        unsafe { self.backend.type_object(class.0) as *mut ManagedObject }
    }

    /// # Safety
    /// `obj` must be null or a live managed object.
    pub unsafe fn class_of(&self, obj: *mut ManagedObject) -> Option<Class> {
        if obj.is_null() {
            return None;
        }
        let c = self.backend.object_class(obj as Ptr);
        (!c.is_null()).then_some(Class(c))
    }

    /// Whether values of `derived` can be assigned to `base`.
    pub fn is_assignable(&self, base: Class, derived: Class) -> bool {
        unsafe { self.backend.is_assignable_from(base.0, derived.0) }
    }

    pub fn class_name(&self, class: Class) -> String {
        unsafe { self.backend.class_name(class.0) }
    }

    /// Allocates a managed string.
    pub fn new_string(&self, s: &str) -> *mut ManagedObject {
        let c = CString::new(s.replace('\0', "")).unwrap();
        unsafe { self.backend.new_string(&c) as *mut ManagedObject }
    }

    /// Native code address of a method, used to install frame hooks. On Mono this
    /// JIT-compiles the method if needed, so call it on an attached thread.
    pub fn method_pointer(&self, method: &Method) -> *const c_void {
        unsafe { self.backend.method_code(method.ptr) }
    }
}

/// Keeps a non-game thread attached to the runtime.
pub struct ThreadGuard<'a> {
    rt: &'a Runtime,
    thread: Ptr,
}

impl Drop for ThreadGuard<'_> {
    fn drop(&mut self) {
        if !self.thread.is_null() {
            unsafe { self.rt.backend.detach_thread(self.thread) };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class(Ptr);
unsafe impl Send for Class {}
unsafe impl Sync for Class {}

#[derive(Debug, Clone)]
pub struct Method {
    ptr: Ptr,
    name: String,
    pub is_static: bool,
}
unsafe impl Send for Method {}
unsafe impl Sync for Method {}

#[derive(Debug, Clone, Copy)]
pub struct Field {
    ptr: Ptr,
    /// Byte offset from the object start (includes the object header) for instance fields.
    pub offset: usize,
    pub is_static: bool,
}
unsafe impl Send for Field {}
unsafe impl Sync for Field {}

impl Field {
    /// Reads an instance field.
    ///
    /// # Safety
    /// `obj` must be a live object containing this field, and `T` its storage type.
    pub unsafe fn get<T: Copy>(&self, obj: *mut ManagedObject) -> T {
        debug_assert!(!self.is_static);
        std::ptr::read_unaligned((obj as *const u8).add(self.offset) as *const T)
    }
}

/// A managed array allocated by [`Runtime::new_array`].
#[derive(Debug, Clone, Copy)]
pub struct ManagedArray {
    pub object: *mut ManagedObject,
    /// Bytes per element.
    pub stride: usize,
    pub len: usize,
}

impl ManagedArray {
    /// Pointer to element `i`'s data (value types are stored inline).
    ///
    /// # Safety
    /// The array must still be alive and `i < len`.
    pub unsafe fn element(&self, i: usize) -> *const u8 {
        debug_assert!(i < self.len);
        (self.object as *const u8).add(layout::ARRAY_DATA + i * self.stride)
    }
}

/// Field layout of a `System.Collections.Generic.List<T>` instantiation, for reading
/// its elements without calling into the runtime.
#[derive(Debug, Clone, Copy)]
pub struct ListLayout {
    items: Field,
    size: Field,
}

impl ListLayout {
    /// Resolves the layout from a live list. Every `List<T>` of reference types shares
    /// it, so resolve once per field and reuse.
    ///
    /// # Safety
    /// `list` must be null or a live managed object.
    pub unsafe fn of(rt: &Runtime, list: *mut ManagedObject) -> Result<Self> {
        let class = rt.class_of(list).ok_or(Error::Null)?;
        match (rt.field(class, "_items"), rt.field(class, "_size")) {
            (Ok(items), Ok(size)) => Ok(Self { items, size }),
            _ => Err(Error::MemberNotFound(format!("{}::_items/_size", rt.class_name(class)))),
        }
    }

    /// The first `max_len` elements of a list of object references.
    ///
    /// # Safety
    /// `list` must be null or a live `List<T>` of reference types with this layout.
    pub unsafe fn elements(&self, list: *mut ManagedObject, max_len: usize) -> Vec<*mut ManagedObject> {
        if list.is_null() {
            return Vec::new();
        }
        let size = (self.size.get::<i32>(list).max(0) as usize).min(max_len);
        read_object_array(self.items.get(list), size)
    }
}

/// Reads a `System.String`.
///
/// # Safety
/// `s` must be null or a live `System.String`.
pub unsafe fn read_string(s: *mut ManagedObject) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let base = s as *const u8;
    let len = *(base.add(layout::STRING_LENGTH) as *const i32);
    if !(0..=1 << 20).contains(&len) {
        return None;
    }
    let chars = std::slice::from_raw_parts(base.add(layout::STRING_CHARS) as *const u16, len as usize);
    Some(String::from_utf16_lossy(chars))
}

/// Elements of a managed value-type array (`T[]` where `T` is a struct such as `Vector3`).
///
/// # Safety
/// `array` must be null or a live array whose elements are laid out as `T`.
pub unsafe fn read_value_array<T: Copy>(array: *mut ManagedObject, max_len: usize) -> Vec<T> {
    if array.is_null() {
        return Vec::new();
    }
    let base = array as *const u8;
    let len = (*(base.add(layout::ARRAY_LENGTH) as *const usize)).min(max_len);
    let data = base.add(layout::ARRAY_DATA) as *const T;
    (0..len).map(|i| std::ptr::read_unaligned(data.add(i))).collect()
}

/// Elements of a managed reference-type array (`T[]` where `T` is a class).
///
/// # Safety
/// `array` must be null or a live array of object references.
pub unsafe fn read_object_array(array: *mut ManagedObject, max_len: usize) -> Vec<*mut ManagedObject> {
    if array.is_null() {
        return Vec::new();
    }
    let base = array as *const u8;
    let len = (*(base.add(layout::ARRAY_LENGTH) as *const usize)).min(max_len);
    let data = base.add(layout::ARRAY_DATA) as *const *mut ManagedObject;
    (0..len).map(|i| *data.add(i)).collect()
}
