//! Remembers what's in the areas around the player: ore, plants, portals and ships.
//!
//! The game only places these when an area first generates, so nothing can list them for
//! the whole world: the map fills in as the player goes. `ZDOMan` keeps every networked
//! object by 64 m sector. One sector is read per call, straight from memory, so the cost
//! stays flat however much is around; only plants and portals need a call into the game.

use std::collections::HashMap;
use std::ffi::c_void;

use game_maps_protocol::{Vec3, WorldPin, ZonePins};
use unity_runtime::{read_object_array, read_string, Class, Field, ListLayout, ManagedArray, ManagedObject, Method, Runtime, Vector3};

use super::super::{to_proto, Cx};

/// Sectors per side of the area read around the player: what the game keeps loaded.
const RADIUS: i32 = 2;
/// `ZoneSystem.SectorToIndex`: sectors are stored 512 to a row, from -256.
const SECTOR_OFFSET: i32 = 256;
const SECTORS_PER_ROW: i32 = 512;
const SECTOR_METERS: f64 = 64.0;
const MAX_OBJECTS: usize = 20000;
/// Where every portal in the world is reported, apart from the areas around the player.
const PORTALS_ZONE: (i32, i32) = (1_000_000, 0);
const MAX_PORTALS: usize = 4096;
/// Things of one kind this close together are one pin: a patch, not every bush.
const PATCH_METERS: f32 = 24.0;
/// How close the wishbone finds silver (`Beacon.m_range`).
const WISHBONE_METERS: f32 = 20.0;
const TICKS_PER_SECOND: i64 = 10_000_000;
/// Bits of a `DateTime` that hold its ticks; the top two are its kind.
const DATE_TICKS: i64 = 0x3FFF_FFFF_FFFF_FFFF;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Mined away for good.
    Deposit,
    /// Silver: buried until a wishbone finds it.
    Buried,
    /// Picked, and grows back.
    Plant,
    /// A portal, named by its tag.
    Portal,
    /// Moves around, so it's tracked by its identity.
    Vehicle,
}

