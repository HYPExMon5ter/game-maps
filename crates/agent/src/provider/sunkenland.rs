//! Sunkenland (Vector3 Studio, Unity 2021.3 Mono, Photon Fusion).
//!
//! Members used (all looked up by name, see `docs/games/sunkenland.md`):
//! - `FPSPlayer.code.myTransform`: the local player. The game moves the player by
//!   setting this transform's position (respawn, beds, teleport gates), so teleports do
//!   the same, after `FPSRigidBodyWalker.code.ResetParent()` detaches the player from
//!   any boat deck they stand on.
//! - `Global.code.player` (`PlayerCharacter`): death and vehicle state, the local
//!   player's own `playerDummy`.
//! - `WorldScene.code`: `allPlayerDummies` (every player, including the local one),
//!   `Assistants` (companions), `PlayerBoatComponents` (the players' boats),
//!   `oceanObjectY` (sea level).
//! - `Global.code.HelicopterControllers`: built helicopters.
//! - `UIInGameTextChat.IsChatInputActive`, `QuantumConsole.IsActive`: suppress hotkeys
//!   while typing.

mod creatures;
mod marks;
mod session;

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use game_maps_protocol::{Capabilities, Creature, GameWorld, LocalPlayer, Marker, MarkerKind, MultiplayerSession, RemotePlayer, TeleportTarget, Vec3, WorldKind, WorldLayer, WorldSnapshot};
use unity_runtime::{read_string, Field, ListLayout, ManagedObject, Method, Runtime, Vector3};

use super::{to_proto, to_unity, Cx, Provider};
use crate::hook::{target, HookTarget, Shape};

const GAME_ID: &str = "sunkenland";
/// Game UI sprites used as live marker icons (exported by `tools/mapgen/sunkenland.py`).
const BOAT_ICON: &str = "icon boat1";
const HELICOPTER_ICON: &str = "icon steering wheel";

/// Game-side per-frame methods, in addition to the engine-level ones.
const FRAME_HOOKS: &[HookTarget] = &[
    target("", "Global", "Update", Shape::Instance0),
    target("", "FPSPlayer", "Update", Shape::Instance0),
];
const REMOTE_REFRESH: Duration = Duration::from_millis(100);
const MARKER_REFRESH: Duration = Duration::from_millis(250);
/// Names change rarely and reading them goes through Fusion's networked properties.
const NAME_REFRESH: Duration = Duration::from_secs(5);
/// How often the loaded world and session are read (they only change when loading another).
const WORLD_REFRESH: Duration = Duration::from_secs(2);
const CREATURE_REFRESH: Duration = Duration::from_millis(250);
/// How often placed flags are searched for (an object search; they rarely change). A
/// search over `SLOW_FLAG_SEARCH` stutters the game (50 ms or more in a late-game world),
/// so those wait for the map to open, with this as the longest gap.
const FLAG_REFRESH: Duration = Duration::from_secs(10);
const SLOW_FLAG_REFRESH: Duration = Duration::from_secs(3600);
const SLOW_FLAG_SEARCH: Duration = Duration::from_millis(2);
/// Creatures farther than this from the player are left out.
const CREATURE_RADIUS: f32 = 500.0;
const MAX_ENTITIES: usize = 256;
/// Teleport ground probe: cast down from this height above sea level.
const PROBE_HEIGHT: f32 = 600.0;
const PROBE_DISTANCE: f32 = 1200.0;
/// Height added above the ground or water surface for teleports.
const GROUND_CLEARANCE: f32 = 1.0;
/// Teleports to pins deeper than this below the sea surface land on the surface instead.
const MAX_TELEPORT_DEPTH: f32 = 3.0;
/// `Physics.DefaultRaycastLayers` (everything except "Ignore Raycast").
const DEFAULT_RAYCAST_LAYERS: i32 = !(1 << 2);
/// `QueryTriggerInteraction.Ignore`: trigger volumes (zones, water triggers) never stop the probe.
const QUERY_TRIGGERS_IGNORE: i32 = 1;

/// `UnityEngine.RaycastHit` (the point comes first).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RaycastHit {
    point: Vector3,
    normal: Vector3,
    face_id: u32,
    distance: f32,
    uv: [f32; 2],
    collider: i32,
}

struct LocalMembers {
    instance: Field,
    transform: Field,
}

struct GlobalMembers {
    instance: Field,
    player: Field,
    ui_combat: Option<Field>,
    helicopters: Option<Field>,
}

struct PlayerMembers {
    dummy: Option<Field>,
    driving: Option<Field>,
    seat: Option<Field>,
    helicopter: Option<Field>,
    is_dead: Option<Method>,
}

struct WorldMembers {
    instance: Field,
    players: Option<Field>,
    assistants: Option<Field>,
    boats: Option<Field>,
    ocean_y: Option<Field>,
}

struct DummyMembers {
    transform: Field,
    name: Option<Method>,
    cached_name: Option<Field>,
}

struct ChatMembers {
    chat: Field,
    active: Field,
}

