//! Sons of the Forest (Endnight, Unity 2022.2 IL2CPP).
//!
//! Members used (all looked up by name, see `docs/games/sons-of-the-forest.md`):
//! - `TheForest.Utils.LocalPlayer`: player transform, camera transform, `IsInCaves`,
//!   `TeleportTo(Vector3)` (the game's own teleport, works for non-host clients
//!   because each client owns its player).
//! - `PlayerTracker.AllPlayers`: every connected player.
//! - `Sons.Multiplayer.Client.CoopPlayerRemoteSetup`: remote player name and color.
//! - `TheForest.UI.Multiplayer.ChatBox.IsChatOpen`: suppress hotkeys while typing.
//! - `Sons.Areas.CaveSystem`: cave extents, forced enter/exit for teleports.
//! - `Sons.Gameplay.GPS.GPSLocator._activeLocators`: live GPS markers (Kelvin, placed locators, vehicles).
//! - `Sons.Ai.Vail.VailActorManager._activeActors`: creatures around the player.
//! - `Sons.Multiplayer.CoopLobbyManager`, `GameServerManager`: the multiplayer session.
//! - `GameSetupManager.GetSelectedSaveId()`, `GetSaveGameType()`: the loaded save.

mod astar;
mod bag;
mod cave_scan;
mod caves;
mod companions;
mod creatures;
mod environment;
mod export;
mod gps;
mod session;
mod world;

use std::ffi::c_void;
use std::time::{Duration, Instant};

use game_maps_protocol::{Capabilities, CompanionOrderOption, CompanionStatus, Creature, Environment, GameWorld, WorldKind, LocalPlayer, Marker, MultiplayerSession, PathInfo, RemotePlayer, TeleportTarget, Vec3, WorldExport, WorldLayer, WorldSnapshot};
use unity_runtime::{read_string, Class, Color, Field, ListLayout, ManagedObject, Method, Runtime, Vector3};

use super::{to_proto, to_unity, Cx, Provider};
use crate::hook::{target, HookTarget, Shape};

const GAME_ID: &str = "sons-of-the-forest";

/// `EventSystem.Update` doesn't run during gameplay in this game; these do.
const FRAME_HOOKS: &[HookTarget] = &[
    target("Sons.Input", "InputSystem", "LateUpdate", Shape::Instance0),
    target("", "FirstPersonCharacter", "Update", Shape::Instance0),
];
const REMOTE_REFRESH: Duration = Duration::from_millis(100);
const LAYER_REFRESH: Duration = Duration::from_millis(250);
const MARKER_REFRESH: Duration = Duration::from_millis(250);
const CREATURE_REFRESH: Duration = Duration::from_millis(250);
const SESSION_REFRESH: Duration = Duration::from_secs(2);
const STATUS_REFRESH: Duration = Duration::from_secs(1);
/// How long the player has to be missing before a new game counts as left.
const LEFT_WORLD: Duration = Duration::from_secs(5);
/// Numbers worlds loaded without a save id (new games until they're saved).
static UNSAVED_WORLDS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
/// Creatures farther than this from the player are left out (the game simulates some
/// actors well beyond what matters around you).
const CREATURE_RADIUS: f32 = 200.0;
const MAX_PLAYERS: usize = 64;
/// Height added above sampled terrain so teleports never land inside the ground.
const GROUND_CLEARANCE: f32 = 1.0;
/// Depth below the terrain surface that counts as underground. Teleports into caves
/// skip the game's entrance triggers, so `IsInCaves` alone can miss them.
const UNDERGROUND_DEPTH: f32 = 4.0;
/// Margin around the objects known to be in a cave, used as its scan bounds when the
/// cave's extents aren't colliders.
const CAVE_BOUNDS_MARGIN: f32 = 40.0;
/// A cave's collision loads a few frames after it's switched on. A teleport into one
/// holds the player at the spot until there's floor under it, for at most this long.
const CAVE_LOAD_WAIT: Duration = Duration::from_secs(5);
/// How far under a teleport's destination to look for that floor.
const FLOOR_BELOW: f32 = 4.0;

/// A teleport into a cave whose floor hasn't loaded yet.
struct PendingLanding {
    dest: Vector3,
    until: Instant,
}

