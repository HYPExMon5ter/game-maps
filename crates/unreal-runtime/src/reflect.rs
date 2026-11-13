//! Properties and functions as the reflection data describes them.

use std::fmt;

/// `EPropertyFlags` that matter for calls.
pub const CPF_PARM: u64 = 0x80;
pub const CPF_OUT_PARM: u64 = 0x100;
pub const CPF_RETURN_PARM: u64 = 0x400;
pub const CPF_REFERENCE_PARM: u64 = 0x0800_0000;

/// `EFunctionFlags`
pub const FUNC_STATIC: u32 = 0x2000;

/// A property's type, with the class, struct or enum it refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Bool,
    Byte(Option<String>),
    Int8,
    Int16,
    Int,
    Int64,
    UInt16,
    UInt32,
    UInt64,
    Float,
    Double,
    Str,
    Name,
    Text,
    Object(String),
    WeakObject(String),
    SoftObject(String),
    Class(String),
    Interface(String),
    Struct(String),
    Enum(String),
    Array(Box<Type>),
    Set(Box<Type>),
    Map(Box<Type>, Box<Type>),
    /// Anything else, by its property class (delegates, field paths, ...).
    Other(String),
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Bool => f.write_str("Bool"),
            Type::Byte(None) => f.write_str("Byte"),
            Type::Byte(Some(e)) => write!(f, "Byte<{e}>"),
            Type::Int8 => f.write_str("Int8"),
            Type::Int16 => f.write_str("Int16"),
            Type::Int => f.write_str("Int"),
            Type::Int64 => f.write_str("Int64"),
            Type::UInt16 => f.write_str("UInt16"),
            Type::UInt32 => f.write_str("UInt32"),
            Type::UInt64 => f.write_str("UInt64"),
            Type::Float => f.write_str("Float"),
            Type::Double => f.write_str("Double"),
            Type::Str => f.write_str("Str"),
            Type::Name => f.write_str("Name"),
            Type::Text => f.write_str("Text"),
            Type::Object(c) => write!(f, "Object<{c}>"),
            Type::WeakObject(c) => write!(f, "WeakObject<{c}>"),
            Type::SoftObject(c) => write!(f, "SoftObject<{c}>"),
            Type::Class(c) => write!(f, "Class<{c}>"),
            Type::Interface(c) => write!(f, "Interface<{c}>"),
            Type::Struct(s) => write!(f, "Struct<{s}>"),
            Type::Enum(e) => write!(f, "Enum<{e}>"),
            Type::Array(t) => write!(f, "Array<{t}>"),
            Type::Set(t) => write!(f, "Set<{t}>"),
            Type::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Type::Other(o) => f.write_str(o),
        }
    }
}

/// A property of a class, struct or function.
#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub ty: Type,
    pub offset: usize,
    pub size: usize,
    pub array_dim: usize,
    pub flags: u64,
    /// Bool properties: which byte (from `offset`) and bit.
    pub bit: Option<(usize, u8)>,
}

impl Property {
    pub fn is_param(&self) -> bool {
        self.flags & CPF_PARM != 0
    }

    pub fn is_return(&self) -> bool {
        self.flags & CPF_RETURN_PARM != 0
    }

    pub fn is_out(&self) -> bool {
        self.flags & CPF_OUT_PARM != 0 && self.flags & CPF_REFERENCE_PARM == 0
    }
}

/// A function that can be called with `ProcessEvent`.
#[derive(Debug, Clone)]
pub struct Function {
    pub ptr: usize,
    pub name: String,
    /// The class that declares it.
    pub owner: usize,
    pub flags: u32,
    /// Bytes the parameters take.
    pub params_size: usize,
    pub params: Vec<Property>,
}

impl Function {
    pub fn is_static(&self) -> bool {
        self.flags & FUNC_STATIC != 0
    }

    pub fn param(&self, name: &str) -> Option<&Property> {
        self.params.iter().find(|p| p.name == name)
    }

    /// `Name(Type A, Type B) -> Type`, for messages and dumps.
    pub fn signature(&self) -> String {
        let args: Vec<String> = self
            .params
            .iter()
            .filter(|p| !p.is_return())
            .map(|p| format!("{}{} {}", if p.is_out() { "out " } else { "" }, p.ty, p.name))
            .collect();
        let ret = self.params.iter().find(|p| p.is_return()).map(|p| format!(" -> {}", p.ty)).unwrap_or_default();
        format!("{}({}){ret}", self.name, args.join(", "))
    }
}

/// The parameters of one call, laid out as `ProcessEvent` expects them.
pub struct Args<'f> {
    pub(crate) function: &'f Function,
    /// 8-byte aligned, like the engine's own parameter structs.
    pub(crate) buf: Vec<u64>,
}

impl<'f> Args<'f> {
    pub fn new(function: &'f Function) -> Self {
        Self { function, buf: vec![0u64; function.params_size.div_ceil(8).max(1)] }
    }