/// `Mainframe.code.SaveManager.CurrentWorldGuid/Name` for your own worlds, and
/// `Mainframe.code.WorldManager.WorldIdentifier/WorldName` (from the host) when joining.
struct SaveMembers {
    mainframe: Field,
    save_manager: Method,
    world_valid: Option<Method>,
    world_guid: Method,
    world_name: Option<Method>,
    world_manager: Option<Method>,
    session_world_name: Option<Method>,
    session_world_id: Option<Method>,
}

struct ConsoleMembers {
    mainframe: Field,
    console: Field,
    active: Field,
}

struct TeleportMembers {
    set_position: Method,
    raycast: Option<Method>,
    walker: Option<WalkerMembers>,
    /// `RPGCamera.code` and its `cam`.
    camera: Option<(Field, Field)>,
}

/// `FPSRigidBodyWalker`, which moves the player.
struct WalkerMembers {
    instance: Field,
    reset_parent: Method,
    /// `isFloating` and the rest of the water state (bools), cleared when teleporting.
    water_state: Vec<Field>,
}

/// Fields of the companion and vehicle classes used for markers.
struct EntityMembers {
    assistant_position: Option<Field>,
    assistant_default_name: Option<Field>,
    assistant_name: Option<Method>,
    boat_position: Option<Field>,
    boat_display_name: Option<Method>,
    boat_destroyed: Option<Field>,
}

pub struct Sunkenland {
    local: LocalMembers,
    global: Option<GlobalMembers>,
    player: PlayerMembers,
    world: Option<WorldMembers>,
    dummy: Option<DummyMembers>,
    chat: Option<ChatMembers>,
    console: Option<ConsoleMembers>,
    teleport: Option<TeleportMembers>,
    entities: EntityMembers,
    save: Option<SaveMembers>,
    creature_members: Option<creatures::Creatures>,
    mark_members: Option<marks::MapMarks>,
    /// Map marks, read with the world (they rarely change).
    map_marks: Vec<Marker>,
    logged_marks: bool,
    logged_mark_error: bool,
    flag_members: Option<marks::Flags>,
    flags: Vec<Marker>,
    last_flag_refresh: Option<Instant>,
    flag_refresh: Duration,
    logged_flags: bool,
    logged_flag_error: bool,
    session_members: Option<session::Session>,
    /// One layout serves every `List<T>` of reference types.
    list_layout: Option<ListLayout>,
    notes: Vec<String>,

    last_remote_refresh: Option<Instant>,
    others: Vec<RemotePlayer>,
    logged_first_scan: bool,
    last_marker_refresh: Option<Instant>,
    markers: Vec<Marker>,
    /// Display names per object, refreshed every [`NAME_REFRESH`].
    names: HashMap<usize, String>,
    names_at: Option<Instant>,
    last_world_refresh: Option<Instant>,
    game_world: Option<GameWorld>,
    session: Option<MultiplayerSession>,
    last_creature_refresh: Option<Instant>,
    nearby: Vec<Creature>,
    logged_creatures: bool,
}

// Raw object pointers are only dereferenced on the main thread.
unsafe impl Send for Sunkenland {}

fn static_field(rt: &Runtime, class: unity_runtime::Class, name: &str) -> Option<Field> {
    rt.field(class, name).ok().filter(|f| f.is_static)
}

