//! Name-based access to a running Unreal Engine 5 game from code inside its process: its
//! objects, their properties, and calls to their functions, all looked up by **name**
//! through the engine's own reflection data.
//!
//! Nothing here is written down per game or per engine version. The name pool and the
//! object table are found by their shape in the executable's data (see [`names`] and
//! [`objects`]), the reflection structs' layouts by reading engine types whose contents
//! are known (see [`layout`]), and `UObject::ProcessEvent` by what its code checks. That
//! keeps integrations working across most game updates, the way `unity-runtime` does for
//! Unity games.
//!
//! Safety model: objects are plain addresses ([`Obj`]). Anything that reads them or calls
//! into the game is `unsafe`: the caller must be on the game thread and know the object
//! is alive ([`Unreal::alive`] checks one held across frames).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub mod dump;
mod layout;
mod mem;
mod names;
mod objects;
mod reflect;
mod types;
mod usmap;

pub use layout::Layout;
pub use mem::Image;
pub use names::FName;
pub use reflect::{Args, Function, Property, Type};
pub use types::*;

use layout::{Probe, FIELD_NEXT, OBJECT_CLASS, OBJECT_FLAGS, OBJECT_NAME, OBJECT_OUTER};
use names::Names;
use objects::Objects;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not an Unreal Engine game, or its engine isn't ready yet ({0} not found)")]
    NoEngine(&'static str),
    #[error("unexpected engine layout: {0} not found")]
    Layout(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Type(String),
    #[error("null object")]
    Null,
}

pub type Result<T> = std::result::Result<T, Error>;

/// A `UObject` (or `UClass`, `UFunction`, ...) by address.
pub type Obj = usize;

/// `EObjectFlags`
const RF_CLASS_DEFAULT_OBJECT: u32 = 0x10;
const RF_ARCHETYPE_OBJECT: u32 = 0x20;
const RF_BEGIN_DESTROYED: u32 = 0x8000;
const RF_FINISH_DESTROYED: u32 = 0x10000;

/// Functions looked up by class and name, `None` for ones that don't exist.
type FunctionCache = Mutex<HashMap<(Obj, String), Option<Arc<Function>>>>;

/// The engine of the current process.
pub struct Unreal {
    image: Image,
    names: Names,
    objects: Objects,
    layout: Layout,
    /// `UObject::ProcessEvent`'s slot in every object's virtual table.
    process_event: usize,
    struct_class: Obj,
    structs: Mutex<HashMap<String, Obj>>,
    properties: Mutex<HashMap<(Obj, String), Option<Property>>>,
    functions: FunctionCache,
}

impl Unreal {
    /// Finds the engine's tables in the current process. Returns [`Error::NoEngine`] if
    /// this isn't an Unreal game or it hasn't started its engine yet: callers injected
    /// early should retry.
    pub fn load() -> Result<Self> {
        let image = Image::main().ok_or(Error::NoEngine("executable"))?;
        let names = Names::find(&image).ok_or(Error::NoEngine("name pool"))?;
        let objects = Objects::find(&image).ok_or(Error::NoEngine("object array"))?;
        let probe = Probe { names: &names, objects: &objects };
        let layout = Layout::find(&probe)?;
        let struct_class = probe.find("/Script/CoreUObject", "Struct", "Class")?;
        let mut ue = Self {
            image,
            names,
            objects,
            layout,
            process_event: 0,
            struct_class,
            structs: Mutex::new(HashMap::new()),
            properties: Mutex::new(HashMap::new()),
            functions: Mutex::new(HashMap::new()),
        };
        ue.process_event = ue.find_process_event().ok_or_else(|| Error::Layout("UObject::ProcessEvent".into()))?;
        Ok(ue)
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn process_event_index(&self) -> usize {
        self.process_event
    }

    /// How many object slots are in use.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// The object in slot `index`, if any.
    pub fn object(&self, index: usize) -> Option<Obj> {
        self.objects.get(index)
    }

    // --- names and identity -----------------------------------------------------------

    pub fn fname(&self, name: FName) -> String {
        self.names.text(name)
    }

    /// Whether `name` is `text`, without building a string.
    pub fn fname_is(&self, name: FName, text: &str) -> bool {
        self.names.is(name, text)
    }

    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn name(&self, obj: Obj) -> String {
        mem::try_read(obj + OBJECT_NAME).map(|n| self.names.text(n)).unwrap_or_default()
    }

    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn name_is(&self, obj: Obj, text: &str) -> bool {
        mem::try_read(obj + OBJECT_NAME).is_some_and(|n| self.names.is(n, text))
    }

    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn class_of(&self, obj: Obj) -> Obj {
        mem::try_ptr(obj + OBJECT_CLASS).unwrap_or(0)
    }

    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn outer(&self, obj: Obj) -> Option<Obj> {
        mem::try_ptr(obj + OBJECT_OUTER)
    }

    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn flags(&self, obj: Obj) -> u32 {
        mem::read(obj + OBJECT_FLAGS)
    }

    /// The object's path, as `UObject::GetPathName` writes it: `/Script/Engine.Actor`,
    /// `/Script/Engine.Actor:K2_GetActorLocation`.
    ///
    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn path(&self, obj: Obj) -> String {
        let mut chain = vec![obj];
        while let Some(outer) = self.outer(*chain.last().unwrap()).filter(|_| chain.len() < 32) {
            chain.push(outer);
        }
        let mut path = String::new();
        for (i, &o) in chain.iter().rev().enumerate() {
            if i > 0 {
                // ':' after an object whose outer is a package's direct child.
                let parent = chain[chain.len() - i];
                let sep = if i >= 2 && self.outer(parent).is_some_and(|gp| self.outer(gp).is_none()) { ':' } else { '.' };
                path.push(sep);
            }
            path.push_str(&self.name(o));
        }
        path
    }

    /// Whether an object held since an earlier frame still exists: its slot still holds
    /// it, and it isn't being destroyed.
    pub fn alive(&self, obj: Obj) -> bool {
        let Some(index) = mem::try_read::<i32>(obj + objects::OBJECT_INDEX) else { return false };
        index >= 0
            && self.objects.get(index as usize) == Some(obj)
            && mem::try_read::<u32>(obj + OBJECT_FLAGS).is_some_and(|f| f & (RF_BEGIN_DESTROYED | RF_FINISH_DESTROYED) == 0)
    }

    // --- classes and structs ----------------------------------------------------------

    /// # Safety
    /// `strukt` must be a live struct or class.
    pub unsafe fn super_of(&self, strukt: Obj) -> Option<Obj> {
        mem::try_ptr(strukt + self.layout.super_struct)
    }

    /// Whether `obj`'s class is `class` or derives from it.
    ///
    /// # Safety
    /// `obj` must be a live object.
    pub unsafe fn is_a(&self, obj: Obj, class: Obj) -> bool {
        self.derives(self.class_of(obj), class)
    }

    /// Whether struct `strukt` is `base` or derives from it.
    ///
    /// # Safety
    /// `strukt` must be a live struct or class.
    pub unsafe fn derives(&self, strukt: Obj, base: Obj) -> bool {
        // A chain this long is memory being freed under a reader off the game thread.
        const MAX_DEPTH: usize = 64;
        let mut s = Some(strukt).filter(|&c| c != 0);
        for _ in 0..MAX_DEPTH {
            let Some(c) = s else { break };
            if c == base {
                return true;
            }
            s = self.super_of(c);
        }
        false
    }

    /// A class or struct by name (`Actor`) or path (`/Script/Engine.Actor`).
    pub fn find_struct(&self, name: &str) -> Result<Obj> {
        let known = self.structs.lock().unwrap().get(name).copied();
        if let Some(s) = known.filter(|&s| self.alive(s)) {
            return Ok(s);
        }
        let by_path = name.starts_with('/');
        let short = name.rsplit(['.', ':']).next().unwrap_or(name);
        let found = (0..self.objects.len()).filter_map(|i| self.objects.get(i)).find(|&o| unsafe {
            self.name_is(o, short) && self.derives(self.class_of(o), self.struct_class) && !self.derives(self.class_of(o), self.function_class()) && (!by_path || self.path(o) == name)
        });
        let found = found.ok_or_else(|| Error::NotFound(format!("class {name}")))?;
        self.structs.lock().unwrap().insert(name.to_owned(), found);
        Ok(found)
    }

    fn function_class(&self) -> Obj {
        // Read into a local first: a guard held through a fallback that locks again deadlocks.
        let known = self.structs.lock().unwrap().get("\0Function").copied();
        if let Some(f) = known {
            return f;
        }
        let f = (0..self.objects.len())
            .filter_map(|i| self.objects.get(i))
            .find(|&o| unsafe { self.name_is(o, "Function") && self.path(o) == "/Script/CoreUObject.Function" })
            .unwrap_or(0);
        self.structs.lock().unwrap().insert("\0Function".into(), f);
        f
    }

    /// Every live object that is a `class` (not class defaults or templates).
    pub fn instances_of(&self, class: Obj) -> Vec<Obj> {
        (0..self.objects.len())
            .filter_map(|i| self.objects.get(i))
            .filter(|&o| unsafe { self.flags(o) & (RF_CLASS_DEFAULT_OBJECT | RF_ARCHETYPE_OBJECT | RF_BEGIN_DESTROYED) == 0 && self.is_a(o, class) })
            .collect()
    }

    /// Looks through `count` object slots from `from` for a live `class` (not a default
    /// object): a search spread over several frames. Returns what it found and where the
    /// next search should start (back to 0 after the last slot).
    pub fn find_instance_from(&self, class: Obj, from: usize, count: usize) -> (Option<Obj>, usize) {
        let len = self.objects.len();
        let end = (from + count).min(len);
        let found = (from..end)
            .filter_map(|i| self.objects.get(i))
            .find(|&o| unsafe { self.flags(o) & (RF_CLASS_DEFAULT_OBJECT | RF_ARCHETYPE_OBJECT | RF_BEGIN_DESTROYED) == 0 && self.is_a(o, class) });
        (found, if end >= len { 0 } else { end })
    }

    /// The class's default object, which static functions are called on.
    ///
    /// # Safety
    /// `class` must be a live class.
    pub unsafe fn default_object(&self, class: Obj) -> Option<Obj> {
        Some(mem::read::<usize>(class + self.layout.default_object)).filter(|&d| d != 0)
    }

    // --- properties -------------------------------------------------------------------

    /// The properties `strukt` declares itself.
    ///
    /// # Safety
    /// `strukt` must be a live struct, class or function.
    pub unsafe fn own_properties(&self, strukt: Obj) -> Vec<Property> {
        layout::properties(&self.layout, strukt).filter_map(|f| self.read_property(f)).collect()
    }

    /// Size of a struct's instances.
    ///
    /// # Safety
    /// `strukt` must be a live struct or class.
    pub unsafe fn struct_size(&self, strukt: Obj) -> usize {
        mem::read::<i32>(strukt + self.layout.properties_size).max(0) as usize
    }

    pub(crate) unsafe fn field_class_name(&self, field: usize) -> String {
        mem::try_ptr(field + self.layout.field_class).and_then(mem::try_read::<FName>).map(|n| self.names.text(n)).unwrap_or_default()
    }

    /// Whether `field` is a property (its field class's name ends in `Property`).
    unsafe fn is_property(&self, field: usize) -> bool {
        self.field_class_name(field).ends_with("Property")
    }

    /// An array's element property: right after the property, or after its flags
    /// (`EArrayPropertyFlags` comes first from UE 5.5).
    ///
    /// # Safety
    /// `field` must be a live array property.
    pub(crate) unsafe fn array_inner(&self, field: usize) -> Option<usize> {
        let extra = field + self.layout.property_size;
        [extra, extra + 8].into_iter().filter_map(mem::try_ptr).find(|&f| self.is_property(f))
    }

    unsafe fn ref_name(&self, at: usize) -> String {
        mem::try_ptr(at).map(|o| self.name(o)).unwrap_or_else(|| "?".into())
    }

    unsafe fn read_type(&self, field: usize) -> Type {
        let extra = field + self.layout.property_size;
        match self.field_class_name(field).as_str() {
            "BoolProperty" => Type::Bool,
            "ByteProperty" => Type::Byte(mem::try_ptr(extra).map(|e| self.name(e))),
            "Int8Property" => Type::Int8,
            "Int16Property" => Type::Int16,
            "IntProperty" => Type::Int,
            "Int64Property" => Type::Int64,
            "UInt16Property" => Type::UInt16,
            "UInt32Property" => Type::UInt32,
            "UInt64Property" => Type::UInt64,
            "FloatProperty" => Type::Float,
            "DoubleProperty" => Type::Double,
            "StrProperty" => Type::Str,
            "NameProperty" => Type::Name,
            "TextProperty" => Type::Text,
            "ObjectProperty" | "ObjectPtrProperty" => Type::Object(self.ref_name(extra)),
            "WeakObjectProperty" => Type::WeakObject(self.ref_name(extra)),
            "SoftObjectProperty" | "LazyObjectProperty" => Type::SoftObject(self.ref_name(extra)),
            "ClassProperty" | "ClassPtrProperty" | "SoftClassProperty" => Type::Class(self.ref_name(extra + 8)),
            "InterfaceProperty" => Type::Interface(self.ref_name(extra)),
            "StructProperty" => Type::Struct(self.ref_name(extra)),
            "EnumProperty" => Type::Enum(self.ref_name(extra + 8)),
            "ArrayProperty" => Type::Array(Box::new(self.array_inner(field).map(|i| self.read_type(i)).unwrap_or(Type::Other("?".into())))),
            "SetProperty" => Type::Set(Box::new(mem::try_ptr(extra).map(|i| self.read_type(i)).unwrap_or(Type::Other("?".into())))),
            "MapProperty" => Type::Map(
                Box::new(mem::try_ptr(extra).map(|i| self.read_type(i)).unwrap_or(Type::Other("?".into()))),
                Box::new(mem::try_ptr(extra + 8).map(|i| self.read_type(i)).unwrap_or(Type::Other("?".into()))),
            ),
            other => Type::Other(other.to_owned()),
        }
    }

    unsafe fn read_property(&self, field: usize) -> Option<Property> {
        let l = &self.layout;
        let name = self.names.text(mem::try_read(field + l.field_name)?);
        let ty = self.read_type(field);
        let bit = (ty == Type::Bool).then(|| {
            let [_, byte_offset, byte_mask, _] = mem::read::<[u8; 4]>(field + l.property_size);
            (byte_offset as usize, byte_mask)
        });
        Some(Property {
            name,
            ty,
            offset: mem::read::<i32>(field + l.offset).max(0) as usize,
            size: mem::read::<i32>(field + l.element_size).max(0) as usize,
            array_dim: mem::read::<i32>(field + l.array_dim).max(1) as usize,
            flags: mem::read(field + l.property_flags),
            bit,
        })
    }

    /// Property `name` of `strukt` or a struct it derives from.
    pub fn property(&self, strukt: Obj, name: &str) -> Result<Property> {
        let key = (strukt, name.to_owned());
        if let Some(found) = self.properties.lock().unwrap().get(&key) {
            return found.clone().ok_or_else(|| Error::NotFound(format!("property {name}")));
        }
        let found = unsafe {
            let mut s = Some(strukt);
            let mut hit = None;
            while let (Some(c), None) = (s, &hit) {
                hit = self.own_properties(c).into_iter().find(|p| p.name == name);
                s = self.super_of(c);
            }
            hit
        };
        self.properties.lock().unwrap().insert(key, found.clone());
        found.ok_or_else(|| Error::NotFound(format!("property {name}")))
    }

    /// Property `name`, but only if its type is `ty` (as [`Type`] displays it, such as
    /// `Object<PlayerState>` or `Struct<Vector>`). A game update that changes the type
    /// then loses a feature instead of misreading memory.
    pub fn property_of(&self, strukt: Obj, name: &str, ty: &str) -> Result<Property> {
        let p = self.property(strukt, name)?;
        if p.ty.to_string() == ty {
            Ok(p)
        } else {
            Err(Error::Type(format!("property {name} is {}, not {ty}", p.ty)))
        }
    }

    /// Reads `prop` of `obj`.
    ///
    /// # Safety
    /// `obj` must be a live instance of the struct `prop` belongs to, and `T` must be
    /// the property's type in memory.
    pub unsafe fn get<T: Copy>(&self, obj: Obj, prop: &Property) -> T {
        debug_assert!(std::mem::size_of::<T>() <= prop.size * prop.array_dim);
        mem::read(obj + prop.offset)
    }

    /// Reads bool property `prop` of `obj`.
    ///
    /// # Safety
    /// `obj` must be a live instance of the struct `prop` belongs to.
    pub unsafe fn get_bool(&self, obj: Obj, prop: &Property) -> bool {
        match prop.bit {
            Some((byte, mask)) => mem::read::<u8>(obj + prop.offset + byte) & mask != 0,
            None => mem::read::<u8>(obj + prop.offset) != 0,
        }
    }

    /// Writes `prop` of `obj`.
    ///
    /// # Safety
    /// As [`Unreal::get`], and the game must not mind the value changing under it.
    pub unsafe fn set<T: Copy>(&self, obj: Obj, prop: &Property, value: T) {
        std::ptr::write_unaligned((obj + prop.offset) as *mut T, value)
    }

    /// Reads an object property: `Some` if it's set and the object is alive.
    ///
    /// # Safety
    /// As [`Unreal::get`].
    pub unsafe fn get_obj(&self, obj: Obj, prop: &Property) -> Option<Obj> {
        Some(self.get::<usize>(obj, prop)).filter(|&o| o != 0 && self.alive(o))
    }

    // --- functions --------------------------------------------------------------------

    /// Function `name` of `class` or a class it derives from.
    pub fn function(&self, class: Obj, name: &str) -> Result<Arc<Function>> {
        let key = (class, name.to_owned());
        if let Some(found) = self.functions.lock().unwrap().get(&key) {
            return found.clone().ok_or_else(|| Error::NotFound(format!("function {name}")));
        }
        let found = unsafe {
            let mut s = Some(class);
            let mut hit = None;
            while let (Some(c), None) = (s, &hit) {
                hit = self.own_functions(c).find(|&f| self.name_is(f, name)).map(|f| Arc::new(self.read_function(f, c)));
                s = self.super_of(c);
            }
            hit
        };
        self.functions.lock().unwrap().insert(key, found.clone());
        found.ok_or_else(|| Error::NotFound(format!("function {name}")))
    }

    /// The functions `class` declares itself.
    ///
    /// # Safety
    /// `class` must be a live class.
    pub unsafe fn own_functions(&self, class: Obj) -> impl Iterator<Item = Obj> + '_ {
        let mut next = mem::try_ptr(class + self.layout.children);
        std::iter::from_fn(move || {
            let f = next?;
            next = mem::try_ptr(f + FIELD_NEXT);
            Some(f)
        })
    }

    /// # Safety
    /// `f` must be a live function declared by `owner`.
    pub unsafe fn read_function(&self, f: Obj, owner: Obj) -> Function {
        Function {
            ptr: f,
            name: self.name(f),
            owner,
            flags: mem::read(f + self.layout.function_flags),
            params_size: self.struct_size(f),
            params: self.own_properties(f).into_iter().filter(|p| p.is_param()).collect(),
        }
    }

    /// Calls `function` on `obj` with `args`; static functions are called on their
    /// class's default object, and `obj` is ignored.
    ///
    /// # Safety
    /// Game thread only. `obj` must be alive and an instance of the function's class, and
    /// the arguments valid for the function: the game runs it as it is.
    pub unsafe fn call(&self, obj: Obj, args: &mut Args) -> Result<()> {
        let f = args.function;
        let target = if f.is_static() { self.default_object(f.owner).ok_or(Error::Null)? } else { obj };
        if target == 0 {
            return Err(Error::Null);
        }
        let vtable: usize = mem::read(target);
        let process_event: unsafe extern "system" fn(usize, usize, *mut u8) = std::mem::transmute(mem::read::<usize>(vtable + self.process_event * 8));
        process_event(target, f.ptr, args.as_ptr());
        Ok(())
    }

    /// Finds `ProcessEvent` by what it checks: whether the function is native, and whether
    /// it has out parameters.
    fn find_process_event(&self) -> Option<usize> {
        const NATIVE: u32 = 0x400;
        const HAS_OUT_PARMS: u32 = 0x40_0000;
        let obj = self.objects.get(0)?;
        let vtable = mem::try_read::<usize>(obj)?;
        (0..0x100).find(|&i| {
            let Some(mut code) = mem::try_read::<usize>(vtable + i * 8).filter(|&c| self.image.is_code(c)) else { return false };
            // Follow a jump thunk.
            if mem::try_read::<u8>(code) == Some(0xE9) {
                code = (code as i64 + 5 + mem::try_read::<i32>(code + 1).unwrap_or(0) as i64) as usize;
            }
            tests_flag(code, 0x400, self.layout.function_flags, NATIVE) && tests_flag(code, 0xF00, self.layout.function_flags, HAS_OUT_PARMS)
        })
    }

    // --- engine -----------------------------------------------------------------------

    /// The engine's version string (`5.4.4-0+UE5`), read by calling into the engine:
    /// also proves calls work.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn engine_version(&self) -> Result<String> {
        let class = self.find_struct("/Script/Engine.KismetSystemLibrary")?;
        let f = self.function(class, "GetEngineVersion")?;
        let mut args = Args::new(&f);
        self.call(0, &mut args)?;
        // The string was allocated by the game; it's small, and freed with the game.
        Ok(args.get::<FString>("ReturnValue")?.to_string_lossy())
    }
}

