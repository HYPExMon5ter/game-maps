//! Stranded Deep (Beam Team Games, Unity Mono). A world is 49 zones of 250 m placed from
//! its seed: islands, reefs, open sea, three boss arenas and the aircraft carrier. Every
//! zone's terrain is in memory from the start, so the map comes from the running game
//! (see [`StrandedDeep::read_world_map`]); what's on an island only exists once it's been
//! visited, so that's remembered as zones load (`scan`).
//!
//! Members used (all looked up by name, see `docs/games/stranded-deep.md`):
//! - `Beam.PlayerRegistry`: `_LocalPlayer`, `_AllPlayers` (local co-op and online players),
//!   `_LocalPeer` and `_ServerPeer`, whose `_onlineName` and `_worldSeed` are shared.
//! - `Beam.Player`: `_movement` (`Beam.Movement`: `_vehicle`, `_fallStartLevel`,
//!   `ResetChecks`), `_statistics` (`_isKnockedOut`).
//! - `Beam.Game._mode` / `_state`, `StrandedWorld.LOCAL_WORLD_SEED`, the save slot in
//!   `Options._generalSettings.LastSaveSlotUsed`.
//! - `StrandedWorld._zones`: each `Zone`'s terrain, name, biome, `HasVisited`, `Loaded` and
//!   `SaveContainer`. `StrandedWorld.GetHeight` is the ground anywhere.
//! - `Photon.Bolt.Matchmaking.BoltMatchmaking.CurrentSession` and its token's `SessionId`,
//!   the same for host and client.
//! - `Piscus_Creature` and `Beam.LandAnimal`: sharks, whales, bosses, crabs, boars, snakes.
//! - `Beam.GameTime`, `Beam.GameCalendar`, `AtmosphereStorm`: the time, day and weather.

mod objects;
mod scan;
mod terrain;

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use game_maps_protocol::{Capabilities, Creature, CreatureKind, Environment, GameWorld, LocalPlayer, MultiplayerSession, PathInfo, RemotePlayer, TeleportTarget, Vec3, WorldKind, WorldLayer, WorldPin, WorldSnapshot, ZonePins, WORLD_MAP_NOT_READY};
use unity_runtime::{read_object_array, read_string, read_value_array, Class, Field, ListLayout, ManagedObject, Method, Runtime, Vector3, WeakRef};

use super::{to_proto, to_unity, Cx, Provider};
use crate::hook::{target, HookTarget, Shape};
use crate::worldmap::{Terrain, WorldMapCapture};
use scan::{LoadedZone, Scanner, TransformMembers};
use terrain::ZoneTerrain;

const GAME_ID: &str = "stranded-deep";

/// `GameTime` ticks every frame in a world; the menus have uGUI's `EventSystem`.
const FRAME_HOOKS: &[HookTarget] = &[target("Beam", "GameTime", "Update", Shape::Instance0)];

const REMOTE_REFRESH: Duration = Duration::from_millis(200);
const CREATURE_REFRESH: Duration = Duration::from_millis(500);
/// How often the world, session, time and what's been visited are read.
const WORLD_REFRESH: Duration = Duration::from_secs(2);
const INPUT_REFRESH: Duration = Duration::from_millis(250);
const CREATURE_RADIUS: f32 = 200.0;
/// Creature names kept before starting over; creatures come and go all session.
const MAX_CREATURE_NAMES: usize = 4096;
/// A boss arena reaches this far from its zone's middle, as far as its fight does.
const ARENA_RADIUS: f32 = 250.0;
/// How often the scene is searched for a boss that isn't found yet (about 2 ms).
const BOSS_SEARCH: Duration = Duration::from_secs(3);
const MAX_ENTITIES: usize = 256;
/// The world's zones, as `StrandedWorld.WORLD_ZONES_SQUARED` has it.
const WORLD_ZONES: usize = 49;
/// Height added above the ground for teleports.
const GROUND_CLEARANCE: f32 = 1.2;
/// Where the player arrives over water: at the surface, swimming.
const SWIM_HEIGHT: f32 = 0.2;
/// A visited zone's island is revealed this far around its center in the fog of war.
const VISITED_REVEAL: f32 = 175.0;
/// The app's fog of war (see `lib/fog.ts`): 8 m cells over the map's square.
const FOG_CELL: f32 = 8.0;

/// `Beam.GameMode` (single player is 0, local co-op 1 to 3).
const MODE_SERVER: i32 = 4;
const MODE_CLIENT: i32 = 5;
/// `Beam.GameState`: in a world while starting a new game or loading one.
const STATE_NEW_GAME: i32 = 2;
const STATE_LOAD_GAME: i32 = 3;

/// `Zone.BiomeType`.
const BIOME_ISLAND: i32 = 4;
const BIOME_ISLAND_SMALL: i32 = 5;
const BIOME_ISLAND_ROCK: i32 = 6;
const BIOME_MISSION: i32 = 7;
const BIOME_CARRIER: i32 = 8;

/// The boss arenas are the game's own maps, the same in every world (`MISSION_EEL`,
/// `MISSION_SHARK` and `MISSION_SQUID`, checked in the game).
fn mission_boss(zone_id: &str) -> Option<&'static str> {
    match zone_id {
        "INTERNAL_MISSION_0" => Some("The Great Abaia"),
        "INTERNAL_MISSION_1" => Some("Megalodon"),
        "INTERNAL_MISSION_2" => Some("Lusca the Great"),
        _ => None,
    }
}

const CARRIER_NAME: &str = "Aircraft carrier";

struct PlayerMembers {
    local: Field,
    all: Field,
    local_peer: Option<Field>,
    server_peer: Option<Field>,
    movement: Field,
    statistics: Option<Field>,
    peer: Option<Field>,
    knocked_out: Option<Field>,
    vehicle: Option<Field>,
    fall_start: Option<Field>,
    reset_checks: Option<Method>,
    /// `Player.CurrentMissionGoal`: set while in a boss fight.
    mission: Option<Field>,
}

struct PeerFields {
    online_name: Field,
    world_seed: Field,
    /// `Beam.EGameWildlifeMode`, the world's setting.
    wildlife: Option<Field>,
}

/// `EGameWildlifeMode.Passive`: animals never attack.
const WILDLIFE_PASSIVE: i32 = 1;

struct GameMembers {
    mode: Field,
    state: Field,
    local_seed: Option<Field>,
    /// `Options._generalSettings` and its `LastSaveSlotUsed`.
    slot: Option<(Field, Field)>,
}

struct SessionMembers {
    current: Method,
    token: Method,
    id: Field,
}

struct WorldMembers {
    class: Class,
    zones: Field,
    height: Option<Method>,
}

struct ZoneFields {
    terrain: Field,
    name: Field,
    id: Field,
    biome: Field,
    visited: Field,
    loaded: Option<Field>,
    container: Option<Field>,
}

struct TerrainMembers {
    data: Method,
    size: Method,
    resolution: Method,
    heights: Method,
}