/// A world export in progress: entities are collected at once, caves over many frames.
struct ExportJob {
    export: WorldExport,
    caves: Option<cave_scan::CaveScanJob>,
}

struct LocalMembers {
    instance: Option<Field>,
    transform: Field,
    camera: Option<Field>,
    is_in_caves: Option<Method>,
    teleport: Option<Method>,
    /// `LocalPlayer.Stats` and its `IsDying()` / `IsDead()`.
    stats: Option<(Field, Method, Method)>,
}

struct RemoteMembers {
    tracker_instance: Field,
    all_players: Field,
    game_object: Class,
    component: Class,
    remote_setup: Class,
    name: Option<Field>,
    color: Option<Field>,
}

struct TerrainMembers {
    active: Method,
    sample_height: Method,
}

pub struct SonsOfTheForest {
    companions: Option<companions::Companions>,
    environment: Option<environment::EnvironmentMembers>,
    companion_status: Vec<CompanionStatus>,
    world_status: Option<Environment>,
    bag: Option<bag::DroppedBag>,
    dropped_bag: Option<Vec3>,
    last_status_refresh: Option<Instant>,
    local: LocalMembers,
    remote: Option<RemoteMembers>,
    terrain: Option<TerrainMembers>,
    chat_open: Option<Field>,
    caves: Option<caves::Caves>,
    export_job: Option<ExportJob>,
    landing: Option<PendingLanding>,
    gps: Option<gps::GpsMarkers>,
    creatures: Option<creatures::Creatures>,
    session_members: Option<session::Session>,
    world_members: Option<world::World>,
    astar: Option<astar::AStar>,
    /// Route timings reported so far (the first few go to the log).
    routes_logged: u32,
    /// `List<T>` field layout, resolved from the first list instance we see.
    list_layout: Option<ListLayout>,
    notes: Vec<String>,

    last_layer_check: Option<Instant>,
    layer: WorldLayer,
    last_remote_refresh: Option<Instant>,
    others: Vec<RemotePlayer>,
    logged_player_shape: bool,
    /// Transforms of the remote players, from the last remote refresh.
    remote_transforms: Vec<usize>,
    last_marker_refresh: Option<Instant>,
    markers: Vec<Marker>,
    last_creature_refresh: Option<Instant>,
    nearby: Vec<Creature>,
    logged_creatures: bool,
    last_session_refresh: Option<Instant>,
    world: Option<GameWorld>,
    logged_world_miss: bool,
    /// When the player went missing, while they are.
    no_player_since: Option<Instant>,
    /// Id for the world loaded now while it has no save id (see `refresh_session`).
    unsaved_world: Option<String>,
    session: Option<MultiplayerSession>,
}

fn first_field(rt: &Runtime, class: Class, names: &[&str]) -> Option<Field> {
    names.iter().find_map(|n| rt.field(class, n).ok())
}

