//! Mono backend: the embedding API exported by Unity's Mono runtime
//! (`mono-2.0-bdwgc.dll`, or `mono.dll` in old Unity versions).
//!
//! Differences from IL2CPP that this module hides:
//! - Assemblies are enumerated with `mono_assembly_foreach`.
//! - Static fields are read through the class's `MonoVTable` in the root domain.
//! - Methods only become native code once JIT-compiled (`mono_compile_method`).
//! - Internal calls are looked up per method (`mono_lookup_internal_call`), not by name.
//!
//! Object, string and array layouts match IL2CPP on 64-bit, so the shared readers in
//! the crate root work unchanged.

use std::ffi::{c_char, c_void, CStr};

use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

use crate::backend::{cstr, Backend, Ptr, WeakHandles};
use crate::{Error, Result};

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

type AssemblyCallback = unsafe extern "C" fn(Ptr, *mut c_void);

api! {
    mono_get_root_domain: fn() -> Ptr;
    mono_thread_attach: fn(Ptr) -> Ptr;
    mono_thread_detach: fn(Ptr);
    mono_assembly_foreach: fn(AssemblyCallback, *mut c_void);
    mono_assembly_get_image: fn(Ptr) -> Ptr;
    mono_image_get_name: fn(Ptr) -> *const c_char;
    mono_class_from_name: fn(Ptr, *const c_char, *const c_char) -> Ptr;
    mono_class_get_nested_types: fn(Ptr, *mut Ptr) -> Ptr;
    mono_class_get_method_from_name: fn(Ptr, *const c_char, i32) -> Ptr;
    mono_class_get_methods: fn(Ptr, *mut Ptr) -> Ptr;
    mono_class_get_parent: fn(Ptr) -> Ptr;
    mono_class_get_field_from_name: fn(Ptr, *const c_char) -> Ptr;
    mono_class_get_name: fn(Ptr) -> *const c_char;
    mono_class_get_type: fn(Ptr) -> Ptr;
    mono_class_is_assignable_from: fn(Ptr, Ptr) -> i32;
    mono_class_vtable: fn(Ptr, Ptr) -> Ptr;
    mono_field_get_offset: fn(Ptr) -> u32;
    mono_field_get_type: fn(Ptr) -> Ptr;
    mono_field_get_flags: fn(Ptr) -> u32;
    mono_field_get_parent: fn(Ptr) -> Ptr;
    mono_field_static_get_value: fn(Ptr, Ptr, *mut c_void);
    mono_method_get_name: fn(Ptr) -> *const c_char;
    mono_method_signature: fn(Ptr) -> Ptr;
    mono_signature_get_params: fn(Ptr, *mut Ptr) -> Ptr;
    mono_method_get_flags: fn(Ptr, *mut u32) -> u32;
    mono_type_get_name: fn(Ptr) -> *mut c_char;
    mono_type_get_object: fn(Ptr, Ptr) -> Ptr;
    mono_free: fn(*mut c_void);
    mono_runtime_invoke: fn(Ptr, Ptr, *mut Ptr, *mut Ptr) -> Ptr;
    mono_object_unbox: fn(Ptr) -> *mut c_void;
    mono_object_get_class: fn(Ptr) -> Ptr;
    mono_string_new: fn(Ptr, *const c_char) -> Ptr;
    mono_object_new: fn(Ptr, Ptr) -> Ptr;
    mono_runtime_object_init: fn(Ptr);
    mono_array_new: fn(Ptr, Ptr, usize) -> Ptr;
    mono_array_element_size: fn(Ptr) -> i32;
    mono_compile_method: fn(Ptr) -> *const c_void;
    mono_lookup_internal_call: fn(Ptr) -> *const c_void;
}

/// `METHOD_IMPL_ATTRIBUTE_INTERNAL_CALL` from ECMA-335.
const METHOD_IMPL_ATTRIBUTE_INTERNAL_CALL: u32 = 0x1000;