struct CreatureMembers {
    sea: Class,
    /// `PiscusInspectionManager` and its `piscuses`: every shark, whale and boss there is.
    sea_list: (Class, Field),
    /// `LandAnimalSpawner` and its `_activeCreature`.
    spawner: Option<(Class, Field)>,
    can_attack: Option<Field>,
    whale: Option<Class>,
    /// Boss classes and their names.
    bosses: Vec<(Class, &'static str)>,
    /// Land animals that leave players alone.
    harmless: Vec<Class>,
}

struct EnvMembers {
    time: Class,
    hours: Field,
    minutes: Field,
    calendar: Option<(Class, Field)>,
    /// `AtmosphereStorm`, its `_currentWeatherEvent`, and that's `_hasStarted`.
    storm: Option<(Class, Field, Field)>,
}

/// One zone's details that don't change in a world.
#[derive(Clone)]
struct ZoneInfo {
    index: usize,
    id: String,
    name_key: String,
    biome: i32,
    center: Vec3,
}

pub struct StrandedDeep {
    player: PlayerMembers,
    peer: Option<PeerFields>,
    game: GameMembers,
    session_members: Option<SessionMembers>,
    world_members: WorldMembers,
    zone_fields: ZoneFields,
    terrain_members: Option<TerrainMembers>,
    creature_members: Option<CreatureMembers>,
    env: Option<EnvMembers>,
    /// `Beam.Language.Localization.GetLanguageHandler`.
    localization: Option<Method>,
    /// `DeveloperConsolePresenter._Instance` and `get_IsOpen`.
    console: Option<(Field, Method)>,
    list_layout: Option<ListLayout>,
    scanner: Option<Scanner>,
    notes: Vec<String>,

    /// Objects found once and kept while they live (see [`StrandedDeep::singleton`]).
    world_object: Option<WeakRef>,
    time_object: Option<WeakRef>,
    calendar_object: Option<WeakRef>,
    storm_object: Option<WeakRef>,
    names: HashMap<String, String>,
    spawned: bool,
    game_world: Option<GameWorld>,
    session: Option<MultiplayerSession>,
    environment: Option<Environment>,
    others: Vec<RemotePlayer>,
    nearby: Vec<Creature>,
    knocked_out: bool,
    typing: bool,
    zones: Vec<ZoneInfo>,
    visited: Vec<usize>,
    /// Where each island's pin is: the land nearest the zone's middle.
    pin_spots: HashMap<usize, Vec3>,
    last_world_refresh: Option<Instant>,
    last_remote_refresh: Option<Instant>,
    last_creature_refresh: Option<Instant>,
    last_input_refresh: Option<Instant>,
    last_scan_note: Option<Instant>,
    logged_first_scan: bool,
    /// `PiscusInspectionManager`, found once.
    sea_manager: Option<WeakRef>,
    /// The boss of the arena the player is in, once found.
    boss: Option<WeakRef>,
    last_boss_search: Option<Instant>,
    /// Creature names by object.
    creature_names: HashMap<usize, String>,
    /// Time spent on the first creature reads, for the log.
    creature_time: Duration,
    creature_reads: u32,
}

// Raw object pointers are only dereferenced on the main thread.
unsafe impl Send for StrandedDeep {}

fn static_field(rt: &Runtime, class: Class, name: &str) -> Option<Field> {
    rt.field(class, name).ok().filter(|f| f.is_static)
}

impl StrandedDeep {
    pub fn detect(rt: &Runtime) -> Option<Self> {
        // `StrandedWorld` and `Beam.PlayerRegistry` together are specific enough to this game.
        let world = rt.class("", "StrandedWorld").ok()?;
        let registry = rt.class("Beam", "PlayerRegistry").ok()?;
        let player = rt.class("Beam", "Player").ok()?;
        let movement = rt.class("Beam", "Movement").ok()?;
        let zone = rt.class("", "Zone").ok()?;
        let game = rt.class("Beam", "Game").ok()?;

        let statistics = rt.class("Beam", "Statistics").ok();
        let players = PlayerMembers {
            local: static_field(rt, registry, "_LocalPlayer")?,
            all: static_field(rt, registry, "_AllPlayers")?,
            local_peer: static_field(rt, registry, "_LocalPeer"),
            server_peer: static_field(rt, registry, "_ServerPeer"),
            movement: rt.field(player, "_movement").ok()?,
            statistics: rt.field(player, "_statistics").ok(),
            peer: rt.field(player, "<Peer>k__BackingField").ok(),
            knocked_out: statistics.and_then(|s| rt.field_of(s, "_isKnockedOut", "Boolean").ok()),
            vehicle: rt.field(movement, "_vehicle").ok(),
            fall_start: rt.field_of(movement, "_fallStartLevel", "Single").ok(),
            reset_checks: rt.method(movement, "ResetChecks", 0).ok(),
            mission: rt.field(player, "<CurrentMissionGoal>k__BackingField").ok(),
        };
        let peer = rt.class("Funlabs", "Peer").ok().and_then(|c| {
            Some(PeerFields { online_name: rt.field_of(c, "_onlineName", "String").ok()?, world_seed: rt.field_of(c, "_worldSeed", "Int32").ok()?, wildlife: rt.field_of(c, "_wildlife", "EGameWildlifeMode").ok() })
        });
        let game_members = GameMembers {
            mode: static_field(rt, game, "_mode")?,
            state: static_field(rt, game, "_state")?,
            local_seed: static_field(rt, world, "LOCAL_WORLD_SEED"),
            slot: (|| {
                let options = rt.class("", "Options").ok()?;
                let general = rt.class("", "GeneralSettings").ok()?;
                Some((static_field(rt, options, "_generalSettings")?, rt.field_of(general, "LastSaveSlotUsed", "Int32").ok()?))
            })(),
        };
        let session_members = (|| {
            let matchmaking = rt.class("Photon.Bolt.Matchmaking", "BoltMatchmaking").ok()?;
            let extensions = rt.class("Funlabs", "MPExtensions").ok()?;
            let token = rt.class("Funlabs", "MultiplayerTokens/Session").ok()?;
            Some(SessionMembers { current: rt.method(matchmaking, "get_CurrentSession", 0).ok()?, token: rt.method(extensions, "GetToken", 1).ok()?, id: rt.field_of(token, "SessionId", "String").ok()? })
        })();
        let world_members = WorldMembers { class: world, zones: rt.field(world, "_zones").ok()?, height: rt.method_with_params(world, "GetHeight", &["UnityEngine.Vector3"]).ok() };
        let zone_fields = ZoneFields {
            terrain: rt.field(zone, "_terrain").ok()?,
            name: rt.field_of(zone, "ZoneName", "String").ok()?,
            id: rt.field_of(zone, "Id", "String").ok()?,
            biome: rt.field(zone, "_biome").ok()?,
            visited: rt.field_of(zone, "<HasVisited>k__BackingField", "Boolean").ok()?,
            loaded: rt.field_of(zone, "<Loaded>k__BackingField", "Boolean").ok(),
            container: rt.field(zone, "SaveContainer").ok(),
        };
        let terrain_members = (|| {
            let terrain = rt.class("UnityEngine", "Terrain").ok()?;
            let data = rt.class("UnityEngine", "TerrainData").ok()?;
            Some(TerrainMembers {
                data: rt.method(terrain, "get_terrainData", 0).ok()?,
                size: rt.method(data, "get_size", 0).ok()?,
                resolution: rt.method(data, "get_heightmapResolution", 0).ok()?,
                heights: rt.method_with_params(data, "GetHeights", &["System.Int32", "System.Int32", "System.Int32", "System.Int32"]).ok()?,
            })
        })();
        let creature_members = rt.class("", "Piscus_Creature").ok().and_then(|sea| {
            let class = |ns: &str, name: &str| rt.class(ns, name).ok();
            let manager = class("", "PiscusInspectionManager")?;
            Some(CreatureMembers {
                sea,
                sea_list: (manager, rt.field(manager, "piscuses").ok()?),
                spawner: class("", "LandAnimalSpawner").and_then(|c| Some((c, rt.field(c, "_activeCreature").ok()?))),
                can_attack: rt.field_of(sea, "_canAttack", "Boolean").ok(),
                whale: class("", "Whale_Creature"),
                bosses: [("Boss_Shark", "Megalodon"), ("Boss_Squid", "Lusca the Great"), ("Boss_Eel", "The Great Abaia")].into_iter().filter_map(|(c, name)| Some((class("", c)?, name))).collect(),
                harmless: ["LandCrab"].into_iter().filter_map(|c| class("Beam", c)).collect(),
            })
        });
        let env = (|| {
            let time = rt.class("Beam", "GameTime").ok()?;
            Some(EnvMembers {
                time,
                hours: rt.field_of(time, "_hours", "Int32").ok()?,
                minutes: rt.field_of(time, "_minutes", "Int32").ok()?,
                calendar: rt.class("Beam", "GameCalendar").ok().and_then(|c| Some((c, rt.field_of(c, "_daysElapsed", "Int32").ok()?))),
                storm: (|| {
                    let storm = rt.class("", "AtmosphereStorm").ok()?;
                    let event = rt.class("", "WeatherEvent").ok()?;
                    Some((storm, rt.field(storm, "_currentWeatherEvent").ok()?, rt.field_of(event, "_hasStarted", "Boolean").ok()?))
                })(),
            })
        })();
        let localization = rt.class("Beam.Language", "Localization").ok().and_then(|c| rt.method(c, "GetLanguageHandler", 0).ok());
        let console = rt.class("Beam.Developer.UI", "DeveloperConsolePresenter").ok().and_then(|c| Some((static_field(rt, c, "_Instance")?, rt.method(c, "get_IsOpen", 0).ok()?)));
        let spawner_class = creature_members.as_ref().and_then(|c| c.spawner.map(|(class, _)| class));
        let scanner = TransformMembers::resolve(rt).filter(|_| zone_fields.container.is_some() && zone_fields.loaded.is_some()).map(|t| Scanner::new(t, spawner_class));

        let notes = vec![format!(
            "Stranded Deep: peers={} session={} slot={} height={} terrain={} creatures={} land_animals={} bosses={} environment={} weather={} names={} console={} scanner={} knocked_out={} vehicle={} reset={} mission={} wildlife={}",
            peer.is_some(),
            session_members.is_some(),
            game_members.slot.is_some(),
            world_members.height.is_some(),
            terrain_members.is_some(),
            creature_members.is_some(),
            creature_members.as_ref().is_some_and(|c| c.spawner.is_some()),
            creature_members.as_ref().map_or(0, |c| c.bosses.len()),
            env.is_some(),
            env.as_ref().is_some_and(|e| e.storm.is_some()),
            localization.is_some(),
            console.is_some(),
            scanner.is_some(),
            players.knocked_out.is_some(),
            players.vehicle.is_some(),
            players.reset_checks.is_some(),
            players.mission.is_some(),
            peer.as_ref().is_some_and(|p| p.wildlife.is_some()),
        )];

        Some(Self {
            player: players,
            peer,
            game: game_members,
            session_members,
            world_members,
            zone_fields,
            terrain_members,
            creature_members,
            env,
            localization,
            console,
            list_layout: None,
            scanner,
            notes,
            world_object: None,
            time_object: None,
            calendar_object: None,
            storm_object: None,
            names: HashMap::new(),
            spawned: false,
            game_world: None,
            session: None,
            environment: None,
            others: Vec::new(),
            nearby: Vec::new(),
            knocked_out: false,
            typing: false,
            zones: Vec::new(),
            visited: Vec::new(),
            pin_spots: HashMap::new(),
            last_world_refresh: None,
            last_remote_refresh: None,
            last_creature_refresh: None,
            last_input_refresh: None,
            last_scan_note: None,
            logged_first_scan: false,
            sea_manager: None,
            boss: None,
            last_boss_search: None,
            creature_names: HashMap::new(),
            creature_time: Duration::ZERO,
            creature_reads: 0,
        })
    }