/// Prefab, category (from `game.json`), name.
const THINGS: &[(&str, &str, &str, Kind)] = &[
    ("MineRock_Copper", "copper", "Copper", Kind::Deposit),
    ("rock4_copper", "copper", "Copper", Kind::Deposit),
    ("rock4_copper_frac", "copper", "Copper", Kind::Deposit),
    ("MineRock_Tin", "tin", "Tin", Kind::Deposit),
    ("silvervein", "silver", "Silver", Kind::Buried),
    ("silvervein_frac", "silver", "Silver", Kind::Buried),
    ("MineRock_Obsidian", "obsidian", "Obsidian", Kind::Deposit),
    ("MineRock_Meteorite", "flametal", "Flametal", Kind::Deposit),
    ("mudpile", "scrap", "Muddy Scrap Pile", Kind::Deposit),
    ("mudpile2", "scrap", "Muddy Scrap Pile", Kind::Deposit),
    ("mudpile_old", "scrap", "Muddy Scrap Pile", Kind::Deposit),
    ("mudpile_frac", "scrap", "Muddy Scrap Pile", Kind::Deposit),
    ("mudpile2_frac", "scrap", "Muddy Scrap Pile", Kind::Deposit),
    ("mudpile_beacon", "scrap", "Muddy Scrap Pile", Kind::Buried),
    ("FlametalRockstand", "flametal", "Flametal", Kind::Deposit),
    ("Pickable_Tin", "tin", "Tin", Kind::Deposit),
    ("Pickable_Obsidian", "obsidian", "Obsidian", Kind::Deposit),
    ("Pickable_Meteorite", "flametal", "Flametal", Kind::Deposit),
    ("Pickable_BogIronOre", "scrap", "Bog Iron", Kind::Deposit),
    ("Pickable_SurtlingCoreStand", "cores", "Surtling Core", Kind::Deposit),
    ("Pickable_BlackCoreStand", "cores", "Black Core", Kind::Deposit),
    ("Pickable_MoltenCoreStand", "cores", "Molten Core", Kind::Deposit),
    ("Pickable_SulfurRock", "herbs", "Sulfur", Kind::Deposit),
    ("LingonberryBush", "berries", "Lingonberries", Kind::Plant),
    ("Pickable_Mushroom_JotunPuffs", "mushrooms", "Jotun Puffs", Kind::Plant),
    ("Pickable_Mushroom_Magecap", "mushrooms", "Magecap", Kind::Plant),
    ("Pickable_Fiddlehead", "herbs", "Fiddlehead", Kind::Plant),
    ("Pickable_SmokePuff", "herbs", "Smoke Puff", Kind::Plant),
    ("RaspberryBush", "berries", "Raspberries", Kind::Plant),
    ("BlueberryBush", "berries", "Blueberries", Kind::Plant),
    ("CloudberryBush", "berries", "Cloudberries", Kind::Plant),
    ("Pickable_Mushroom", "mushrooms", "Mushrooms", Kind::Plant),
    ("Pickable_Mushroom_yellow", "mushrooms", "Yellow Mushrooms", Kind::Plant),
    ("Pickable_Mushroom_blue", "mushrooms", "Blue Mushrooms", Kind::Plant),
    ("Pickable_Thistle", "herbs", "Thistle", Kind::Plant),
    ("Pickable_Dandelion", "herbs", "Dandelion", Kind::Plant),
    ("Pickable_Flax_Wild", "crops", "Flax", Kind::Plant),
    ("Pickable_Barley_Wild", "crops", "Barley", Kind::Plant),
    ("portal_wood", "portal", "Portal", Kind::Portal),
    ("portal_stone", "portal", "Stone Portal", Kind::Portal),
    ("Raft", "ship", "Raft", Kind::Vehicle),
    ("Karve", "ship", "Karve", Kind::Vehicle),
    ("VikingShip", "ship", "Longship", Kind::Vehicle),
    ("VikingShip_Ashlands", "ship", "Drakkar", Kind::Vehicle),
    ("Cart", "cart", "Cart", Kind::Vehicle),
];

/// `String.GetStableHashCode`, how the game names prefabs and object values.
pub fn stable_hash(text: &str) -> i32 {
    let chars: Vec<u16> = text.encode_utf16().collect();
    let (mut a, mut b) = (5381i32, 5381i32);
    let mut i = 0;
    while i < chars.len() && chars[i] != 0 {
        a = (a.wrapping_shl(5).wrapping_add(a)) ^ i32::from(chars[i]);
        if i == chars.len() - 1 || chars[i + 1] == 0 {
            break;
        }
        b = (b.wrapping_shl(5).wrapping_add(b)) ^ i32::from(chars[i + 1]);
        i += 2;
    }
    a.wrapping_add(b.wrapping_mul(1566083941))
}

/// The sector holding a world position (`ZoneSystem.GetZone`).
pub fn sector_of(x: f32, z: f32) -> (i32, i32) {
    (((f64::from(x) + 32.0) / SECTOR_METERS).floor() as i32, ((f64::from(z) + 32.0) / SECTOR_METERS).floor() as i32)
}

/// One thing seen in a sector.
#[derive(Clone)]
struct Seen {
    thing: usize,
    position: Vec3,
    /// Name from the game (a portal's tag).
    label: Option<String>,
    /// Identity of something that moves.
    id: Option<String>,
    /// Seconds until a picked plant is back; `None` while it's there to pick.
    regrows_in: Option<u32>,
}

