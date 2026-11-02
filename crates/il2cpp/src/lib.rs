//! Name-based access to a Unity IL2CPP runtime from code running inside the game process.
//!
//! Everything is looked up by namespace/type/member **name** through the runtime's own
//! exported API (`GameAssembly.dll`), never by hard-coded offsets or signatures. That
//! keeps game integrations working across most game updates.
//!
//! Safety model: the runtime hands out raw pointers whose validity depends on game
//! state. Wrappers here are thin `Copy` handles; methods that dereference game memory
//! are `unsafe` and document what the caller must guarantee. Managed calls go through
//! `il2cpp_runtime_invoke` with an exception out-parameter, so managed exceptions come
//! back as [`Error::ManagedException`] instead of unwinding through Rust frames.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::sync::Mutex;

use windows::core::{w, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

mod unity;
pub use unity::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GameAssembly.dll is not loaded in this process")]
    NotIl2Cpp,
    #[error("IL2CPP export `{0}` is missing")]
    MissingExport(&'static str),
    #[error("IL2CPP domain is not initialized yet")]
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

// Opaque runtime types.
#[repr(C)]
pub struct Il2CppDomain(c_void);
#[repr(C)]
pub struct Il2CppAssembly(c_void);
#[repr(C)]
pub struct Il2CppImage(c_void);
#[repr(C)]
pub struct Il2CppClass(c_void);
#[repr(C)]
pub struct MethodInfo(c_void);
#[repr(C)]
pub struct FieldInfo(c_void);
#[repr(C)]
pub struct Il2CppType(c_void);
#[repr(C)]
pub struct Il2CppThread(c_void);
#[repr(C)]
pub struct Il2CppObject(c_void);

/// `FIELD_ATTRIBUTE_STATIC` from ECMA-335.
const FIELD_ATTRIBUTE_STATIC: i32 = 0x0010;

/// Layout constants that are stable across IL2CPP versions on 64-bit targets.
mod layout {
    /// `Il2CppString::length` (i32), followed by UTF-16 `chars`.
    pub const STRING_LENGTH: usize = 0x10;
    pub const STRING_CHARS: usize = 0x14;
    /// `Il2CppArray::max_length` (usize) and first element.
    pub const ARRAY_LENGTH: usize = 0x18;
    pub const ARRAY_DATA: usize = 0x20;
}

macro_rules! api {
    ($( $field:ident : fn($($arg:ty),*) $(-> $ret:ty)? ;)*) => {
        #[allow(non_snake_case)]
        struct Api {
            $( $field: unsafe extern "C" fn($($arg),*) $(-> $ret)?, )*
        }

        impl Api {
            unsafe fn load(module: HMODULE) -> Result<Self> {
                Ok(Self {
                    $( $field: {
                        let name = concat!(stringify!($field), "\0");
                        let proc = GetProcAddress(module, PCSTR(name.as_ptr()))
                            .ok_or(Error::MissingExport(stringify!($field)))?;
                        std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            unsafe extern "C" fn($($arg),*) $(-> $ret)?,
                        >(proc)
                    }, )*
                })
            }
        }
    };
}

api! {
    il2cpp_domain_get: fn() -> *mut Il2CppDomain;
    il2cpp_domain_get_assemblies: fn(*mut Il2CppDomain, *mut usize) -> *const *const Il2CppAssembly;
    il2cpp_assembly_get_image: fn(*const Il2CppAssembly) -> *const Il2CppImage;
    il2cpp_thread_attach: fn(*mut Il2CppDomain) -> *mut Il2CppThread;
    il2cpp_thread_detach: fn(*mut Il2CppThread);
    il2cpp_class_from_name: fn(*const Il2CppImage, *const c_char, *const c_char) -> *mut Il2CppClass;
    il2cpp_class_get_method_from_name: fn(*mut Il2CppClass, *const c_char, i32) -> *const MethodInfo;
    il2cpp_class_get_field_from_name: fn(*mut Il2CppClass, *const c_char) -> *mut FieldInfo;
    il2cpp_class_get_type: fn(*mut Il2CppClass) -> *const Il2CppType;
    il2cpp_class_get_name: fn(*mut Il2CppClass) -> *const c_char;
    il2cpp_class_is_assignable_from: fn(*mut Il2CppClass, *mut Il2CppClass) -> bool;
    il2cpp_type_get_object: fn(*const Il2CppType) -> *mut Il2CppObject;
    il2cpp_field_get_offset: fn(*mut FieldInfo) -> usize;
    il2cpp_field_get_flags: fn(*mut FieldInfo) -> i32;
    il2cpp_field_static_get_value: fn(*mut FieldInfo, *mut c_void);
    il2cpp_runtime_invoke: fn(*const MethodInfo, *mut c_void, *mut *mut c_void, *mut *mut Il2CppObject) -> *mut Il2CppObject;
    il2cpp_resolve_icall: fn(*const c_char) -> *const c_void;
    il2cpp_object_unbox: fn(*mut Il2CppObject) -> *mut c_void;
    il2cpp_object_get_class: fn(*mut Il2CppObject) -> *mut Il2CppClass;
    il2cpp_string_new: fn(*const c_char) -> *mut Il2CppObject;
    il2cpp_class_get_methods: fn(*mut Il2CppClass, *mut *mut c_void) -> *const MethodInfo;
    il2cpp_method_get_name: fn(*const MethodInfo) -> *const c_char;
    il2cpp_method_get_param_count: fn(*const MethodInfo) -> u32;
    il2cpp_method_get_param: fn(*const MethodInfo, u32) -> *const Il2CppType;
    il2cpp_method_get_flags: fn(*const MethodInfo, *mut u32) -> u32;
    il2cpp_type_get_name: fn(*const Il2CppType) -> *mut c_char;
    il2cpp_free: fn(*mut c_void);
    il2cpp_array_new: fn(*mut Il2CppClass, usize) -> *mut Il2CppObject;
    il2cpp_array_element_size: fn(*mut Il2CppClass) -> i32;
}

/// `METHOD_ATTRIBUTE_STATIC` from ECMA-335.
const METHOD_ATTRIBUTE_STATIC: u32 = 0x0010;

/// Handle to the IL2CPP runtime of the current process. Cheap to share; lookups are cached.
pub struct Runtime {
    api: Api,
    domain: *mut Il2CppDomain,
    classes: Mutex<HashMap<(String, String), usize>>,
}

// The runtime is process-global and its lookup functions are thread-safe once the
// calling thread is attached. Cached pointers are immutable metadata.
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

impl Runtime {
    /// Binds to `GameAssembly.dll` in the current process.
    ///
    /// Returns [`Error::DomainNotReady`] if the module is loaded but the runtime hasn't
    /// finished initializing; callers should retry.
    pub fn load() -> Result<Self> {
        unsafe {
            let module = GetModuleHandleW(w!("GameAssembly.dll")).map_err(|_| Error::NotIl2Cpp)?;
            let api = Api::load(module)?;
            let domain = (api.il2cpp_domain_get)();
            if domain.is_null() {
                return Err(Error::DomainNotReady);
            }
            let mut count = 0usize;
            (api.il2cpp_domain_get_assemblies)(domain, &mut count);
            if count == 0 {
                return Err(Error::DomainNotReady);
            }
            Ok(Self { api, domain, classes: Mutex::new(HashMap::new()) })
        }
    }

    /// Registers the calling thread with the runtime's GC. Required before a thread
    /// that wasn't created by the game touches managed objects. Detaches on drop.
    pub fn attach_thread(&self) -> ThreadGuard<'_> {
        let thread = unsafe { (self.api.il2cpp_thread_attach)(self.domain) };
        ThreadGuard { rt: self, thread }
    }

    /// Finds a class by namespace (may be empty) and name across all loaded assemblies.
    /// Nested types use `Outer/Inner` as `name`.
    pub fn class(&self, namespace: &str, name: &str) -> Result<Class> {
        let key = (namespace.to_owned(), name.to_owned());
        if let Some(&ptr) = self.classes.lock().unwrap().get(&key) {
            return Ok(Class(ptr as *mut Il2CppClass));
        }
        let full = if namespace.is_empty() { name.to_owned() } else { format!("{namespace}.{name}") };
        let ns = CString::new(namespace).map_err(|_| Error::ClassNotFound(full.clone()))?;
        let nm = CString::new(name).map_err(|_| Error::ClassNotFound(full.clone()))?;
        let found = unsafe {
            let mut count = 0usize;
            let assemblies = (self.api.il2cpp_domain_get_assemblies)(self.domain, &mut count);
            (0..count).find_map(|i| {
                let image = (self.api.il2cpp_assembly_get_image)(*assemblies.add(i));
                let class = (self.api.il2cpp_class_from_name)(image, ns.as_ptr(), nm.as_ptr());
                (!class.is_null()).then_some(class)
            })
        };
        let class = found.ok_or(Error::ClassNotFound(full))?;
        self.classes.lock().unwrap().insert(key, class as usize);
        Ok(Class(class))
    }

    /// Looks up a method by name and parameter count (walks base classes).
    pub fn method(&self, class: Class, name: &str, args: i32) -> Result<Method> {
        let c = CString::new(name).map_err(|_| Error::MemberNotFound(name.into()))?;
        let m = unsafe { (self.api.il2cpp_class_get_method_from_name)(class.0, c.as_ptr(), args) };
        if m.is_null() {
            return Err(Error::MemberNotFound(format!("{}::{name}({args})", self.class_name(class))));
        }
        Ok(self.wrap_method(m))
    }

    /// Finds the overload of `name` (declared on `class` itself) whose parameter types
    /// are exactly `params`, given as full type names such as `UnityEngine.Vector3`.
    pub fn method_with_params(&self, class: Class, name: &str, params: &[&str]) -> Result<Method> {
        let mut iter: *mut c_void = std::ptr::null_mut();
        unsafe {
            loop {
                let m = (self.api.il2cpp_class_get_methods)(class.0, &mut iter);
                if m.is_null() {
                    break;
                }
                if cstr((self.api.il2cpp_method_get_name)(m)) != name
                    || (self.api.il2cpp_method_get_param_count)(m) as usize != params.len()
                {
                    continue;
                }
                let matches = params.iter().enumerate().all(|(i, want)| {
                    let raw = (self.api.il2cpp_type_get_name)((self.api.il2cpp_method_get_param)(m, i as u32));
                    let got = cstr(raw);
                    (self.api.il2cpp_free)(raw as *mut c_void);
                    got == *want
                });
                if matches {
                    return Ok(self.wrap_method(m));
                }
            }
        }
        Err(Error::MemberNotFound(format!("{}::{name}({})", self.class_name(class), params.join(", "))))
    }

    fn wrap_method(&self, m: *const MethodInfo) -> Method {
        let flags = unsafe { (self.api.il2cpp_method_get_flags)(m, std::ptr::null_mut()) };
        let name = unsafe { cstr((self.api.il2cpp_method_get_name)(m)) };
        Method { ptr: m, name, is_static: flags & METHOD_ATTRIBUTE_STATIC != 0 }
    }

    /// Looks up a field by name (walks base classes).
    pub fn field(&self, class: Class, name: &str) -> Result<Field> {
        let c = CString::new(name).map_err(|_| Error::MemberNotFound(name.into()))?;
        let f = unsafe { (self.api.il2cpp_class_get_field_from_name)(class.0, c.as_ptr()) };
        if f.is_null() {
            return Err(Error::MemberNotFound(format!("{}::{name}", self.class_name(class))));
        }
        let (offset, flags) = unsafe { ((self.api.il2cpp_field_get_offset)(f), (self.api.il2cpp_field_get_flags)(f)) };
        Ok(Field { ptr: f, offset, is_static: flags & FIELD_ATTRIBUTE_STATIC != 0 })
    }

    /// Resolves an engine internal call, e.g. `UnityEngine.Time::get_frameCount()`.
    ///
    /// # Safety
    /// `F` must be a function pointer type matching the icall's native signature.
    pub unsafe fn icall<F: Copy>(&self, name: &str) -> Result<F> {
        assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*const c_void>());
        let c = CString::new(name).map_err(|_| Error::IcallNotFound(name.into()))?;
        let p = (self.api.il2cpp_resolve_icall)(c.as_ptr());
        if p.is_null() {
            return Err(Error::IcallNotFound(name.into()));
        }
        Ok(std::mem::transmute_copy::<*const c_void, F>(&p))
    }

    /// Reads a static field.
    ///
    /// # Safety
    /// `T` must match the field's storage type (object references are `*mut Il2CppObject`).
    pub unsafe fn static_value<T: Copy + Default>(&self, field: Field) -> T {
        debug_assert!(field.is_static);
        let mut out = T::default();
        (self.api.il2cpp_field_static_get_value)(field.ptr, &mut out as *mut T as *mut c_void);
        out
    }

    /// Reads a field whether it is static or an instance field (`instance` is ignored for
    /// static fields; a null `instance` yields `T::default()` for instance fields).
    ///
    /// # Safety
    /// Same as [`Runtime::static_value`] / [`Field::get`].
    pub unsafe fn read_field<T: Copy + Default>(&self, field: Field, instance: *mut Il2CppObject) -> T {
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
    pub unsafe fn invoke(&self, method: &Method, instance: *mut Il2CppObject, args: &mut [*mut c_void]) -> Result<*mut Il2CppObject> {
        let mut exc: *mut Il2CppObject = std::ptr::null_mut();
        let params = if args.is_empty() { std::ptr::null_mut() } else { args.as_mut_ptr() };
        let ret = (self.api.il2cpp_runtime_invoke)(method.ptr, instance as *mut c_void, params, &mut exc);
        if !exc.is_null() {
            return Err(Error::ManagedException(method.name.clone()));
        }
        Ok(ret)
    }

    /// Unboxes a value-type result from [`Runtime::invoke`].
    ///
    /// # Safety
    /// `boxed` must be a boxed `T`.
    pub unsafe fn unbox<T: Copy>(&self, boxed: *mut Il2CppObject) -> Result<T> {
        if boxed.is_null() {
            return Err(Error::Null);
        }
        Ok(*((self.api.il2cpp_object_unbox)(boxed) as *const T))
    }

    /// Name of a boxed enum value (`System.Enum.ToString()`), e.g. as returned by
    /// [`Runtime::invoke`] for an enum-typed getter.
    ///
    /// # Safety
    /// `boxed` must be null or a boxed enum. Must run on an attached thread.
    pub unsafe fn enum_name(&self, boxed: *mut Il2CppObject) -> Option<String> {
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
        let array = (self.api.il2cpp_array_new)(element.0, len);
        if array.is_null() {
            return Err(Error::Null);
        }
        let class = (self.api.il2cpp_object_get_class)(array);
        let stride = (self.api.il2cpp_array_element_size)(class).max(0) as usize;
        Ok(ManagedArray { object: array, stride, len })
    }

    /// The `System.Type` object for a class, as needed by `GetComponent(Type)`.
    pub fn type_object(&self, class: Class) -> *mut Il2CppObject {
        unsafe { (self.api.il2cpp_type_get_object)((self.api.il2cpp_class_get_type)(class.0)) }
    }

    /// # Safety
    /// `obj` must be null or a live managed object.
    pub unsafe fn class_of(&self, obj: *mut Il2CppObject) -> Option<Class> {
        if obj.is_null() {
            return None;
        }
        let c = (self.api.il2cpp_object_get_class)(obj);
        (!c.is_null()).then_some(Class(c))
    }

    /// Whether values of `derived` can be assigned to `base`.
    pub fn is_assignable(&self, base: Class, derived: Class) -> bool {
        unsafe { (self.api.il2cpp_class_is_assignable_from)(base.0, derived.0) }
    }

    pub fn class_name(&self, class: Class) -> String {
        unsafe { cstr((self.api.il2cpp_class_get_name)(class.0)) }
    }

    /// Allocates a managed string.
    pub fn new_string(&self, s: &str) -> *mut Il2CppObject {
        let c = CString::new(s.replace('\0', "")).unwrap();
        unsafe { (self.api.il2cpp_string_new)(c.as_ptr()) }
    }

    /// Native code address of a method (`MethodInfo::methodPointer`, the first field in
    /// every IL2CPP version). Used to install frame hooks.
    pub fn method_pointer(&self, method: &Method) -> *const c_void {
        unsafe { *(method.ptr as *const *const c_void) }
    }
}

/// Keeps a non-game thread attached to the runtime.
pub struct ThreadGuard<'a> {
    rt: &'a Runtime,
    thread: *mut Il2CppThread,
}

