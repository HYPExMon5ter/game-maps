//! World export for Sons of the Forest: every loaded pickup, GPS point of interest and
//! cave entrance, with item names and types resolved through the item database.
//!
//! Pickup positions aren't stored in the scene files; they only exist once the world
//! is loaded, so this runs inside the game on request from the dev tooling.

use std::collections::{BTreeMap, HashMap};

use game_maps_protocol::{ExportEntry, WorldExport};
use unity_runtime::{read_string, Class, Field, ManagedObject, Method, Runtime};

use super::caves::Caves;
use super::{SonsOfTheForest, GAME_ID};
use crate::provider::{to_proto, Cx};

struct ItemInfo {
    name: String,
    item_type: String,
    /// Localized display name, description, and inventory icon texture name.
    title: Option<String>,
    description: Option<String>,
    icon: Option<String>,
}

struct ItemLookup {
    by_id: Option<Method>,
    db_instance: Option<Field>,
    get_name: Option<Method>,
    get_type: Option<Method>,
    get_ui_data: Option<Method>,
    ui_title: Option<Method>,
    ui_description: Option<Method>,
    ui_icon: Option<Field>,
    cache: HashMap<i32, Option<ItemInfo>>,
}

impl ItemLookup {
    fn new(rt: &Runtime) -> Self {
        let db = rt.class("Sons.Items.Core", "ItemDatabaseManager").ok();
        let data = rt.class("Sons.Items.Core", "ItemData").ok();
        let ui = rt.class("", "ItemUiData").ok();
        Self {
            get_ui_data: data.and_then(|c| rt.method(c, "get_UiData", 0).ok()),
            ui_title: ui.and_then(|c| rt.method(c, "GetTitleLocalized", 0).ok()),
            ui_description: ui.and_then(|c| rt.method(c, "GetDescription", 0).ok()),
            ui_icon: ui.and_then(|c| rt.field(c, "_icon").ok()),
            by_id: db.and_then(|c| rt.method(c, "ItemById", 1).ok()),
            db_instance: db.and_then(|c| rt.field(c, "_instance").ok()),
            get_name: data.and_then(|c| rt.method(c, "get_Name", 0).ok()),
            get_type: data.and_then(|c| rt.method(c, "get_Type", 0).ok()),
            cache: HashMap::new(),
        }
    }

    unsafe fn get(&mut self, rt: &Runtime, id: i32) -> Option<&ItemInfo> {
        if !self.cache.contains_key(&id) {
            let info = self.resolve(rt, id);
            self.cache.insert(id, info);
        }
        self.cache[&id].as_ref()
    }

    unsafe fn resolve(&self, rt: &Runtime, id: i32) -> Option<ItemInfo> {
        let by_id = self.by_id.as_ref()?;
        let instance = if by_id.is_static { std::ptr::null_mut() } else { rt.read_field(self.db_instance?, std::ptr::null_mut()) };
        let mut id = id;
        let mut args = [&mut id as *mut i32 as *mut std::ffi::c_void];
        let data = rt.invoke(by_id, instance, &mut args).ok()?;
        if data.is_null() {
            return None;
        }
        let name = read_string(rt.invoke(self.get_name.as_ref()?, data, &mut []).ok()?)?;
        let item_type = self
            .get_type
            .as_ref()
            .and_then(|m| rt.invoke(m, data, &mut []).ok())
            .and_then(|boxed| rt.enum_name(boxed))
            .unwrap_or_default();
        let ui = self.get_ui_data.as_ref().and_then(|m| rt.invoke(m, data, &mut []).ok()).filter(|p| !p.is_null());
        let call = |m: &Option<Method>| -> Option<String> {
            let obj = ui?;
            read_string(rt.invoke(m.as_ref()?, obj, &mut []).ok()?).filter(|s| !s.trim().is_empty())
        };
        let title = call(&self.ui_title);
        let description = call(&self.ui_description);
        let icon = match (ui, self.ui_icon) {
            (Some(obj), Some(f)) => {
                let tex: *mut ManagedObject = f.get(obj);
                (!tex.is_null()).then(|| object_name(rt, tex)).flatten()
            }
            _ => None,
        };
        Some(ItemInfo { name, item_type, title, description, icon })
    }
}

/// `UnityEngine.Object.name` without a `Unity` handle (item data lives outside scenes).
unsafe fn object_name(rt: &Runtime, obj: *mut ManagedObject) -> Option<String> {
    let get_name = rt.method(rt.class("UnityEngine", "Object").ok()?, "get_name", 0).ok()?;
    read_string(rt.invoke(&get_name, obj, &mut []).ok()?)
}

