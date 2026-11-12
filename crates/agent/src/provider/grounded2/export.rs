//! World export for the map generator (`tools/pins/grounded2.py`).
//!
//! Grounded 2 keeps everything that can be picked up, harvested or fought as "LOD actors":
//! light objects for the whole world, which only become real actors near a player. They
//! exist from the start of a new game, so one pass over them lists every item, resource,
//! science pickup and creature in the world, with no exploring needed.
//!
//! What a LOD actor stands for is its "lite data" asset, which it doesn't expose; the
//! game's own `IsFullActorTypeLite` tells whether it's a given one. The lite data names the
//! real actor and, like the game's resource finder uses, what it drops.

use std::collections::{BTreeMap, HashMap};

use game_maps_protocol::{ExportEntry, WorldExport};
use unreal_runtime::{Args, FName, FVector, Obj, TArray};

use crate::unreal::{to_proto, UeCx};

/// `DataTableRowHandle`: data table, then row name.
#[repr(C)]
#[derive(Clone, Copy)]
struct RowHandle {
    table: Obj,
    row: FName,
}

/// What a lite data asset says about the things made from it.
struct Lite {
    asset: Obj,
    name: String,
    /// The real actor's class.
    actor: Option<String>,
    /// Item rows it drops.
    drops: Vec<String>,
    in_resource_finder: bool,
}

/// Lists every LOD actor in the world.
///
/// # Safety
/// Game thread only.
pub unsafe fn export(cx: &UeCx) -> Result<WorldExport, String> {
    let ue = &cx.ue;
    let class = |path: &str| ue.find_struct(path).map_err(|e| e.to_string());
    let base = class("/Script/Maine.BaseLODActor")?;
    let get_location = ue.function(base, "GetActorLocation").map_err(|e| e.to_string())?;
    let is_lite = ue.function(base, "IsFullActorTypeLite").map_err(|e| e.to_string())?;
    let spawned_item = class("/Script/Maine.SpawnedItemLODActor").ok();
    let get_item = spawned_item.and_then(|c| ue.function(c, "GetItem").ok());
    let item_row = class("/Script/Maine.Item").ok().and_then(|c| ue.property_of(c, "ItemDataRowHandle", "Struct<DataTableRowHandle_NetCrc32>").ok());
    let character = class("/Script/Maine.CharacterLODActor").ok();
    let character_name = character.and_then(|c| ue.function(c, "GetCharacterTypeName").ok());
    let lites = lite_assets(cx)?;
    let mut notes = vec![format!("{} lite data assets loaded", lites.values().map(Vec::len).sum::<usize>())];

    let mut entries = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    // The candidates for each LOD class, last match first: alike things are listed together.
    let mut order: HashMap<Obj, Vec<usize>> = HashMap::new();
    for lod in ue.instances_of(base) {
        let Some(location) = cx.call_get::<FVector>(lod, &get_location) else { continue };
        let lod_class = ue.class_of(lod);
        let lod_name = ue.name(lod_class);
        let mut props = BTreeMap::new();
        props.insert("lod".to_owned(), lod_name.clone());
        let is_item = spawned_item.is_some_and(|c| ue.is_a(lod, c));
        let is_character = character.is_some_and(|c| ue.is_a(lod, c));
        if !is_item && !is_character {
            if let Some(candidates) = lites.get(&lod_class) {
                let order = order.entry(lod_class).or_insert_with(|| (0..candidates.len()).collect());
                let hit = order.iter().position(|&i| {
                    let mut args = Args::new(&is_lite);
                    args.set("BaseType", candidates[i].asset).is_ok() && ue.call(lod, &mut args).is_ok() && args.get_bool("ReturnValue").unwrap_or(false)
                });
                if let Some(at) = hit {
                    let i = order.remove(at);
                    order.insert(0, i);
                    let lite = &candidates[i];
                    props.insert("lite".to_owned(), lite.name.clone());
                    if let Some(a) = &lite.actor {
                        props.insert("actor".to_owned(), a.clone());
                    }
                    if !lite.drops.is_empty() {
                        props.insert("drops".to_owned(), lite.drops.join(","));
                    }
                    if lite.in_resource_finder {
                        props.insert("resource_finder".to_owned(), "1".to_owned());
                    }
                }
            }
        }
        if let (true, Some(f), Some(row)) = (is_item, &get_item, &item_row) {
            if let Some(item) = cx.call_get::<Obj>(lod, f).filter(|&i| i != 0 && ue.alive(i)) {
                let handle: RowHandle = ue.get(item, row);
                props.insert("item".to_owned(), ue.fname(handle.row));
            }
        }
        if let (true, Some(f)) = (is_character, &character_name) {
            // `LocString`: table id, string id.
            if let Some([table, id, ..]) = cx.call_get::<[i32; 4]>(lod, f) {
                props.insert("name_table".to_owned(), table.to_string());
                props.insert("name_id".to_owned(), id.to_string());
            }
        }
        let name = props.get("item").or_else(|| props.get("actor")).or_else(|| props.get("lite")).cloned().unwrap_or_else(|| lod_name.clone());
        *counts.entry(lod_name).or_default() += 1;
        entries.push(ExportEntry { kind: "lod".into(), name, position: to_proto(location), active: true, props });
    }
    notes.extend(counts.iter().map(|(k, v)| format!("{k}: {v}")));
    Ok(WorldExport { game: "grounded-2".into(), entries, notes, caves: Vec::new() })
}