impl Drop for ThreadGuard<'_> {
    fn drop(&mut self) {
        if !self.thread.is_null() {
            unsafe { (self.rt.api.il2cpp_thread_detach)(self.thread) };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Class(*mut Il2CppClass);
unsafe impl Send for Class {}
unsafe impl Sync for Class {}

#[derive(Debug, Clone)]
pub struct Method {
    ptr: *const MethodInfo,
    name: String,
    pub is_static: bool,
}
unsafe impl Send for Method {}
unsafe impl Sync for Method {}

#[derive(Debug, Clone, Copy)]
pub struct Field {
    ptr: *mut FieldInfo,
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
    pub unsafe fn get<T: Copy>(&self, obj: *mut Il2CppObject) -> T {
        debug_assert!(!self.is_static);
        std::ptr::read_unaligned((obj as *const u8).add(self.offset) as *const T)
    }
}

/// A managed array allocated by [`Runtime::new_array`].
#[derive(Debug, Clone, Copy)]
pub struct ManagedArray {
    pub object: *mut Il2CppObject,
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

/// Reads a `System.String`.
///
/// # Safety
/// `s` must be null or a live `System.String`.
pub unsafe fn read_string(s: *mut Il2CppObject) -> Option<String> {
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

/// Elements of a managed reference-type array (`T[]` where `T` is a class).
///
/// # Safety
/// `array` must be null or a live array of object references.
pub unsafe fn read_object_array(array: *mut Il2CppObject, max_len: usize) -> Vec<*mut Il2CppObject> {
    if array.is_null() {
        return Vec::new();
    }
    let base = array as *const u8;
    let len = (*(base.add(layout::ARRAY_LENGTH) as *const usize)).min(max_len);
    let data = base.add(layout::ARRAY_DATA) as *const *mut Il2CppObject;
    (0..len).map(|i| *data.add(i)).collect()
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
}