/// Merges things of a kind into patches, and makes them pins. Sorted first, so the same
/// things give the same ids from one scan to the next.
fn to_pins(mut seen: Vec<Seen>, finder: Option<Vec3>) -> Vec<WorldPin> {
    seen.sort_by(|a, b| a.position.x.total_cmp(&b.position.x).then(a.position.z.total_cmp(&b.position.z)));
    let near = |a: Vec3, b: Vec3, meters: f32| (a.x - b.x).hypot(a.z - b.z) < meters;
    let mut pins: Vec<(WorldPin, usize)> = Vec::new();
    for s in seen {
        let (_, category, name, kind) = THINGS[s.thing];
        let found = finder.is_some_and(|f| near(f, s.position, WISHBONE_METERS));
        let patch = matches!(kind, Kind::Deposit | Kind::Buried | Kind::Plant).then(|| pins.iter_mut().find(|(p, _)| p.name == name && near(p.position, s.position, PATCH_METERS))).flatten();
        match patch {
            Some((pin, count)) => {
                *count += 1;
                pin.hidden &= !found;
                // Worth going to as soon as any of it is back.
                pin.respawn_seconds = match (pin.respawn_seconds, s.regrows_in) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                };
            }
            None => {
                let id = match s.id {
                    Some(id) => format!("{category}:{id}"),
                    None => format!("{category}:{name}@{:.0},{:.0}", s.position.x, s.position.z),
                };
                let pin = WorldPin { id, category: category.to_owned(), name: s.label.unwrap_or_else(|| name.to_owned()), position: s.position, hidden: kind == Kind::Buried && !found, respawn_seconds: s.regrows_in, respawn_at: None };
                pins.push((pin, 1));
            }
        }
    }
    pins.into_iter().map(|(p, _)| p).collect()
}

pub struct Scanner {
    instance: Field,
    sectors: Field,
    /// `ZDOMan.GetPortals`: every portal the game knows of, by sector.
    portals: Option<Method>,
    prefab: Field,
    position: Field,
    uid: Field,
    /// `ZDOID.ID` inside `uid`: its other half is only an index for this session.
    object_id: Field,
    get_string: Option<Method>,
    get_bool: Option<Method>,
    get_long: Option<Method>,
    /// `ZNet.GetTime`.
    time: Option<Method>,
    /// `ZNetScene.instance` and `GetPrefab(int)`, and `Pickable.m_respawnTimeMinutes`.
    prefabs: Option<(Method, Method, Class, Field)>,
    things: HashMap<i32, usize>,
    /// Minutes each plant takes to grow back, as they're first needed.
    regrow_minutes: HashMap<usize, f32>,
    layout: Option<ListLayout>,
    /// Next sector around the player, 0 to (2 * RADIUS + 1)² - 1.
    cursor: i32,
    /// What was last reported for each sector.
    reported: HashMap<(i32, i32), Vec<WorldPin>>,
}

impl Scanner {
    pub fn resolve(rt: &Runtime, net: Class) -> Option<Self> {
        let man = rt.class("", "ZDOMan").ok()?;
        let zdo = rt.class("", "ZDO").ok()?;
        let scene = rt.class("", "ZNetScene").ok();
        let pickable = rt.class("", "Pickable").ok();
        let prefabs = (|| {
            let (scene, pickable) = (scene?, pickable?);
            Some((rt.method(scene, "get_instance", 0).ok()?, rt.method_with_params(scene, "GetPrefab", &["System.Int32"]).ok()?, pickable, rt.field(pickable, "m_respawnTimeMinutes").ok()?))
        })();
        Some(Self {
            instance: rt.field(man, "s_instance").ok().filter(|f| f.is_static)?,
            sectors: rt.field(man, "m_objectsBySector").ok()?,
            portals: rt.method(man, "GetPortals", 0).ok(),
            prefab: rt.field(zdo, "m_prefab").ok()?,
            position: rt.field(zdo, "m_position").ok()?,
            uid: rt.field(zdo, "m_uid").ok()?,
            object_id: rt.class("", "ZDOID").ok().and_then(|c| rt.field(c, "<ID>k__BackingField").ok())?,
            get_string: rt.method_with_params(zdo, "GetString", &["System.Int32", "System.String"]).ok(),
            get_bool: rt.method_with_params(zdo, "GetBool", &["System.Int32", "System.Boolean"]).ok(),
            get_long: rt.method_with_params(zdo, "GetLong", &["System.Int32", "System.Int64"]).ok(),
            time: rt.method(net, "GetTime", 0).ok(),
            prefabs,
            things: THINGS.iter().enumerate().map(|(i, t)| (stable_hash(t.0), i)).collect(),
            regrow_minutes: HashMap::new(),
            layout: None,
            cursor: 0,
            reported: HashMap::new(),
        })
    }

