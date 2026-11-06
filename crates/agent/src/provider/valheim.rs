//! Valheim (Iron Gate, Unity Mono). Worlds are generated from a seed, so there's no map
//! pack: the map comes from the running game (see [`Valheim::read_world_map`]).
//!
//! Members used (all looked up by name, see `docs/games/valheim.md`):
//! - `Player.m_localPlayer`: the local player, gone while dead and respawning.
//!   `Player.TeleportTo` is what portals call: it fades, loads the area and lands safely.
//! - `Player.GetAllPlayers()`: players loaded around you; `ZNet.GetPlayerList()` has the
//!   rest, with a position if they chose to share it.
//! - `ZNet.m_world`: `m_uid`, `m_seed` and `m_name` of the world, on clients too.
//! - `Minimap`: the world's textures, what's explored, and the map's pins.
//!   `AddPin` and `RemovePin` put the player's pins on it, saved with their character like
//!   pins placed in the game, so a cartography table shares them. `Chat.SendPing` is the
//!   game's ping, which everyone in the world sees.
//! - `ZoneSystem`: every location on the host; clients only get the ones with an icon.
//! - `Character.GetAllCharacters()`: creatures loaded around you.
//! - `EnvMan`: the day, time and weather.

mod locations;
mod scan;
mod terrain;

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use game_maps_protocol::{Wind, ZonePins, Capabilities, Creature, CreatureKind, Environment, GamePin, GameWorld, LocalPlayer, Marker, MarkerKind, MultiplayerSession, PathInfo, RemotePlayer, TeleportTarget, Vec3, WorldKind, WorldLayer, WorldPin, WorldSnapshot};
use unity_runtime::{read_object_array, read_string, read_value_array, Class, Field, ListLayout, ManagedArray, ManagedObject, Method, Quaternion, Runtime, Vector3};

use super::{to_proto, to_unity, Cx, Provider};
use crate::hook::{target, HookTarget, Shape};
use crate::worldmap::{Terrain, WorldMapCapture};
use terrain::Color32;

const GAME_ID: &str = "valheim";

const FRAME_HOOKS: &[HookTarget] = &[target("", "Game", "Update", Shape::Instance0), target("", "ZNet", "Update", Shape::Instance0)];

const REMOTE_REFRESH: Duration = Duration::from_millis(200);
const CREATURE_REFRESH: Duration = Duration::from_millis(250);
const MARKER_REFRESH: Duration = Duration::from_secs(1);
/// How often the world, session and time are read.
const WORLD_REFRESH: Duration = Duration::from_secs(2);
/// Typing state is read through calls that allocate, so not on every frame.
const INPUT_REFRESH: Duration = Duration::from_millis(100);
/// How often an area around the player is read for what's in it: all 25 in 5 seconds.
const SCAN_REFRESH: Duration = Duration::from_millis(200);
/// Option: teleports take no notice of what can't go through a portal.
const OPTION_TELEPORT_RESTRICTED: &str = "teleportRestricted";
/// Option: the player has the app's mini-map on, so the game's only gets in the way.
const OPTION_HIDE_MINIMAP: &str = "hideGameMinimap";
/// `Minimap.MapMode.Small`.
const MAP_MODE_SMALL: i32 = 1;
/// What a wishbone is called in the game's data.
const WISHBONE: &str = "$item_wishbone";
/// Bosses, as `ZoneSystem` records them and `game.json`'s checklist names them.
const BOSS_KEYS: &[&str] = &["defeated_eikthyr", "defeated_gdking", "defeated_bonemass", "defeated_dragon", "defeated_goblinking", "defeated_queen", "defeated_fader"];
const CREATURE_RADIUS: f32 = 200.0;
const MAX_ENTITIES: usize = 256;
const MAX_MAP_PINS: usize = 2048;
const MAX_LOCATIONS: usize = 65536;
/// `game.json` projects the map onto this many meters per side.
const MAP_SIDE_METERS: f32 = 24576.0;
/// `ZoneSystem.m_waterLevel`.
const SEA_LEVEL: f32 = 30.0;
/// Height added above the ground for teleports.
const GROUND_CLEARANCE: f32 = 1.5;
/// Dungeons sit this high in the sky, above the map: their floors aren't near the ground.
const DUNGEON_HEIGHT: f32 = 1000.0;

/// `Character.Faction`.
const FACTION_PLAYERS: i32 = 0;
const FACTION_ANIMALS: i32 = 1;
const FACTION_PLAYER_SPAWNED: i32 = 11;
const FACTION_TRAINING_DUMMY: i32 = 12;

/// `Minimap.PinType`.
const PIN_ICON0: i32 = 0;
const PIN_ICON3: i32 = 3;
const PIN_DEATH: i32 = 4;
const PIN_BED: i32 = 5;
const PIN_ICON4: i32 = 6;
const PIN_SHOUT: i32 = 7;
const PIN_BOSS: i32 = 9;
const PIN_PING: i32 = 12;
const PIN_HILDIR1: i32 = 14;
const PIN_HILDIR3: i32 = 16;
/// Pins from the app show as the game's X.
const PIN_FROM_APP: i32 = PIN_ICON3;
/// Pins this close together are the same pin (saved positions are exact).
const SAME_PIN_METERS: f32 = 1.0;
/// Room for a `Splatform.PlatformUserID`: `AddPin` takes one by value, left empty.
const NO_AUTHOR_BYTES: usize = 64;

fn marker_icon(pin_type: i32) -> Option<(&'static str, &'static str)> {
    match pin_type {
        PIN_BED => Some(("builtin:bed", "Bed")),
        PIN_BOSS => Some(("builtin:skull", "Boss")),
        PIN_PING | PIN_SHOUT => Some(("builtin:radio", "Ping")),
        PIN_ICON0..=PIN_ICON3 | PIN_ICON4 => Some(("builtin:pin", "Map pin")),
        PIN_HILDIR1..=PIN_HILDIR3 => Some(("builtin:key", "Hildir's chest")),
        _ => None,
    }
}

struct PlayerMembers {
    local: Field,
    all: Method,
    name: Method,
    is_dead: Method,
    is_teleporting: Method,
    is_attached: Method,
    teleport_to: Method,
    is_teleportable: Option<Method>,
    /// `Humanoid.m_utilityItem`, its `m_shared`, and that's `m_name`.
    utility_item: Option<(Field, Field, Field)>,
}

struct NetMembers {
    instance: Field,
    world: Field,
    is_server: Method,
    open_server: Option<Field>,
    player_list: Option<Method>,
    game: Field,
}

struct WorldFields {
    name: Field,
    uid: Field,
    seed: Field,
    menu: Option<Field>,
}

/// `ZNet.PlayerInfo`, a struct.
struct PlayerInfoFields {
    name: Field,
    public_position: Field,
    position: Field,
}