impl SonsOfTheForest {
    pub fn detect(rt: &Runtime) -> Option<Self> {
        let lp = rt.class("TheForest.Utils", "LocalPlayer").ok()?;
        let mut notes = Vec::new();

        let transform = first_field(rt, lp, &["<Transform>k__BackingField", "_transform"])?;
        let local = LocalMembers {
            instance: rt.field(lp, "_instance").ok(),
            transform,
            camera: first_field(rt, lp, &["<MainCamTr>k__BackingField", "_mainCamTr"]),
            is_in_caves: rt.method(lp, "get_IsInCaves", 0).ok(),
            teleport: rt.method_with_params(lp, "TeleportTo", &["UnityEngine.Vector3"]).ok(),
            stats: (|| {
                let stats = rt.class("", "PlayerStats").ok()?;
                Some((first_field(rt, lp, &["<Stats>k__BackingField", "_stats"])?, rt.method(stats, "IsDying", 0).ok()?, rt.method(stats, "IsDead", 0).ok()?))
            })(),
        };
        notes.push(format!("Death detection: {}", local.stats.is_some()));
        notes.push(format!(
            "LocalPlayer: transform(static={}) camera={} caves={} teleport={}",
            local.transform.is_static,
            local.camera.is_some(),
            local.is_in_caves.is_some(),
            local.teleport.as_ref().map(|m| if m.is_static { "static" } else { "instance" }).unwrap_or("missing"),
        ));

        let remote = (|| {
            let tracker = rt.class("", "PlayerTracker").ok()?;
            let setup = rt.class("Sons.Multiplayer.Client", "CoopPlayerRemoteSetup").ok()?;
            Some(RemoteMembers {
                tracker_instance: rt.field(tracker, "_instance").ok()?,
                all_players: rt.field(tracker, "AllPlayers").ok()?,
                game_object: rt.class("UnityEngine", "GameObject").ok()?,
                component: rt.class("UnityEngine", "Component").ok()?,
                remote_setup: setup,
                name: rt.field(setup, "_cachedPlayerName").ok(),
                color: rt.field(setup, "_cachedPlayerColor").ok(),
            })
        })();
        notes.push(match &remote {
            Some(r) => format!("Remote players: ok (name={} color={})", r.name.is_some(), r.color.is_some()),
            None => "Remote players: unavailable".into(),
        });

        let terrain = (|| {
            let t = rt.class("UnityEngine", "Terrain").ok()?;
            Some(TerrainMembers {
                active: rt.method(t, "get_activeTerrain", 0).ok()?,
                sample_height: rt.method_with_params(t, "SampleHeight", &["UnityEngine.Vector3"]).ok()?,
            })
        })();
        notes.push(format!("Terrain sampling: {}", terrain.is_some()));

        let chat_open = rt
            .class("TheForest.UI.Multiplayer", "ChatBox")
            .ok()
            .and_then(|c| rt.field(c, "<IsChatOpen>k__BackingField").ok())
            .filter(|f| f.is_static);
        notes.push(format!("Chat detection: {}", chat_open.is_some()));

        let caves = caves::Caves::resolve(rt);
        notes.push(format!("Cave systems: {}", caves.is_some()));
        let gps = gps::GpsMarkers::resolve(rt);
        notes.push(format!("GPS markers: {}", gps.is_some()));
        let creatures = creatures::Creatures::resolve(rt);
        notes.push(format!("Creatures: {}", creatures.is_some()));
        let session_members = session::Session::resolve(rt);
        notes.push(format!("Multiplayer session: {}", session_members.is_some()));
        let world_members = match world::World::resolve(rt) {
            Ok((w, namespace)) => {
                notes.push(format!("Save detection: ok ({namespace}.GameSetupManager)"));
                Some(w)
            }
            Err(e) => {
                notes.push(format!("Save detection: unavailable ({e})"));
                None
            }
        };
        let astar = match astar::AStar::resolve(rt) {
            Ok(a) => {
                notes.push("Routes (A* Pathfinding): ok".into());
                Some(a)
            }
            Err(e) => {
                notes.push(format!("Routes (A* Pathfinding): unavailable ({e})"));
                None
            }
        };

        let companions = match companions::Companions::resolve(rt) {
            Ok(c) => {
                notes.push("Companions: ok".into());
                Some(c)
            }
            Err(e) => {
                notes.push(format!("Companions: unavailable ({e})"));
                None
            }
        };
        let environment = environment::EnvironmentMembers::resolve(rt);
        notes.push(format!("Time and weather: {}", environment.is_some()));
        let bag = bag::DroppedBag::resolve(rt);
        notes.push(format!("Dropped bag: {}", bag.is_some()));

        Some(Self {
            companions,
            environment,
            companion_status: Vec::new(),
            world_status: None,
            bag,
            dropped_bag: None,
            last_status_refresh: None,
            local,
            remote,
            terrain,
            chat_open,
            caves,
            export_job: None,
            landing: None,
            gps,
            creatures,
            session_members,
            world_members,
            astar,
            routes_logged: 0,
            list_layout: None,
            notes,
            last_layer_check: None,
            layer: WorldLayer::Surface,
            last_remote_refresh: None,
            others: Vec::new(),
            logged_player_shape: false,
            remote_transforms: Vec::new(),
            last_marker_refresh: None,
            markers: Vec::new(),
            last_creature_refresh: None,
            nearby: Vec::new(),
            logged_creatures: false,
            last_session_refresh: None,
            world: None,
            logged_world_miss: false,
            no_player_since: None,
            unsaved_world: None,
            session: None,
        })
    }

