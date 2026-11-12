//! Where the reflection structs keep their members. These move between engine versions
//! and build settings, so each is found by looking at engine types whose contents are
//! known (the `Vector` struct has `X`, `Y` and `Z`, `Actor` has `RootComponent`, ...)
//! instead of being written down per version.

use crate::mem;
use crate::names::{FName, Names};
use crate::objects::Objects;
use crate::{Error, Result};

/// `UObjectBase` is the same in every UE4 and UE5 version.
pub const OBJECT_FLAGS: usize = 0x8;
pub const OBJECT_CLASS: usize = 0x10;
pub const OBJECT_NAME: usize = 0x18;
pub const OBJECT_OUTER: usize = 0x20;
/// `UField::Next`.
pub const FIELD_NEXT: usize = 0x28;

#[derive(Debug, Clone, Copy, Default)]
pub struct Layout {
    /// `UStruct`
    pub super_struct: usize,
    pub children: usize,
    pub child_properties: usize,
    pub properties_size: usize,
    /// `FField`
    pub field_class: usize,
    pub field_next: usize,
    pub field_name: usize,
    /// `FProperty`
    pub array_dim: usize,
    pub element_size: usize,
    pub property_flags: usize,
    pub offset: usize,
    /// Where a property type's own members start (`sizeof(FProperty)`).
    pub property_size: usize,
    /// `UFunction::FunctionFlags`
    pub function_flags: usize,
    /// `UClass::ClassDefaultObject`
    pub default_object: usize,
    /// `UEnum::Names`
    pub enum_names: usize,
    /// Enums keep names and values in two arrays, pointed to from `enum_names` and the
    /// next 8 bytes, with the count after them (UE 5.6), instead of one array of pairs.
    pub enum_split: bool,
}

/// Lookups the layout search needs before any layout is known.
pub(crate) struct Probe<'a> {
    pub names: &'a Names,
    pub objects: &'a Objects,
}

impl Probe<'_> {
    pub fn name_of(&self, obj: usize) -> Option<std::sync::Arc<str>> {
        let name = mem::try_read::<FName>(obj + OBJECT_NAME)?;
        if name.number != 0 {
            return None;
        }
        self.names.entry(name.index)
    }

    fn class_name(&self, obj: usize) -> Option<std::sync::Arc<str>> {
        self.name_of(mem::try_ptr(obj + OBJECT_CLASS)?)
    }

    /// The object named `name` whose class is named `class`, in package `package`.
    pub fn find(&self, package: &str, name: &str, class: &str) -> Result<usize> {
        (0..self.objects.len())
            .filter_map(|i| self.objects.get(i))
            .find(|&o| {
                self.name_of(o).is_some_and(|n| &*n == name)
                    && self.class_name(o).is_some_and(|c| &*c == class)
                    && mem::try_ptr(o + OBJECT_OUTER).and_then(|p| self.name_of(p)).is_some_and(|p| &*p == package)
            })
            .ok_or_else(|| Error::NotFound(format!("{package}.{name} ({class})")))
    }

    fn field_name(&self, field: usize, at: usize) -> Option<std::sync::Arc<str>> {
        let name = mem::try_read::<FName>(field + at)?;
        (name.number == 0).then(|| self.names.entry(name.index)).flatten()
    }
}

fn layout_error(what: &str) -> Error {
    Error::Layout(what.into())
}

/// Offsets in `0..limit` (step `step`) where `pred` holds.
fn search(from: usize, to: usize, step: usize, mut pred: impl FnMut(usize) -> bool) -> Option<usize> {
    (from..to).step_by(step).find(|&o| pred(o))
}