impl Sunkenland {
    pub fn detect(rt: &Runtime) -> Option<Self> {
        let fps = rt.class("", "FPSPlayer").ok()?;
        // `WorldScene` and `Global` together are specific enough to this game.
        let world_class = rt.class("", "WorldScene").ok()?;
        let global_class = rt.class("", "Global").ok()?;
        let mut notes = Vec::new();

        let local = LocalMembers { instance: static_field(rt, fps, "code")?, transform: rt.field(fps, "myTransform").ok()? };

        let global = (|| {
            Some(GlobalMembers {
                instance: static_field(rt, global_class, "code")?,
                player: rt.field(global_class, "player").ok()?,
                ui_combat: rt.field(global_class, "uiCombat").ok(),
                helicopters: rt.field(global_class, "HelicopterControllers").ok(),
            })
        })();

        let player = match rt.class("", "PlayerCharacter") {
            Ok(pc) => PlayerMembers {
                dummy: rt.field(pc, "playerDummy").ok(),
                driving: rt.field(pc, "drivingObject").ok(),
                seat: rt.field(pc, "seatObject").ok(),
                helicopter: rt.field(pc, "CurHelicopter").ok(),
                is_dead: rt.method(pc, "get_IsDead", 0).ok(),
            },
            Err(_) => PlayerMembers { dummy: None, driving: None, seat: None, helicopter: None, is_dead: None },
        };

        let world = static_field(rt, world_class, "code").map(|instance| WorldMembers {
            instance,
            players: rt.field(world_class, "allPlayerDummies").ok(),
            assistants: rt.field(world_class, "Assistants").ok(),
            boats: rt.field(world_class, "PlayerBoatComponents").ok(),
            ocean_y: rt.field(world_class, "oceanObjectY").ok(),
        });

        let dummy = rt.class("", "PlayerDummy").ok().and_then(|d| {
            Some(DummyMembers {
                transform: rt.field(d, "myTransform").ok()?,
                name: rt.method(d, "get_CharacterName", 0).ok(),
                cached_name: rt.field(d, "cache_CharacterName").ok(),
            })
        });

        let chat = (|| {
            let combat = rt.class("", "UICombat").ok()?;
            let chat = rt.class("", "UIInGameTextChat").ok()?;
            Some(ChatMembers { chat: rt.field(combat, "uiInGameTextChat").ok()?, active: rt.field(chat, "<IsChatInputActive>k__BackingField").ok()? })
        })();

        let console = (|| {
            let mf = rt.class("", "Mainframe").ok()?;
            let qc = rt.class("QFSW.QC", "QuantumConsole").ok()?;
            Some(ConsoleMembers {
                mainframe: static_field(rt, mf, "code")?,
                console: rt.field(mf, "quantumConsole").ok()?,
                active: rt.field(qc, "<IsActive>k__BackingField").ok()?,
            })
        })();

        let teleport = (|| {
            let transform = rt.class("UnityEngine", "Transform").ok()?;
            let physics = rt.class("UnityEngine", "Physics").ok();
            let walker = rt.class("", "FPSRigidBodyWalker").ok();
            let camera = rt.class("", "RPGCamera").ok();
            Some(TeleportMembers {
                set_position: rt.method_with_params(transform, "set_position", &["UnityEngine.Vector3"]).ok()?,
                raycast: physics.and_then(|p| {
                    rt.method_with_params(
                        p,
                        "Raycast",
                        &[
                            "UnityEngine.Vector3",
                            "UnityEngine.Vector3",
                            "UnityEngine.RaycastHit&",
                            "System.Single",
                            "System.Int32",
                            "UnityEngine.QueryTriggerInteraction",
                        ],
                    )
                    .ok()
                }),
                walker: walker.and_then(|w| {
                    Some(WalkerMembers {
                        instance: static_field(rt, w, "code")?,
                        reset_parent: rt.method(w, "ResetParent", 0).ok()?,
                        water_state: ["isFloating", "isSwimming", "isUnderWater", "isInWater"].into_iter().filter_map(|n| rt.field(w, n).ok()).collect(),
                    })
                }),
                camera: camera.and_then(|c| Some((static_field(rt, c, "code")?, rt.field(c, "cam").ok()?))),
            })
        })();

        let assistant = rt.class("", "Assistant").ok();
        let boat = rt.class("", "Boat").ok();
        let entities = EntityMembers {
            assistant_position: assistant.and_then(|c| rt.field(c, "TransformPosition").ok()),
            assistant_default_name: assistant.and_then(|c| rt.field(c, "DefaultName").ok()),
            assistant_name: assistant.and_then(|c| rt.method(c, "get_AssistantName", 0).ok()),
            boat_position: boat.and_then(|c| rt.field(c, "TransformPosition").ok()),
            boat_display_name: boat.and_then(|c| rt.method(c, "get_DisplayName", 0).ok()),
            boat_destroyed: boat.and_then(|c| rt.field(c, "Destroied").ok()),
        };

        notes.push(format!(
            "Sunkenland: global={} world={} players={} chat={} console={} teleport={} ground_probe={}",
            global.is_some(),
            world.is_some(),
            dummy.is_some() && world.as_ref().is_some_and(|w| w.players.is_some()),
            chat.is_some(),
            console.is_some(),
            teleport.is_some(),
            teleport.as_ref().is_some_and(|t| t.raycast.is_some()),
        ));
        let save = (|| {
            let mf = rt.class("", "Mainframe").ok()?;
            let sm = rt.class("", "SaveManager").ok()?;
            let wm = rt.class("", "WorldManager").ok();
            Some(SaveMembers {
                mainframe: static_field(rt, mf, "code")?,
                save_manager: rt.method(mf, "get_SaveManager", 0).ok()?,
                world_valid: rt.method(sm, "get_IsCurrentWorldValid", 0).ok(),
                world_guid: rt.method(sm, "get_CurrentWorldGuid", 0).ok()?,
                world_name: rt.method(sm, "get_CurrentWorldName", 0).ok(),
                world_manager: rt.method(mf, "get_WorldManager", 0).ok(),
                session_world_name: wm.and_then(|c| rt.method(c, "get_WorldName", 0).ok()),
                session_world_id: wm.and_then(|c| rt.method(c, "get_WorldIdentifier", 0).ok()),
            })
        })();
        notes.push(format!(
            "Sunkenland world detection: own={} valid_check={} name={} joined={}",
            save.is_some(),
            save.as_ref().is_some_and(|s| s.world_valid.is_some()),
            save.as_ref().is_some_and(|s| s.world_name.is_some()),
            save.as_ref().is_some_and(|s| s.world_manager.is_some() && s.session_world_name.is_some()),
        ));
        let creature_members = creatures::Creatures::resolve(rt, world_class);
        let session_members = session::Session::resolve(rt);
        let mark_members = marks::MapMarks::resolve(rt, global_class);
        let flag_members = marks::Flags::resolve(rt);
        notes.push(format!("Sunkenland map marks: {} flags: {}", mark_members.is_some(), flag_members.is_some()));
        notes.push(format!(
            "Sunkenland creatures={} ({} types) session={}",
            creature_members.is_some(),
            creature_members.as_ref().map_or(0, |c| c.type_count()),
            session_members.is_some(),
        ));
        notes.push(format!(
            "Sunkenland markers: companions={} boats={} helicopters={}",
            entities.assistant_position.is_some(),
            entities.boat_position.is_some(),
            global.as_ref().is_some_and(|g| g.helicopters.is_some()),
        ));

        Some(Self {
            local,
            global,
            player,
            world,
            dummy,
            chat,
            console,
            teleport,
            entities,
            save,
            creature_members,
            mark_members,
            map_marks: Vec::new(),
            logged_marks: false,
            logged_mark_error: false,
            flag_members,
            flags: Vec::new(),
            last_flag_refresh: None,
            flag_refresh: FLAG_REFRESH,
            logged_flags: false,
            logged_flag_error: false,
            session_members,
            list_layout: None,
            notes,
            last_remote_refresh: None,
            others: Vec::new(),
            logged_first_scan: false,
            last_marker_refresh: None,
            markers: Vec::new(),
            names: HashMap::new(),
            names_at: None,
            last_world_refresh: None,
            game_world: None,
            session: None,
            last_creature_refresh: None,
            nearby: Vec::new(),
            logged_creatures: false,
        })
    }

