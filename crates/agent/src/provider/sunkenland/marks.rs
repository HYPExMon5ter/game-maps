//! What players placed in the world. Flags and similar markers are `Buoy` objects, found
//! with one object search every few seconds (no list of them exists; `IsAI` ones are the
//! game's own location flags). Map marks, drawn on the game's map:
//! `Global.code.uiCompass.get_MapMarks()`, a Fusion `NetworkDictionary<long,
//! MapMarkNetworkStruct>`. It's a generic struct, so it's walked through its own
//! enumerator: the methods are found on the closed types of the live values.

use game_maps_protocol::{Marker, MarkerKind};
use unity_runtime::{read_string, Class, Field, ManagedObject, Method, Runtime, Unity, Vector3};

use crate::provider::to_proto;

const MARK_ICON: &str = "icon flag";
const MAX_MARKS: usize = 256;
/// `MarkerType`s with a marker of their own elsewhere.
const SKIPPED_TYPES: &[&str] = &["Boat", "PrisonCage"];

#[repr(C)]
#[derive(Clone, Copy)]
struct Vector2 {
    x: f32,
    y: f32,
}

pub struct MapMarks {
    compass: Field,
    get_marks: Method,
}

/// `Buoy` members: the markers players put in the world.
pub struct Flags {
    class: Class,
    is_ai: Method,
    name: Method,
    marker_type: Field,
    position: Field,
    /// `MarkerType` values to leave out.
    skipped: Vec<i32>,
}

impl Flags {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let class = rt.class("", "Buoy").ok()?;
        let types = rt.class("", "MarkerType").ok()?;
        let skipped = SKIPPED_TYPES.iter().filter_map(|n| rt.field(types, n).ok().filter(|f| f.is_static)).map(|f| unsafe { rt.static_value::<i32>(f) }).collect();
        Some(Self {
            class,
            is_ai: rt.method(class, "get_IsAI", 0).ok()?,
            name: rt.method(class, "get_BuoyName", 0).ok()?,
            marker_type: rt.field(class, "M_MarkerType").ok()?,
            position: rt.field(class, "transformPosition").ok()?,
            skipped,
        })
    }

    /// Flags and markers players placed.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn read(&self, rt: &Runtime, unity: &Unity) -> Result<Vec<Marker>, String> {
        let mut flags = Vec::new();
        for buoy in unity.find_objects(rt, self.class, false).map_err(|e| e.to_string())? {
            let is_ai = rt.invoke(&self.is_ai, buoy, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(true);
            if is_ai || self.skipped.contains(&self.marker_type.get::<i32>(buoy)) {
                continue;
            }
            let name = rt.invoke(&self.name, buoy, &mut []).ok().and_then(|n| {
                let to_string = rt.method(rt.class_of(n)?, "ToString", 0).ok()?;
                read_string(rt.invoke_boxed(&to_string, n, &mut []).ok()?)
            });
            flags.push(Marker {
                id: format!("flag:{:x}", buoy as usize),
                kind: MarkerKind::Locator,
                label: Some(name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Flag".into())),
                icon: Some(MARK_ICON.into()),
                position: to_proto(self.position.get::<Vector3>(buoy)),
            });
            if flags.len() == MAX_MARKS {
                break;
            }
        }
        Ok(flags)
    }
}

impl MapMarks {
    /// `global`: the `Global` class.
    pub fn resolve(rt: &Runtime, global: Class) -> Option<Self> {
        let compass = rt.class("", "UICompass").ok()?;
        Some(Self { compass: rt.field(global, "uiCompass").ok()?, get_marks: rt.method(compass, "get_MapMarks", 0).ok()? })
    }

    /// The marks in the world, at sea level `y` (they're stored in 2D).
    ///
    /// # Safety
    /// Must run on the game's main thread; `global` must be the live `Global`.
    pub unsafe fn read(&self, rt: &Runtime, global: *mut ManagedObject, y: f32) -> Result<Vec<Marker>, String> {
        let text = |e: unity_runtime::Error| e.to_string();
        let compass: *mut ManagedObject = self.compass.get(global);
        if compass.is_null() {
            return Ok(Vec::new());
        }
        let dict = rt.invoke(&self.get_marks, compass, &mut []).map_err(text)?;
        let class = rt.class_of(dict).ok_or("no dictionary")?;
        let enumerator = rt.invoke_boxed(&rt.method(class, "GetEnumerator", 0).map_err(text)?, dict, &mut []).map_err(text)?;
        let class = rt.class_of(enumerator).ok_or("no enumerator")?;
        let (next, current) = (rt.method(class, "MoveNext", 0).map_err(text)?, rt.method(class, "get_Current", 0).map_err(text)?);

        let mut marks = Vec::new();
        while marks.len() < MAX_MARKS && rt.unbox::<bool>(rt.invoke_boxed(&next, enumerator, &mut []).map_err(text)?).map_err(text)? {
            let pair = rt.invoke_boxed(&current, enumerator, &mut []).map_err(text)?;
            let class = rt.class_of(pair).ok_or("no pair")?;
            let key: i64 = rt.unbox(rt.invoke_boxed(&rt.method(class, "get_Key", 0).map_err(text)?, pair, &mut []).map_err(text)?).map_err(text)?;
            let mark = rt.invoke_boxed(&rt.method(class, "get_Value", 0).map_err(text)?, pair, &mut []).map_err(text)?;
            let class = rt.class_of(mark).ok_or("no mark")?;
            let p: Vector2 = rt.unbox(rt.invoke_boxed(&rt.method(class, "get_Position", 0).map_err(text)?, mark, &mut []).map_err(text)?).map_err(text)?;
            let name = rt.invoke_boxed(&rt.method(class, "get_MarkName", 0).map_err(text)?, mark, &mut []).map_err(text)?;
            let to_string = rt.method(rt.class_of(name).ok_or("no name")?, "ToString", 0).map_err(text)?;
            let label = read_string(rt.invoke_boxed(&to_string, name, &mut []).map_err(text)?).filter(|n| !n.trim().is_empty());
            marks.push(Marker {
                id: format!("mark:{key}"),
                kind: MarkerKind::Locator,
                label: Some(label.unwrap_or_else(|| "Map mark".into())),
                icon: Some(MARK_ICON.into()),
                position: to_proto(Vector3 { x: p.x, y, z: p.y }),
            });
        }
        Ok(marks)
    }
}
