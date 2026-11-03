//! IL2CPP backend: the runtime API exported by `GameAssembly.dll`.

use std::ffi::{c_char, c_void, CStr, CString};

use windows::core::{w, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

use crate::backend::{cstr, Backend, Ptr};
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

api! {
    il2cpp_domain_get: fn() -> Ptr;
    il2cpp_domain_get_assemblies: fn(Ptr, *mut usize) -> *const Ptr;
    il2cpp_assembly_get_image: fn(Ptr) -> Ptr;
    il2cpp_thread_attach: fn(Ptr) -> Ptr;
    il2cpp_thread_detach: fn(Ptr);
    il2cpp_class_from_name: fn(Ptr, *const c_char, *const c_char) -> Ptr;
    il2cpp_class_get_nested_types: fn(Ptr, *mut Ptr) -> Ptr;
    il2cpp_class_get_method_from_name: fn(Ptr, *const c_char, i32) -> Ptr;
    il2cpp_class_get_field_from_name: fn(Ptr, *const c_char) -> Ptr;
    il2cpp_class_get_type: fn(Ptr) -> Ptr;
    il2cpp_class_get_name: fn(Ptr) -> *const c_char;
    il2cpp_class_get_parent: fn(Ptr) -> Ptr;
    il2cpp_class_is_assignable_from: fn(Ptr, Ptr) -> bool;
    il2cpp_type_get_object: fn(Ptr) -> Ptr;
    il2cpp_field_get_offset: fn(Ptr) -> usize;
    il2cpp_field_get_flags: fn(Ptr) -> i32;
    il2cpp_field_static_get_value: fn(Ptr, *mut c_void);
    il2cpp_runtime_invoke: fn(Ptr, Ptr, *mut Ptr, *mut Ptr) -> Ptr;
    il2cpp_resolve_icall: fn(*const c_char) -> *const c_void;
    il2cpp_object_unbox: fn(Ptr) -> *mut c_void;
    il2cpp_object_get_class: fn(Ptr) -> Ptr;
    il2cpp_string_new: fn(*const c_char) -> Ptr;
    il2cpp_object_new: fn(Ptr) -> Ptr;
    il2cpp_runtime_object_init: fn(Ptr);
    il2cpp_class_get_methods: fn(Ptr, *mut Ptr) -> Ptr;
    il2cpp_method_get_name: fn(Ptr) -> *const c_char;
    il2cpp_method_get_param_count: fn(Ptr) -> u32;
    il2cpp_method_get_param: fn(Ptr, u32) -> Ptr;
    il2cpp_method_get_flags: fn(Ptr, *mut u32) -> u32;
    il2cpp_type_get_name: fn(Ptr) -> *mut c_char;
    il2cpp_free: fn(*mut c_void);
    il2cpp_array_new: fn(Ptr, usize) -> Ptr;
    il2cpp_array_element_size: fn(Ptr) -> i32;
}

pub(crate) struct Il2Cpp {
    api: Api,
    domain: Ptr,
}

// The runtime is process-global and its API is thread-safe for attached threads.
unsafe impl Send for Il2Cpp {}
unsafe impl Sync for Il2Cpp {}

impl Il2Cpp {
    /// Binds to `GameAssembly.dll` in the current process.
    pub(crate) fn load() -> Result<Self> {
        unsafe {
            let module = GetModuleHandleW(w!("GameAssembly.dll")).map_err(|_| Error::NoRuntime)?;
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
            Ok(Self { api, domain })
        }
    }
}

impl Backend for Il2Cpp {
    unsafe fn attach_thread(&self) -> Ptr {
        (self.api.il2cpp_thread_attach)(self.domain)
    }

    unsafe fn detach_thread(&self, thread: Ptr) {
        (self.api.il2cpp_thread_detach)(thread)
    }

    unsafe fn images(&self) -> Vec<Ptr> {
        let mut count = 0usize;
        let assemblies = (self.api.il2cpp_domain_get_assemblies)(self.domain, &mut count);
        (0..count).map(|i| (self.api.il2cpp_assembly_get_image)(*assemblies.add(i))).collect()
    }

    unsafe fn class_from_name(&self, image: Ptr, namespace: &CStr, name: &CStr) -> Ptr {
        (self.api.il2cpp_class_from_name)(image, namespace.as_ptr(), name.as_ptr())
    }

    unsafe fn nested_class(&self, class: Ptr, name: &str) -> Ptr {
        let mut iter: Ptr = std::ptr::null_mut();
        loop {
            let nested = (self.api.il2cpp_class_get_nested_types)(class, &mut iter);
            if nested.is_null() || self.class_name(nested) == name {
                return nested;
            }
        }
    }

    unsafe fn class_name(&self, class: Ptr) -> String {
        cstr((self.api.il2cpp_class_get_name)(class))
    }

    unsafe fn class_parent(&self, class: Ptr) -> Ptr {
        (self.api.il2cpp_class_get_parent)(class)
    }

    unsafe fn is_assignable_from(&self, base: Ptr, derived: Ptr) -> bool {
        (self.api.il2cpp_class_is_assignable_from)(base, derived)
    }

    unsafe fn type_object(&self, class: Ptr) -> Ptr {
        (self.api.il2cpp_type_get_object)((self.api.il2cpp_class_get_type)(class))
    }

    unsafe fn method_from_name(&self, class: Ptr, name: &CStr, args: i32) -> Ptr {
        (self.api.il2cpp_class_get_method_from_name)(class, name.as_ptr(), args)
    }

    unsafe fn methods(&self, class: Ptr) -> Vec<Ptr> {
        let mut iter: Ptr = std::ptr::null_mut();
        let mut out = Vec::new();
        loop {
            let m = (self.api.il2cpp_class_get_methods)(class, &mut iter);
            if m.is_null() {
                return out;
            }
            out.push(m);
        }
    }

    unsafe fn method_name(&self, method: Ptr) -> String {
        cstr((self.api.il2cpp_method_get_name)(method))
    }

    unsafe fn method_param_types(&self, method: Ptr) -> Vec<String> {
        let count = (self.api.il2cpp_method_get_param_count)(method);
        (0..count)
            .map(|i| {
                let raw = (self.api.il2cpp_type_get_name)((self.api.il2cpp_method_get_param)(method, i));
                let name = cstr(raw);
                (self.api.il2cpp_free)(raw as *mut c_void);
                name
            })
            .collect()
    }

    unsafe fn method_flags(&self, method: Ptr) -> (u32, u32) {
        let mut iflags = 0u32;
        let flags = (self.api.il2cpp_method_get_flags)(method, &mut iflags);
        (flags, iflags)
    }

    unsafe fn method_code(&self, method: Ptr) -> *const c_void {
        // `MethodInfo::methodPointer` is the first field in every IL2CPP version.
        *(method as *const *const c_void)
    }

    unsafe fn field_from_name(&self, class: Ptr, name: &CStr) -> Ptr {
        (self.api.il2cpp_class_get_field_from_name)(class, name.as_ptr())
    }

    unsafe fn field_offset(&self, field: Ptr) -> usize {
        (self.api.il2cpp_field_get_offset)(field)
    }

    unsafe fn field_flags(&self, field: Ptr) -> u32 {
        (self.api.il2cpp_field_get_flags)(field) as u32
    }

    unsafe fn field_static_get(&self, field: Ptr, out: *mut c_void) {
        (self.api.il2cpp_field_static_get_value)(field, out)
    }

    unsafe fn icall_by_name(&self, name: &str) -> *const c_void {
        match CString::new(name) {
            Ok(c) => (self.api.il2cpp_resolve_icall)(c.as_ptr()),
            Err(_) => std::ptr::null(),
        }
    }

    unsafe fn icall_for_method(&self, _method: Ptr) -> *const c_void {
        std::ptr::null()
    }

    unsafe fn invoke(&self, method: Ptr, instance: Ptr, args: *mut Ptr, exc: *mut Ptr) -> Ptr {
        (self.api.il2cpp_runtime_invoke)(method, instance, args, exc)
    }

    unsafe fn unbox(&self, boxed: Ptr) -> *mut c_void {
        (self.api.il2cpp_object_unbox)(boxed)
    }

    unsafe fn object_class(&self, obj: Ptr) -> Ptr {
        (self.api.il2cpp_object_get_class)(obj)
    }

    unsafe fn new_string(&self, s: &CStr) -> Ptr {
        (self.api.il2cpp_string_new)(s.as_ptr())
    }

    unsafe fn new_object(&self, class: Ptr) -> Ptr {
        let obj = (self.api.il2cpp_object_new)(class);
        if !obj.is_null() {
            (self.api.il2cpp_runtime_object_init)(obj);
        }
        obj
    }

    unsafe fn new_array(&self, element: Ptr, len: usize) -> Ptr {
        (self.api.il2cpp_array_new)(element, len)
    }

    unsafe fn array_element_size(&self, array_class: Ptr) -> i32 {
        (self.api.il2cpp_array_element_size)(array_class)
    }
}