    unsafe fn local_transform(&self, cx: &Cx) -> *mut ManagedObject {
        let fps: *mut ManagedObject = cx.rt.static_value(self.local.instance);
        if !cx.unity.is_alive(fps) {
            return std::ptr::null_mut();
        }
        self.local.transform.get(fps)
    }

    /// `Global.code.player`, the local `PlayerCharacter`.
    unsafe fn player_character(&self, cx: &Cx) -> *mut ManagedObject {
        let Some(g) = &self.global else { return std::ptr::null_mut() };
        let global: *mut ManagedObject = cx.rt.static_value(g.instance);
        if !cx.unity.is_alive(global) {
            return std::ptr::null_mut();
        }
        let player: *mut ManagedObject = g.player.get(global);
        if cx.unity.is_alive(player) {
            player
        } else {
            std::ptr::null_mut()
        }
    }

    /// The world that's loaded: your own (by its id) or the one you joined (by what its
    /// host publishes).
    unsafe fn read_world(&self, cx: &Cx, joined: bool) -> Option<GameWorld> {
        let s = self.save.as_ref()?;
        let mainframe: *mut ManagedObject = cx.rt.static_value(s.mainframe);
        if !cx.unity.is_alive(mainframe) {
            return None;
        }
        let call = |m: &Method, this: *mut ManagedObject| cx.rt.invoke(m, this, &mut []).ok().filter(|o| !o.is_null());
        let manager = call(&s.save_manager, mainframe)?;
        let own = s.world_valid.as_ref().is_none_or(|m| call(m, manager).and_then(|b| cx.rt.unbox::<bool>(b).ok()).unwrap_or(false));
        if own && !joined {
            let id = call(&s.world_guid, manager).and_then(|g| cx.rt.guid_or_string(g)).filter(|g| !g.is_empty() && !g.bytes().all(|b| b == b'0' || b == b'-'))?;
            let name = s.world_name.as_ref().and_then(|m| read_string(call(m, manager)?)).filter(|n| !n.is_empty());
            let kind = if self.session.is_some() { WorldKind::Hosted } else { WorldKind::SinglePlayer };
            return Some(GameWorld { id: format!("world:{id}"), name, kind });
        }
        // Your own world, still loading: nothing to report yet.
        if !joined {
            return None;
        }
        let world_manager = call(s.world_manager.as_ref()?, mainframe)?;
        let name = read_string(call(s.session_world_name.as_ref()?, world_manager)?).filter(|n| !n.is_empty())?;
        let id = s.session_world_id.as_ref().and_then(|m| read_string(call(m, world_manager)?)).filter(|i| !i.is_empty());
        Some(GameWorld { id: format!("joined:{}", id.as_deref().unwrap_or(&name)), name: Some(name), kind: WorldKind::Joined })
    }

    unsafe fn refresh_flags(&mut self, cx: &Cx) {
        let Some(members) = &self.flag_members else { return };
        let started = Instant::now();
        match members.read(cx.rt, cx.unity) {
            Ok(flags) => {
                if !self.logged_flags {
                    self.logged_flags = true;
                    let ms = started.elapsed().as_secs_f32() * 1000.0;
                    self.notes.push(format!("Flags on first read, in {ms:.2} ms: {:?}", flags.iter().map(|f| (&f.label, f.position)).collect::<Vec<_>>()));
                }
                self.flags = flags;
                let took = started.elapsed();
                self.flag_refresh = if took > SLOW_FLAG_SEARCH { SLOW_FLAG_REFRESH } else { FLAG_REFRESH };
                if took > SLOW_FLAG_SEARCH {
                    crate::filelog::write(&format!("slow flag search: {:.1} ms", took.as_secs_f32() * 1000.0));
                }
            }
            // Often only that the world isn't fully up yet: the next search tries again.
            Err(e) => {
                if !self.logged_flag_error {
                    self.logged_flag_error = true;
                    self.notes.push(format!("Flags unavailable: {e}"));
                }
            }
        }
    }