struct MinimapMembers {
    instance: Field,
    pins: Field,
    generated: Field,
    size: Field,
    pixel_size: Field,
    map: Field,
    mask: Field,
    height: Field,
    explored: Field,
    explored_others: Field,
    pixels32: Method,
    raw_data: Method,
    add_pin: Option<Method>,
    remove_pin: Option<Method>,
    /// `m_smallRoot`, `m_mode` and `GameObject.SetActive`.
    small_map: Option<(Field, Field, Method)>,
}

/// `Minimap.PinData`.
struct PinFields {
    name: Field,
    kind: Field,
    position: Field,
}

struct ZoneMembers {
    instance: Field,
    locations: Method,
    icons: Field,
    location_class: Class,
    location: Field,
    position: Field,
    placed: Field,
    prefab_name: Field,
    global_key: Option<Method>,
}

struct GroundMembers {
    instance: Method,
    height: Method,
}

struct CreatureMembers {
    all: Method,
    name: Field,
    faction: Field,
    tamed: Field,
}

struct EnvMembers {
    instance: Field,
    day: Method,
    fraction: Method,
    wind_direction: Option<Method>,
    wind_strength: Option<Method>,
    current: Field,
    env_name: Field,
}

struct InputMembers {
    chat: Option<(Field, Method)>,
    /// `Chat.SendPing`.
    ping: Option<Method>,
    console: Option<Method>,
    text_input: Option<Method>,
    map_text: Option<Method>,
}

struct Localization {
    instance: Method,
    localize: Method,
}

pub struct Valheim {
    player: PlayerMembers,
    net: NetMembers,
    world_fields: WorldFields,
    player_info: Option<PlayerInfoFields>,
    minimap: Option<MinimapMembers>,
    pin_fields: Option<PinFields>,
    zones: Option<ZoneMembers>,
    ground: Option<GroundMembers>,
    creature_members: Option<CreatureMembers>,
    env: Option<EnvMembers>,
    input: InputMembers,
    localization: Option<Localization>,
    /// One layout serves every `List<T>` of reference types.
    list_layout: Option<ListLayout>,
    notes: Vec<String>,

    /// Localized names by the game's text keys.
    names: HashMap<String, String>,
    /// A player has been seen in the current world (they're away while respawning).
    spawned: bool,
    game_world: Option<GameWorld>,
    session: Option<MultiplayerSession>,
    environment: Option<Environment>,
    others: Vec<RemotePlayer>,
    nearby: Vec<Creature>,
    markers: Vec<Marker>,
    dropped_bag: Option<Vec3>,
    dead: bool,
    typing: bool,
    /// The player's pins from the app, as last put on the game's map.
    game_pins: Vec<GamePin>,
    /// Pins waiting for the game's map to be ready.
    pending_pins: Option<Vec<GamePin>>,
    scanner: Option<scan::Scanner>,
    last_scan: Option<Instant>,
    /// Bosses beaten in this world.
    progress: Vec<String>,
    /// Option [`OPTION_TELEPORT_RESTRICTED`].
    teleport_restricted: bool,
    /// Option [`OPTION_HIDE_MINIMAP`].
    hide_minimap: bool,
    /// The game's mini-map is hidden, and needs bringing back.
    minimap_hidden: bool,
    last_world_refresh: Option<Instant>,
    last_remote_refresh: Option<Instant>,
    last_creature_refresh: Option<Instant>,
    last_marker_refresh: Option<Instant>,
    last_input_refresh: Option<Instant>,
    logged_first_scan: bool,
}

// Raw object pointers are only dereferenced on the main thread.
unsafe impl Send for Valheim {}

fn static_field(rt: &Runtime, class: Class, name: &str) -> Option<Field> {
    rt.field(class, name).ok().filter(|f| f.is_static)
}

