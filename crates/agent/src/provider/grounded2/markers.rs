//! The game's own map markers (other players' waypoints, signs players built, the
//! backpack left where someone died), the player's waypoint as a ping, and chat.

use std::collections::HashSet;
use std::sync::Arc;

use game_maps_protocol::{Marker, MarkerKind, Vec3};
use unreal_runtime::{Args, FString, FVector, Function, Obj, Property, TArray, Unreal};

use crate::unreal::{from_proto, to_proto, UeCx};

/// `EHUDMarkerType`
const MARKER_BUILDING: u8 = 3;
const DROPPED_BACKPACK: u8 = 7;
const PLAYER_WAYPOINT: u8 = 20;
/// Object slots looked through per tick for the chat box, which is made when the HUD is
/// (not at all in single player).
const CHAT_SEARCH_SLOTS: usize = 8 * 1024;
/// Pause between full searches that found nothing.
const CHAT_SEARCH_PAUSE: std::time::Duration = std::time::Duration::from_secs(10);

pub struct MarkerMembers {
    hud_markers: Property,
    replicated: Property,
    /// `HUDMarkerArray`s on the manager, each holding its markers' list.
    arrays: Vec<Property>,
    array_markers: Property,
    location: Property,
    kind: Property,
    name: Property,
    restricted_to: Property,
    pub player_guid: Option<Property>,
    add_waypoint: Option<Arc<Function>>,
    chat_class: Option<Obj>,
    chat_open: Option<Arc<Function>>,
}

impl MarkerMembers {
    pub fn resolve(ue: &Unreal) -> unreal_runtime::Result<Self> {
        let class = |path: &str| ue.find_struct(path);
        let manager = class("/Script/Maine.HUDMarkerManagerComponent")?;
        let marker = class("/Script/Maine.HUDMarkerData")?;
        let arrays = [
            "RegisteredActorMarkerData",
            "RegisteredLODActorMarkerData",
            "RegisteredSpecificLocationMarkerData",
            "RegisteredPlayerWaypointMarkerData",
        ]
        .iter()
        .filter_map(|name| ue.property_of(manager, name, "Struct<HUDMarkerArray>").ok())
        .collect();
        let chat_class = class("/Script/Maine.ChatBoxWidget").ok();
        Ok(Self {
            hud_markers: ue.property_of(class("/Script/Maine.SurvivalGameState")?, "HUDMarkerManagerComponent", "Object<HUDMarkerManagerComponent>")?,
            replicated: ue.property_of(manager, "ReplicatedMarkerList", "Array<Object<HUDMarkerData>>")?,
            arrays,
            array_markers: ue.property_of(class("/Script/Maine.HUDMarkerArray")?, "MarkerArray", "Array<Object<HUDMarkerData>>")?,
            location: ue.property_of(marker, "Location", "Struct<Vector_NetQuantize>")?,
            kind: ue.property_of(marker, "MarkerType", "Enum<EHUDMarkerType>")?,
            name: ue.property_of(marker, "DisplayName", "Str")?,
            restricted_to: ue.property_of(marker, "RestrictedToPlayer", "Struct<Guid>")?,
            player_guid: class("/Script/Maine.SurvivalPlayerState").ok().and_then(|s| ue.property_of(s, "PlayerGuid", "Struct<Guid>").ok()),
            add_waypoint: class("/Script/Maine.SurvivalPlayerController").ok().and_then(|c| ue.function(c, "ServerAddWaypoint").ok()),
            chat_open: chat_class.and_then(|c| ue.function(c, "IsChatBoxOpen").ok()),
            chat_class,
        })
    }
}

/// What the game's markers say this tick.
#[derive(Default)]
pub struct MarkerRead {
    pub markers: Vec<Marker>,
    /// The local player's dropped backpack.
    pub backpack: Option<Vec3>,
}

#[derive(Default)]
pub struct Markers {
    chat: Option<Obj>,
    /// Where the search for the chat box goes on next tick.
    chat_cursor: usize,
    /// When the last full search found nothing.
    chat_pause: Option<std::time::Instant>,
}