    /// A new world: nothing has been reported in it.
    pub fn reset(&mut self) {
        self.reported.clear();
        self.cursor = 0;
    }

    unsafe fn regrow_minutes(&mut self, cx: &Cx, hash: i32, thing: usize) -> f32 {
        if let Some(&minutes) = self.regrow_minutes.get(&thing) {
            return minutes;
        }
        let minutes = (|| {
            let (instance, get, pickable, field) = self.prefabs.as_ref()?;
            let scene = cx.rt.invoke(instance, std::ptr::null_mut(), &mut []).ok().filter(|s| !s.is_null())?;
            let mut hash = hash;
            let mut args = [&mut hash as *mut i32 as *mut c_void];
            let prefab = cx.rt.invoke(get, scene, &mut args).ok().filter(|p| !p.is_null())?;
            let component = cx.unity.get_component(cx.rt, prefab, *pickable).ok().flatten()?;
            Some(field.get::<f32>(component))
        })()
        .unwrap_or(0.0);
        self.regrow_minutes.insert(thing, minutes);
        minutes
    }

    /// Seconds until a plant is back, if it's been picked.
    unsafe fn regrows_in(&mut self, cx: &Cx, zdo: *mut ManagedObject, hash: i32, thing: usize, now_ticks: Option<i64>) -> Option<u32> {
        let (get_bool, get_long) = (self.get_bool.as_ref()?, self.get_long.as_ref()?);
        let (mut key, mut fallback) = (stable_hash("picked"), false);
        let mut args = [&mut key as *mut i32 as *mut c_void, &mut fallback as *mut bool as *mut c_void];
        let picked: bool = cx.rt.unbox(cx.rt.invoke(get_bool, zdo, &mut args).ok()?).ok()?;
        if !picked {
            return None;
        }
        let (mut key, mut fallback) = (stable_hash("picked_time"), 0i64);
        let mut args = [&mut key as *mut i32 as *mut c_void, &mut fallback as *mut i64 as *mut c_void];
        let picked_at: i64 = cx.rt.unbox(cx.rt.invoke(get_long, zdo, &mut args).ok()?).ok()?;
        let minutes = self.regrow_minutes(cx, hash, thing);
        let since = (now_ticks? - picked_at) / TICKS_PER_SECOND;
        Some((f64::from(minutes) * 60.0 - since as f64).max(0.0) as u32)
    }

    unsafe fn portal_name(&self, cx: &Cx, zdo: *mut ManagedObject) -> Option<String> {
        let mut key = stable_hash("tag");
        let mut args = [&mut key as *mut i32 as *mut c_void, cx.rt.new_string("") as *mut c_void];
        read_string(cx.rt.invoke(self.get_string.as_ref()?, zdo, &mut args).ok()?).filter(|t| !t.trim().is_empty())
    }

    /// Every portal in the world, once per round. They're kept apart from other objects, and
    /// a host has them all.
    unsafe fn all_portals(&mut self, cx: &Cx) -> Option<ZonePins> {
        let man: *mut ManagedObject = cx.rt.static_value(self.instance);
        if man.is_null() {
            return None;
        }
        let by_sector = cx.rt.invoke(self.portals.as_ref()?, man, &mut []).ok().filter(|d| !d.is_null())?;
        let class = cx.rt.class_of(by_sector)?;
        let values = cx.rt.invoke(&cx.rt.method(class, "get_Values", 0).ok()?, by_sector, &mut []).ok()?;
        let object = cx.rt.class("System", "Object").ok()?;
        let (lists, count) = super::copy_collection(cx.rt, values, object, MAX_PORTALS).ok()?;
        let mut seen = Vec::new();
        for list in read_object_array(lists.object, count) {
            if list.is_null() {
                continue;
            }
            let layout = match self.layout {
                Some(l) => l,
                None => *self.layout.insert(ListLayout::of(cx.rt, list).ok()?),
            };
            for zdo in layout.elements(list, MAX_PORTALS) {
                if let Some(s) = self.read_portal(cx, zdo) {
                    seen.push(s);
                }
            }
        }
        // Nothing on a client doesn't mean the world has none.
        if seen.is_empty() {
            return None;
        }
        self.report(PORTALS_ZONE, to_pins(seen, None))
    }