struct Exporter<'a, 'c> {
    cx: &'a Cx<'c>,
    game: &'a SonsOfTheForest,
    items: ItemLookup,
    /// Cave members and this frame's cave systems.
    caves: Option<(Caves, Vec<*mut ManagedObject>)>,
    out: WorldExport,
}

impl<'a, 'c> Exporter<'a, 'c> {
    fn class(&mut self, namespace: &str, name: &str) -> Option<Class> {
        match self.cx.rt.class(namespace, name) {
            Ok(c) => Some(c),
            Err(e) => {
                self.out.notes.push(format!("{e}"));
                None
            }
        }
    }

    fn field(&mut self, class: Class, name: &str) -> Option<Field> {
        match self.cx.rt.field(class, name) {
            Ok(f) => Some(f),
            Err(e) => {
                self.out.notes.push(format!("{e}"));
                None
            }
        }
    }

    /// Every loaded object of `class` (active or not), as a component.
    unsafe fn objects(&mut self, class: Class) -> Vec<*mut ManagedObject> {
        match self.cx.unity.find_objects(self.cx.rt, class, true) {
            Ok(v) => v,
            Err(e) => {
                self.out.notes.push(format!("scan of {} failed: {e}", self.cx.rt.class_name(class)));
                Vec::new()
            }
        }
    }

    /// Builds an entry at `component`'s position. `None` if it has no live transform.
    unsafe fn entry(&mut self, kind: &str, component: *mut ManagedObject, at: Option<*mut ManagedObject>) -> Option<ExportEntry> {
        let (rt, unity) = (self.cx.rt, self.cx.unity);
        let go = unity.component_game_object(rt, component).ok()?;
        let transform = match at {
            Some(t) if unity.is_alive(t) => t,
            _ => unity.component_transform(rt, component).ok()?,
        };
        let pos = unity.position(rt, transform).ok()?;
        let mut props = BTreeMap::new();
        if let Some(class) = rt.class_of(component) {
            props.insert("class".into(), rt.class_name(class));
        }
        if let Some(ground) = self.game.ground_height(self.cx, pos) {
            props.insert("terrain_y".into(), format!("{ground:.2}"));
        }
        if let Some((caves, systems)) = &self.caves {
            if let Some(system) = caves.containing(rt, systems, pos) {
                props.insert("cave_area".into(), caves.area(system).to_string());
            }
        }
        Some(ExportEntry {
            kind: kind.into(),
            name: unity.object_name(rt, go).unwrap_or_default(),
            position: to_proto(pos),
            active: unity.active_in_hierarchy(rt, go).unwrap_or(false),
            props,
        })
    }

    unsafe fn add_item(&mut self, mut entry: ExportEntry, item_id: i32) {
        entry.props.insert("item_id".into(), item_id.to_string());
        if let Some(info) = self.items.get(self.cx.rt, item_id) {
            entry.props.insert("item".into(), info.name.clone());
            entry.props.insert("item_type".into(), info.item_type.clone());
            for (key, value) in [("title", &info.title), ("description", &info.description), ("icon", &info.icon)] {
                if let Some(v) = value {
                    entry.props.insert(key.into(), v.clone());
                }
            }
        }
        self.out.entries.push(entry);
    }

    unsafe fn pickups(&mut self) {
        let Some(class) = self.class("Sons.Gameplay", "PickUp") else { return };
        let Some(item_id) = self.field(class, "_itemId") else { return };
        let objects = self.objects(class);
        self.out.notes.push(format!("PickUp: {}", objects.len()));
        for obj in objects {
            if let Some(e) = self.entry("pickup", obj, None) {
                self.add_item(e, item_id.get::<i32>(obj));
            }
        }
    }

    unsafe fn virtual_pickups(&mut self) {
        let Some(class) = self.class("Sons.Gameplay", "VirtualPickup") else { return };
        let Some(item_id) = self.field(class, "_itemIdCached") else { return };
        let objects = self.objects(class);
        self.out.notes.push(format!("VirtualPickup: {}", objects.len()));
        for obj in objects {
            if let Some(e) = self.entry("virtual_pickup", obj, None) {
                self.add_item(e, item_id.get::<i32>(obj));
            }
        }
    }

