//! Type mappings (`.usmap`) for reading the game's packages offline.
//!
//! UE5 games save properties without their names or types ("unversioned"), so tools that
//! read the game's files (CUE4Parse, used by the map generator) need the layout of every
//! class and struct. This writes it from the running game, in the format those tools
//! share (version `LargeEnums`, uncompressed).

use std::collections::HashMap;

use crate::{mem, FName, Obj, Unreal};

const MAGIC: u16 = 0x30C4;
/// `EUsmapVersion::LargeEnums`: 16-bit name lengths and enum sizes.
const VERSION: u8 = 3;

/// `EPropertyType` as `.usmap` files number them.
fn type_code(class: &str) -> u8 {
    match class {
        "ByteProperty" => 0,
        "BoolProperty" => 1,
        "IntProperty" => 2,
        "FloatProperty" => 3,
        "ObjectProperty" | "ObjectPtrProperty" | "ClassProperty" | "ClassPtrProperty" => 4,
        "NameProperty" => 5,
        "DelegateProperty" => 6,
        "DoubleProperty" => 7,
        "ArrayProperty" => 8,
        "StructProperty" => 9,
        "StrProperty" => 10,
        "TextProperty" => 11,
        "InterfaceProperty" => 12,
        "MulticastDelegateProperty" | "MulticastInlineDelegateProperty" | "MulticastSparseDelegateProperty" => 13,
        "WeakObjectProperty" => 14,
        "LazyObjectProperty" => 15,
        "SoftObjectProperty" | "SoftClassProperty" => 17,
        "UInt64Property" => 18,
        "UInt32Property" => 19,
        "UInt16Property" => 20,
        "Int64Property" => 21,
        "Int16Property" => 22,
        "Int8Property" => 23,
        "MapProperty" => 24,
        "SetProperty" => 25,
        "EnumProperty" => 26,
        "FieldPathProperty" => 27,
        "OptionalProperty" => 28,
        "Utf8StrProperty" => 29,
        "AnsiStrProperty" => 30,
        _ => 0xFF,
    }
}

struct Writer {
    names: Vec<String>,
    index: HashMap<String, i32>,
    body: Vec<u8>,
}

impl Writer {
    fn name(&mut self, name: &str) -> i32 {
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        let i = self.names.len() as i32;
        self.names.push(name.to_owned());
        self.index.insert(name.to_owned(), i);
        i
    }

    fn u8(&mut self, v: u8) {
        self.body.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.body.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.body.extend_from_slice(&v.to_le_bytes());
    }
}