    unsafe fn read_portal(&self, cx: &Cx, zdo: *mut ManagedObject) -> Option<Seen> {
        if zdo.is_null() {
            return None;
        }
        let thing = *self.things.get(&self.prefab.get::<i32>(zdo))?;
        (THINGS[thing].3 == Kind::Portal).then(|| Seen { thing, position: to_proto(self.position.get::<Vector3>(zdo)), label: self.portal_name(cx, zdo), id: Some(self.identity(zdo)), regrows_in: None })
    }

    unsafe fn identity(&self, zdo: *mut ManagedObject) -> String {
        format!("{:x}", self.object_id.get_inline::<u32>((zdo as *const u8).add(self.uid.offset)))
    }

    /// `pins` for an area, if they aren't what was reported last.
    fn report(&mut self, zone: (i32, i32), pins: Vec<WorldPin>) -> Option<ZonePins> {
        if self.reported.get(&zone).is_some_and(|last| same_pins(last, &pins)) {
            return None;
        }
        self.reported.insert(zone, pins.clone());
        Some(ZonePins { x: zone.0, y: zone.1, pins })
    }

    /// Reads the next sector around `around`. `Some` when what's there changed.
    /// `wishbone`: the player has one on, so silver near them counts as found.
    pub unsafe fn next(&mut self, cx: &Cx, net: *mut ManagedObject, around: Vector3, wishbone: bool) -> Option<ZonePins> {
        if self.cursor == 0 {
            self.cursor = 1;
            if let Some(portals) = self.all_portals(cx) {
                return Some(portals);
            }
        }
        let side = 2 * RADIUS + 1;
        let (cx0, cz0) = sector_of(around.x, around.z);
        let (x, z) = (cx0 + self.cursor % side - RADIUS, cz0 + self.cursor / side - RADIUS);
        self.cursor = (self.cursor + 1) % (side * side);
        let (row, column) = (z + SECTOR_OFFSET, x + SECTOR_OFFSET);
        if !(0..SECTORS_PER_ROW).contains(&row) || !(0..SECTORS_PER_ROW).contains(&column) {
            return None;
        }
        let man: *mut ManagedObject = cx.rt.static_value(self.instance);
        if man.is_null() {
            return None;
        }
        let sectors: *mut ManagedObject = self.sectors.get(man);
        if sectors.is_null() {
            return None;
        }
        let index = (row * SECTORS_PER_ROW + column) as usize;
        let array = ManagedArray::of(cx.rt, sectors, index + 1);
        if array.len != index + 1 {
            return None;
        }
        let list = std::ptr::read_unaligned(array.element(index) as *const *mut ManagedObject);
        if list.is_null() {
            return None;
        }
        let layout = match self.layout {
            Some(l) => l,
            None => *self.layout.insert(ListLayout::of(cx.rt, list).ok()?),
        };
        let objects = layout.elements(list, MAX_OBJECTS);
        // An area with nothing in it isn't loaded here: that says nothing about what it has.
        if objects.is_empty() {
            return None;
        }

        let now_ticks = self.time.as_ref().and_then(|m| cx.rt.unbox::<i64>(cx.rt.invoke(m, net, &mut []).ok()?).ok()).map(|t| t & DATE_TICKS);
        let mut seen = Vec::new();
        for zdo in objects {
            if zdo.is_null() {
                continue;
            }
            let hash: i32 = self.prefab.get(zdo);
            let Some(&thing) = self.things.get(&hash) else { continue };
            let kind = THINGS[thing].3;
            let position = to_proto(self.position.get::<Vector3>(zdo));
            let identity = (kind == Kind::Vehicle || kind == Kind::Portal).then(|| self.identity(zdo));
            let label = if kind == Kind::Portal { self.portal_name(cx, zdo) } else { None };
            let regrows_in = if kind == Kind::Plant { self.regrows_in(cx, zdo, hash, thing, now_ticks) } else { None };
            seen.push(Seen { thing, position, label, id: identity, regrows_in });
        }
        self.report((x, z), to_pins(seen, wishbone.then_some(to_proto(around))))
    }
}