    fn slot(&self, name: &str, size: usize) -> crate::Result<&Property> {
        let p = self.function.param(name).ok_or_else(|| crate::Error::NotFound(format!("parameter {name} of {}", self.function.name)))?;
        if p.size * p.array_dim != size || p.bit.is_some() {
            return Err(crate::Error::Type(format!("parameter {name} of {} is {} ({} bytes), not {size} bytes", self.function.name, p.ty, p.size)));
        }
        Ok(p)
    }

    pub(crate) fn as_ptr(&mut self) -> *mut u8 {
        self.buf.as_mut_ptr() as *mut u8
    }

    /// Sets parameter `name`, which must be exactly the size of `T`.
    pub fn set<T: Copy>(&mut self, name: &str, value: T) -> crate::Result<&mut Self> {
        let offset = self.slot(name, std::mem::size_of::<T>())?.offset;
        unsafe { std::ptr::write_unaligned(self.as_ptr().add(offset) as *mut T, value) };
        Ok(self)
    }

    pub fn get<T: Copy>(&self, name: &str) -> crate::Result<T> {
        let offset = self.slot(name, std::mem::size_of::<T>())?.offset;
        Ok(unsafe { std::ptr::read_unaligned((self.buf.as_ptr() as *const u8).add(offset) as *const T) })
    }

    /// Where parameter `name` is in the buffer, for reading a struct's members.
    pub fn param_ptr(&self, name: &str) -> crate::Result<*const u8> {
        let p = self.function.param(name).ok_or_else(|| crate::Error::NotFound(format!("parameter {name} of {}", self.function.name)))?;
        Ok(unsafe { (self.buf.as_ptr() as *const u8).add(p.offset) })
    }

    pub fn set_bool(&mut self, name: &str, value: bool) -> crate::Result<&mut Self> {
        let (offset, (byte, mask)) = self.bool_slot(name)?;
        unsafe {
            let at = self.as_ptr().add(offset + byte);
            *at = if value { *at | mask } else { *at & !mask };
        }
        Ok(self)
    }

    pub fn get_bool(&self, name: &str) -> crate::Result<bool> {
        let (offset, (byte, mask)) = self.bool_slot(name)?;
        Ok(unsafe { *(self.buf.as_ptr() as *const u8).add(offset + byte) } & mask != 0)
    }

    fn bool_slot(&self, name: &str) -> crate::Result<(usize, (usize, u8))> {
        let p = self.function.param(name).ok_or_else(|| crate::Error::NotFound(format!("parameter {name} of {}", self.function.name)))?;
        let bit = p.bit.ok_or_else(|| crate::Error::Type(format!("parameter {name} of {} is {}, not Bool", self.function.name, p.ty)))?;
        Ok((p.offset, bit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function() -> Function {
        let prop = |name: &str, ty: Type, offset, size, flags, bit| Property { name: name.into(), ty, offset, size, array_dim: 1, flags, bit };
        Function {
            ptr: 0,
            name: "K2_TeleportTo".into(),
            owner: 0,
            flags: 0x400,
            params_size: 0x38,
            params: vec![
                prop("DestLocation", Type::Struct("Vector".into()), 0, 24, CPF_PARM, None),
                prop("DestRotation", Type::Struct("Rotator".into()), 24, 24, CPF_PARM, None),
                prop("ReturnValue", Type::Bool, 0x30, 1, CPF_PARM | CPF_OUT_PARM | CPF_RETURN_PARM, Some((0, 0xFF))),
            ],
        }
    }

    #[test]
    fn lays_out_parameters_by_name() {
        let f = function();
        let mut args = Args::new(&f);
        args.set("DestLocation", [1.0f64, 2.0, 3.0]).unwrap();
        args.set_bool("ReturnValue", true).unwrap();
        assert_eq!(args.get::<[f64; 3]>("DestLocation").unwrap(), [1.0, 2.0, 3.0]);
        assert!(args.get_bool("ReturnValue").unwrap());
        assert_eq!(args.buf.len(), 7);
    }

    #[test]
    fn refuses_the_wrong_size_or_name() {
        let f = function();
        let mut args = Args::new(&f);
        assert!(args.set("DestLocation", [1.0f32, 2.0, 3.0]).is_err());
        assert!(args.set("Nope", 1u8).is_err());
        assert!(args.get::<u8>("ReturnValue").is_err());
        assert!(args.set_bool("DestRotation", true).is_err());
    }

    #[test]
    fn describes_types_and_signatures() {
        let t = Type::Array(Box::new(Type::Object("PlayerState".into())));
        assert_eq!(t.to_string(), "Array<Object<PlayerState>>");
        assert_eq!(function().signature(), "K2_TeleportTo(Struct<Vector> DestLocation, Struct<Rotator> DestRotation) -> Bool");
    }
}