/// Whether the `len` bytes of code at `code` test `flag` in a `u32` at `offset` of a
/// register, as `test dword [reg+offset], flag` or one of its bytes.
fn tests_flag(code: usize, len: usize, offset: usize, flag: u32) -> bool {
    if !mem::readable(code, len + 16) {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(code as *const u8, len + 16) };
    (0..len).any(|i| {
        let op = bytes[i];
        if op != 0xF7 && op != 0xF6 {
            return false;
        }
        let modrm = bytes[i + 1];
        let md = modrm >> 6;
        if modrm & 0x38 != 0 || md == 0 || md == 3 {
            return false;
        }
        let mut j = i + 2;
        if modrm & 7 == 4 {
            j += 1; // SIB
        }
        let disp = if md == 1 {
            let d = bytes[j] as i8 as i64;
            j += 1;
            d
        } else {
            let d = i32::from_le_bytes(bytes[j..j + 4].try_into().unwrap()) as i64;
            j += 4;
            d
        };
        if op == 0xF7 {
            disp == offset as i64 && u32::from_le_bytes(bytes[j..j + 4].try_into().unwrap()) == flag
        } else {
            let k = flag.trailing_zeros() as i64 / 8;
            disp == offset as i64 + k && bytes[j] as u32 == (flag >> (8 * k)) & 0xFF
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(bytes: &[u8]) -> Vec<u8> {
        let mut v = bytes.to_vec();
        v.resize(0x500, 0xCC);
        v
    }

    #[test]
    fn spots_flag_tests_in_code() {
        // test dword ptr [rbx+0B0h], 400h
        let a = code(&[0x90, 0xF7, 0x83, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00]);
        assert!(tests_flag(a.as_ptr() as usize, 0x400, 0xB0, 0x400));
        assert!(!tests_flag(a.as_ptr() as usize, 0x400, 0xB0, 0x40_0000));
        // test byte ptr [rdi+0B2h], 40h  (bit 22 of the dword at 0B0h)
        let b = code(&[0xF6, 0x87, 0xB2, 0x00, 0x00, 0x00, 0x40]);
        assert!(tests_flag(b.as_ptr() as usize, 0x400, 0xB0, 0x40_0000));
        // test byte ptr [rcx+59h], 4  (disp8, bit 10 of the dword at 58h)
        let c = code(&[0xF6, 0x41, 0x59, 0x04]);
        assert!(tests_flag(c.as_ptr() as usize, 0x400, 0x58, 0x400));
        // test eax, 400h: a register, not the function's flags
        let d = code(&[0xF7, 0xC0, 0x00, 0x04, 0x00, 0x00]);
        assert!(!tests_flag(d.as_ptr() as usize, 0x400, 0, 0x400));
    }
}