/// Whether a report would change nothing. Regrowing plants count down, which isn't news.
fn same_pins(a: &[WorldPin], b: &[WorldPin]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.id == q.id && p.name == q.name && p.hidden == q.hidden && p.respawn_seconds.is_some() == q.respawn_seconds.is_some() && p.position.distance_sq(q.position) < 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(prefab: &str, x: f32, z: f32) -> Seen {
        Seen { thing: THINGS.iter().position(|t| t.0 == prefab).unwrap(), position: Vec3::new(x, 0.0, z), label: None, id: None, regrows_in: None }
    }

    #[test]
    fn hashes_names_like_the_game() {
        // ZDOVars: "tag" and "picked" go through the same function.
        assert_eq!(stable_hash(""), 5381i32.wrapping_add(5381i32.wrapping_mul(1566083941)));
        assert_ne!(stable_hash("tag"), stable_hash("gat"));
    }

    #[test]
    fn finds_the_sector_like_the_game() {
        assert_eq!(sector_of(0.0, 0.0), (0, 0));
        assert_eq!(sector_of(31.9, -32.1), (0, -1));
        assert_eq!(sector_of(32.0, 95.9), (1, 1));
    }

    #[test]
    fn a_patch_is_one_pin() {
        let pins = to_pins(vec![seen("RaspberryBush", 0.0, 0.0), seen("RaspberryBush", 5.0, 5.0), seen("RaspberryBush", 100.0, 0.0), seen("BlueberryBush", 2.0, 0.0)], None);
        let names: Vec<&str> = pins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Raspberries", "Blueberries", "Raspberries"]);
    }

    #[test]
    fn ids_stay_the_same_between_scans() {
        let a = to_pins(vec![seen("MineRock_Tin", 1.0, 2.0), seen("MineRock_Tin", 3.0, 2.0)], None);
        let b = to_pins(vec![seen("MineRock_Tin", 3.0, 2.0), seen("MineRock_Tin", 1.0, 2.0)], None);
        assert_eq!(a[0].id, b[0].id);
    }

    #[test]
    fn silver_stays_hidden_until_a_wishbone_is_near() {
        let silver = || vec![seen("silvervein", 100.0, 0.0)];
        assert!(to_pins(silver(), None)[0].hidden);
        assert!(to_pins(silver(), Some(Vec3::new(0.0, 0.0, 0.0)))[0].hidden);
        assert!(!to_pins(silver(), Some(Vec3::new(90.0, 0.0, 0.0)))[0].hidden);
        assert!(!to_pins(vec![seen("MineRock_Tin", 0.0, 0.0)], None)[0].hidden);
    }

    #[test]
    fn a_patch_is_ready_when_any_of_it_is() {
        let mut picked = seen("RaspberryBush", 0.0, 0.0);
        picked.regrows_in = Some(60);
        let pins = to_pins(vec![picked.clone(), seen("RaspberryBush", 1.0, 0.0)], None);
        assert_eq!(pins[0].respawn_seconds, None);
        let mut sooner = picked.clone();
        sooner.regrows_in = Some(30);
        assert_eq!(to_pins(vec![picked, sooner], None)[0].respawn_seconds, Some(30));
    }

    #[test]
    fn ships_keep_their_identity_wherever_they_go() {
        let mut ship = seen("Karve", 10.0, 10.0);
        ship.id = Some("abc".into());
        assert_eq!(to_pins(vec![ship], None)[0].id, "ship:abc");
    }
}
