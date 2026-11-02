//! Live GPS markers: everything the in-game GPS map shows (`GPSLocator._activeLocators`),
//! including Kelvin's tracker, locators the player places, and vehicles as they move.
//! Kelvin's locator follows his world-simulation actor, so he stays on the map even
//! when he's far away and not spawned.

use std::collections::HashMap;

use game_maps_protocol::{Marker, MarkerKind};
use il2cpp::{read_object_array, Field, Il2CppObject, Method, Runtime, Unity, Vector3};

use crate::provider::to_proto;

/// GPS icon (sprite) name of Kelvin's tracker.
const KELVIN_ICON: &str = "RobbyLogo";
/// Upper bound on locators read per refresh (the game has ~100 at a time).
const MAX_LOCATORS: usize = 1024;

pub struct GpsMarkers {
    active_locators: Field,
    current_icon_id: Field,
    is_active: Option<Field>,
    get_icon_data: Method,
    position: Method,
    /// `List<T>` layout of `_activeLocators`, resolved from the live instance.
    list_layout: Option<(Field, Field)>,
    /// Icon sprite name per (locator, icon id); icons rarely change, names allocate.
    icon_names: HashMap<(usize, i32), Option<String>>,
}

// Raw object pointers are only dereferenced on the main thread.
unsafe impl Send for GpsMarkers {}

impl GpsMarkers {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let class = rt.class("Sons.Gameplay.GPS", "GPSLocator").ok()?;
        Some(Self {
            active_locators: rt.field(class, "_activeLocators").ok().filter(|f| f.is_static)?,
            current_icon_id: rt.field(class, "_currentLocatorIconId").ok()?,
            is_active: rt.field(class, "_isActive").ok(),
            get_icon_data: rt.method(class, "get_IconData", 0).ok()?,
            position: rt.method(class, "Position", 0).ok()?,
            list_layout: None,
            icon_names: HashMap::new(),
        })
    }

    /// Reads all active, visible markers. Main thread only.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity) -> Result<Vec<Marker>, String> {
        let list: *mut Il2CppObject = rt.static_value(self.active_locators);
        if list.is_null() {
            return Ok(Vec::new());
        }
        if self.list_layout.is_none() {
            let class = rt.class_of(list).ok_or("GPS locator list has no class")?;
            match (rt.field(class, "_items"), rt.field(class, "_size")) {
                (Ok(items), Ok(size)) => self.list_layout = Some((items, size)),
                _ => return Err(format!("GPS locators are stored in an unsupported `{}`", rt.class_name(class))),
            }
        }
        let (items, size) = self.list_layout.unwrap();
        let count = (size.get::<i32>(list).max(0) as usize).min(MAX_LOCATORS);
        let locators = read_object_array(items.get(list), count);

        let mut markers = Vec::with_capacity(locators.len());
        for locator in locators {
            if !unity.is_alive(locator) || self.is_active.is_some_and(|f| !f.get::<bool>(locator)) {
                continue;
            }
            // Locators without a sprite are hidden duplicates (e.g. Kelvin has two).
            let Some(icon) = self.icon_name(rt, unity, locator) else { continue };
            let Ok(boxed) = rt.invoke(&self.position, locator, &mut []) else { continue };
            let Ok(pos) = rt.unbox::<Vector3>(boxed) else { continue };
            let companion = icon == KELVIN_ICON;
            markers.push(Marker {
                id: format!("gps:{:x}", locator as usize),
                kind: if companion { MarkerKind::Companion } else { MarkerKind::Locator },
                label: companion.then(|| "Kelvin".to_owned()),
                icon: Some(icon),
                position: to_proto(pos),
            });
        }
        // Forget cached icons of locators that are gone.
        if self.icon_names.len() > markers.len() * 4 + 64 {
            self.icon_names.clear();
        }
        Ok(markers)
    }

    unsafe fn icon_name(&mut self, rt: &Runtime, unity: &Unity, locator: *mut Il2CppObject) -> Option<String> {
        let key = (locator as usize, self.current_icon_id.get::<i32>(locator));
        if let Some(name) = self.icon_names.get(&key) {
            return name.clone();
        }
        let name = (|| {
            let data = rt.invoke(&self.get_icon_data, locator, &mut []).ok().filter(|d| !d.is_null())?;
            let icon = rt.field(rt.class_of(data)?, "_icon").ok()?;
            unity.object_name(rt, icon.get(data))
        })();
        self.icon_names.insert(key, name.clone());
        name
    }
}