    unsafe fn local_instance(&self, cx: &Cx) -> *mut ManagedObject {
        match self.local.instance {
            Some(f) if f.is_static => cx.rt.static_value(f),
            _ => std::ptr::null_mut(),
        }
    }

    unsafe fn local_transform(&self, cx: &Cx) -> *mut ManagedObject {
        let inst = self.local_instance(cx);
        cx.rt.read_field(self.local.transform, inst)
    }

    unsafe fn read_local(&mut self, cx: &Cx) -> Option<LocalPlayer> {
        let transform = self.local_transform(cx);
        if !cx.unity.is_alive(transform) {
            return None;
        }
        let pos = cx.unity.position(cx.rt, transform).ok()?;
        let position = to_proto(pos);
        if !position.is_finite() || position == Vec3::ZERO {
            return None;
        }

        let inst = self.local_instance(cx);
        let forward = self
            .local
            .camera
            .map(|f| cx.rt.read_field::<*mut ManagedObject>(f, inst))
            .and_then(|cam| cx.unity.rotation(cx.rt, cam).ok())
            .map(|q| to_proto(q.forward()));

        let now = Instant::now();
        if self.last_layer_check.is_none_or(|t| now - t >= LAYER_REFRESH) {
            self.last_layer_check = Some(now);
            let in_caves = self.local.is_in_caves.as_ref().is_some_and(|m| {
                let target = if m.is_static { std::ptr::null_mut() } else { inst };
                cx.rt.invoke(m, target, &mut []).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(false)
            });
            let below_ground = self.ground_height(cx, pos).is_some_and(|g| g - pos.y > UNDERGROUND_DEPTH);
            self.layer = if in_caves || below_ground { WorldLayer::Caves } else { WorldLayer::Surface };
        }

        Some(LocalPlayer { position, forward, layer: self.layer, camera: None })
    }

    unsafe fn refresh_remote(&mut self, cx: &Cx, local_transform: *mut ManagedObject) {
        let Some(r) = &self.remote else { return };
        let tracker: *mut ManagedObject = cx.rt.static_value(r.tracker_instance);
        if !cx.unity.is_alive(tracker) {
            self.others.clear();
            return;
        }
        let list: *mut ManagedObject = cx.rt.read_field(r.all_players, tracker);
        if list.is_null() {
            self.others.clear();
            return;
        }
        let layout = match self.list_layout {
            Some(l) => l,
            None => match ListLayout::of(cx.rt, list) {
                Ok(l) => *self.list_layout.insert(l),
                Err(_) => {
                    self.remote = None;
                    return;
                }
            },
        };
        let items = layout.elements(list, MAX_PLAYERS);

        let mut others = Vec::with_capacity(items.len());
        let mut transforms = Vec::with_capacity(items.len());
        for elem in items {
            let Some(class) = cx.rt.class_of(elem) else { continue };
            if !cx.unity.is_alive(elem) {
                continue;
            }
            let go = if cx.rt.is_assignable(r.game_object, class) {
                elem
            } else if cx.rt.is_assignable(r.component, class) {
                match cx.unity.component_game_object(cx.rt, elem) {
                    Ok(go) => go,
                    Err(_) => continue,
                }
            } else {
                continue;
            };
            let Ok(transform) = cx.unity.game_object_transform(cx.rt, go) else { continue };
            transforms.push(transform as usize);
            if transform == local_transform {
                continue;
            }
            // Only remote players carry this component; it also excludes the local player.
            let Ok(Some(setup)) = cx.unity.get_component(cx.rt, go, r.remote_setup) else { continue };
            let Ok(pos) = cx.unity.position(cx.rt, transform) else { continue };
            let forward = cx.unity.rotation(cx.rt, transform).ok().map(|q| to_proto(q.forward()));
            let name = r
                .name
                .and_then(|f| read_string(f.get::<*mut ManagedObject>(setup)))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Player".into());
            let color = r.color.map(|f| f.get::<Color>(setup).to_srgb8());
            others.push(RemotePlayer {
                id: format!("{:x}", go as usize),
                name,
                color,
                position: to_proto(pos),
                forward,
            });
        }

        if !self.logged_player_shape {
            self.logged_player_shape = true;
            self.notes.push(format!("Remote players seen on first scan: {}", others.len()));
        }
        self.others = others;
        self.remote_transforms = transforms;
    }