    /// Whether the game is in a world (not the menus, the intro or the map editor).
    fn in_world(&self, cx: &Cx) -> bool {
        let state: i32 = unsafe { cx.rt.static_value(self.game.state) };
        state == STATE_NEW_GAME || state == STATE_LOAD_GAME
    }

    unsafe fn local_player(&self, cx: &Cx) -> *mut ManagedObject {
        if !self.in_world(cx) {
            return std::ptr::null_mut();
        }
        let player: *mut ManagedObject = cx.rt.static_value(self.player.local);
        if cx.unity.is_alive(player) {
            player
        } else {
            std::ptr::null_mut()
        }
    }

    /// The game's one `class` object: the one found before if it's still alive, else the
    /// first in the scene. Searching the scene takes milliseconds, so what's found is kept,
    /// as a weak reference: a raw pointer kept across frames can outlive the object, and
    /// reading it then crashes the game.
    unsafe fn singleton(cx: &Cx, cached: &mut Option<WeakRef>, class: Class) -> *mut ManagedObject {
        let current = cached.as_ref().map_or(std::ptr::null_mut(), |w| w.get());
        if cx.unity.is_alive(current) {
            return current;
        }
        let found = cx.unity.find_objects(cx.rt, class, false).ok().and_then(|o| o.first().copied()).unwrap_or(std::ptr::null_mut());
        *cached = cx.rt.weak_ref(found);
        found
    }

    unsafe fn world_object(&mut self, cx: &Cx) -> Option<*mut ManagedObject> {
        let world = Self::singleton(cx, &mut self.world_object, self.world_members.class);
        cx.unity.is_alive(world).then_some(world)
    }

    /// Elements of a `List<T>` of reference types.
    unsafe fn list(&mut self, cx: &Cx, list: *mut ManagedObject, max: usize) -> Vec<*mut ManagedObject> {
        if list.is_null() {
            return Vec::new();
        }
        let layout = match self.list_layout {
            Some(l) => l,
            None => match ListLayout::of(cx.rt, list) {
                Ok(l) => *self.list_layout.insert(l),
                Err(e) => {
                    self.notes.push(format!("unsupported list type: {e}"));
                    return Vec::new();
                }
            },
        };
        layout.elements(list, max)
    }

    /// The zones of the loaded world, in the game's order.
    unsafe fn zone_objects(&mut self, cx: &Cx) -> Vec<*mut ManagedObject> {
        let Some(world) = self.world_object(cx) else { return Vec::new() };
        let array: *mut ManagedObject = self.world_members.zones.get(world);
        read_object_array(array, WORLD_ZONES).into_iter().filter(|z| cx.unity.is_alive(*z)).collect()
    }

