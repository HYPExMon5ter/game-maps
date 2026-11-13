//! A text listing of every class, struct and enum the game has loaded, with their
//! properties and functions: what a provider's names are looked up in.

use std::io::{self, Write};

use crate::{Obj, Unreal};

impl Unreal {
    /// Writes the listing. Slow (seconds): for development, never during play.
    ///
    /// # Safety
    /// The objects must not change meanwhile: game thread, or a game sitting still.
    pub unsafe fn dump(&self, out: &mut impl Write) -> io::Result<()> {
        let core = |name: &str| self.find_struct(&format!("/Script/CoreUObject.{name}")).ok();
        let (Some(class_class), Some(struct_class), Some(enum_class), Some(function_class)) = (core("Class"), core("ScriptStruct"), core("Enum"), core("Function")) else {
            return Err(io::Error::other("core classes not found"));
        };
        let mut objects: Vec<(String, Obj)> = (0..self.object_count())
            .filter_map(|i| self.object(i))
            .filter(|&o| self.is_a(o, class_class) || self.is_a(o, struct_class) || self.is_a(o, enum_class))
            .filter(|&o| !self.is_a(o, function_class))
            .map(|o| (self.path(o), o))
            .collect();
        objects.sort();
        writeln!(out, "// {} types; layout {:?}; ProcessEvent slot {}", objects.len(), self.layout(), self.process_event_index())?;
        for (path, o) in objects {
            // Off the game thread, objects can go while this runs.
            if !self.alive(o) {
                continue;
            }
            if self.is_a(o, enum_class) {
                writeln!(out, "\nenum {path}")?;
                for (name, value) in crate::layout::enum_entries(self.layout(), o) {
                    writeln!(out, "    {} = {value}", self.fname(name))?;
                }
                continue;
            }
            let kind = if self.is_a(o, class_class) { "class" } else { "struct" };
            let parent = self.super_of(o).map(|s| format!(" : {}", self.name(s))).unwrap_or_default();
            writeln!(out, "\n{kind} {path}{parent}  // size 0x{:X}", self.struct_size(o))?;
            for p in self.own_properties(o) {
                let dim = if p.array_dim > 1 { format!("[{}]", p.array_dim) } else { String::new() };
                writeln!(out, "    0x{:04X} {} {}{dim}", p.offset, p.ty, p.name)?;
            }
            if kind == "class" {
                for f in self.own_functions(o) {
                    let f = self.read_function(f, o);
                    let stat = if f.is_static() { "static " } else { "" };
                    writeln!(out, "    fn {stat}{}", f.signature())?;
                }
            }
        }
        Ok(())
    }
}