    unsafe fn refresh_markers(&mut self, cx: &Cx) {
        let mut players: Vec<*mut ManagedObject> = self.remote_transforms.iter().map(|&t| t as *mut ManagedObject).collect();
        players.push(self.local_transform(cx));
        let Some(gps) = self.gps.as_mut() else { return };
        match gps.read(cx.rt, cx.unity, &players) {
            Ok(markers) => self.markers = markers,
            Err(e) => {
                self.notes.push(format!("GPS markers disabled: {e}"));
                self.gps = None;
                self.markers.clear();
            }
        }
    }

    unsafe fn refresh_session(&mut self, cx: &Cx) {
        if let Some(w) = &self.world_members {
            let world = match w.read(cx.rt) {
                Some(world) => Some(world),
                // The game clears the save id while quitting: still the same world.
                None if self.world.as_ref().is_some_and(|w| !w.id.starts_with("unsaved:")) => self.world.clone(),
                None => {
                    // A new game has no save id until it's first saved: report it as a
                    // world of its own, so nothing lands in another world meanwhile.
                    if !self.logged_world_miss {
                        self.logged_world_miss = true;
                        self.notes.push(format!("World: no save id yet ({})", w.describe(cx.rt)));
                    }
                    let id = self.unsaved_world.get_or_insert_with(|| {
                        let n = UNSAVED_WORLDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        format!("unsaved:{}-{n}", std::process::id())
                    });
                    let kind = if w.is_client(cx.rt) {
                        WorldKind::Joined
                    } else if self.session.is_some() {
                        WorldKind::Hosted
                    } else {
                        WorldKind::SinglePlayer
                    };
                    Some(GameWorld { id: id.clone(), name: None, kind })
                }
            };
            if world != self.world {
                self.notes.push(match &world {
                    Some(w) => format!("World: {} ({:?})", w.id, w.kind),
                    None => "World: none".into(),
                });
                self.world = world;
            }
        }
        let Some(members) = &self.session_members else { return };
        let session = members.id(cx.rt).map(|id| MultiplayerSession { id, player_name: members.player_name(cx.rt) });
        if session != self.session {
            self.notes.push(match &session {
                // Never the id: it's the room's password, and logs get shared.
                Some(s) => format!("Multiplayer session joined as {:?}", s.player_name),
                None => "Not in a multiplayer session".into(),
            });
            self.session = session;
        }
    }

    unsafe fn is_dying(&self, cx: &Cx) -> bool {
        let Some((field, dying, dead)) = &self.local.stats else { return false };
        let stats: *mut ManagedObject = cx.rt.read_field(*field, self.local_instance(cx));
        if !cx.unity.is_alive(stats) {
            return false;
        }
        let flag = |m: &Method| cx.rt.invoke(m, stats, &mut []).and_then(|b| cx.rt.unbox::<bool>(b)).unwrap_or(false);
        flag(dying) || flag(dead)
    }

    unsafe fn refresh_status(&mut self, cx: &Cx) {
        self.world_status = self.environment.as_ref().and_then(|e| e.read(cx.rt, cx.unity));
        if let (Some(c), Some(creatures)) = (self.companions.as_mut(), self.creatures.as_ref()) {
            self.companion_status = c.read(cx.rt, cx.unity, creatures.kelvin, creatures.virginia);
        }
        let dying = self.is_dying(cx);
        if let Some(bag) = self.bag.as_mut() {
            let dropped = bag.read(cx.rt, cx.unity, dying);
            if dropped != self.dropped_bag {
                self.notes.push(match dropped {
                    Some(p) => format!("Dropped bag at {:.1}, {:.1}, {:.1}", p.x, p.y, p.z),
                    None => "No dropped bag".into(),
                });
                self.dropped_bag = dropped;
            }
        }
    }