    /// What doesn't change about each zone in this world, read once.
    unsafe fn ensure_zones(&mut self, cx: &Cx) -> bool {
        if self.zones.len() == WORLD_ZONES {
            return true;
        }
        let objects = self.zone_objects(cx);
        if objects.len() != WORLD_ZONES {
            return false;
        }
        let f = &self.zone_fields;
        let mut zones = Vec::with_capacity(WORLD_ZONES);
        for (index, zone) in objects.into_iter().enumerate() {
            let Ok(transform) = cx.unity.component_transform(cx.rt, zone) else { return false };
            let Ok(center) = cx.unity.position(cx.rt, transform) else { return false };
            zones.push(ZoneInfo {
                index,
                id: read_string(f.id.get(zone)).unwrap_or_default(),
                name_key: read_string(f.name.get(zone)).unwrap_or_default(),
                biome: f.biome.get(zone),
                center: to_proto(center),
            });
        }
        // Zones sit at the origin until the world is laid out.
        if zones.iter().filter(|z| z.center == Vec3::ZERO).count() > 1 {
            return false;
        }
        self.zones = zones;
        true
    }

    /// The text for one of the game's terms, like `MAP_NAME_STEM_4`.
    unsafe fn localized(&mut self, cx: &Cx, term: &str) -> String {
        if let Some(text) = self.names.get(term) {
            return text.clone();
        }
        let text = (|| {
            // A plain object, not a Unity one: null is the only check.
            let handler = cx.rt.invoke(self.localization.as_ref()?, std::ptr::null_mut(), &mut []).ok().filter(|h| !h.is_null())?;
            let class = cx.rt.class_of(handler)?;
            let localize = cx.rt.method(class, "Localize", 2).ok()?;
            let string = cx.rt.class("System", "String").ok()?;
            let none = cx.rt.new_array(string, 0).ok()?;
            let mut args = [cx.rt.new_string(term) as *mut c_void, none.object as *mut c_void];
            read_string(cx.rt.invoke(&localize, handler, &mut args).ok()?)
        })()
        .filter(|t| !t.trim().is_empty() && t != term);
        let text = text.unwrap_or_else(|| term.to_owned());
        self.names.insert(term.to_owned(), text.clone());
        text
    }

    /// An island's name as the game shows it: its terms, each localized.
    unsafe fn island_name(&mut self, cx: &Cx, key: &str) -> Option<String> {
        if key.is_empty() || key.contains('?') {
            return None;
        }
        let words: Vec<String> = key.split_whitespace().map(|term| if term.starts_with("MAP_NAME_") { self.localized(cx, term) } else { term.to_owned() }).collect();
        // Untranslated terms mean the game's text isn't loaded: better no name than codes.
        (!words.iter().any(|w| w.starts_with("MAP_NAME_"))).then(|| words.join(" "))
    }

    unsafe fn peer_name(&self, peer: *mut ManagedObject) -> Option<String> {
        if peer.is_null() {
            return None;
        }
        read_string(self.peer.as_ref()?.online_name.get(peer)).filter(|n| !n.trim().is_empty())
    }

    unsafe fn player_name(&self, cx: &Cx, player: *mut ManagedObject) -> Option<String> {
        let peer: *mut ManagedObject = self.player.peer?.get(player);
        if !cx.unity.is_alive(peer) {
            return None;
        }
        self.peer_name(peer)
    }

    /// The world that's loaded, and the session that goes with it.
    unsafe fn read_world(&self, cx: &Cx) -> Option<(GameWorld, Option<MultiplayerSession>)> {
        let mode: i32 = cx.rt.static_value(self.game.mode);
        let server_peer: *mut ManagedObject = self.player.server_peer.map_or(std::ptr::null_mut(), |f| cx.rt.static_value(f));
        let seed = if mode == MODE_SERVER || mode == MODE_CLIENT {
            let peer = self.peer.as_ref()?;
            if !cx.unity.is_alive(server_peer) {
                return None;
            }
            peer.world_seed.get::<i32>(server_peer)
        } else {
            self.game.local_seed.map(|f| cx.rt.static_value::<i32>(f))?
        };
        let slot = self.game.slot.and_then(|(settings, slot)| {
            let general: *mut ManagedObject = cx.rt.static_value(settings);
            (!general.is_null()).then(|| slot.get::<i32>(general))
        });
        let local_peer: *mut ManagedObject = self.player.local_peer.map_or(std::ptr::null_mut(), |f| cx.rt.static_value(f));
        let player_name = if cx.unity.is_alive(local_peer) { self.peer_name(local_peer) } else { None };
        let (kind, id) = match mode {
            MODE_CLIENT => {
                // Someone else's save: their name and the seed tell it from others.
                let host = self.peer_name(server_peer).unwrap_or_default();
                (WorldKind::Joined, format!("joined-{seed:x}-{:x}", fnv1a(host.as_bytes())))
            }
            _ => (if mode == MODE_SERVER { WorldKind::Hosted } else { WorldKind::SinglePlayer }, format!("slot{}-{seed:x}", slot.unwrap_or(0))),
        };
        let session = (mode == MODE_SERVER || mode == MODE_CLIENT).then(|| {
            // The session's id is only known to players in it; the seed and host are a fallback.
            let id = self.session_id(cx).unwrap_or_else(|| format!("{seed:x}-{:x}", fnv1a(self.peer_name(server_peer).unwrap_or_default().as_bytes())));
            MultiplayerSession { id, player_name: player_name.clone() }
        });
        Some((GameWorld { id, name: None, kind }, session))
    }

    /// The online session's id, which host and client share.
    unsafe fn session_id(&self, cx: &Cx) -> Option<String> {
        let s = self.session_members.as_ref()?;
        let session = cx.rt.invoke(&s.current, std::ptr::null_mut(), &mut []).ok().filter(|s| !s.is_null())?;
        let mut args = [session as *mut c_void];
        let token = cx.rt.invoke(&s.token, std::ptr::null_mut(), &mut args).ok().filter(|t| !t.is_null())?;
        read_string(s.id.get(token)).filter(|id| !id.is_empty())
    }

    unsafe fn read_environment(&mut self, cx: &Cx) -> Option<Environment> {
        let e = self.env.as_ref()?;
        let (time_class, hours, minutes) = (e.time, e.hours, e.minutes);
        let calendar = e.calendar;
        let storm = e.storm;
        let time = Self::singleton(cx, &mut self.time_object, time_class);
        if !cx.unity.is_alive(time) {
            return None;
        }
        let (hour, minute): (i32, i32) = (hours.get(time), minutes.get(time));
        let day = calendar.and_then(|(class, elapsed)| {
            let calendar = Self::singleton(cx, &mut self.calendar_object, class);
            // Days survived: the app shows the first day as Day 1.
            cx.unity.is_alive(calendar).then(|| elapsed.get::<i32>(calendar))
        });
        let weather = storm.map(|(class, current, started)| {
            let storm = Self::singleton(cx, &mut self.storm_object, class);
            let event: *mut ManagedObject = if cx.unity.is_alive(storm) { current.get(storm) } else { std::ptr::null_mut() };
            if cx.unity.is_alive(event) && started.get::<bool>(event) {
                cx.unity.object_name(cx.rt, event).map(|n| weather_name(&n)).unwrap_or_else(|| "Stormy".into())
            } else {
                "Clear".into()
            }
        });
        Some(Environment { day: day.and_then(|d| u32::try_from(d).ok()), hour: hour.clamp(0, 23) as u8, minute: minute.clamp(0, 59) as u8, season: None, weather, wind: None })
    }

