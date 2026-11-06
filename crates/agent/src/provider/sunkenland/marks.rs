//! What players placed in the world. Flags and similar markers are `Buoy` objects, found
//! with one object search every few seconds (no list of them exists; `IsAI` ones are the
//! game's own location flags). Map marks, drawn on the game's map:
//! `Global.code.uiCompass.get_MapMarks()`, a Fusion `NetworkDictionary<long,
//! MapMarkNetworkStruct>`. It's a generic struct, so it's walked through its own
//! enumerator: the methods are found on the closed types of the live values.

use std::ffi::c_void;

use game_maps_protocol::{GamePin, Marker, MarkerKind};
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

/// Map marks from the app have keys with this in their top bits, so they can be told from
/// the player's own: the game only uses the time as a key, which is far smaller.
const APP_MARK: i64 = 0x7A00_0000_0000_0000;
/// How many characters a map mark's name holds (`NetworkString<_8>`).
const MARK_NAME_CHARS: usize = 8;

pub struct MapMarks {
    compass: Field,
    get_marks: Method,
    writer: Option<MarkWriter>,
}

/// `Mainframe.code.GlobalData`, which holds the marks everyone in the world sees.
struct MarkWriter {
    mainframe: Field,
    global_data: Method,
    add: Method,
    remove: Method,
    /// `Global.code.UIMap.MaxMapMarksAmount`.
    ui_map: Method,
    limit: Method,
    mark: Class,
    set_position: Method,
    get_name: Method,
    set_name: Method,
}

/// The key of a pin's mark: the same for the same spot, from one session to the next.
fn mark_key(pin: &GamePin) -> i64 {
    let (x, z) = (pin.position.x.round() as i32 as u32 as i64, pin.position.z.round() as i32 as u32 as i64);
    APP_MARK | ((x & 0xFFFFFF) << 24) | (z & 0xFFFFFF)
}