    unsafe fn refresh_creatures(&mut self, cx: &Cx, around: Vector3) {
        let Some(creatures) = self.creatures.as_mut() else { return };
        match creatures.read(cx.rt, cx.unity, around, CREATURE_RADIUS) {
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
                self.creatures = None;
                self.nearby.clear();
            }
        }
    }

    unsafe fn ground_height(&self, cx: &Cx, at: Vector3) -> Option<f32> {
        let t = self.terrain.as_ref()?;
        let terrain = cx.rt.invoke(&t.active, std::ptr::null_mut(), &mut []).ok()?;
        if !cx.unity.is_alive(terrain) {
            return None;
        }
        let mut p = at;
        let mut args = [&mut p as *mut Vector3 as *mut c_void];
        let h: f32 = cx.rt.unbox(cx.rt.invoke(&t.sample_height, terrain, &mut args).ok()?).ok()?;
        let origin = cx.unity.component_transform(cx.rt, terrain).ok()?;
        let base = cx.unity.position(cx.rt, origin).ok()?;
        Some(base.y + h)
    }

    /// Puts the player at `dest` with the game's own teleport.
    unsafe fn move_player(&self, cx: &Cx, dest: &mut Vector3) -> Result<(), String> {
        let method = self.local.teleport.as_ref().ok_or("teleport is not available in this game version")?;
        let instance = if method.is_static { std::ptr::null_mut() } else { self.local_instance(cx) };
        if !method.is_static && instance.is_null() {
            return Err("player instance unavailable".into());
        }
        let mut args = [dest as *mut Vector3 as *mut c_void];
        cx.rt.invoke(method, instance, &mut args).map(|_| ()).map_err(|e| e.to_string())
    }

    /// Whether there's solid ground within `FLOOR_BELOW` under a spot underground.
    unsafe fn floor_under(&mut self, cx: &Cx, at: Vector3) -> bool {
        let Some(physics) = cx.unity.physics() else { return true };
        physics.sync_transforms(cx.rt);
        let Ok(mask) = self.solid(cx, true) else { return true };
        let down = Vector3 { x: 0.0, y: -1.0, z: 0.0 };
        physics.raycast(cx.rt, at, down, FLOOR_BELOW, mask).is_some()
    }
}