    unsafe fn refresh_map_marks(&mut self, cx: &Cx) {
        let Some(g) = &self.global else { return };
        let Some(members) = &self.mark_members else { return };
        let global: *mut ManagedObject = cx.rt.static_value(g.instance);
        if !cx.unity.is_alive(global) {
            return;
        }
        let sea = self.world_scene(cx).and_then(|(w, scene)| w.ocean_y.map(|f| f.get::<f32>(scene))).unwrap_or(0.0);
        match members.read(cx.rt, global, sea) {
            Ok(marks) => {
                if !self.logged_marks {
                    self.logged_marks = true;
                    self.notes.push(format!("Map marks on first read: {:?}", marks.iter().map(|m| (&m.label, m.position)).collect::<Vec<_>>()));
                }
                self.map_marks = marks;
            }
            Err(e) => {
                if !self.logged_mark_error {
                    self.logged_mark_error = true;
                    self.notes.push(format!("Map marks unavailable: {e}"));
                }
            }
        }
    }

    unsafe fn refresh_creatures(&mut self, cx: &Cx, around: Vector3) {
        let Some(scene) = self.world_scene(cx).map(|(_, s)| s) else { return };
        let Some(members) = self.creature_members.as_mut() else { return };
        match members.read(cx.rt, cx.unity, scene, around, CREATURE_RADIUS) {
            Ok(found) => {
                if !self.logged_creatures {
                    self.logged_creatures = true;
                    let names: Vec<String> = found.iter().map(|c| format!("{} ({:?})", c.name, c.kind)).collect();
                    self.notes.push(format!("Creatures around on first read: {} [{}]", found.len(), names.join(", ")));
                }
                self.nearby = found;
            }
            Err(e) => {
                self.notes.push(format!("Creatures disabled: {e}"));
                self.creature_members = None;
                self.nearby.clear();
            }
        }
    }

    unsafe fn world_scene(&self, cx: &Cx) -> Option<(&WorldMembers, *mut ManagedObject)> {
        let w = self.world.as_ref()?;
        let scene: *mut ManagedObject = cx.rt.static_value(w.instance);
        cx.unity.is_alive(scene).then_some((w, scene))
    }

    /// Elements of a `List<T>` field of `owner`.
    unsafe fn list(&mut self, cx: &Cx, owner: *mut ManagedObject, field: Option<Field>) -> Vec<*mut ManagedObject> {
        let Some(field) = field else { return Vec::new() };
        let list: *mut ManagedObject = field.get(owner);
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
        layout.elements(list, MAX_ENTITIES).into_iter().filter(|&o| cx.unity.is_alive(o)).collect()
    }

    unsafe fn read_local(&self, cx: &Cx) -> Option<LocalPlayer> {
        let transform = self.local_transform(cx);
        if !cx.unity.is_alive(transform) {
            return None;
        }
        let position = to_proto(cx.unity.position(cx.rt, transform).ok()?);
        if !position.is_finite() || position == Vec3::ZERO {
            return None;
        }
        // The player transform yaws with the camera (the game's own map uses it).
        let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
        Some(LocalPlayer { position, forward, layer: WorldLayer::Surface, camera: None })
    }

    /// Display name of an object, cached for [`NAME_REFRESH`].
    unsafe fn cached_name(&mut self, obj: *mut ManagedObject, read: impl FnOnce() -> Option<String>) -> Option<String> {
        if let Some(n) = self.names.get(&(obj as usize)) {
            return Some(n.clone());
        }
        let name = read().filter(|n| !n.trim().is_empty())?;
        self.names.insert(obj as usize, name.clone());
        Some(name)
    }

    unsafe fn refresh_remote(&mut self, cx: &Cx) {
        let Some((w, scene)) = self.world_scene(cx) else {
            self.others.clear();
            return;
        };
        let players_field = w.players;
        let Some(d) = &self.dummy else { return };
        let (transform_f, name_m, cached_f) = (d.transform, d.name.clone(), d.cached_name);
        let player = self.player_character(cx);
        let local_dummy: *mut ManagedObject = match (self.player.dummy, player.is_null()) {
            (Some(f), false) => f.get(player),
            _ => std::ptr::null_mut(),
        };
        let local_transform = self.local_transform(cx);

        let mut others = Vec::new();
        for dummy in self.list(cx, scene, players_field) {
            if dummy == local_dummy {
                continue;
            }
            let transform: *mut ManagedObject = transform_f.get(dummy);
            if transform == local_transform || !cx.unity.is_alive(transform) {
                continue;
            }
            let Ok(pos) = cx.unity.position(cx.rt, transform) else { continue };
            let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
            let name = self
                .cached_name(dummy, || {
                    let networked = name_m.as_ref().and_then(|m| read_string(cx.rt.invoke(m, dummy, &mut []).ok()?));
                    networked.or_else(|| cached_f.and_then(|f| read_string(f.get(dummy))))
                })
                .unwrap_or_else(|| "Player".into());
            others.push(RemotePlayer { id: format!("{:x}", dummy as usize), name, color: None, position: to_proto(pos), forward });
        }
        if !self.logged_first_scan {
            self.logged_first_scan = true;
            self.notes.push(format!("Remote players seen on first scan: {}", others.len()));
        }
        self.others = others;
    }