impl Unreal {
    /// The `.usmap` of every class, struct and enum loaded now.
    ///
    /// # Safety
    /// The objects must not change meanwhile: game thread, or a game sitting still.
    pub unsafe fn usmap(&self) -> Result<Vec<u8>, crate::Error> {
        let core = |name: &str| self.find_struct(&format!("/Script/CoreUObject.{name}"));
        let (struct_class, enum_class, function_class) = (core("Struct")?, core("Enum")?, core("Function")?);
        let mut enums = Vec::new();
        let mut structs = Vec::new();
        for o in (0..self.object_count()).filter_map(|i| self.object(i)) {
            if self.flags(o) & 0x10 != 0 {
                continue; // class default objects
            }
            if self.is_a(o, enum_class) {
                enums.push(o);
            } else if self.is_a(o, struct_class) && !self.is_a(o, function_class) {
                structs.push(o);
            }
        }
        let mut w = Writer { names: Vec::new(), index: HashMap::new(), body: Vec::new() };

        w.body.extend_from_slice(&(enums.len() as u32).to_le_bytes());
        for e in enums {
            let name = self.name(e);
            let id = w.name(&name);
            w.i32(id);
            let entries = crate::layout::enum_entries(self.layout(), e);
            w.u16(entries.len() as u16);
            for (entry, _) in entries {
                let text = self.fname(entry);
                let short = text.rsplit("::").next().unwrap_or(&text).to_owned();
                let id = w.name(&short);
                w.i32(id);
            }
        }

        w.body.extend_from_slice(&(structs.len() as u32).to_le_bytes());
        for s in structs {
            let name = self.name(s);
            let id = w.name(&name);
            w.i32(id);
            let parent = match self.super_of(s) {
                Some(p) => w.name(&self.name(p)),
                None => -1,
            };
            w.i32(parent);
            let fields: Vec<usize> = crate::layout::properties(self.layout(), s).collect();
            let total: usize = fields.iter().map(|&f| mem::read::<i32>(f + self.layout().array_dim).max(1) as usize).sum();
            w.u16(total as u16);
            w.u16(fields.len() as u16);
            let mut schema = 0u16;
            for f in fields {
                let dim = mem::read::<i32>(f + self.layout().array_dim).max(1);
                w.u16(schema);
                w.u8(dim as u8);
                let name = self.fname(mem::read(f + self.layout().field_name));
                let id = w.name(&name);
                w.i32(id);
                self.write_type(&mut w, f);
                schema += dim as u16;
            }
        }

        let mut data = Vec::new();
        data.extend_from_slice(&(w.names.len() as u32).to_le_bytes());
        for n in &w.names {
            data.extend_from_slice(&(n.len() as u16).to_le_bytes());
            data.extend_from_slice(n.as_bytes());
        }
        data.extend_from_slice(&w.body);

        let mut out = Vec::with_capacity(data.len() + 16);
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.push(VERSION);
        out.extend_from_slice(&0i32.to_le_bytes()); // no package versioning
        out.push(0); // not compressed
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        Ok(out)
    }

    unsafe fn write_type(&self, w: &mut Writer, field: usize) {
        let l = self.layout();
        let class = mem::try_ptr(field + l.field_class).and_then(mem::try_read::<FName>).map(|n| self.fname(n)).unwrap_or_default();
        let extra = field + l.property_size;
        let ref_name = |at: usize| mem::try_ptr(at).map(|o: Obj| self.name(o)).unwrap_or_default();
        match class.as_str() {
            "EnumProperty" => {
                w.u8(26);
                match mem::try_ptr(extra) {
                    Some(inner) => self.write_type(w, inner),
                    None => w.u8(0),
                }
                let id = w.name(&ref_name(extra + 8));
                w.i32(id);
            }
            // A byte with an enum is written as an enum over a byte.
            "ByteProperty" if mem::try_ptr(extra).is_some() => {
                w.u8(26);
                w.u8(0);
                let id = w.name(&ref_name(extra));
                w.i32(id);
            }
            "StructProperty" => {
                w.u8(9);
                let id = w.name(&ref_name(extra));
                w.i32(id);
            }
            "ArrayProperty" | "SetProperty" | "OptionalProperty" => {
                w.u8(type_code(&class));
                let inner = if class == "ArrayProperty" { self.array_inner(field) } else { mem::try_ptr(extra) };
                match inner {
                    Some(inner) => self.write_type(w, inner),
                    None => w.u8(0xFF),
                }
            }
            "MapProperty" => {
                w.u8(24);
                for at in [extra, extra + 8] {
                    match mem::try_ptr(at) {
                        Some(inner) => self.write_type(w, inner),
                        None => w.u8(0xFF),
                    }
                }
            }
            other => w.u8(type_code(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_types_as_usmap_does() {
        assert_eq!(type_code("StructProperty"), 9);
        assert_eq!(type_code("EnumProperty"), 26);
        assert_eq!(type_code("ClassProperty"), 4);
        assert_eq!(type_code("SoftClassProperty"), 17);
        assert_eq!(type_code("Whatever"), 0xFF);
    }

    #[test]
    fn names_are_written_once() {
        let mut w = Writer { names: Vec::new(), index: HashMap::new(), body: Vec::new() };
        assert_eq!(w.name("Vector"), 0);
        assert_eq!(w.name("X"), 1);
        assert_eq!(w.name("Vector"), 0);
    }
}