/// Every loaded lite data asset, by the LOD actor class made from it.
unsafe fn lite_assets(cx: &UeCx) -> Result<HashMap<Obj, Vec<Lite>>, String> {
    let ue = &cx.ue;
    let class = |path: &str| ue.find_struct(path).map_err(|e| e.to_string());
    let lite_class = class("/Script/Maine.ActorLiteData")?;
    let prop = |c: Obj, name: &str, ty: &str| ue.property_of(c, name, ty).map_err(|e| e.to_string());
    let lod_class = prop(lite_class, "LODActorClass", "Class<Object>")?;
    let classes = prop(lite_class, "Classes", "Array<Class<Actor>>")?;
    let components = prop(lite_class, "Components", "Array<Object<ComponentLiteData>>")?;
    let loot_class = class("/Script/Maine.LootComponentLiteData").ok();
    let loot_data = loot_class.and_then(|c| ue.property_of(c, "LootData", "Array<Struct<LootData>>").ok());
    let in_finder = loot_class.and_then(|c| ue.property(c, "bShowLootInResourceFinder").ok());
    let loot_struct = class("/Script/Maine.LootData").ok();
    let loot_size = loot_struct.map(|s| ue.struct_size(s)).unwrap_or(0);
    let loot_item = loot_struct.and_then(|s| ue.property_of(s, "ItemData", "Struct<DataTableRowHandle>").ok());

    let mut out: HashMap<Obj, Vec<Lite>> = HashMap::new();
    // Default objects aren't assets; `instances_of` leaves them out.
    for asset in ue.instances_of(lite_class) {
        let Some(lod) = ue.get_obj(asset, &lod_class) else { continue };
        let actor = ue.get::<TArray<Obj>>(asset, &classes).to_vec().into_iter().find(|&c| c != 0 && ue.alive(c)).map(|c| ue.name(c));
        let mut drops = Vec::new();
        let mut in_resource_finder = false;
        for component in ue.get::<TArray<Obj>>(asset, &components).to_vec() {
            let (Some(lc), Some(ld), Some(li)) = (loot_class, &loot_data, &loot_item) else { break };
            if component == 0 || !ue.alive(component) || !ue.is_a(component, lc) {
                continue;
            }
            in_resource_finder |= in_finder.as_ref().is_some_and(|p| ue.get_bool(component, p));
            let array: TArray<u8> = ue.get(component, ld);
            for i in 0..array.num.max(0) as usize {
                let handle: RowHandle = std::ptr::read_unaligned((array.data as usize + i * loot_size + li.offset) as *const RowHandle);
                let row = ue.fname(handle.row);
                if !row.is_empty() && row != "None" && !drops.contains(&row) {
                    drops.push(row);
                }
            }
        }
        out.entry(lod).or_default().push(Lite { asset, name: ue.name(asset), actor, drops, in_resource_finder });
    }
    Ok(out)
}