impl Valheim {
    pub fn detect(rt: &Runtime) -> Option<Self> {
        // `ZNet` and `ZoneSystem` together are specific enough to this game.
        let net = rt.class("", "ZNet").ok()?;
        let zone = rt.class("", "ZoneSystem").ok()?;
        let player = rt.class("", "Player").ok()?;
        let world = rt.class("", "World").ok()?;
        let game = rt.class("", "Game").ok()?;

        let players = PlayerMembers {
            local: static_field(rt, player, "m_localPlayer")?,
            all: rt.method(player, "GetAllPlayers", 0).ok()?,
            name: rt.method(player, "GetPlayerName", 0).ok()?,
            is_dead: rt.method(player, "IsDead", 0).ok()?,
            is_teleporting: rt.method(player, "IsTeleporting", 0).ok()?,
            is_attached: rt.method(player, "IsAttached", 0).ok()?,
            teleport_to: rt.method_with_params(player, "TeleportTo", &["UnityEngine.Vector3", "UnityEngine.Quaternion", "System.Boolean"]).ok()?,
            is_teleportable: rt.method(player, "IsTeleportable", 1).ok(),
            utility_item: (|| {
                let item = rt.class("", "ItemDrop/ItemData").ok()?;
                let shared = rt.class("", "ItemDrop/ItemData/SharedData").ok()?;
                Some((rt.field(player, "m_utilityItem").ok()?, rt.field(item, "m_shared").ok()?, rt.field(shared, "m_name").ok()?))
            })(),
        };
        let net_members = NetMembers {
            instance: static_field(rt, net, "m_instance")?,
            world: static_field(rt, net, "m_world")?,
            is_server: rt.method(net, "IsServer", 0).ok()?,
            open_server: static_field(rt, net, "m_openServer"),
            player_list: rt.method(net, "GetPlayerList", 0).ok(),
            game: static_field(rt, game, "<instance>k__BackingField")?,
        };
        let world_fields = WorldFields {
            name: rt.field(world, "m_name").ok()?,
            uid: rt.field(world, "m_uid").ok()?,
            seed: rt.field(world, "m_seed").ok()?,
            menu: rt.field(world, "m_menu").ok(),
        };

        let player_info = rt.class("", "ZNet/PlayerInfo").ok().and_then(|c| {
            Some(PlayerInfoFields { name: rt.field(c, "m_name").ok()?, public_position: rt.field(c, "m_publicPosition").ok()?, position: rt.field(c, "m_position").ok()? })
        });

        let minimap = (|| {
            let map = rt.class("", "Minimap").ok()?;
            let texture = rt.class("UnityEngine", "Texture2D").ok()?;
            Some(MinimapMembers {
                instance: static_field(rt, map, "s_instance")?,
                pins: rt.field(map, "m_pins").ok()?,
                generated: rt.field(map, "m_hasGenerated").ok()?,
                size: rt.field(map, "m_textureSize").ok()?,
                pixel_size: rt.field(map, "m_pixelSize").ok()?,
                map: rt.field(map, "m_mapTexture").ok()?,
                mask: rt.field(map, "m_forestMaskTexture").ok()?,
                height: rt.field(map, "m_heightTexture").ok()?,
                explored: rt.field(map, "m_explored").ok()?,
                explored_others: rt.field(map, "m_exploredOthers").ok()?,
                pixels32: rt.method(texture, "GetPixels32", 0).ok()?,
                raw_data: rt.method(texture, "GetRawTextureData", 0).ok()?,
                add_pin: rt.method(map, "AddPin", 7).ok(),
                remove_pin: rt.method(map, "RemovePin", 1).ok(),
                small_map: (|| Some((rt.field(map, "m_smallRoot").ok()?, rt.field(map, "m_mode").ok()?, rt.method(rt.class("UnityEngine", "GameObject").ok()?, "SetActive", 1).ok()?)))(),
            })
        })();
        let pin_fields = rt.class("", "Minimap/PinData").ok().and_then(|c| {
            Some(PinFields { name: rt.field(c, "m_name").ok()?, kind: rt.field(c, "m_type").ok()?, position: rt.field(c, "m_pos").ok()? })
        });

        let zones = (|| {
            let instance = rt.class("", "ZoneSystem/LocationInstance").ok()?;
            let location = rt.class("", "ZoneSystem/ZoneLocation").ok()?;
            Some(ZoneMembers {
                instance: static_field(rt, zone, "s_instance")?,
                locations: rt.method(zone, "GetLocationList", 0).ok()?,
                icons: rt.field(zone, "m_locationIcons").ok()?,
                location_class: instance,
                location: rt.field(instance, "m_location").ok()?,
                position: rt.field(instance, "m_position").ok()?,
                placed: rt.field(instance, "m_placed").ok()?,
                prefab_name: rt.field(location, "m_prefabName").ok()?,
                global_key: rt.method_with_params(zone, "GetGlobalKey", &["System.String"]).ok(),
            })
        })();

        let ground = rt.class("", "WorldGenerator").ok().and_then(|c| {
            Some(GroundMembers { instance: rt.method(c, "get_instance", 0).ok()?, height: rt.method_with_params(c, "GetHeight", &["System.Single", "System.Single"]).ok()? })
        });

        let creature_members = rt.class("", "Character").ok().and_then(|c| {
            Some(CreatureMembers { all: rt.method(c, "GetAllCharacters", 0).ok()?, name: rt.field(c, "m_name").ok()?, faction: rt.field(c, "m_faction").ok()?, tamed: rt.field(c, "m_tamed").ok()? })
        });

        let env = (|| {
            let man = rt.class("", "EnvMan").ok()?;
            let setup = rt.class("", "EnvSetup").ok()?;
            Some(EnvMembers {
                instance: static_field(rt, man, "s_instance")?,
                day: rt.method(man, "GetDay", 0).ok()?,
                fraction: rt.method(man, "GetDayFraction", 0).ok()?,
                wind_direction: rt.method(man, "GetWindDir", 0).ok(),
                wind_strength: rt.method(man, "GetWindIntensity", 0).ok(),
                current: rt.field(man, "m_currentEnv").ok()?,
                env_name: rt.field(setup, "m_name").ok()?,
            })
        })();

        let input = InputMembers {
            chat: rt.class("", "Chat").ok().and_then(|c| Some((static_field(rt, c, "m_instance")?, rt.method(c, "HasFocus", 0).ok()?))),
            ping: rt.class("", "Chat").ok().and_then(|c| rt.method_with_params(c, "SendPing", &["UnityEngine.Vector3"]).ok()),
            console: rt.class("", "Console").ok().and_then(|c| rt.method(c, "IsVisible", 0).ok()),
            text_input: rt.class("", "TextInput").ok().and_then(|c| rt.method(c, "IsVisible", 0).ok()),
            map_text: rt.class("", "Minimap").ok().and_then(|c| rt.method(c, "InTextInput", 0).ok()),
        };

        let localization = rt.class("", "Localization").ok().and_then(|c| {
            Some(Localization { instance: rt.method(c, "get_instance", 0).ok()?, localize: rt.method_with_params(c, "Localize", &["System.String"]).ok()? })
        });

        let scanner = scan::Scanner::resolve(rt, net);
        let notes = vec![format!(
            "Valheim: players_list={} map={} pins={} game_pins={} ping={} locations={} ground={} creatures={} environment={} chat={} console={} text={} map_text={} names={} scanner={} bosses={} wishbone={} teleportable={}",
            player_info.is_some() && net_members.player_list.is_some(),
            minimap.is_some(),
            pin_fields.is_some(),
            minimap.as_ref().is_some_and(|m| m.add_pin.is_some() && m.remove_pin.is_some()),
            input.ping.is_some(),
            zones.is_some(),
            ground.is_some(),
            creature_members.is_some(),
            env.is_some(),
            input.chat.is_some(),
            input.console.is_some(),
            input.text_input.is_some(),
            input.map_text.is_some(),
            localization.is_some(),
            scanner.is_some(),
            zones.as_ref().is_some_and(|z| z.global_key.is_some()),
            players.utility_item.is_some(),
            players.is_teleportable.is_some(),
        )];

        Some(Self {
            player: players,
            net: net_members,
            world_fields,
            player_info,
            minimap,
            pin_fields,
            zones,
            ground,
            creature_members,
            env,
            input,
            localization,
            list_layout: None,
            notes,
            names: HashMap::new(),
            spawned: false,
            game_world: None,
            session: None,
            environment: None,
            others: Vec::new(),
            nearby: Vec::new(),
            markers: Vec::new(),
            dropped_bag: None,
            dead: false,
            typing: false,
            game_pins: Vec::new(),
            pending_pins: None,
            scanner,
            last_scan: None,
            progress: Vec::new(),
            teleport_restricted: false,
            hide_minimap: false,
            minimap_hidden: false,
            last_world_refresh: None,
            last_remote_refresh: None,
            last_creature_refresh: None,
            last_marker_refresh: None,
            last_input_refresh: None,
            logged_first_scan: false,
        })
    }

    unsafe fn local_player(&self, cx: &Cx) -> *mut ManagedObject {
        let player: *mut ManagedObject = cx.rt.static_value(self.player.local);
        if cx.unity.is_alive(player) {
            player
        } else {
            std::ptr::null_mut()
        }
    }

    unsafe fn net(&self, cx: &Cx) -> Option<*mut ManagedObject> {
        let net: *mut ManagedObject = cx.rt.static_value(self.net.instance);
        cx.unity.is_alive(net).then_some(net)
    }