impl Layout {
    pub(crate) fn find(p: &Probe) -> Result<Self> {
        let mut l = Layout::default();
        let core = "/Script/CoreUObject";
        let engine = "/Script/Engine";
        let struct_class = p.find(core, "Struct", "Class")?;
        let field_class = p.find(core, "Field", "Class")?;
        let class_class = p.find(core, "Class", "Class")?;
        l.super_struct = search(0x28, 0xA0, 8, |o| {
            mem::try_read::<usize>(struct_class + o) == Some(field_class) && mem::try_read::<usize>(class_class + o) == Some(struct_class)
        })
        .ok_or_else(|| layout_error("UStruct::SuperStruct"))?;

        // Vector: X, Y, Z of 8 bytes (4 before large world coordinates).
        let vector = p.find(core, "Vector", "ScriptStruct")?;
        let guid = p.find(core, "Guid", "ScriptStruct")?;
        let (child_properties, field_name) = (0x30..0xA0)
            .step_by(8)
            .find_map(|o| {
                let x = mem::try_ptr(vector + o)?;
                let a = mem::try_ptr(guid + o)?;
                let name = search(0x8, 0x40, 8, |n| p.field_name(x, n).as_deref() == Some("X") && p.field_name(a, n).as_deref() == Some("A"))?;
                Some((o, name))
            })
            .ok_or_else(|| layout_error("UStruct::ChildProperties"))?;
        l.child_properties = child_properties;
        l.field_name = field_name;
        let x = mem::try_ptr(vector + l.child_properties).ok_or_else(|| layout_error("Vector.X"))?;
        l.field_next = search(0x8, 0x40, 8, |o| mem::try_ptr(x + o).is_some_and(|y| p.field_name(y, l.field_name).as_deref() == Some("Y")))
            .ok_or_else(|| layout_error("FField::Next"))?;
        let y = mem::try_ptr(x + l.field_next).ok_or_else(|| layout_error("Vector.Y"))?;
        let z = mem::try_ptr(y + l.field_next).ok_or_else(|| layout_error("Vector.Z"))?;
        l.field_class = search(0x8, 0x30, 8, |o| {
            mem::try_ptr(x + o).and_then(|c| p.field_name(c, 0)).is_some_and(|n| matches!(&*n, "DoubleProperty" | "FloatProperty"))
        })
        .ok_or_else(|| layout_error("FField::Class"))?;

        let int = |f: usize, o: usize| mem::try_read::<i32>(f + o);
        let after_name = l.field_name + 8;
        l.element_size = search(after_name, after_name + 0x40, 4, |o| {
            let s = int(x, o);
            matches!(s, Some(4 | 8)) && int(y, o) == s && int(z, o) == s && int(x, o - 4) == Some(1)
        })
        .ok_or_else(|| layout_error("FProperty::ElementSize"))?;
        l.array_dim = l.element_size - 4;
        l.property_flags = l.element_size + 4;
        let size = int(x, l.element_size).unwrap_or(0);
        l.offset = search(l.property_flags + 8, l.property_flags + 0x40, 4, |o| int(x, o) == Some(0) && int(y, o) == Some(size) && int(z, o) == Some(2 * size))
            .ok_or_else(|| layout_error("FProperty::Offset_Internal"))?;
        l.properties_size = search(l.child_properties + 8, l.child_properties + 0x40, 4, |o| int(vector, o) == Some(3 * size) && int(guid, o) == Some(16))
            .ok_or_else(|| layout_error("UStruct::PropertiesSize"))?;

        // Actor: its first function, its RootComponent, and its default object.
        let actor = p.find(engine, "Actor", "Class")?;
        l.children = search(0x30, 0xA0, 8, |o| {
            o != l.super_struct && o != l.child_properties && mem::try_ptr(actor + o).and_then(|f| p.class_name(f)).is_some_and(|c| &*c == "Function")
        })
        .ok_or_else(|| layout_error("UStruct::Children"))?;
        let scene_component = p.find(engine, "SceneComponent", "Class")?;
        let root = properties(&l, actor).find(|&f| p.field_name(f, l.field_name).as_deref() == Some("RootComponent")).ok_or_else(|| layout_error("Actor.RootComponent"))?;
        l.property_size = search(l.offset + 4, l.offset + 0x60, 8, |o| mem::try_read::<usize>(root + o) == Some(scene_component))
            .ok_or_else(|| layout_error("sizeof(FProperty)"))?;
        l.default_object = search(l.properties_size, l.properties_size + 0x180, 8, |o| {
            mem::try_ptr(actor + o).and_then(|d| p.name_of(d)).is_some_and(|n| &*n == "Default__Actor")
        })
        .ok_or_else(|| layout_error("UClass::ClassDefaultObject"))?;

        // A static and an instance function, both native and callable from blueprints.
        let get_location = p.find("Actor", "K2_GetActorLocation", "Function")?;
        let get_version = p.find("KismetSystemLibrary", "GetEngineVersion", "Function")?;
        const NATIVE: u32 = 0x400;
        const STATIC: u32 = 0x2000;
        const BLUEPRINT_CALLABLE: u32 = 0x0400_0000;
        l.function_flags = search(l.properties_size + 4, l.properties_size + 0x60, 4, |o| {
            let (Some(instance), Some(stat)) = (mem::try_read::<u32>(get_location + o), mem::try_read::<u32>(get_version + o)) else { return false };
            let both = NATIVE | BLUEPRINT_CALLABLE;
            instance & both == both && stat & both == both && stat & STATIC != 0 && instance & STATIC == 0
        })
        .ok_or_else(|| layout_error("UFunction::FunctionFlags"))?;

        let net_role = p.find(engine, "ENetRole", "Enum")?;
        let role_none = |names: usize| p.field_name(names, 0).is_some_and(|n| n.ends_with("ROLE_None"));
        let pairs = search(0x28, 0x80, 8, |o| {
            let (Some(data), Some(count)) = (mem::try_ptr(net_role + o), int(net_role, o + 8)) else { return false };
            (2..=16).contains(&count) && role_none(data)
        });
        let split = || {
            search(0x28, 0x80, 8, |o| {
                let (Some(names), Some(values), Some(count)) = (untagged_ptr(net_role + o), untagged_ptr(net_role + o + 8), int(net_role, o + 16)) else { return false };
                (2..=16).contains(&count) && role_none(names) && mem::try_read::<i64>(values + 8) == Some(1)
            })
        };
        match (pairs, pairs.is_none().then(split).flatten()) {
            (Some(o), _) => l.enum_names = o,
            (None, Some(o)) => (l.enum_names, l.enum_split) = (o, true),
            (None, None) => return Err(layout_error("UEnum::Names")),
        }
        Ok(l)
    }
}