fn is_app_mark(key: i64) -> bool {
    key & (0xFF << 56) == APP_MARK
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
        let writer = (|| {
            let frame = rt.class("", "Mainframe").ok()?;
            let data = rt.class("", "GlobalData").ok()?;
            let map = rt.class("", "UIMap").ok()?;
            let mark = rt.class("", "MapMarkNetworkStruct").ok()?;
            Some(MarkWriter {
                mainframe: rt.field(frame, "code").ok().filter(|f| f.is_static)?,
                global_data: rt.method(frame, "get_GlobalData", 0).ok()?,
                add: rt.method(data, "RPC_RequestAddNewMapMark", 2).ok()?,
                remove: rt.method(data, "RPC_RequestRemoveMapMark", 1).ok()?,
                ui_map: rt.method(global, "get_UIMap", 0).ok()?,
                limit: rt.method(map, "get_MaxMapMarksAmount", 0).ok()?,
                mark,
                set_position: rt.method(mark, "set_Position", 1).ok()?,
                get_name: rt.method(mark, "get_MarkName", 0).ok()?,
                set_name: rt.method(mark, "set_MarkName", 1).ok()?,
            })
        })();
        Some(Self { compass: rt.field(global, "uiCompass").ok()?, get_marks: rt.method(compass, "get_MapMarks", 0).ok()?, writer })
    }

    pub fn can_write(&self) -> bool {
        self.writer.is_some()
    }

    /// Brings the marks from the app in line with `pins`. Everyone in the world sees marks,
    /// and there's a limit to them, so what doesn't fit stays in the app alone.
    ///
    /// # Safety
    /// Must run on the game's main thread; `global` must be the live `Global`.
    pub unsafe fn sync(&self, rt: &Runtime, unity: &Unity, global: *mut ManagedObject, pins: &[GamePin]) -> Result<(), String> {
        let text = |e: unity_runtime::Error| e.to_string();
        let w = self.writer.as_ref().ok_or("map marks can't be written in this game version")?;
        let frame: *mut ManagedObject = rt.static_value(w.mainframe);
        let data = rt.invoke(&w.global_data, frame, &mut []).map_err(text)?;
        if !unity.is_alive(data) {
            return Err("the world isn't loaded".into());
        }
        let map = rt.invoke(&w.ui_map, global, &mut []).map_err(text)?;
        let limit: i32 = rt.unbox(rt.invoke(&w.limit, map, &mut []).map_err(text)?).map_err(text)?;
        let keys = self.keys(rt, global)?;
        let wanted: Vec<i64> = pins.iter().map(mark_key).collect();
        for &key in keys.iter().filter(|k| is_app_mark(**k) && !wanted.contains(k)) {
            let mut key = key;
            let mut args = [&mut key as *mut i64 as *mut c_void];
            rt.invoke(&w.remove, data, &mut args).map_err(text)?;
        }
        let mut count = keys.iter().filter(|k| !is_app_mark(**k) || wanted.contains(k)).count() as i32;
        for pin in pins {
            let mut key = mark_key(pin);
            if keys.contains(&key) || count >= limit {
                continue;
            }
            let mark = rt.new_value(w.mark).map_err(text)?;
            let mut at = Vector2 { x: pin.position.x, y: pin.position.z };
            let mut args = [&mut at as *mut Vector2 as *mut c_void];
            rt.invoke_boxed(&w.set_position, mark, &mut args).map_err(text)?;
            let name = rt.invoke_boxed(&w.get_name, mark, &mut []).map_err(text)?;
            let set = rt.method_with_params(rt.class_of(name).ok_or("no name")?, "Set", &["System.String"]).map_err(text)?;
            let short: String = pin.name.chars().take(MARK_NAME_CHARS).collect();
            let mut args = [rt.new_string(&short) as *mut c_void];
            rt.invoke_boxed(&set, name, &mut args).map_err(text)?;
            let mut args = [rt.unboxed(name)];
            rt.invoke_boxed(&w.set_name, mark, &mut args).map_err(text)?;
            let mut args = [&mut key as *mut i64 as *mut c_void, rt.unboxed(mark)];
            rt.invoke(&w.add, data, &mut args).map_err(text)?;
            count += 1;
        }
        Ok(())
    }

    /// The keys of the marks in the world.
    unsafe fn keys(&self, rt: &Runtime, global: *mut ManagedObject) -> Result<Vec<i64>, String> {
        let mut keys = Vec::new();
        self.walk(rt, global, |key, _| keys.push(key))?;
        Ok(keys)
    }

    /// The marks in the world, at sea level `y` (they're stored in 2D).
    ///
    /// # Safety
    /// Must run on the game's main thread; `global` must be the live `Global`.
    pub unsafe fn read(&self, rt: &Runtime, global: *mut ManagedObject, y: f32) -> Result<Vec<Marker>, String> {
        let text = |e: unity_runtime::Error| e.to_string();
        let mut marks = Vec::new();
        self.walk(rt, global, |key, mark| {
            // The app draws its own pins.
            if is_app_mark(key) {
                return;
            }
            let read = || -> Result<Marker, String> {
                let class = rt.class_of(mark).ok_or("no mark")?;
                let p: Vector2 = rt.unbox(rt.invoke_boxed(&rt.method(class, "get_Position", 0).map_err(text)?, mark, &mut []).map_err(text)?).map_err(text)?;
                let name = rt.invoke_boxed(&rt.method(class, "get_MarkName", 0).map_err(text)?, mark, &mut []).map_err(text)?;
                let to_string = rt.method(rt.class_of(name).ok_or("no name")?, "ToString", 0).map_err(text)?;
                let label = read_string(rt.invoke_boxed(&to_string, name, &mut []).map_err(text)?).filter(|n| !n.trim().is_empty());
                Ok(Marker { id: format!("mark:{key}"), kind: MarkerKind::Locator, label: Some(label.unwrap_or_else(|| "Map mark".into())), icon: Some(MARK_ICON.into()), position: to_proto(Vector3 { x: p.x, y, z: p.y }) })
            };
            if let Ok(marker) = read() {
                marks.push(marker);
            }
        })?;
        Ok(marks)
    }

    /// Calls `visit` with each mark's key and boxed value.
    unsafe fn walk(&self, rt: &Runtime, global: *mut ManagedObject, mut visit: impl FnMut(i64, *mut ManagedObject)) -> Result<(), String> {
        let text = |e: unity_runtime::Error| e.to_string();
        let compass: *mut ManagedObject = self.compass.get(global);
        if compass.is_null() {
            return Ok(());
        }
        let dict = rt.invoke(&self.get_marks, compass, &mut []).map_err(text)?;
        let class = rt.class_of(dict).ok_or("no dictionary")?;
        let enumerator = rt.invoke_boxed(&rt.method(class, "GetEnumerator", 0).map_err(text)?, dict, &mut []).map_err(text)?;
        let class = rt.class_of(enumerator).ok_or("no enumerator")?;
        let (next, current) = (rt.method(class, "MoveNext", 0).map_err(text)?, rt.method(class, "get_Current", 0).map_err(text)?);

        let mut seen = 0;
        while seen < MAX_MARKS && rt.unbox::<bool>(rt.invoke_boxed(&next, enumerator, &mut []).map_err(text)?).map_err(text)? {
            seen += 1;
            let pair = rt.invoke_boxed(&current, enumerator, &mut []).map_err(text)?;
            let class = rt.class_of(pair).ok_or("no pair")?;
            let key: i64 = rt.unbox(rt.invoke_boxed(&rt.method(class, "get_Key", 0).map_err(text)?, pair, &mut []).map_err(text)?).map_err(text)?;
            visit(key, rt.invoke_boxed(&rt.method(class, "get_Value", 0).map_err(text)?, pair, &mut []).map_err(text)?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_maps_protocol::Vec3;

    fn pin(x: f32, z: f32) -> GamePin {
        GamePin { name: "Base".into(), position: Vec3::new(x, 0.0, z) }
    }

    #[test]
    fn a_spot_keeps_its_key() {
        assert_eq!(mark_key(&pin(10.2, -5.0)), mark_key(&pin(9.9, -5.4)));
        assert_ne!(mark_key(&pin(10.0, 5.0)), mark_key(&pin(5.0, 10.0)));
    }

    #[test]
    fn app_marks_are_told_from_the_players() {
        assert!(is_app_mark(mark_key(&pin(-4000.0, 1700.0))));
        // `DateTime.Ticks`, which is what the game uses.
        assert!(!is_app_mark(639_000_000_000_000_000));
    }
}