    unsafe fn refresh_remote(&mut self, cx: &Cx, player: *mut ManagedObject) {
        let list: *mut ManagedObject = cx.rt.static_value(self.player.all);
        let local_name = self.player_name(cx, player);
        let mut others = Vec::new();
        for (i, other) in self.list(cx, list, MAX_ENTITIES).into_iter().enumerate() {
            if other == player || !cx.unity.is_alive(other) {
                continue;
            }
            let Ok(transform) = cx.unity.component_transform(cx.rt, other) else { continue };
            let Ok(position) = cx.unity.position(cx.rt, transform) else { continue };
            let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
            // Split-screen players share the machine's peer, so they have no name of their own.
            let name = self.player_name(cx, other).filter(|n| Some(n) != local_name.as_ref()).unwrap_or_else(|| format!("Player {}", i + 1));
            others.push(RemotePlayer { id: format!("player:{name}"), name, color: None, position: to_proto(position), forward });
        }
        if !self.logged_first_scan {
            self.logged_first_scan = true;
            self.notes.push(format!("Players seen on first scan: {}", others.len()));
        }
        self.others = others;
    }

    /// Whether the world's wildlife is set to never attack.
    unsafe fn wildlife_passive(&self, cx: &Cx) -> bool {
        let Some(field) = self.peer.as_ref().and_then(|p| p.wildlife) else { return false };
        // The host's settings are the world's; in single player that's the local peer.
        [self.player.server_peer, self.player.local_peer].into_iter().flatten().map(|f| cx.rt.static_value::<*mut ManagedObject>(f)).find(|p| cx.unity.is_alive(*p)).is_some_and(|p| field.get::<i32>(p) == WILDLIFE_PASSIVE)
    }

    /// Sharks and whales from the game's own list, the animal each spawner the scanner found
    /// has out, and the boss of the arena the player is in. Field reads but for finding the
    /// boss: searching the scene for every creature cost 4 ms.
    unsafe fn creature_objects(&mut self, cx: &Cx, around: Vector3) -> Vec<*mut ManagedObject> {
        let Some(m) = &self.creature_members else { return Vec::new() };
        let ((manager_class, piscuses), spawner, sea) = (m.sea_list, m.spawner, m.sea);
        let boss_classes: Vec<Class> = m.bosses.iter().map(|(c, _)| *c).collect();
        let mut out = Vec::new();
        // Bosses aren't on the game's list: they're looked for in the scene, only in an arena.
        let in_arena = self.zones.iter().any(|z| z.biome == BIOME_MISSION && (z.center.x - around.x).hypot(z.center.z - around.z) < ARENA_RADIUS);
        let boss = self.boss.as_ref().map_or(std::ptr::null_mut(), |b| b.get());
        if !in_arena {
            self.boss = None;
        } else if cx.unity.is_alive(boss) {
            out.push(boss);
        } else if self.last_boss_search.is_none_or(|t| t.elapsed() >= BOSS_SEARCH) {
            self.last_boss_search = Some(Instant::now());
            let found = cx.unity.find_objects(cx.rt, sea, false).unwrap_or_default().into_iter().find(|c| cx.rt.class_of(*c).is_some_and(|own| boss_classes.contains(&own)));
            self.boss = found.and_then(|b| cx.rt.weak_ref(b));
            out.extend(found);
        }
        let manager = Self::singleton(cx, &mut self.sea_manager, manager_class);
        if cx.unity.is_alive(manager) {
            let list: *mut ManagedObject = piscuses.get(manager);
            out.extend(self.list(cx, list, MAX_ENTITIES));
        }
        if let (Some((_, active)), Some(scanner)) = (spawner, &self.scanner) {
            out.extend(scanner.spawners().map(|s| s.get()).filter(|s| cx.unity.is_alive(*s)).map(|s| active.get::<*mut ManagedObject>(s)));
        }
        out.retain(|c| cx.unity.is_alive(*c));
        out
    }

    unsafe fn refresh_creatures(&mut self, cx: &Cx, around: Vector3) {
        let passive = self.wildlife_passive(cx);
        let objects = self.creature_objects(cx, around);
        let Some(m) = &self.creature_members else { return };
        if self.creature_names.len() > MAX_CREATURE_NAMES {
            self.creature_names.clear();
        }
        let mut found = Vec::new();
        for c in objects {
            let Ok(transform) = cx.unity.component_transform(cx.rt, c) else { continue };
            let Ok(p) = cx.unity.position(cx.rt, transform) else { continue };
            if (p.x - around.x).hypot(p.z - around.z) > CREATURE_RADIUS {
                continue;
            }
            let Some(own) = cx.rt.class_of(c) else { continue };
            let (kind, name) = if let Some((_, boss)) = m.bosses.iter().find(|(b, _)| *b == own) {
                (CreatureKind::Hostile, (*boss).to_owned())
            } else {
                // Names allocate, so each creature's is read once.
                let name = self.creature_names.entry(c as usize).or_insert_with(|| objects::creature_name(&cx.unity.object_name(cx.rt, c).unwrap_or_default())).clone();
                let kind = if cx.rt.is_assignable(m.sea, own) {
                    let gentle = passive || m.whale == Some(own) || m.can_attack.is_some_and(|f| !f.get::<bool>(c));
                    if gentle { CreatureKind::Animal } else { CreatureKind::Hostile }
                } else if passive || m.harmless.contains(&own) {
                    CreatureKind::Animal
                } else {
                    CreatureKind::Hostile
                };
                (kind, name)
            };
            found.push(Creature { id: format!("{:x}", c as usize), kind, name, position: to_proto(p) });
        }
        self.nearby = found;
    }

    /// Which zones the game counts as visited.
    unsafe fn read_visited(&mut self, cx: &Cx) -> Vec<usize> {
        let field = self.zone_fields.visited;
        self.zone_objects(cx).into_iter().enumerate().filter(|(_, z)| field.get::<bool>(*z)).map(|(i, _)| i).collect()
    }

    /// The zones the game has loaded, for the scanner.
    unsafe fn loaded_zones(&mut self, cx: &Cx) -> Vec<LoadedZone> {
        let (Some(loaded), Some(container)) = (self.zone_fields.loaded, self.zone_fields.container) else { return Vec::new() };
        let centers: Vec<Vec3> = self.zones.iter().map(|z| z.center).collect();
        self.zone_objects(cx)
            .into_iter()
            .enumerate()
            .filter(|(_, z)| loaded.get::<bool>(*z))
            .filter_map(|(index, z)| {
                let container: *mut ManagedObject = container.get(z);
                cx.unity.is_alive(container).then(|| LoadedZone { index, container, center: centers.get(index).copied().unwrap_or(Vec3::ZERO) })
            })
            .collect()
    }