/// A pointer whose lowest bit may be used as a flag.
pub(crate) fn untagged_ptr(at: usize) -> Option<usize> {
    mem::try_read::<usize>(at).map(|p| p & !1).filter(|&p| mem::readable(p, 8))
}

/// An enum's names and values.
///
/// # Safety
/// `e` must be a live enum.
pub(crate) unsafe fn enum_entries(l: &Layout, e: usize) -> Vec<(FName, i64)> {
    if l.enum_split {
        let (Some(names), Some(values)) = (untagged_ptr(e + l.enum_names), untagged_ptr(e + l.enum_names + 8)) else { return Vec::new() };
        let count = mem::read::<i32>(e + l.enum_names + 16).clamp(0, 0x10000) as usize;
        (0..count).map(|i| (mem::read::<FName>(names + i * 8), mem::read::<i64>(values + i * 8))).collect()
    } else {
        mem::read::<crate::TArray<(FName, i64)>>(e + l.enum_names).to_vec()
    }
}

/// The properties a struct declares itself (not its parents').
pub(crate) fn properties(l: &Layout, strukt: usize) -> impl Iterator<Item = usize> + '_ {
    let mut next = mem::try_ptr(strukt + l.child_properties);
    std::iter::from_fn(move || {
        let field = next?;
        next = mem::try_ptr(field + l.field_next);
        Some(field)
    })
}
