//! What the game's map panel says about a place (`UIMapLocationInfo`): how much of it has
//! been scavenged, and what's there to find.
//!
//! It's read when the player opens a place's pin, never while they're just playing: going
//! through a place's loot takes calls into the game, and nothing needs it until then.

use std::collections::BTreeMap;
use std::ffi::c_void;

use game_maps_protocol::{PlaceInfo, PlaceItem, PlaceLoot};
use unity_runtime::{read_string, Field, ManagedArray, ManagedObject, Method, Runtime, Unity};

use super::static_field;

const MAX_ITEMS: usize = 48;
/// Where a `HashSet<T>` slot keeps its value, after its hash code and next index.
const SLOT_VALUE: usize = 8;

pub struct Places {
    /// Rebuilds the lists below, which go stale as things are taken. The game's panel does
    /// the same every time it opens.
    refresh: Method,
    name: Method,
    scavenged: Method,
    level: Field,
    special: Method,
    scarce: Method,
    vegetation: Method,
    mine: Method,
    items: Field,
    find_item: Method,
    item_name: Method,
    active: Method,
    /// `LearningResource.Broken`: research stays where it is once it's been taken.
    research: Option<(unity_runtime::Class, Field)>,
}

impl Places {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let location = rt.class("", "Location").ok()?;
        let items = rt.class("", "RM").ok()?;
        let item = rt.class("", "Item").ok()?;
        let game_object = rt.class("UnityEngine", "GameObject").ok()?;
        Some(Self {
            refresh: rt.method(location, "InitLootInfo", 0).ok()?,
            name: rt.method(location, "get_DisplayName", 0).ok()?,
            scavenged: rt.method(location, "get_ScavengedPercentageString", 0).ok()?,
            level: rt.field_of(location, "level", "Int32").ok()?,
            special: rt.method(location, "get_SpecialLoots", 0).ok()?,
            scarce: rt.method(location, "get_ScarceLootItemIDs", 0).ok()?,
            vegetation: rt.method(location, "get_VegetationLootItemIDs", 0).ok()?,
            mine: rt.method(location, "get_MineLootItemIDs", 0).ok()?,
            items: static_field(rt, items, "code")?,
            find_item: rt.method(items, "TryGetItemByID", 2).ok()?,
            item_name: rt.method(item, "GetDisplayName", 0).ok()?,
            active: rt.method(game_object, "get_activeSelf", 0).ok()?,
            research: rt.class("", "LearningResource").ok().and_then(|c| Some((c, rt.field_of(c, "Broken", "Boolean").ok()?))),
        })
    }

    /// What's known about `location`, as the game's panel would show it.
    ///
    /// # Safety
    /// Must run on the game's main thread, with `location` alive.
    pub unsafe fn read(&self, rt: &Runtime, unity: &Unity, location: *mut ManagedObject) -> Result<PlaceInfo, String> {
        rt.invoke(&self.refresh, location, &mut []).map_err(|e| e.to_string())?;
        let text = |m: &Method| rt.invoke(m, location, &mut []).ok().and_then(|s| read_string(s));
        let list = |m: &Method| self.names(rt, &set_items::<i32>(rt, rt.invoke(m, location, &mut []).unwrap_or(std::ptr::null_mut())));
        let mut loot = vec![self.special_loot(rt, unity, location)];
        for (title, method) in [("Scarce", &self.scarce), ("Plants", &self.vegetation), ("Minerals", &self.mine)] {
            loot.push(PlaceLoot { title: title.into(), items: list(method).into_iter().map(|name| PlaceItem { name, taken: false }).collect() });
        }
        loot.retain(|group| !group.items.is_empty());
        Ok(PlaceInfo {
            name: text(&self.name).unwrap_or_default(),
            level: self.level.get(location),
            // "?%" where there's nothing to scavenge.
            scavenged: text(&self.scavenged).and_then(|p| p.strip_suffix('%')?.parse().ok()),
            loot,
        })
    }

    /// The one-off finds, each marked if it's gone.
    unsafe fn special_loot(&self, rt: &Runtime, unity: &Unity, location: *mut ManagedObject) -> PlaceLoot {
        let set = rt.invoke(&self.special, location, &mut []).unwrap_or(std::ptr::null_mut());
        let mut items: BTreeMap<String, bool> = BTreeMap::new();
        for loot in set_items::<*mut ManagedObject>(rt, set) {
            if !unity.is_alive(loot) {
                continue;
            }
            let Some(class) = rt.class_of(loot) else { continue };
            let Ok(ids) = rt.method(class, "get_LootItemIDs", 0) else { continue };
            let taken = self.taken(rt, unity, loot);
            let ids = set_items::<i32>(rt, rt.invoke(&ids, loot, &mut []).unwrap_or(std::ptr::null_mut()));
            for name in self.names(rt, &ids) {
                // Something that's in two places isn't gone until both are.
                *items.entry(name).or_insert(true) &= taken;
            }
        }
        PlaceLoot { title: "Key finds".into(), items: items.into_iter().take(MAX_ITEMS).map(|(name, taken)| PlaceItem { name, taken }).collect() }
    }

    unsafe fn taken(&self, rt: &Runtime, unity: &Unity, loot: *mut ManagedObject) -> bool {
        if let Some((class, broken)) = self.research {
            if rt.class_of(loot) == Some(class) {
                return broken.get(loot);
            }
        }
        let Ok(object) = unity.component_game_object(rt, loot) else { return false };
        !rt.invoke(&self.active, object, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(true)
    }

    /// Item names, sorted, each once.
    unsafe fn names(&self, rt: &Runtime, ids: &[i32]) -> Vec<String> {
        let items: *mut ManagedObject = rt.static_value(self.items);
        let mut names: Vec<String> = ids.iter().take(MAX_ITEMS).filter_map(|&id| self.item_name(rt, items, id)).collect();
        names.sort();
        names.dedup();
        names
    }

    unsafe fn item_name(&self, rt: &Runtime, items: *mut ManagedObject, mut id: i32) -> Option<String> {
        let mut item: *mut ManagedObject = std::ptr::null_mut();
        let mut args = [&mut id as *mut i32 as *mut c_void, &mut item as *mut *mut ManagedObject as *mut c_void];
        let found = rt.invoke(&self.find_item, items, &mut args).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(false);
        if !found || item.is_null() {
            return None;
        }
        read_string(rt.invoke(&self.item_name, item, &mut []).ok()?).filter(|n| !n.is_empty())
    }
}

/// What's in a `HashSet<T>`, read from its slots: `T` is a value or a managed reference.
unsafe fn set_items<T: Copy>(rt: &Runtime, set: *mut ManagedObject) -> Vec<T> {
    let Some(class) = (!set.is_null()).then(|| rt.class_of(set)).flatten() else { return Vec::new() };
    // Named differently across versions of the class library.
    let field = |new: &str, old: &str| rt.field(class, new).or_else(|_| rt.field(class, old)).ok();
    let (Some(slots), Some(used)) = (field("_slots", "m_slots"), field("_lastIndex", "m_lastIndex")) else { return Vec::new() };
    let slots: *mut ManagedObject = slots.get(set);
    let used: i32 = used.get(set);
    if slots.is_null() {
        return Vec::new();
    }
    let array = ManagedArray::of(rt, slots, used.max(0) as usize);
    if array.stride < SLOT_VALUE + std::mem::size_of::<T>() {
        return Vec::new();
    }
    (0..array.len)
        .filter_map(|i| {
            let slot = array.element(i);
            // A free slot has a negative hash code.
            ((slot as *const i32).read_unaligned() >= 0).then(|| (slot.add(SLOT_VALUE) as *const T).read_unaligned())
        })
        .collect()
}