    /// Switchers stay in the scene while their pickup is swapped out for a virtual one,
    /// so they cover pickups that aren't currently spawned.
    unsafe fn pickup_switchers(&mut self) {
        let Some(class) = self.class("Sons.Gameplay", "VirtualPickupSwitcher") else { return };
        let Some(pickup_class) = self.class("Sons.Gameplay", "PickUp") else { return };
        let (Some(pickup), Some(root), Some(item_id)) =
            (self.field(class, "_pickup"), self.field(class, "_rootTransform"), self.field(pickup_class, "_itemId"))
        else {
            return;
        };
        let objects = self.objects(class);
        self.out.notes.push(format!("VirtualPickupSwitcher: {}", objects.len()));
        for obj in objects {
            let p: *mut ManagedObject = pickup.get(obj);
            if !self.cx.unity.is_alive(p) {
                continue;
            }
            if let Some(e) = self.entry("pickup_switcher", obj, Some(root.get(obj))) {
                self.add_item(e, item_id.get::<i32>(p));
            }
        }
    }

    unsafe fn gps_locators(&mut self) {
        let rt = self.cx.rt;
        let Some(class) = self.class("Sons.Gameplay.GPS", "GPSLocator") else { return };
        let poi = self.field(class, "_pointOfInterestName");
        let icon_id = self.field(class, "_currentLocatorIconId");
        let is_active = self.field(class, "_isActive");
        let icon_data = rt.method(class, "get_IconData", 0).ok();
        let actor_id = self.field(class, "_worldActorId");
        let tracked = rt.method(class, "Position", 0).ok();
        let objects = self.objects(class);
        self.out.notes.push(format!("GPSLocator: {}", objects.len()));
        for obj in objects {
            let Some(mut e) = self.entry("gps_locator", obj, None) else { continue };
            if let Some(name) = poi.and_then(|f| read_string(f.get(obj))).filter(|s| !s.is_empty()) {
                e.props.insert("poi".into(), name);
            }
            if let Some(f) = icon_id {
                e.props.insert("icon_id".into(), f.get::<i32>(obj).to_string());
            }
            if let Some(f) = is_active {
                e.props.insert("locator_active".into(), f.get::<bool>(obj).to_string());
            }
            if let Some(f) = actor_id {
                e.props.insert("world_actor_id".into(), f.get::<i32>(obj).to_string());
            }
            if let Some(p) = tracked.as_ref().and_then(|m| rt.invoke(m, obj, &mut []).ok()).and_then(|b| rt.unbox::<unity_runtime::Vector3>(b).ok()) {
                e.props.insert("tracked_position".into(), format!("{:.1},{:.1},{:.1}", p.x, p.y, p.z));
            }
            if let Some(icon) = icon_data.as_ref().and_then(|m| self.icon_name(m, obj)) {
                e.props.insert("icon".into(), icon);
            }
            self.out.entries.push(e);
        }
    }

    /// Sprite name of a locator's current icon (`IconData._icon`).
    unsafe fn icon_name(&self, get_icon_data: &Method, locator: *mut ManagedObject) -> Option<String> {
        let rt = self.cx.rt;
        let data = rt.invoke(get_icon_data, locator, &mut []).ok()?;
        let field = rt.field(rt.class_of(data)?, "_icon").ok()?;
        self.cx.unity.object_name(rt, field.get(data))
    }

    unsafe fn cave_entrances(&mut self) {
        let Some(class) = self.class("Sons.Areas", "CaveEntrance") else { return };
        let area = self.field(class, "_areaId");
        let objects = self.objects(class);
        self.out.notes.push(format!("CaveEntrance: {}", objects.len()));
        for obj in objects {
            let Some(mut e) = self.entry("cave_entrance", obj, None) else { continue };
            if let Some(f) = area {
                e.props.insert("area".into(), f.get::<i32>(obj).to_string());
            }
            self.out.entries.push(e);
        }
    }
}

/// # Safety
/// Main thread only.
pub(super) unsafe fn export(game: &SonsOfTheForest, cx: &Cx) -> WorldExport {
    let mut ex = Exporter {
        cx,
        game,
        items: ItemLookup::new(cx.rt),
        caves: Caves::resolve(cx.rt).map(|c| {
            let systems = c.systems(cx.rt, cx.unity);
            (c, systems)
        }),
        out: WorldExport { game: GAME_ID.into(), ..Default::default() },
    };
    ex.pickups();
    ex.virtual_pickups();
    ex.pickup_switchers();
    ex.gps_locators();
    ex.cave_entrances();
    ex.out
}