    unsafe fn refresh_markers(&mut self, cx: &Cx) {
        let mut markers = Vec::new();
        if let Some((w, scene)) = self.world_scene(cx) {
            let (assistants_f, boats_f) = (w.assistants, w.boats);

            let (pos_f, default_f, name_m) = (self.entities.assistant_position, self.entities.assistant_default_name, self.entities.assistant_name.clone());
            if let Some(pos_f) = pos_f {
                for a in self.list(cx, scene, assistants_f) {
                    let position = to_proto(pos_f.get::<Vector3>(a));
                    if !position.is_finite() || position == Vec3::ZERO {
                        continue;
                    }
                    let label = self.cached_name(a, || {
                        name_m.as_ref().and_then(|m| network_string(cx.rt, m, a)).or_else(|| default_f.and_then(|f| read_string(f.get(a))))
                    });
                    markers.push(Marker { id: format!("assistant:{:x}", a as usize), kind: MarkerKind::Companion, label, icon: None, position });
                }
            }

            let (pos_f, name_m, destroyed_f) = (self.entities.boat_position, self.entities.boat_display_name.clone(), self.entities.boat_destroyed);
            if let Some(pos_f) = pos_f {
                for b in self.list(cx, scene, boats_f) {
                    if destroyed_f.is_some_and(|f| f.get::<bool>(b)) {
                        continue;
                    }
                    let position = to_proto(pos_f.get::<Vector3>(b));
                    if !position.is_finite() || position == Vec3::ZERO {
                        continue;
                    }
                    let label = self.cached_name(b, || name_m.as_ref().and_then(|m| read_string(cx.rt.invoke(m, b, &mut []).ok()?))).or(Some("Boat".into()));
                    markers.push(Marker { id: format!("boat:{:x}", b as usize), kind: MarkerKind::Locator, label, icon: Some(BOAT_ICON.into()), position });
                }
            }
        }

        let helicopters = self.global.as_ref().and_then(|g| g.helicopters);
        if let Some(g) = &self.global {
            let global: *mut ManagedObject = cx.rt.static_value(g.instance);
            if cx.unity.is_alive(global) {
                for h in self.list(cx, global, helicopters) {
                    let Ok(t) = cx.unity.component_transform(cx.rt, h) else { continue };
                    let Ok(pos) = cx.unity.position(cx.rt, t) else { continue };
                    markers.push(Marker {
                        id: format!("helicopter:{:x}", h as usize),
                        kind: MarkerKind::Locator,
                        label: Some("Helicopter".into()),
                        icon: Some(HELICOPTER_ICON.into()),
                        position: to_proto(pos),
                    });
                }
            }
        }
        self.markers = markers;
    }

    /// World y of the sea surface (`WorldScene.oceanObjectY`).
    unsafe fn sea_level(&self, cx: &Cx) -> f32 {
        self.world_scene(cx).and_then(|(w, scene)| w.ocean_y.map(|f| f.get::<f32>(scene))).unwrap_or(0.0)
    }