    /// Each zone's terrain.
    unsafe fn read_terrains(&mut self, cx: &Cx) -> Result<Vec<(usize, ZoneTerrain)>, String> {
        let t = self.terrain_members.as_ref().ok_or("the terrain is unavailable in this game version")?;
        let (data_m, size_m, resolution_m, heights_m) = (t.data.clone(), t.size.clone(), t.resolution.clone(), t.heights.clone());
        let terrain_f = self.zone_fields.terrain;
        let biomes: Vec<i32> = self.zones.iter().map(|z| z.biome).collect();
        let mut out = Vec::with_capacity(WORLD_ZONES);
        for (index, zone) in self.zone_objects(cx).into_iter().enumerate() {
            let terrain: *mut ManagedObject = terrain_f.get(zone);
            if !cx.unity.is_alive(terrain) {
                continue;
            }
            let data = cx.rt.invoke(&data_m, terrain, &mut []).map_err(|e| e.to_string())?;
            if !cx.unity.is_alive(data) {
                continue;
            }
            let size: Vector3 = cx.rt.unbox(cx.rt.invoke(&size_m, data, &mut []).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let resolution: i32 = cx.rt.unbox(cx.rt.invoke(&resolution_m, data, &mut []).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            if !(2..=4097).contains(&resolution) {
                return Err(format!("unexpected terrain resolution {resolution}"));
            }
            let transform = cx.unity.component_transform(cx.rt, terrain).map_err(|e| e.to_string())?;
            let origin = cx.unity.position(cx.rt, transform).map_err(|e| e.to_string())?;
            let (mut x, mut y, mut w, mut h) = (0i32, 0i32, resolution, resolution);
            let mut args = [&mut x as *mut i32 as *mut c_void, &mut y as *mut i32 as *mut c_void, &mut w as *mut i32 as *mut c_void, &mut h as *mut i32 as *mut c_void];
            let array = cx.rt.invoke(&heights_m, data, &mut args).map_err(|e| e.to_string())?;
            let n = resolution as usize;
            let heights = read_value_array::<f32>(array, n * n);
            if heights.len() != n * n {
                return Err(format!("zone {index} has {} heights, not {}", heights.len(), n * n));
            }
            out.push((
                index,
                ZoneTerrain { origin_x: origin.x, origin_z: origin.z, size_x: size.x, size_z: size.z, base_y: origin.y, scale_y: size.y, resolution: n, heights, rocky: biomes.get(index) == Some(&BIOME_ISLAND_ROCK) },
            ));
        }
        if out.len() < WORLD_ZONES {
            return Err(WORLD_MAP_NOT_READY.into());
        }
        Ok(out)
    }

    /// The world's places: islands, boss arenas and the carrier.
    unsafe fn read_pins(&mut self, cx: &Cx) -> Vec<WorldPin> {
        let mut pins = Vec::new();
        for zone in self.zones.clone() {
            let at = self.pin_spots.get(&zone.index).copied().unwrap_or(zone.center);
            let (category, name) = match zone.biome {
                BIOME_ISLAND | BIOME_ISLAND_SMALL | BIOME_ISLAND_ROCK => ("island", self.island_name(cx, &zone.name_key).unwrap_or_else(|| "Island".into())),
                BIOME_MISSION => ("boss", mission_boss(&zone.id).unwrap_or("Boss arena").to_owned()),
                BIOME_CARRIER => ("carrier", CARRIER_NAME.to_owned()),
                _ => continue,
            };
            pins.push(WorldPin { id: format!("zone-{}", zone.index), category: category.into(), name, position: at, hidden: false, respawn_seconds: None, respawn_at: None });
        }
        pins
    }

    /// Where the player arrives over `(x, z)`: on the ground, or swimming at the surface.
    unsafe fn arrival_height(&mut self, cx: &Cx, x: f32, z: f32) -> Result<f32, String> {
        let method = self.world_members.height.clone().ok_or("teleport is unavailable in this game version")?;
        let world = self.world_object(cx).ok_or("not in a world")?;
        let mut at = Vector3 { x, y: 0.0, z };
        let mut args = [&mut at as *mut Vector3 as *mut c_void];
        let ground: f32 = cx.rt.unbox(cx.rt.invoke(&method, world, &mut args).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(if ground > terrain::SEA_LEVEL { ground + GROUND_CLEARANCE } else { terrain::SEA_LEVEL + SWIM_HEIGHT })
    }

    fn reset_world(&mut self) {
        self.zones.clear();
        self.visited.clear();
        self.pin_spots.clear();
        self.world_object = None;
        self.sea_manager = None;
        self.boss = None;
        self.creature_names.clear();
        if let Some(scanner) = self.scanner.as_mut() {
            scanner.reset();
        }
    }
}

/// The weather, from the name of the game's weather event ("Weather Event - Rain").
fn weather_name(event: &str) -> String {
    let name = event.rsplit(" - ").next().unwrap_or(event).trim();
    if name.is_empty() {
        "Stormy".into()
    } else {
        name.to_owned()
    }
}

/// A short stable hash, for ids made from names.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(*b)).wrapping_mul(0x0100_0193))
}

/// The land nearest a zone's middle, where its pin goes, at ground height.
fn land_near_middle(terrain: &ZoneTerrain, center: Vec3) -> Option<Vec3> {
    let n = terrain.resolution;
    let mut best: Option<(f32, Vec3)> = None;
    for row in (0..n).step_by(2) {
        for col in (0..n).step_by(2) {
            let x = terrain.origin_x + col as f32 / (n - 1) as f32 * terrain.size_x;
            let z = terrain.origin_z + row as f32 / (n - 1) as f32 * terrain.size_z;
            let Some(h) = terrain.height_at(x, z) else { continue };
            // Above the surf, so the pin isn't on a wave-washed sandbar.
            if h < terrain::SEA_LEVEL + 1.0 {
                continue;
            }
            let d = (x - center.x).hypot(z - center.z);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, Vec3::new(x, h, z)));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The app's fog bits (see `lib/fog.ts`) with what's around `centers` revealed.
fn fog_bits(centers: &[Vec3]) -> Vec<u8> {
    let side = (terrain::MAP_SIDE / FOG_CELL).ceil() as usize;
    let mut bits = vec![0u8; (side * side).div_ceil(8)];
    let r = VISITED_REVEAL / FOG_CELL;
    for c in centers {
        let cx = (c.x - terrain::MAP_MIN) / FOG_CELL;
        let cy = (c.z - terrain::MAP_MIN) / FOG_CELL;
        let rows = ((cy - r).floor().max(0.0) as usize)..=((cy + r).ceil().min(side as f32 - 1.0).max(0.0) as usize);
        for row in rows {
            for col in ((cx - r).floor().max(0.0) as usize)..=((cx + r).ceil().min(side as f32 - 1.0).max(0.0) as usize) {
                let (dx, dy) = (col as f32 + 0.5 - cx, row as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    let i = row * side + col;
                    bits[i >> 3] |= 1 << (i & 7);
                }
            }
        }
    }
    bits
}

impl Provider for StrandedDeep {
    fn game_id(&self) -> &'static str {
        GAME_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { live_location: true, teleport: self.world_members.height.is_some(), other_players: true, hotkeys: true, game_map: false, game_pings: false }
    }

    fn frame_hooks(&self) -> &'static [HookTarget] {
        FRAME_HOOKS
    }

    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    fn reconnected(&mut self) {
        // What was read while the app was away is read again, so nothing's missed.
        if let Some(scanner) = self.scanner.as_mut() {
            scanner.reset();
        }
    }

    unsafe fn next_zone_pins(&mut self, cx: &Cx) -> Option<ZonePins> {
        let player = self.local_player(cx);
        if player.is_null() || self.zones.is_empty() || self.scanner.is_none() {
            return None;
        }
        let transform = cx.unity.component_transform(cx.rt, player).ok()?;
        let around = to_proto(cx.unity.position(cx.rt, transform).ok()?);
        let loaded = self.loaded_zones(cx);
        let scanner = self.scanner.as_mut()?;
        let pins = scanner.next(cx, &loaded, around);
        if let Some(found) = &pins {
            let now = Instant::now();
            if self.last_scan_note.is_none_or(|t| now - t >= Duration::from_secs(60)) {
                self.last_scan_note = Some(now);
                let average = scanner.busy / scanner.steps.max(1);
                self.notes.push(format!("Zone {} read: {} pins ({} steps, {average:?} per step on average)", found.x, found.pins.len(), scanner.steps));
            }
        }
        pins
    }

    unsafe fn snapshot(&mut self, cx: &Cx, frame: u64) -> WorldSnapshot {
        let now = Instant::now();
        let player = self.local_player(cx);
        let local = (|| {
            if player.is_null() {
                return None;
            }
            let transform = cx.unity.component_transform(cx.rt, player).ok()?;
            let position = to_proto(cx.unity.position(cx.rt, transform).ok()?);
            if !position.is_finite() || position == Vec3::ZERO {
                return None;
            }
            let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
            Some(LocalPlayer { position, forward, layer: WorldLayer::Surface, camera: None })
        })();

        if self.last_world_refresh.is_none_or(|t| now - t >= WORLD_REFRESH) {
            self.last_world_refresh = Some(now);
            let read = if player.is_null() { None } else { self.read_world(cx) };
            let (world, session) = read.map_or((None, None), |(w, s)| (Some(w), s));
            if session != self.session {
                self.notes.push(match &session {
                    // Never the id: it's the room's password, and logs get shared.
                    Some(s) => format!("Multiplayer session joined as {:?}", s.player_name),
                    None => "Not in a multiplayer session".into(),
                });
                self.session = session;
            }
            if world != self.game_world {
                self.notes.push(match &world {
                    Some(w) => format!("World: {} ({:?})", w.id, w.kind),
                    None => "World: none".into(),
                });
                self.reset_world();
                if world.is_none() {
                    self.spawned = false;
                }
                self.game_world = world;
            }
            if self.game_world.is_some() && self.ensure_zones(cx) {
                self.visited = self.read_visited(cx);
            }
            self.environment = if self.game_world.is_some() { self.read_environment(cx) } else { None };
            self.knocked_out = !player.is_null() && self.player.statistics.zip(self.player.knocked_out).is_some_and(|(stats, ko)| {
                let stats: *mut ManagedObject = stats.get(player);
                !stats.is_null() && ko.get::<bool>(stats)
            });
        }

        match &local {
            None => {
                self.others.clear();
                self.nearby.clear();
            }
            Some(p) => {
                self.spawned = true;
                if self.last_remote_refresh.is_none_or(|t| now - t >= REMOTE_REFRESH) {
                    self.last_remote_refresh = Some(now);
                    self.refresh_remote(cx, player);
                }
                if self.last_creature_refresh.is_none_or(|t| now - t >= CREATURE_REFRESH) {
                    self.last_creature_refresh = Some(now);
                    let started = Instant::now();
                    self.refresh_creatures(cx, to_unity(p.position));
                    self.creature_time += started.elapsed();
                    self.creature_reads += 1;
                    if self.creature_reads == 100 {
                        self.notes.push(format!("Creatures read in {:?} on average", self.creature_time / 100));
                    }
                }
            }
        }

        let visited = self.visited.iter().filter_map(|&i| {
            let zone = self.zones.get(i)?;
            matches!(zone.biome, BIOME_ISLAND | BIOME_ISLAND_SMALL | BIOME_ISLAND_ROCK | BIOME_MISSION | BIOME_CARRIER).then(|| self.pin_spots.get(&i).copied().unwrap_or(zone.center))
        });
        WorldSnapshot {
            frame,
            dying: self.spawned && self.game_world.is_some() && self.knocked_out,
            local,
            others: self.others.clone(),
            creatures: self.nearby.clone(),
            session: self.session.clone(),
            world: self.game_world.clone(),
            environment: self.environment.clone(),
            visited: visited.collect(),
            ..Default::default()
        }
    }

    unsafe fn teleport(&mut self, cx: &Cx, target: &TeleportTarget) -> Result<(), String> {
        let player = self.local_player(cx);
        if player.is_null() {
            return Err("not in a world".into());
        }
        if self.knocked_out {
            return Err("you're knocked out".into());
        }
        let movement: *mut ManagedObject = self.player.movement.get(player);
        if !cx.unity.is_alive(movement) {
            return Err("not in a world".into());
        }
        if self.player.vehicle.is_some_and(|f| cx.unity.is_alive(f.get::<*mut ManagedObject>(movement))) {
            return Err("get off the raft or boat first".into());
        }
        // The game ends a fight when the player swims away from it, and only sees that from
        // inside the arena: teleporting out would leave it running.
        if self.player.mission.is_some_and(|f| cx.unity.is_alive(f.get::<*mut ManagedObject>(player))) {
            return Err("you're in a boss fight: swim away from it first".into());
        }
        let mut dest = match target {
            TeleportTarget::Position { position, snap_to_ground, .. } => {
                let mut p = to_unity(*position);
                let arrival = self.arrival_height(cx, p.x, p.z)?;
                // Pins can be under water (wrecks): arrive above them at the surface.
                p.y = if *snap_to_ground || p.y < terrain::SEA_LEVEL { arrival } else { p.y.max(arrival) };
                p
            }
            TeleportTarget::Player { id } => {
                let position = self.others.iter().find(|o| &o.id == id).map(|o| o.position).ok_or("player not found")?;
                let mut p = to_unity(position);
                p.x += 1.0;
                p.y += 0.5;
                p
            }
        };
        if !(dest.x.is_finite() && dest.y.is_finite() && dest.z.is_finite()) {
            return Err("invalid destination".into());
        }

        // Off a raft deck, ladder or ledge first, as the game does after loading.
        if let Some(reset) = &self.player.reset_checks {
            cx.rt.invoke(reset, movement, &mut []).map_err(|e| format!("could not let go of where you are: {e}"))?;
        }
        // A fall is measured from where it started: from here, arriving lower would hurt.
        if let Some(fall) = self.player.fall_start {
            fall.set(movement, dest.y);
        }
        let transform = cx.unity.component_transform(cx.rt, player).map_err(|e| e.to_string())?;
        let class = cx.rt.class("UnityEngine", "Transform").map_err(|e| e.to_string())?;
        let set_position = cx.rt.method(class, "set_position", 1).map_err(|e| e.to_string())?;
        let mut args = [&mut dest as *mut Vector3 as *mut c_void];
        cx.rt.invoke(&set_position, transform, &mut args).map_err(|e| e.to_string())?;
        // The character controller keeps its own position until physics catches up.
        if let Some(physics) = cx.unity.physics() {
            physics.sync_transforms(cx.rt);
        }
        Ok(())
    }

    /// Islands are crossed by sea, and the game only builds navigation on loaded islands.
    unsafe fn find_path(&mut self, _cx: &Cx, _from: Vec3, _to: Vec3) -> Result<PathInfo, String> {
        Err("Stranded Deep has no routes across its sea".into())
    }

    unsafe fn read_world_map(&mut self, cx: &Cx, terrain: bool) -> Result<WorldMapCapture, String> {
        if !self.spawned || self.game_world.is_none() || !self.ensure_zones(cx) {
            return Err(WORLD_MAP_NOT_READY.into());
        }
        let started = Instant::now();
        let mut draw = None;
        if terrain || self.pin_spots.is_empty() {
            let terrains = self.read_terrains(cx)?;
            let read = started.elapsed();
            let centers: HashMap<usize, Vec3> = self.zones.iter().map(|z| (z.index, z.center)).collect();
            self.pin_spots = terrains.iter().filter_map(|(i, t)| Some((*i, land_near_middle(t, *centers.get(i)?)?))).collect();
            self.notes.push(format!("Stranded Deep terrain read in {read:?}"));
            if terrain {
                let zones: Vec<ZoneTerrain> = terrains.into_iter().map(|(_, t)| t).collect();
                draw = Some(Box::new(move || -> Terrain { terrain::draw(&zones) }) as Box<dyn FnOnce() -> Terrain + Send>);
            }
        }
        let pins = self.read_pins(cx);
        let visited: Vec<Vec3> = self.read_visited(cx).into_iter().filter_map(|i| self.zones.get(i).map(|z| z.center)).collect();
        self.notes.push(format!("Stranded Deep map read in {:?}: {} places, {} zones visited", started.elapsed(), pins.len(), visited.len()));
        Ok(WorldMapCapture { size: terrain::IMAGE_SIDE as u32, pins, explored: fog_bits(&visited), terrain: draw })
    }

    unsafe fn input_blocked(&mut self, cx: &Cx) -> bool {
        let now = Instant::now();
        if self.last_input_refresh.is_some_and(|t| now - t < INPUT_REFRESH) {
            return self.typing;
        }
        self.last_input_refresh = Some(now);
        self.typing = self.console.as_ref().is_some_and(|(instance, open)| {
            let console: *mut ManagedObject = cx.rt.static_value(*instance);
            cx.unity.is_alive(console) && cx.rt.invoke(open, console, &mut []).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(false)
        });
        self.typing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_comes_from_the_event_name() {
        assert_eq!(weather_name("Weather Event - Rain"), "Rain");
        assert_eq!(weather_name("Storm"), "Storm");
    }

    #[test]
    fn island_pins_go_on_land() {
        // A terrain of 3x3 samples, sea except the north-east corner.
        let mut heights = vec![0.5; 9];
        heights[8] = 0.8;
        let t = ZoneTerrain { origin_x: 0.0, origin_z: 0.0, size_x: 100.0, size_z: 100.0, base_y: -100.0, scale_y: 150.0, resolution: 3, heights, rocky: false };
        let spot = land_near_middle(&t, Vec3::new(50.0, 0.0, 50.0)).unwrap();
        assert_eq!((spot.x, spot.z), (100.0, 100.0));
        assert!(spot.y > 0.0);
    }

    #[test]
    fn fog_matches_the_apps_layout() {
        // 4800 m at 8 m cells: 600 x 600 bits.
        let bits = fog_bits(&[]);
        assert_eq!(bits.len(), 600 * 600 / 8);
        // The map's south-west corner is the first cell.
        let bits = fog_bits(&[Vec3::new(terrain::MAP_MIN + 4.0, 0.0, terrain::MAP_MIN + 4.0)]);
        assert_eq!(bits[0] & 1, 1);
    }

    #[test]
    fn hashes_are_stable() {
        assert_eq!(fnv1a(b""), 0x811c_9dc5);
        assert_eq!(fnv1a(b"a"), 0xe40c_292c);
    }

    #[test]
    #[ignore = "needs a Stranded Deep install: set STRANDED_DEEP_DIR and run with --ignored"]
    fn resolves_members_against_game_assemblies() {
        use std::ffi::{c_char, CString};

        use unity_runtime::BackendKind;
        use windows::core::{s, HSTRING};
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

        let game = std::env::var("STRANDED_DEEP_DIR").expect("STRANDED_DEEP_DIR");
        let managed = format!("{game}/Stranded_Deep_Data/Managed");
        unsafe {
            let mono = LoadLibraryW(&HSTRING::from(format!("{game}/MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll"))).expect("mono dll");
            macro_rules! f {
                ($name:literal, $ty:ty) => {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(GetProcAddress(mono, s!($name)).expect($name))
                };
            }
            let set_dirs = f!("mono_set_dirs", unsafe extern "C" fn(*const c_char, *const c_char));
            let set_path = f!("mono_set_assemblies_path", unsafe extern "C" fn(*const c_char));
            let jit_init = f!("mono_jit_init_version", unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void);
            let open = f!("mono_domain_assembly_open", unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void);
            let managed_c = CString::new(managed.clone()).unwrap();
            let etc = CString::new(format!("{game}/MonoBleedingEdge/etc")).unwrap();
            set_dirs(managed_c.as_ptr(), etc.as_ptr());
            set_path(managed_c.as_ptr());
            let domain = jit_init(c"game-maps-test".as_ptr(), c"v4.0.30319".as_ptr());
            assert!(!domain.is_null());
            for entry in std::fs::read_dir(&managed).unwrap().flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "dll") {
                    let p = CString::new(path.to_string_lossy().into_owned()).unwrap();
                    open(domain, p.as_ptr());
                }
            }
        }
        let rt = Runtime::load().expect("runtime");
        assert_eq!(rt.kind(), BackendKind::Mono);
        let _thread = rt.attach_thread();
        drop(unity_runtime::Unity::new(&rt).expect("engine members"));
        // Objects kept across frames are weak references: the game's Mono must have them.
        let s = rt.new_string("kept");
        let weak = unsafe { rt.weak_ref(s) }.expect("weak references");
        assert_eq!(unsafe { weak.get() }, s);
        let mut p = StrandedDeep::detect(&rt).expect("provider detected");
        let notes = p.take_notes().join("\n");
        println!("{notes}");
        for want in ["peers=true", "session=true", "slot=true", "height=true", "terrain=true", "creatures=true", "land_animals=true", "bosses=3", "environment=true", "weather=true", "names=true", "console=true", "scanner=true", "knocked_out=true", "vehicle=true", "reset=true", "mission=true", "wildlife=true"] {
            assert!(notes.contains(want), "missing {want} in:\n{notes}");
        }
        assert!(p.capabilities().teleport);
        for t in crate::hook::ENGINE_TARGETS.iter().chain(p.frame_hooks()) {
            if t.namespace == "UnityEngine.Rendering" {
                continue;
            }
            let class = rt.class(t.namespace, t.class).unwrap_or_else(|e| panic!("{e}"));
            rt.method(class, t.method, 0).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