/// Module names of Unity's Mono runtime, newest first.
const MODULES: [PCWSTR; 3] = [w!("mono-2.0-bdwgc.dll"), w!("mono-2.0-sgen.dll"), w!("mono.dll")];

/// The game's scripts assembly; the domain is ready for lookups once it is loaded.
const GAME_ASSEMBLY: &str = "Assembly-CSharp";

pub(crate) struct Mono {
    api: Api,
    domain: Ptr,
    weak: Option<WeakHandles>,
}

// The runtime is process-global and its API is thread-safe for attached threads.
unsafe impl Send for Mono {}
unsafe impl Sync for Mono {}

impl Mono {
    /// Binds to Unity's Mono runtime in the current process.
    pub(crate) fn load() -> Result<Self> {
        unsafe {
            let module = MODULES.iter().find_map(|m| GetModuleHandleW(*m).ok()).ok_or(Error::NoRuntime)?;
            let api = Api::load(module)?;
            let domain = (api.mono_get_root_domain)();
            if domain.is_null() {
                return Err(Error::DomainNotReady);
            }
            let mono = Self { api, domain, weak: WeakHandles::load(module, "mono_") };
            let ready = mono.images().iter().any(|&i| cstr((mono.api.mono_image_get_name)(i)) == GAME_ASSEMBLY);
            if !ready {
                return Err(Error::DomainNotReady);
            }
            Ok(mono)
        }
    }
}

unsafe extern "C" fn collect_assembly(assembly: Ptr, out: *mut c_void) {
    (*(out as *mut Vec<Ptr>)).push(assembly);
}

impl Backend for Mono {
    unsafe fn attach_thread(&self) -> Ptr {
        (self.api.mono_thread_attach)(self.domain)
    }

    unsafe fn detach_thread(&self, thread: Ptr) {
        (self.api.mono_thread_detach)(thread)
    }

    unsafe fn images(&self) -> Vec<Ptr> {
        let mut assemblies: Vec<Ptr> = Vec::new();
        (self.api.mono_assembly_foreach)(collect_assembly, &mut assemblies as *mut Vec<Ptr> as *mut c_void);
        assemblies
            .into_iter()
            .map(|a| (self.api.mono_assembly_get_image)(a))
            .filter(|i| !i.is_null())
            .collect()
    }

    unsafe fn class_from_name(&self, image: Ptr, namespace: &CStr, name: &CStr) -> Ptr {
        (self.api.mono_class_from_name)(image, namespace.as_ptr(), name.as_ptr())
    }

    unsafe fn nested_class(&self, class: Ptr, name: &str) -> Ptr {
        let mut iter: Ptr = std::ptr::null_mut();
        loop {
            let nested = (self.api.mono_class_get_nested_types)(class, &mut iter);
            if nested.is_null() || self.class_name(nested) == name {
                return nested;
            }
        }
    }

    unsafe fn class_name(&self, class: Ptr) -> String {
        cstr((self.api.mono_class_get_name)(class))
    }

    unsafe fn class_parent(&self, class: Ptr) -> Ptr {
        (self.api.mono_class_get_parent)(class)
    }

    unsafe fn is_assignable_from(&self, base: Ptr, derived: Ptr) -> bool {
        (self.api.mono_class_is_assignable_from)(base, derived) != 0
    }

    unsafe fn type_object(&self, class: Ptr) -> Ptr {
        (self.api.mono_type_get_object)(self.domain, (self.api.mono_class_get_type)(class))
    }

    unsafe fn method_from_name(&self, class: Ptr, name: &CStr, args: i32) -> Ptr {
        (self.api.mono_class_get_method_from_name)(class, name.as_ptr(), args)
    }

    unsafe fn methods(&self, class: Ptr) -> Vec<Ptr> {
        let mut iter: Ptr = std::ptr::null_mut();
        let mut out = Vec::new();
        loop {
            let m = (self.api.mono_class_get_methods)(class, &mut iter);
            if m.is_null() {
                return out;
            }
            out.push(m);
        }
    }