impl Markers {
    /// The game's markers worth showing on the map.
    ///
    /// # Safety
    /// Game thread only; `game_state` must be the world's `SurvivalGameState`, and
    /// `player_state` the local player's.
    pub unsafe fn read(&self, cx: &UeCx, m: &MarkerMembers, game_state: Obj, player_state: Option<Obj>, player_name: Option<&str>) -> MarkerRead {
        let ue = &cx.ue;
        let mut out = MarkerRead::default();
        let Some(manager) = ue.get_obj(game_state, &m.hud_markers) else { return out };
        let mine: Option<[u32; 4]> = player_state.zip(m.player_guid.as_ref()).map(|(s, p)| ue.get(s, p));
        let mut all: Vec<Obj> = ue.get::<TArray<Obj>>(manager, &m.replicated).to_vec();
        for array in &m.arrays {
            all.extend(ue.get::<TArray<Obj>>(manager + array.offset, &m.array_markers).to_vec());
        }
        let mut seen = HashSet::new();
        for marker in all {
            if marker == 0 || !seen.insert(marker) || !ue.alive(marker) {
                continue;
            }
            let kind: u8 = ue.get(marker, &m.kind);
            if !matches!(kind, MARKER_BUILDING | DROPPED_BACKPACK | PLAYER_WAYPOINT) {
                continue;
            }
            let position = to_proto(ue.get::<FVector>(marker, &m.location));
            let restricted: [u32; 4] = ue.get(marker, &m.restricted_to);
            if kind == DROPPED_BACKPACK {
                if mine.is_some_and(|g| g == restricted) {
                    out.backpack = Some(position);
                }
                continue;
            }
            let name = ue.get::<FString>(marker, &m.name).to_string_lossy();
            let label = match kind {
                PLAYER_WAYPOINT if name.is_empty() => "Waypoint".to_owned(),
                PLAYER_WAYPOINT if Some(name.as_str()) == player_name => "Your waypoint".to_owned(),
                PLAYER_WAYPOINT => format!("{name}'s waypoint"),
                _ if name.is_empty() => "Marker".to_owned(),
                _ => name,
            };
            out.markers.push(Marker { id: format!("{marker:x}"), kind: MarkerKind::Locator, label: Some(label), icon: None, position });
        }
        out
    }

    /// Puts the player's waypoint at `position`, which everyone in the world sees.
    ///
    /// # Safety
    /// Game thread only; `controller` must be the local player controller.
    pub unsafe fn ping(cx: &UeCx, m: &MarkerMembers, controller: Obj, position: Vec3) -> Result<(), String> {
        let f = m.add_waypoint.as_ref().ok_or("the game's waypoints weren't found")?;
        let mut args = Args::new(f);
        args.set("Location", from_proto(position)).map_err(|e| e.to_string())?;
        args.set_bool("ShouldPointToGround", true).map_err(|e| e.to_string())?;
        cx.ue.call(controller, &mut args).map_err(|e| e.to_string())
    }

    /// Whether the chat box is open, so typing doesn't trigger hotkeys.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn chat_open(&mut self, cx: &UeCx, m: &MarkerMembers) -> bool {
        let (Some(class), Some(f)) = (m.chat_class, m.chat_open.as_ref()) else { return false };
        if self.chat.is_none_or(|c| !cx.ue.alive(c)) {
            if self.chat_pause.is_some_and(|t| t.elapsed() < CHAT_SEARCH_PAUSE) {
                return false;
            }
            let (found, next) = cx.ue.find_instance_from(class, self.chat_cursor, CHAT_SEARCH_SLOTS);
            self.chat = found;
            self.chat_cursor = next;
            self.chat_pause = (found.is_none() && next == 0).then(std::time::Instant::now);
        }
        self.chat.is_some_and(|c| cx.call_get::<bool>(c, f).unwrap_or(false))
    }
}