    /// `method` on `this` (null for static methods), as a `bool`.
    unsafe fn flag(cx: &Cx, method: &Method, this: *mut ManagedObject) -> bool {
        cx.rt.invoke(method, this, &mut []).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(false)
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

    /// The game's text for a key like `$enemy_greyling`.
    unsafe fn localized(&mut self, cx: &Cx, key: &str) -> String {
        if let Some(name) = self.names.get(key) {
            return name.clone();
        }
        let text = self.localization.as_ref().and_then(|l| {
            let instance = cx.rt.invoke(&l.instance, std::ptr::null_mut(), &mut []).ok().filter(|i| !i.is_null())?;
            let mut args = [cx.rt.new_string(key) as *mut c_void];
            read_string(cx.rt.invoke(&l.localize, instance, &mut args).ok()?)
        });
        let name = text.map(|t| locations::without_markup(&t)).filter(|t| !t.is_empty()).unwrap_or_else(|| key.trim_start_matches('$').to_owned());
        self.names.insert(key.to_owned(), name.clone());
        name
    }

    unsafe fn read_local(&self, cx: &Cx, player: *mut ManagedObject) -> Option<LocalPlayer> {
        let transform = cx.unity.component_transform(cx.rt, player).ok()?;
        let position = to_proto(cx.unity.position(cx.rt, transform).ok()?);
        if !position.is_finite() || position == Vec3::ZERO {
            return None;
        }
        let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
        Some(LocalPlayer { position, forward, layer: WorldLayer::Surface, camera: None })
    }

    /// The world that's loaded, and the session that goes with it.
    unsafe fn read_world(&self, cx: &Cx, player: *mut ManagedObject) -> Option<(GameWorld, Option<MultiplayerSession>)> {
        let game: *mut ManagedObject = cx.rt.static_value(self.net.game);
        if !cx.unity.is_alive(game) {
            return None;
        }
        let net = self.net(cx)?;
        let world: *mut ManagedObject = cx.rt.static_value(self.net.world);
        if world.is_null() || self.world_fields.menu.is_some_and(|f| f.get::<bool>(world)) {
            return None;
        }
        let uid: i64 = self.world_fields.uid.get(world);
        let seed: i32 = self.world_fields.seed.get(world);
        let name = read_string(self.world_fields.name.get(world)).filter(|n| !n.is_empty());
        let server = Self::flag(cx, &self.net.is_server, net);
        let open = self.net.open_server.is_some_and(|f| cx.rt.static_value::<bool>(f));
        let kind = match (server, open) {
            (false, _) => WorldKind::Joined,
            (true, true) => WorldKind::Hosted,
            (true, false) => WorldKind::SinglePlayer,
        };
        let session = (kind != WorldKind::SinglePlayer).then(|| MultiplayerSession {
            // Only players in this world know both.
            id: format!("{uid:x}-{seed:x}"),
            player_name: if player.is_null() { None } else { cx.rt.invoke(&self.player.name, player, &mut []).ok().and_then(|n| read_string(n)) },
        });
        Some((GameWorld { id: format!("world-{uid}"), name, kind }, session))
    }

    unsafe fn read_environment(&self, cx: &Cx) -> Option<Environment> {
        let e = self.env.as_ref()?;
        let man: *mut ManagedObject = cx.rt.static_value(e.instance);
        if !cx.unity.is_alive(man) {
            return None;
        }
        let fraction: f32 = cx.rt.unbox(cx.rt.invoke(&e.fraction, man, &mut []).ok()?).ok()?;
        let day: i32 = cx.rt.unbox(cx.rt.invoke(&e.day, man, &mut []).ok()?).ok()?;
        let minutes = (fraction.clamp(0.0, 0.9999) * 24.0 * 60.0) as u32;
        let current: *mut ManagedObject = e.current.get(man);
        let weather = if current.is_null() { None } else { read_string(e.env_name.get(current)).map(|n| locations::weather_name(&n)) };
        let wind = (|| {
            let direction: Vector3 = cx.rt.unbox(cx.rt.invoke(e.wind_direction.as_ref()?, man, &mut []).ok()?).ok()?;
            let strength: f32 = cx.rt.unbox(cx.rt.invoke(e.wind_strength.as_ref()?, man, &mut []).ok()?).ok()?;
            let degrees = direction.x.atan2(direction.z).to_degrees().rem_euclid(360.0);
            Some(Wind { degrees: degrees as u16, strength: (strength.clamp(0.0, 1.0) * 100.0) as u8 })
        })();
        Some(Environment { day: u32::try_from(day).ok(), hour: (minutes / 60) as u8, minute: (minutes % 60) as u8, season: None, weather, wind })
    }

    unsafe fn refresh_remote(&mut self, cx: &Cx, player: *mut ManagedObject) {
        let local_name = read_string(cx.rt.invoke(&self.player.name, player, &mut []).unwrap_or(std::ptr::null_mut()));
        let mut others: Vec<RemotePlayer> = Vec::new();
        let loaded = cx.rt.invoke(&self.player.all, std::ptr::null_mut(), &mut []).unwrap_or(std::ptr::null_mut());
        for other in self.list(cx, loaded, MAX_ENTITIES) {
            if other == player || !cx.unity.is_alive(other) {
                continue;
            }
            let Ok(transform) = cx.unity.component_transform(cx.rt, other) else { continue };
            let Ok(position) = cx.unity.position(cx.rt, transform) else { continue };
            let Some(name) = read_string(cx.rt.invoke(&self.player.name, other, &mut []).unwrap_or(std::ptr::null_mut())) else { continue };
            let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
            others.push(RemotePlayer { id: format!("player:{name}"), name, color: None, position: to_proto(position), forward });
        }
        for (name, position) in self.far_players(cx) {
            if Some(&name) != local_name.as_ref() && !others.iter().any(|o| o.name == name) {
                others.push(RemotePlayer { id: format!("player:{name}"), name, color: None, position, forward: None });
            }
        }
        if !self.logged_first_scan {
            self.logged_first_scan = true;
            self.notes.push(format!("Players seen on first scan: {}", others.len()));
        }
        self.others = others;
    }

    /// Everyone in the world who shares their position, from `ZNet`'s list of players.
    unsafe fn far_players(&self, cx: &Cx) -> Vec<(String, Vec3)> {
        let (Some(fields), Some(method), Some(net)) = (&self.player_info, &self.net.player_list, self.net(cx)) else { return Vec::new() };
        let Ok(list) = cx.rt.invoke(method, net, &mut []) else { return Vec::new() };
        let Some(class) = (!list.is_null()).then(|| cx.rt.class_of(list)).flatten() else { return Vec::new() };
        let (Ok(items), Ok(size)) = (cx.rt.field(class, "_items"), cx.rt.field(class, "_size")) else { return Vec::new() };
        let array: *mut ManagedObject = items.get(list);
        if array.is_null() {
            return Vec::new();
        }
        let count = (size.get::<i32>(list).max(0) as usize).min(MAX_ENTITIES);
        let array = ManagedArray::of(cx.rt, array, count);
        (0..array.len)
            .filter_map(|i| {
                let info = array.element(i);
                let position: Vector3 = fields.position.get_inline(info);
                let shared: bool = fields.public_position.get_inline(info);
                let name = read_string(fields.name.get_inline(info))?;
                (shared && !name.is_empty() && to_proto(position) != Vec3::ZERO).then(|| (name, to_proto(position)))
            })
            .collect()
    }

    unsafe fn refresh_creatures(&mut self, cx: &Cx, around: Vector3) {
        let Some(members) = &self.creature_members else { return };
        let (all, name_f, faction_f, tamed_f) = (members.all.clone(), members.name, members.faction, members.tamed);
        let Ok(list) = cx.rt.invoke(&all, std::ptr::null_mut(), &mut []) else { return };
        let mut found = Vec::new();
        for c in self.list(cx, list, MAX_ENTITIES) {
            if !cx.unity.is_alive(c) {
                continue;
            }
            let faction: i32 = faction_f.get(c);
            if faction == FACTION_PLAYERS || faction == FACTION_TRAINING_DUMMY {
                continue;
            }
            let Ok(transform) = cx.unity.component_transform(cx.rt, c) else { continue };
            let Ok(p) = cx.unity.position(cx.rt, transform) else { continue };
            let (dx, dz) = (p.x - around.x, p.z - around.z);
            if dx * dx + dz * dz > CREATURE_RADIUS * CREATURE_RADIUS {
                continue;
            }
            let tamed: bool = tamed_f.get(c);
            let kind = match faction {
                _ if tamed => CreatureKind::Friendly,
                FACTION_PLAYER_SPAWNED => CreatureKind::Friendly,
                FACTION_ANIMALS => CreatureKind::Animal,
                _ => CreatureKind::Hostile,
            };
            let Some(key) = read_string(name_f.get(c)) else { continue };
            let name = self.localized(cx, &key);
            found.push(Creature { id: format!("{:x}", c as usize), kind, name, position: to_proto(p) });
        }
        self.nearby = found;
    }

    /// The pins on the game's own map, and where the last grave is.
    unsafe fn refresh_markers(&mut self, cx: &Cx) {
        let (Some(map), Some(fields)) = (&self.minimap, &self.pin_fields) else { return };
        let minimap: *mut ManagedObject = cx.rt.static_value(map.instance);
        if !cx.unity.is_alive(minimap) {
            return;
        }
        let list: *mut ManagedObject = map.pins.get(minimap);
        let (name_f, kind_f, position_f) = (fields.name, fields.kind, fields.position);
        let mut markers = Vec::new();
        let mut grave = None;
        for pin in self.list(cx, list, MAX_MAP_PINS) {
            if pin.is_null() {
                continue;
            }
            let kind: i32 = kind_f.get(pin);
            let position = to_proto(position_f.get::<Vector3>(pin));
            if kind == PIN_DEATH {
                grave = Some(position);
                continue;
            }
            // Pings start at the origin for a frame; the app draws its own pins itself.
            if position == Vec3::ZERO || self.game_pins.iter().any(|p| same_spot(p.position, position)) {
                continue;
            }
            let Some((icon, default_label)) = marker_icon(kind) else { continue };
            let text = read_string(name_f.get(pin)).filter(|n| !n.trim().is_empty());
            let label = match text {
                Some(t) if t.starts_with('$') => self.localized(cx, &t),
                Some(t) => t,
                None => default_label.to_owned(),
            };
            markers.push(Marker { id: format!("pin:{kind}:{:.0},{:.0}", position.x, position.z), kind: MarkerKind::Locator, label: Some(label), icon: Some(icon.to_owned()), position });
        }
        self.markers = markers;
        self.dropped_bag = grave;
    }

    /// Brings the game's map in line with `pins`: adds those it doesn't have, and removes
    /// those from the app that are gone. Pins saved with the character from earlier stay as
    /// they are, so nothing is added twice.
    unsafe fn apply_game_pins(&mut self, cx: &Cx, pins: Vec<GamePin>) -> Result<(), String> {
        let (Some(map), Some(fields)) = (&self.minimap, &self.pin_fields) else { return Err("the map is unavailable".into()) };
        let (Some(add), Some(remove)) = (map.add_pin.clone(), map.remove_pin.clone()) else { return Err("pins are unavailable in this game version".into()) };
        let minimap: *mut ManagedObject = cx.rt.static_value(map.instance);
        let list_field = map.pins;
        let (kind_f, position_f) = (fields.kind, fields.position);
        let existing: Vec<(*mut ManagedObject, Vec3)> = self
            .list(cx, list_field.get(minimap), MAX_MAP_PINS)
            .into_iter()
            .filter(|p| !p.is_null() && matches!(kind_f.get::<i32>(*p), PIN_ICON0..=PIN_ICON3 | PIN_ICON4))
            .map(|p| (p, to_proto(position_f.get::<Vector3>(p))))
            .collect();

        for gone in self.game_pins.iter().filter(|old| !pins.iter().any(|p| same_spot(p.position, old.position))) {
            if let Some((pin, _)) = existing.iter().find(|(_, at)| same_spot(*at, gone.position)) {
                let mut args = [*pin as *mut c_void];
                cx.rt.invoke(&remove, minimap, &mut args).map_err(|e| e.to_string())?;
            }
        }
        for pin in pins.iter().filter(|p| !existing.iter().any(|(_, at)| same_spot(*at, p.position))) {
            let mut position = to_unity(pin.position);
            let mut kind = PIN_FROM_APP;
            let (mut save, mut checked, mut owner) = (true, false, 0i64);
            let mut author = [0u8; NO_AUTHOR_BYTES];
            let mut args = [
                &mut position as *mut Vector3 as *mut c_void,
                &mut kind as *mut i32 as *mut c_void,
                cx.rt.new_string(&pin.name) as *mut c_void,
                &mut save as *mut bool as *mut c_void,
                &mut checked as *mut bool as *mut c_void,
                &mut owner as *mut i64 as *mut c_void,
                author.as_mut_ptr() as *mut c_void,
            ];
            cx.rt.invoke(&add, minimap, &mut args).map_err(|e| e.to_string())?;
        }
        self.game_pins = pins;
        Ok(())
    }

    /// Every location known here: all of them on the host, those with an icon on clients.
    unsafe fn read_locations(&self, cx: &Cx) -> Result<Vec<WorldPin>, String> {
        let z = self.zones.as_ref().ok_or("locations are unavailable in this game version")?;
        let zones: *mut ManagedObject = cx.rt.static_value(z.instance);
        if !cx.unity.is_alive(zones) {
            return Err("the world isn't loaded".into());
        }
        let net = self.net(cx).ok_or("the world isn't loaded")?;
        let mut found: Vec<(String, Vector3, bool)> = Vec::new();
        if Self::flag(cx, &self.net.is_server, net) {
            let values = cx.rt.invoke(&z.locations, zones, &mut []).map_err(|e| e.to_string())?;
            let (array, count) = copy_collection(cx.rt, values, z.location_class, MAX_LOCATIONS)?;
            for i in 0..count {
                let instance = array.element(i);
                let location: *mut ManagedObject = z.location.get_inline(instance);
                if location.is_null() {
                    continue;
                }
                if let Some(prefab) = read_string(z.prefab_name.get(location)) {
                    found.push((prefab, z.position.get_inline(instance), z.placed.get_inline(instance)));
                }
            }
        } else {
            // The game only tells clients about what's really there.
            found = read_icons(cx.rt, z.icons.get(zones))?.into_iter().map(|(name, at)| (name, at, true)).collect();
        }

        let mut pins = Vec::with_capacity(found.len());
        let mut unknown = 0;
        for (prefab, p, placed) in found {
            match locations::classify(&prefab) {
                Some((category, name)) => {
                    let name = locations::pin_name(category, name, placed);
                    pins.push(WorldPin { id: format!("{prefab}@{:.0},{:.0}", p.x, p.z), category: category.to_owned(), name, position: to_proto(p), hidden: false, respawn_seconds: None, respawn_at: None });
                }
                None => unknown += 1,
            }
        }
        crate::filelog::write(&format!("Valheim: {} locations pinned, {unknown} left out", pins.len()));
        Ok(pins)
    }

    /// `m_explored` and `m_exploredOthers` together, as bytes: a `BitArray` packs bits the
    /// way the app's fog does, in a cell per map pixel.
    unsafe fn read_explored(&self, cx: &Cx, m: &MinimapMembers, minimap: *mut ManagedObject) -> Result<Vec<u8>, String> {
        let words = |bits: *mut ManagedObject| -> Result<Vec<u32>, String> {
            if bits.is_null() {
                return Ok(Vec::new());
            }
            let class = cx.rt.class_of(bits).ok_or("no explored data")?;
            let array = cx.rt.field(class, "m_array").map_err(|e| e.to_string())?;
            Ok(read_value_array::<u32>(array.get(bits), usize::MAX))
        };
        let own = words(m.explored.get(minimap))?;
        let others = words(m.explored_others.get(minimap))?;
        Ok(own.iter().zip(others.iter().chain(std::iter::repeat(&0))).flat_map(|(a, b)| (a | b).to_le_bytes()).collect())
    }

    /// The game has built its map and loaded the character's pins.
    unsafe fn map_ready(&self, cx: &Cx) -> bool {
        self.minimap.as_ref().is_some_and(|m| {
            let minimap: *mut ManagedObject = cx.rt.static_value(m.instance);
            cx.unity.is_alive(minimap) && m.generated.get::<bool>(minimap)
        })
    }

    /// The bosses beaten in this world.
    unsafe fn read_progress(&self, cx: &Cx) -> Vec<String> {
        let Some(z) = &self.zones else { return Vec::new() };
        let Some(method) = &z.global_key else { return Vec::new() };
        let zones: *mut ManagedObject = cx.rt.static_value(z.instance);
        if !cx.unity.is_alive(zones) {
            return Vec::new();
        }
        BOSS_KEYS
            .iter()
            .filter(|key| {
                let mut args = [cx.rt.new_string(key) as *mut c_void];
                cx.rt.invoke(method, zones, &mut args).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(false)
            })
            .map(|key| (*key).to_owned())
            .collect()
    }

    /// Whether the game's portals would refuse what the player is carrying.
    unsafe fn carrying_restricted(&self, cx: &Cx, player: *mut ManagedObject) -> bool {
        let Some(method) = &self.player.is_teleportable else { return false };
        let mut all = false;
        let mut args = [&mut all as *mut bool as *mut c_void];
        !cx.rt.invoke(method, player, &mut args).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(true)
    }

    /// Hides the game's mini-map or brings it back. The game shows it again by itself, after
    /// its big map or a death, so this runs all the time.
    unsafe fn apply_minimap(&mut self, cx: &Cx) {
        let Some(m) = &self.minimap else { return };
        let Some((root, mode, set_active)) = &m.small_map else { return };
        if !self.hide_minimap && !self.minimap_hidden {
            return;
        }
        let minimap: *mut ManagedObject = cx.rt.static_value(m.instance);
        if !cx.unity.is_alive(minimap) {
            return;
        }
        let object: *mut ManagedObject = root.get(minimap);
        if !cx.unity.is_alive(object) {
            return;
        }
        let mut show = !self.hide_minimap && mode.get::<i32>(minimap) == MAP_MODE_SMALL;
        let mut args = [&mut show as *mut bool as *mut c_void];
        if cx.rt.invoke(set_active, object, &mut args).is_ok() {
            self.minimap_hidden = self.hide_minimap;
        }
    }

    /// Whether the player has a wishbone on.
    unsafe fn wearing_wishbone(&self, player: *mut ManagedObject) -> bool {
        let Some((slot, shared, name)) = &self.player.utility_item else { return false };
        let item: *mut ManagedObject = slot.get(player);
        if item.is_null() {
            return false;
        }
        let data: *mut ManagedObject = shared.get(item);
        !data.is_null() && read_string(name.get(data)).as_deref() == Some(WISHBONE)
    }

    /// Height to stand at above `(x, z)`.
    unsafe fn ground_height(&self, cx: &Cx, x: f32, z: f32) -> Result<f32, String> {
        let g = self.ground.as_ref().ok_or("teleport is unavailable")?;
        let generator = cx.rt.invoke(&g.instance, std::ptr::null_mut(), &mut []).map_err(|e| e.to_string())?;
        if generator.is_null() {
            return Err("the world isn't loaded".into());
        }
        let (mut wx, mut wz) = (x, z);
        let mut args = [&mut wx as *mut f32 as *mut c_void, &mut wz as *mut f32 as *mut c_void];
        let height: f32 = cx.rt.unbox(cx.rt.invoke(&g.height, generator, &mut args).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(height.max(SEA_LEVEL))
    }
}

fn same_spot(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < SAME_PIN_METERS && (a.z - b.z).abs() < SAME_PIN_METERS
}

/// Copies a `Dictionary<,>.ValueCollection` or `KeyCollection` into a new `T[]`.
///
/// # Safety
/// `collection` must hold `element`s, and the array is only kept alive by the stack.
unsafe fn copy_collection(rt: &Runtime, collection: *mut ManagedObject, element: Class, max: usize) -> Result<(ManagedArray, usize), String> {
    if collection.is_null() {
        return Err("no collection".into());
    }
    let class = rt.class_of(collection).ok_or("no collection")?;
    let count_method = rt.method(class, "get_Count", 0).map_err(|e| e.to_string())?;
    let count: i32 = rt.unbox(rt.invoke(&count_method, collection, &mut []).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let count = (count.max(0) as usize).min(max);
    let copy = rt.method(class, "CopyTo", 2).map_err(|e| e.to_string())?;
    let array = rt.new_array(element, count).map_err(|e| e.to_string())?;
    let mut index = 0i32;
    let mut args = [array.object as *mut c_void, &mut index as *mut i32 as *mut c_void];
    rt.invoke(&copy, collection, &mut args).map_err(|e| e.to_string())?;
    Ok((array, count))
}

/// The entries of `ZoneSystem.m_locationIcons`, a `Dictionary<Vector3, string>`.
///
/// # Safety
/// `icons` must be null or that dictionary.
unsafe fn read_icons(rt: &Runtime, icons: *mut ManagedObject) -> Result<Vec<(String, Vector3)>, String> {
    if icons.is_null() {
        return Ok(Vec::new());
    }
    let class = rt.class_of(icons).ok_or("no location icons")?;
    let collection = |getter: &str| -> Result<*mut ManagedObject, String> {
        let method = rt.method(class, getter, 0).map_err(|e| e.to_string())?;
        rt.invoke(&method, icons, &mut []).map_err(|e| e.to_string())
    };
    let vector = rt.class("UnityEngine", "Vector3").map_err(|e| e.to_string())?;
    let string = rt.class("System", "String").map_err(|e| e.to_string())?;
    let (keys, count) = copy_collection(rt, collection("get_Keys")?, vector, MAX_LOCATIONS)?;
    let (names, _) = copy_collection(rt, collection("get_Values")?, string, MAX_LOCATIONS)?;
    let positions = (0..count).map(|i| std::ptr::read_unaligned(keys.element(i) as *const Vector3));
    let names = read_object_array(names.object, count).into_iter().map(|n| read_string(n).unwrap_or_default());
    Ok(names.zip(positions).filter(|(name, _)| !name.is_empty()).collect())
}

impl Provider for Valheim {
    fn game_id(&self) -> &'static str {
        GAME_ID
    }

    fn capabilities(&self) -> Capabilities {
        let pins = self.minimap.as_ref().is_some_and(|m| m.add_pin.is_some() && m.remove_pin.is_some());
        Capabilities { live_location: true, teleport: self.ground.is_some(), other_players: true, hotkeys: true, game_map: pins && self.input.ping.is_some() }
    }

    fn frame_hooks(&self) -> &'static [HookTarget] {
        FRAME_HOOKS
    }

    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    unsafe fn app_left(&mut self, cx: &Cx) {
        self.hide_minimap = false;
        self.apply_minimap(cx);
    }

    fn set_options(&mut self, options: &std::collections::BTreeMap<String, bool>) {
        self.teleport_restricted = options.get(OPTION_TELEPORT_RESTRICTED).copied().unwrap_or(false);
        self.hide_minimap = options.get(OPTION_HIDE_MINIMAP).copied().unwrap_or(false);
    }

    unsafe fn next_zone_pins(&mut self, cx: &Cx) -> Option<ZonePins> {
        let now = Instant::now();
        if self.last_scan.is_some_and(|t| now - t < SCAN_REFRESH) {
            return None;
        }
        self.last_scan = Some(now);
        let player = self.local_player(cx);
        if player.is_null() {
            return None;
        }
        let transform = cx.unity.component_transform(cx.rt, player).ok()?;
        let around = cx.unity.position(cx.rt, transform).ok()?;
        // Dungeons are in the sky over their entrance, not somewhere to look around in.
        if around.y > DUNGEON_HEIGHT {
            return None;
        }
        let net = self.net(cx)?;
        let wishbone = self.wearing_wishbone(player);
        self.scanner.as_mut()?.next(cx, net, around, wishbone)
    }

    unsafe fn snapshot(&mut self, cx: &Cx, frame: u64) -> WorldSnapshot {
        let now = Instant::now();
        let player = self.local_player(cx);
        let local = if player.is_null() { None } else { self.read_local(cx, player) };

        if self.last_world_refresh.is_none_or(|t| now - t >= WORLD_REFRESH) {
            self.last_world_refresh = Some(now);
            let read = self.read_world(cx, player);
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
                    Some(w) => format!("World: {} {:?} ({:?})", w.id, w.name, w.kind),
                    None => "World: none".into(),
                });
                // Another character or world: its map has its own pins.
                self.game_pins.clear();
                if let Some(scanner) = self.scanner.as_mut() {
                    scanner.reset();
                }
                if world.is_none() {
                    self.spawned = false;
                    self.dropped_bag = None;
                    self.markers.clear();
                }
                self.game_world = world;
            }
            self.environment = self.read_environment(cx);
            self.progress = self.read_progress(cx);
            // Dungeons have no weather, whatever is going on outside.
            if let (Some(environment), Some(p)) = (self.environment.as_mut(), &local) {
                if p.position.y > DUNGEON_HEIGHT {
                    environment.weather = None;
                }
            }
            self.dead = !player.is_null() && Self::flag(cx, &self.player.is_dead, player);
        }

        match &local {
            None => {
                self.others.clear();
                self.nearby.clear();
            }
            Some(p) => {
                self.spawned = true;
                if self.pending_pins.is_some() && self.map_ready(cx) {
                    if let Err(e) = self.pending_pins.take().map_or(Ok(()), |pins| self.apply_game_pins(cx, pins)) {
                        self.notes.push(format!("Pins not added to the game's map: {e}"));
                    }
                }
                if self.last_remote_refresh.is_none_or(|t| now - t >= REMOTE_REFRESH) {
                    self.last_remote_refresh = Some(now);
                    self.refresh_remote(cx, player);
                }
                if self.last_creature_refresh.is_none_or(|t| now - t >= CREATURE_REFRESH) {
                    self.last_creature_refresh = Some(now);
                    self.refresh_creatures(cx, to_unity(p.position));
                }
                if self.last_marker_refresh.is_none_or(|t| now - t >= MARKER_REFRESH) {
                    self.last_marker_refresh = Some(now);
                    self.refresh_markers(cx);
                    self.apply_minimap(cx);
                }
            }
        }

        // The player object goes away on death and comes back at the bed.
        let dying = self.spawned && self.game_world.is_some() && (local.is_none() || self.dead);
        let indoors = local.as_ref().is_some_and(|p| p.position.y > DUNGEON_HEIGHT);
        WorldSnapshot {
            frame,
            local,
            others: self.others.clone(),
            markers: self.markers.clone(),
            creatures: self.nearby.clone(),
            session: self.session.clone(),
            world: self.game_world.clone(),
            environment: self.environment.clone(),
            dropped_bag: self.dropped_bag,
            dying,
            indoors,
            progress: self.progress.clone(),
            ..Default::default()
        }
    }

    unsafe fn teleport(&mut self, cx: &Cx, target: &TeleportTarget) -> Result<(), String> {
        let player = self.local_player(cx);
        if player.is_null() {
            return Err("not in a world".into());
        }
        if Self::flag(cx, &self.player.is_dead, player) {
            return Err("you are dead".into());
        }
        if Self::flag(cx, &self.player.is_teleporting, player) {
            return Err("already teleporting".into());
        }
        if Self::flag(cx, &self.player.is_attached, player) {
            return Err("get off the ship or seat first".into());
        }
        if !self.teleport_restricted && self.carrying_restricted(cx, player) {
            return Err("you're carrying something that can't go through a portal".into());
        }

        let mut dest = match target {
            TeleportTarget::Position { position, snap_to_ground, .. } => {
                let mut p = to_unity(*position);
                // Pins on the land are as high as the ground was when the world was made.
                if *snap_to_ground || p.y < DUNGEON_HEIGHT {
                    let ground = self.ground_height(cx, p.x, p.z)?;
                    p.y = if *snap_to_ground { ground } else { p.y.max(ground) } + GROUND_CLEARANCE;
                }
                p
            }
            TeleportTarget::Player { id } => {
                let position = self.others.iter().find(|o| &o.id == id).map(|o| o.position).or_else(|| self.markers.iter().find(|m| &m.id == id).map(|m| m.position)).ok_or("player not found")?;
                let mut p = to_unity(position);
                p.x += 1.0;
                p.y += GROUND_CLEARANCE;
                p
            }
        };
        if !(dest.x.is_finite() && dest.y.is_finite() && dest.z.is_finite()) {
            return Err("invalid destination".into());
        }

        let transform = cx.unity.component_transform(cx.rt, player).map_err(|e| e.to_string())?;
        let mut rotation: Quaternion = cx.unity.rotation(cx.rt, transform).map_err(|e| e.to_string())?;
        let mut distant = true;
        let mut args = [&mut dest as *mut Vector3 as *mut c_void, &mut rotation as *mut Quaternion as *mut c_void, &mut distant as *mut bool as *mut c_void];
        let started = cx.rt.invoke(&self.player.teleport_to, player, &mut args).and_then(|b| cx.rt.unbox::<bool>(b)).map_err(|e| e.to_string())?;
        if started {
            Ok(())
        } else {
            Err("wait a moment before teleporting again".into())
        }
    }

    unsafe fn set_game_pins(&mut self, _cx: &Cx, pins: Vec<GamePin>) {
        // Applied with the next snapshot, once the map is there.
        self.pending_pins = Some(pins);
    }

    unsafe fn game_ping(&mut self, cx: &Cx, position: Vec3) -> Result<(), String> {
        let (Some((instance, _)), Some(ping)) = (&self.input.chat, &self.input.ping) else { return Err("pings are unavailable in this game version".into()) };
        let chat: *mut ManagedObject = cx.rt.static_value(*instance);
        if !cx.unity.is_alive(chat) {
            return Err("not in a world".into());
        }
        let mut at = to_unity(position);
        let mut args = [&mut at as *mut Vector3 as *mut c_void];
        cx.rt.invoke(ping, chat, &mut args).map(|_| ()).map_err(|e| e.to_string())
    }

    /// The game only finds its way through the area around it, and a route that stops
    /// a few hundred meters out would be misleading.
    unsafe fn find_path(&mut self, _cx: &Cx, _from: Vec3, _to: Vec3) -> Result<PathInfo, String> {
        Err("Valheim has no routes across its world".into())
    }

    unsafe fn read_world_map(&mut self, cx: &Cx, terrain: bool) -> Result<WorldMapCapture, String> {
        // Until the player has spawned, a new world may still be placing its locations.
        if !self.spawned || !self.map_ready(cx) {
            return Err("the game hasn't built its map yet".into());
        }
        let m = self.minimap.as_ref().ok_or("the map is unavailable in this game version")?;
        let minimap: *mut ManagedObject = cx.rt.static_value(m.instance);
        let size: i32 = m.size.get(minimap);
        let pixel_meters: f32 = m.pixel_size.get(minimap);
        if size <= 0 || size as f32 * pixel_meters != MAP_SIDE_METERS {
            return Err(format!("unexpected map size: {size} pixels of {pixel_meters} m"));
        }
        let size = size as usize;
        let pixels = size * size;

        let explored = self.read_explored(cx, m, minimap)?;
        let draw = if terrain {
            let started = Instant::now();
            let array = |texture: *mut ManagedObject, method: &Method| -> Result<*mut ManagedObject, String> {
                if !cx.unity.is_alive(texture) {
                    return Err("the map's textures are gone".into());
                }
                cx.rt.invoke(method, texture, &mut []).map_err(|e| e.to_string())
            };
            let biome = read_value_array::<Color32>(array(m.map.get(minimap), &m.pixels32)?, pixels);
            let after_biome = started.elapsed();
            let mask = read_value_array::<Color32>(array(m.mask.get(minimap), &m.pixels32)?, pixels);
            let after_mask = started.elapsed();
            let height = read_value_array::<u16>(array(m.height.get(minimap), &m.raw_data)?, pixels);
            if biome.len() != pixels || mask.len() != pixels || height.len() != pixels {
                return Err(format!("unexpected map textures: {} {} {}", biome.len(), mask.len(), height.len()));
            }
            self.notes.push(format!("Valheim map read in {:?} (biome {after_biome:?}, mask at {after_mask:?})", started.elapsed()));
            let meters = pixel_meters;
            Some(Box::new(move || terrain::draw(size, meters, &biome, &mask, &height)) as Box<dyn FnOnce() -> Terrain + Send>)
        } else {
            None
        };
        let started = Instant::now();
        let pins = self.read_locations(cx)?;
        self.notes.push(format!("Valheim locations read in {:?}", started.elapsed()));
        Ok(WorldMapCapture { size: (size * terrain::UPSCALE) as u32, pins, explored, terrain: draw })
    }

    unsafe fn input_blocked(&mut self, cx: &Cx) -> bool {
        let now = Instant::now();
        if self.last_input_refresh.is_some_and(|t| now - t < INPUT_REFRESH) {
            return self.typing;
        }
        self.last_input_refresh = Some(now);
        let i = &self.input;
        let chat = i.chat.as_ref().is_some_and(|(instance, focus)| {
            let chat: *mut ManagedObject = cx.rt.static_value(*instance);
            cx.unity.is_alive(chat) && Self::flag(cx, focus, chat)
        });
        let null = std::ptr::null_mut();
        let open = |m: &Option<Method>| m.as_ref().is_some_and(|m| Self::flag(cx, m, null));
        self.typing = chat || open(&i.console) || open(&i.text_input) || open(&i.map_text);
        self.typing
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_void, CString};

    use unity_runtime::{BackendKind, Runtime};
    use windows::core::{s, HSTRING};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    use super::Valheim;
    use crate::provider::Provider;

    /// Hosts the game's own Mono runtime, loads its managed assemblies (no engine, so
    /// nothing runs) and resolves every member the provider uses by name.
    #[test]
    #[ignore = "needs a Valheim install: set VALHEIM_DIR and run with --ignored"]
    fn resolves_members_against_game_assemblies() {
        let game = std::env::var("VALHEIM_DIR").expect("VALHEIM_DIR");
        let managed = format!("{game}/valheim_Data/Managed");
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
        let unity = unity_runtime::Unity::new(&rt).expect("engine members");
        drop(unity);
        let mut p = Valheim::detect(&rt).expect("provider detected");
        let notes = p.take_notes().join("\n");
        println!("{notes}");
        for want in ["players_list=true", "map=true", "pins=true", "locations=true", "ground=true", "creatures=true", "environment=true", "game_pins=true", "ping=true", "scanner=true", "bosses=true", "wishbone=true", "teleportable=true", "chat=true", "console=true", "text=true", "map_text=true", "names=true"] {
            assert!(notes.contains(want), "missing {want} in:\n{notes}");
        }
        assert!(p.capabilities().teleport);
        for t in crate::hook::ENGINE_TARGETS.iter().chain(p.frame_hooks()) {
            let class = rt.class(t.namespace, t.class).unwrap_or_else(|e| panic!("{e}"));
            let n = if t.shape == crate::hook::Shape::Static3 { 3 } else { 0 };
            rt.method(class, t.method, n).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