    unsafe fn method_name(&self, method: Ptr) -> String {
        cstr((self.api.mono_method_get_name)(method))
    }

    unsafe fn method_param_types(&self, method: Ptr) -> Vec<String> {
        let sig = (self.api.mono_method_signature)(method);
        if sig.is_null() {
            return Vec::new();
        }
        let mut iter: Ptr = std::ptr::null_mut();
        let mut out = Vec::new();
        loop {
            let ty = (self.api.mono_signature_get_params)(sig, &mut iter);
            if ty.is_null() {
                return out;
            }
            let raw = (self.api.mono_type_get_name)(ty);
            out.push(cstr(raw));
            (self.api.mono_free)(raw as *mut c_void);
        }
    }

    unsafe fn method_flags(&self, method: Ptr) -> (u32, u32) {
        let mut iflags = 0u32;
        let flags = (self.api.mono_method_get_flags)(method, &mut iflags);
        (flags, iflags)
    }

    unsafe fn method_code(&self, method: Ptr) -> *const c_void {
        (self.api.mono_compile_method)(method)
    }

    unsafe fn field_from_name(&self, class: Ptr, name: &CStr) -> Ptr {
        (self.api.mono_class_get_field_from_name)(class, name.as_ptr())
    }

    unsafe fn field_type_name(&self, field: Ptr) -> String {
        let raw = (self.api.mono_type_get_name)((self.api.mono_field_get_type)(field));
        let name = cstr(raw);
        (self.api.mono_free)(raw as *mut c_void);
        name
    }

    unsafe fn field_offset(&self, field: Ptr) -> usize {
        (self.api.mono_field_get_offset)(field) as usize
    }

    unsafe fn field_flags(&self, field: Ptr) -> u32 {
        (self.api.mono_field_get_flags)(field)
    }

    unsafe fn field_static_get(&self, field: Ptr, out: *mut c_void) {
        let vtable = (self.api.mono_class_vtable)(self.domain, (self.api.mono_field_get_parent)(field));
        if !vtable.is_null() {
            (self.api.mono_field_static_get_value)(vtable, field, out);
        }
    }

    unsafe fn icall_by_name(&self, _name: &str) -> *const c_void {
        std::ptr::null()
    }

    unsafe fn icall_for_method(&self, method: Ptr) -> *const c_void {
        // Mono warns about (and some builds abort on) lookups for managed methods.
        if self.method_flags(method).1 & METHOD_IMPL_ATTRIBUTE_INTERNAL_CALL == 0 {
            return std::ptr::null();
        }
        (self.api.mono_lookup_internal_call)(method)
    }

    unsafe fn invoke(&self, method: Ptr, instance: Ptr, args: *mut Ptr, exc: *mut Ptr) -> Ptr {
        (self.api.mono_runtime_invoke)(method, instance, args, exc)
    }

    unsafe fn unbox(&self, boxed: Ptr) -> *mut c_void {
        (self.api.mono_object_unbox)(boxed)
    }

    unsafe fn object_class(&self, obj: Ptr) -> Ptr {
        (self.api.mono_object_get_class)(obj)
    }

    unsafe fn new_string(&self, s: &CStr) -> Ptr {
        (self.api.mono_string_new)(self.domain, s.as_ptr())
    }

    unsafe fn new_object(&self, class: Ptr) -> Ptr {
        let obj = (self.api.mono_object_new)(self.domain, class);
        if !obj.is_null() {
            (self.api.mono_runtime_object_init)(obj);
        }
        obj
    }

    unsafe fn new_value(&self, class: Ptr) -> Ptr {
        (self.api.mono_object_new)(self.domain, class)
    }

    unsafe fn new_array(&self, element: Ptr, len: usize) -> Ptr {
        (self.api.mono_array_new)(self.domain, element, len)
    }

    unsafe fn array_element_size(&self, array_class: Ptr) -> i32 {
        (self.api.mono_array_element_size)(array_class)
    }

    fn weak_handles(&self) -> Option<WeakHandles> {
        self.weak
    }
}