impl Provider for SonsOfTheForest {
    fn game_id(&self) -> &'static str {
        GAME_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            live_location: true,
            teleport: self.local.teleport.is_some(),
            other_players: self.remote.is_some(),
            hotkeys: true,
            game_map: false,
            game_pings: false,
        }
    }

    fn frame_hooks(&self) -> &'static [HookTarget] {
        FRAME_HOOKS
    }

    fn reconnected(&mut self) {
        if let Some(bag) = self.bag.as_mut() {
            bag.forget();
        }
    }

    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    unsafe fn companion_orders(&mut self, cx: &Cx, companion: &str) -> Result<Vec<CompanionOrderOption>, String> {
        if companion != companions::KELVIN {
            return Err(format!("{companion} doesn't take orders"));
        }
        let kelvin = self.creatures.as_ref().map(|c| c.kelvin).unwrap_or(std::ptr::null_mut());
        self.companions.as_mut().ok_or("companions aren't available")?.orders(cx.rt, cx.unity, kelvin)
    }

    unsafe fn companion_order(&mut self, cx: &Cx, companion: &str, order: &str) -> Result<(), String> {
        if companion != companions::KELVIN {
            return Err(format!("{companion} doesn't take orders"));
        }
        let kelvin = self.creatures.as_ref().map(|c| c.kelvin).unwrap_or(std::ptr::null_mut());
        let client = self.world.as_ref().is_some_and(|w| w.kind == WorldKind::Joined);
        let player = self.local_transform(cx);
        self.companions.as_mut().ok_or("companions aren't available")?.give_order(cx.rt, cx.unity, kelvin, player, order, client)
    }

    unsafe fn snapshot(&mut self, cx: &Cx, frame: u64) -> WorldSnapshot {
        let local = self.read_local(cx);
        let now = Instant::now();
        if local.is_none() {
            self.others.clear();
            self.markers.clear();
            self.nearby.clear();
            self.session = None;
            self.world = None;
            // A new game keeps its id through a moment without a player: a new id tells
            // the app it's another game, and it drops the old one's data.
            if now - *self.no_player_since.get_or_insert(now) >= LEFT_WORLD {
                self.unsaved_world = None;
            }
            self.companion_status = Default::default();
            self.world_status = None;
            self.dropped_bag = None;
            if let Some(creatures) = self.creatures.as_mut() {
                creatures.forget_companions();
            }
            if let Some(bag) = self.bag.as_mut() {
                bag.forget();
            }
        } else {
            self.no_player_since = None;
            if self.last_remote_refresh.is_none_or(|t| now - t >= REMOTE_REFRESH) {
                self.last_remote_refresh = Some(now);
                let lt = self.local_transform(cx);
                self.refresh_remote(cx, lt);
            }
            if self.last_marker_refresh.is_none_or(|t| now - t >= MARKER_REFRESH) {
                self.last_marker_refresh = Some(now);
                self.refresh_markers(cx);
            }
            if self.last_session_refresh.is_none_or(|t| now - t >= SESSION_REFRESH) {
                self.last_session_refresh = Some(now);
                self.refresh_session(cx);
            }
            if self.last_creature_refresh.is_none_or(|t| now - t >= CREATURE_REFRESH) {
                self.last_creature_refresh = Some(now);
                if let Some(p) = &local {
                    self.refresh_creatures(cx, to_unity(p.position));
                }
            }
            if self.last_status_refresh.is_none_or(|t| now - t >= STATUS_REFRESH) {
                self.last_status_refresh = Some(now);
                self.refresh_status(cx);
            }
        }
        let indoors = local.as_ref().is_some_and(|l| l.layer == WorldLayer::Caves);
        WorldSnapshot {
            frame,
            local,
            others: self.others.clone(),
            markers: self.markers.clone(),
            creatures: self.nearby.clone(),
            session: self.session.clone(),
            world: self.world.clone(),
            companions: self.companion_status.clone(),
            environment: self.world_status.clone(),
            dropped_bag: self.dropped_bag,
            dying: self.is_dying(cx),
            indoors,
            ..Default::default()
        }
    }

    unsafe fn teleport(&mut self, cx: &Cx, target: &TeleportTarget) -> Result<(), String> {
        if self.local.teleport.is_none() {
            return Err("teleport is not available in this game version".into());
        }
        if !cx.unity.is_alive(self.local_transform(cx)) {
            return Err("not in a world".into());
        }
        let (mut dest, underground) = match target {
            TeleportTarget::Position { position, snap_to_ground, layer } => {
                let mut p = to_unity(*position);
                if *snap_to_ground {
                    p.y = self.ground_height(cx, p).ok_or("could not sample ground height here")? + GROUND_CLEARANCE;
                }
                (p, !*snap_to_ground && *layer == WorldLayer::Caves)
            }
            TeleportTarget::Player { id } => {
                let position = self
                    .others
                    .iter()
                    .find(|o| &o.id == id)
                    .map(|o| o.position)
                    .or_else(|| self.markers.iter().find(|m| &m.id == id).map(|m| m.position))
                    // Companions tracked as creatures (Virginia).
                    .or_else(|| self.nearby.iter().find(|c| &c.id == id).map(|c| c.position))
                    .ok_or("player not found")?;
                let mut p = to_unity(position);
                p.x += 1.0;
                p.y += 0.5;
                (p, self.ground_height(cx, p).is_some_and(|g| g - p.y > UNDERGROUND_DEPTH))
            }
        };
        if !(dest.x.is_finite() && dest.y.is_finite() && dest.z.is_finite()) {
            return Err("invalid destination".into());
        }
        if self.export_job.is_some() {
            return Err("a world export is scanning caves; try again when it finishes".into());
        }
        let mut entering_cave = false;
        if let Some(caves) = &self.caves {
            let systems = caves.systems(cx.rt, cx.unity);
            // A cave's extents reach past its entrances and up through the ground above it:
            // being within them doesn't make a spot underground. Entering the cave there
            // puts the game in cave mode out on the surface.
            let target = if underground { caves.containing(cx.rt, &systems, dest) } else { None };
            caves.activate(cx.rt, &systems, target)?;
            entering_cave = target.is_some();
        }
        self.landing = None;
        self.move_player(cx, &mut dest)?;
        // Without floor under it yet, the player would fall through the cave: hold them
        // there until it loads (see `every_frame`).
        if entering_cave && !self.floor_under(cx, dest) {
            self.landing = Some(PendingLanding { dest, until: Instant::now() + CAVE_LOAD_WAIT });
        }
        Ok(())
    }

    unsafe fn every_frame(&mut self, cx: &Cx) {
        let Some((mut dest, until)) = self.landing.as_ref().map(|l| (l.dest, l.until)) else { return };
        let (landed, expired) = (self.floor_under(cx, dest), Instant::now() >= until);
        // Once more where there's floor, so the player stops at rest on it; until then,
        // back to the spot before they fall.
        if self.move_player(cx, &mut dest).is_err() || landed || expired {
            self.landing = None;
            if expired && !landed {
                self.notes.push("teleport: the cave's floor didn't load in time".into());
            }
        }
    }

    unsafe fn begin_export(&mut self, cx: &Cx) -> Result<(), String> {
        let transform = self.local_transform(cx);
        if !cx.unity.is_alive(transform) {
            return Err("load into a world first".into());
        }
        let mut export = export::export(self, cx);
        let caves = match &self.caves {
            Some(caves) => {
                let player = cx.unity.position(cx.rt, transform).ok();
                let fallback = |area: i32| {
                    let key = area.to_string();
                    let inside = export.entries.iter().filter(|e| {
                        e.props.get("cave_area") == Some(&key) || (e.kind == "cave_entrance" && e.props.get("area") == Some(&key))
                    });
                    cave_scan::bounds_around(inside.map(|e| e.position), CAVE_BOUNDS_MARGIN)
                };
                match cave_scan::CaveScanJob::start(caves, cx, player, fallback) {
                    Ok(job) => Some(job),
                    Err(e) => {
                        export.notes.push(format!("cave scan unavailable: {e}"));
                        None
                    }
                }
            }
            None => None,
        };
        self.export_job = Some(ExportJob { export, caves });
        Ok(())
    }

    unsafe fn poll_export(&mut self, cx: &Cx) -> Option<Result<WorldExport, String>> {
        let mut job = self.export_job.take()?;
        if !cx.unity.is_alive(self.local_transform(cx)) {
            return Some(Err("left the world during the export".into()));
        }
        if let (Some(scan), Some(caves)) = (job.caves.as_mut(), self.caves.as_ref()) {
            match scan.poll(self, caves, cx) {
                Ok(false) => {
                    self.export_job = Some(job);
                    return None;
                }
                Ok(true) => {}
                Err(e) => job.export.notes.push(format!("cave scan failed: {e}")),
            }
        }
        let mut export = job.export;
        if let Some(scan) = job.caves {
            export.notes.extend(scan.notes);
            export.caves = scan.results;
        }
        Some(Ok(export))
    }

    unsafe fn solid(&mut self, cx: &Cx, underground: bool) -> Result<i32, String> {
        let physics = cx.unity.physics().ok_or("this game has no physics queries")?;
        // The world's static geometry only: players, characters, held items and pickups
        // have layers of their own, and would block every route next to the player. In
        // caves the terrain's collider spans the cave openings overhead (the cave scan
        // leaves it out the same way).
        let mut layers = vec!["Default", "TreeWS", "CullingMedium", "CullingFar", "CullingNone", "Prop", "BasicCollider", "Blocker"];
        if !underground {
            layers.push("Terrain");
        }
        Ok(physics.mask_of(cx.rt, &layers))
    }

    unsafe fn find_path(&mut self, cx: &Cx, from: Vec3, to: Vec3) -> Result<PathInfo, String> {
        let astar = self.astar.as_mut().ok_or("the game's pathfinding was not found")?;
        let started = Instant::now();
        let route = astar.find(cx.rt, to_unity(from), to_unity(to));
        if self.routes_logged < 5 {
            self.routes_logged += 1;
            let outcome = match &route {
                Ok(r) => format!("{} corners, complete={}", r.corners.len(), r.complete),
                Err(e) => e.clone(),
            };
            self.notes.push(format!("route in {:.1} ms: {outcome}", started.elapsed().as_secs_f64() * 1000.0));
        }
        let route = route?;
        Ok(PathInfo { corners: route.corners.into_iter().map(to_proto).collect(), complete: route.complete })
    }

    unsafe fn input_blocked(&mut self, cx: &Cx) -> bool {
        self.chat_open.is_some_and(|f| cx.rt.static_value::<bool>(f))
    }
}
