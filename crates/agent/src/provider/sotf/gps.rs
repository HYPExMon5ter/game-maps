//! Live GPS markers: everything the in-game GPS map shows (`GPSLocator._activeLocators`),
//! including Kelvin's tracker, locators the player places, and vehicles as they move.
//! Every player carries a GPS tracker of their own; those locators sit below the
//! player's transform and are left out (players are drawn as players).
//! Kelvin's locator follows his world-simulation actor, so he stays on the map even
//! when he's far away and not spawned.

use std::collections::HashMap;

use game_maps_protocol::{Marker, MarkerKind};
use unity_runtime::{Field, ListLayout, ManagedObject, Method, Runtime, Unity, Vector3};

use crate::provider::to_proto;

/// GPS icon (sprite) name of Kelvin's tracker.
const KELVIN_ICON: &str = "RobbyLogo";
/// How far below a player's transform a carried tracker can sit (hand bones, inventory).
const CARRIED_DEPTH: usize = 48;
/// Upper bound on locators read per refresh (the game has ~100 at a time).
const MAX_LOCATORS: usize = 1024;

pub struct GpsMarkers {
    active_locators: Field,
    current_icon_id: Field,
    is_active: Option<Field>,
    get_icon_data: Method,
    position: Method,
    /// `List<T>` layout of `_activeLocators`, resolved from the live instance.
    list_layout: Option<ListLayout>,
    /// Icon sprite name per (locator, icon id); icons rarely change, names allocate.
    icon_names: HashMap<(usize, i32), Option<String>>,
    /// Last summary written to the log.
    debug: String,
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
            debug: String::new(),
        })
    }

    /// Reads all active, visible markers. Main thread only.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    /// `players`: transforms of every player (local and remote); locators below them are
    /// carried by that player.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity, players: &[*mut ManagedObject]) -> Result<Vec<Marker>, String> {
        let list: *mut ManagedObject = rt.static_value(self.active_locators);
        if list.is_null() {
            return Ok(Vec::new());
        }
        let layout = match self.list_layout {
            Some(l) => l,
            None => *self.list_layout.insert(ListLayout::of(rt, list).map_err(|e| format!("GPS locators are stored in an unsupported list: {e}"))?),
        };
        let locators = layout.elements(list, MAX_LOCATORS);

        let (mut unnamed, mut carried_count) = (0, 0);
        let mut markers = Vec::with_capacity(locators.len());
        let total = locators.len();
        for locator in locators {
            if !unity.is_alive(locator) || self.is_active.is_some_and(|f| !f.get::<bool>(locator)) {
                continue;
            }
            // Locators without a sprite are hidden duplicates (e.g. Kelvin has two).
            let Some(icon) = self.icon_name(rt, unity, locator) else {
                unnamed += 1;
                continue;
            };
            if !players.is_empty() {
                let carried = unity.component_transform(rt, locator).and_then(|t| unity.is_within(rt, t, players, CARRIED_DEPTH));
                if carried.unwrap_or(false) {
                    carried_count += 1;
                    continue;
                }
            }
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
        // Why locators are or aren't shown, when that changes.
        let mut icons: Vec<&str> = markers.iter().filter_map(|m| m.icon.as_deref()).collect();
        icons.sort_unstable();
        let line = format!("gps locators {total} shown {icons:?} no sprite {unnamed} carried {carried_count}");
        if line != self.debug {
            crate::filelog::write(&line);
            self.debug = line.clone();
        }
        // Forget cached icons of locators that are gone.
        if self.icon_names.len() > total * 4 + 64 {
            self.icon_names.clear();
        }
        Ok(markers)
    }

    unsafe fn icon_name(&mut self, rt: &Runtime, unity: &Unity, locator: *mut ManagedObject) -> Option<String> {
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