    /// Height to stand at above `(x, z)`: the first solid surface below the probe, or
    /// the sea surface when that is higher (open water, sunken ruins).
    unsafe fn surface_height(&self, cx: &Cx, x: f32, z: f32) -> Result<f32, String> {
        let t = self.teleport.as_ref().ok_or("teleport is unavailable")?;
        let sea = self.sea_level(cx);
        let Some(raycast) = &t.raycast else { return Ok(sea) };
        let mut origin = Vector3 { x, y: sea + PROBE_HEIGHT, z };
        let mut down = Vector3 { x: 0.0, y: -1.0, z: 0.0 };
        let mut hit = RaycastHit::default();
        let (mut distance, mut mask, mut triggers) = (PROBE_DISTANCE, DEFAULT_RAYCAST_LAYERS, QUERY_TRIGGERS_IGNORE);
        let mut args = [
            &mut origin as *mut Vector3 as *mut c_void,
            &mut down as *mut Vector3 as *mut c_void,
            // `out` parameters, like by-value structs, take a pointer to the storage.
            &mut hit as *mut RaycastHit as *mut c_void,
            &mut distance as *mut f32 as *mut c_void,
            &mut mask as *mut i32 as *mut c_void,
            &mut triggers as *mut i32 as *mut c_void,
        ];
        let hit_any: bool = cx.rt.unbox(cx.rt.invoke(raycast, std::ptr::null_mut(), &mut args).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        Ok(if hit_any && hit.point.y.is_finite() { hit.point.y.max(sea) } else { sea })
    }
}

/// Reads a Fusion `NetworkString<T>` property as a string.
unsafe fn network_string(rt: &Runtime, getter: &Method, obj: *mut ManagedObject) -> Option<String> {
    let boxed = rt.invoke(getter, obj, &mut []).ok().filter(|b| !b.is_null())?;
    let value = rt.method(rt.class_of(boxed)?, "get_Value", 0).ok()?;
    read_string(rt.invoke_boxed(&value, boxed, &mut []).ok()?)
}

impl Provider for Sunkenland {
    fn game_id(&self) -> &'static str {
        GAME_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            live_location: true,
            teleport: self.teleport.is_some(),
            other_players: self.dummy.is_some() && self.world.as_ref().is_some_and(|w| w.players.is_some()),
            hotkeys: true,
            game_map: false,
        }
    }

    fn frame_hooks(&self) -> &'static [HookTarget] {
        FRAME_HOOKS
    }

    fn map_opened(&mut self) {
        self.last_flag_refresh = None;
    }

    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    unsafe fn snapshot(&mut self, cx: &Cx, frame: u64) -> WorldSnapshot {
        let local = self.read_local(cx);
        let now = Instant::now();
        if local.is_none() {
            self.others.clear();
            self.markers.clear();
            self.names.clear();
            self.game_world = None;
            self.session = None;
            self.nearby.clear();
            self.map_marks.clear();
            self.flags.clear();
            self.last_flag_refresh = None;
        } else {
            if self.last_world_refresh.is_none_or(|t| now - t >= WORLD_REFRESH) {
                self.last_world_refresh = Some(now);
                let state = self.session_members.as_mut().and_then(|m| m.read(cx.rt));
                let joined = state.as_ref().is_some_and(|s| s.joined);
                let session = state.and_then(|s| s.session);
                if session != self.session {
                    self.notes.push(match &session {
                        // Never the id: it's the room's password, and logs get shared.
                        Some(s) => format!("Multiplayer session joined as {:?}", s.player_name),
                        None => "Not in a multiplayer session".into(),
                    });
                    self.session = session;
                }
                self.refresh_map_marks(cx);
                let world = self.read_world(cx, joined);
                if world != self.game_world {
                    self.notes.push(match &world {
                        Some(w) => format!("World: {} {:?} ({:?})", w.id, w.name, w.kind),
                        None => "World: none".into(),
                    });
                    self.game_world = world;
                }
            }
            if self.names_at.is_none_or(|t| now - t >= NAME_REFRESH) {
                self.names_at = Some(now);
                self.names.clear();
            }
            if self.last_flag_refresh.is_none_or(|t| now - t >= self.flag_refresh) {
                self.last_flag_refresh = Some(now);
                self.refresh_flags(cx);
            }
            if self.last_creature_refresh.is_none_or(|t| now - t >= CREATURE_REFRESH) {
                self.last_creature_refresh = Some(now);
                if let Some(p) = &local {
                    self.refresh_creatures(cx, to_unity(p.position));
                }
            }
            if self.last_remote_refresh.is_none_or(|t| now - t >= REMOTE_REFRESH) {
                self.last_remote_refresh = Some(now);
                self.refresh_remote(cx);
            }
            if self.last_marker_refresh.is_none_or(|t| now - t >= MARKER_REFRESH) {
                self.last_marker_refresh = Some(now);
                self.refresh_markers(cx);
            }
        }
        WorldSnapshot { frame, local, others: self.others.clone(), markers: self.markers.iter().chain(&self.map_marks).chain(&self.flags).cloned().collect(), creatures: self.nearby.clone(), session: self.session.clone(), world: self.game_world.clone(), ..Default::default() }
    }

    unsafe fn teleport(&mut self, cx: &Cx, target: &TeleportTarget) -> Result<(), String> {
        let transform = self.local_transform(cx);
        if !cx.unity.is_alive(transform) {
            return Err("not in a world".into());
        }
        let t = self.teleport.as_ref().ok_or("teleport is not available in this game version")?;
        let player = self.player_character(cx);
        if !player.is_null() {
            if self.player.is_dead.as_ref().is_some_and(|m| cx.rt.invoke(m, player, &mut []).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(false)) {
                return Err("you are dead".into());
            }
            let occupied = [self.player.driving, self.player.seat, self.player.helicopter]
                .into_iter()
                .flatten()
                .any(|f| cx.unity.is_alive(f.get::<*mut ManagedObject>(player)));
            if occupied {
                return Err("leave the boat, seat or helicopter first".into());
            }
        }

        let mut dest = match target {
            TeleportTarget::Position { position, snap_to_ground, .. } => {
                let mut p = to_unity(*position);
                let sea = self.sea_level(cx);
                if *snap_to_ground {
                    p.y = self.surface_height(cx, p.x, p.z)? + GROUND_CLEARANCE;
                } else if p.y < sea - MAX_TELEPORT_DEPTH {
                    // Deep targets (sunken locations, underwater loot) would drown the
                    // player: arrive at the surface right above and dive from there.
                    p.y = sea + GROUND_CLEARANCE;
                }
                p
            }
            TeleportTarget::Player { id } => {
                let position = self
                    .others
                    .iter()
                    .find(|o| &o.id == id)
                    .map(|o| o.position)
                    .or_else(|| self.markers.iter().chain(&self.map_marks).chain(&self.flags).find(|m| &m.id == id).map(|m| m.position))
                    .ok_or("player not found")?;
                let mut p = to_unity(position);
                p.x += 1.0;
                p.y += 0.5;
                p
            }
        };
        if !(dest.x.is_finite() && dest.y.is_finite() && dest.z.is_finite()) {
            return Err("invalid destination".into());
        }

        // Standing on a boat deck parents the player to the boat; detach first so the
        // boat doesn't carry them off.
        if let Some(w) = &t.walker {
            let walker: *mut ManagedObject = cx.rt.static_value(w.instance);
            if cx.unity.is_alive(walker) {
                cx.rt.invoke(&w.reset_parent, walker, &mut []).map_err(|e| format!("leaving the boat deck failed: {e}"))?;
                // While floating, the walker pulls the player toward the water height with
                // no collision, which would drag them through the ground at the
                // destination on the first frame. The game clears this when it moves the
                // player out of the water, and sets it again next frame if they're in it.
                for field in &w.water_state {
                    field.set(walker, false);
                }
            }
        }
        // The game takes the water depth from the camera, which follows a frame later: left
        // behind, it would put the player back in the water for that frame.
        if let Some((instance, cam)) = &t.camera {
            let rig: *mut ManagedObject = cx.rt.static_value(*instance);
            let camera = if cx.unity.is_alive(rig) { cam.get::<*mut ManagedObject>(rig) } else { std::ptr::null_mut() };
            if cx.unity.is_alive(camera) {
                if let Ok(camera) = cx.unity.component_transform(cx.rt, camera) {
                    if let (Ok(from), Ok(seen)) = (cx.unity.position(cx.rt, transform), cx.unity.position(cx.rt, camera)) {
                        let mut at = Vector3 { x: seen.x + dest.x - from.x, y: seen.y + dest.y - from.y, z: seen.z + dest.z - from.z };
                        let mut args = [&mut at as *mut Vector3 as *mut c_void];
                        // It catches up by itself, so a failure here doesn't stop the teleport.
                        let _ = cx.rt.invoke(&t.set_position, camera, &mut args);
                    }
                }
            }
        }
        let mut args = [&mut dest as *mut Vector3 as *mut c_void];
        cx.rt.invoke(&t.set_position, transform, &mut args).map(|_| ()).map_err(|e| e.to_string())
    }

    unsafe fn input_blocked(&mut self, cx: &Cx) -> bool {
        let chat = (|| {
            let (c, g) = (self.chat.as_ref()?, self.global.as_ref()?);
            let global: *mut ManagedObject = cx.rt.static_value(g.instance);
            let combat: *mut ManagedObject = g.ui_combat.filter(|_| cx.unity.is_alive(global))?.get(global);
            if !cx.unity.is_alive(combat) {
                return None;
            }
            let chat: *mut ManagedObject = c.chat.get(combat);
            cx.unity.is_alive(chat).then(|| c.active.get::<bool>(chat))
        })();
        let console = (|| {
            let c = self.console.as_ref()?;
            let mainframe: *mut ManagedObject = cx.rt.static_value(c.mainframe);
            if !cx.unity.is_alive(mainframe) {
                return None;
            }
            let console: *mut ManagedObject = c.console.get(mainframe);
            cx.unity.is_alive(console).then(|| c.active.get::<bool>(console))
        })();
        chat.unwrap_or(false) || console.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{c_char, c_void, CString};

    use unity_runtime::{BackendKind, Runtime};
    use windows::core::{s, HSTRING};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    use super::Sunkenland;
    use crate::provider::Provider;

    /// Hosts the game's own Mono runtime, loads its managed assemblies (no engine, so
    /// nothing runs) and resolves every member the provider uses by name.
    #[test]
    #[ignore = "needs a Sunkenland install: set SUNKENLAND_DIR and run with --ignored"]
    fn resolves_members_against_game_assemblies() {
        let game = std::env::var("SUNKENLAND_DIR").expect("SUNKENLAND_DIR");
        let managed = format!("{game}/Sunkenland_Data/Managed");
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
        let mut p = Sunkenland::detect(&rt).expect("provider detected");
        let notes = p.take_notes().join("\n");
        println!("{notes}");
        for want in ["global=true", "world=true", "players=true", "chat=true", "console=true", "teleport=true", "ground_probe=true", "companions=true", "boats=true", "helicopters=true", "own=true", "valid_check=true", "name=true", "joined=true", "creatures=true (10 types)", "session=true", "map marks: true flags: true"] {
            assert!(notes.contains(want), "missing {want} in:\n{notes}");
        }
        let caps = p.capabilities();
        assert!(caps.teleport && caps.other_players);
        for t in crate::hook::ENGINE_TARGETS.iter().chain(p.frame_hooks()) {
            let class = rt.class(t.namespace, t.class).unwrap_or_else(|e| panic!("{e}"));
            let n = if t.shape == crate::hook::Shape::Static3 { 3 } else { 0 };
            rt.method(class, t.method, n).unwrap_or_else(|e| panic!("{e}"));
        }
    }
}
