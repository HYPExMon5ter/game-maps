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

mod astar;
mod cave_scan;
mod caves;
mod creatures;
mod export;
mod gps;
mod session;

use std::ffi::c_void;
use std::time::{Duration, Instant};

use game_maps_protocol::{Capabilities, Creature, LocalPlayer, Marker, MultiplayerSession, PathInfo, RemotePlayer, TeleportTarget, Vec3, WorldExport, WorldLayer, WorldSnapshot};
use unity_runtime::{read_string, Class, Color, Field, ListLayout, ManagedObject, Method, Physics, Runtime, Vector3};

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
    local: LocalMembers,
    remote: Option<RemoteMembers>,
    terrain: Option<TerrainMembers>,
    chat_open: Option<Field>,
    caves: Option<caves::Caves>,
    export_job: Option<ExportJob>,
    gps: Option<gps::GpsMarkers>,
    creatures: Option<creatures::Creatures>,
    session_members: Option<session::Session>,
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
        };
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

        Some(Self {
            local,
            remote,
            terrain,
            chat_open,
            caves,
            export_job: None,
            gps,
            creatures,
            session_members,
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
        let Some(members) = &self.session_members else { return };
        let session = members.id(cx.rt).map(|id| MultiplayerSession { id, player_name: members.player_name(cx.rt) });
        if session != self.session {
            self.notes.push(match &session {
                Some(s) => format!("Multiplayer session joined: {} as {:?}", s.id, s.player_name),
                None => "Not in a multiplayer session".into(),
            });
            self.session = session;
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
        }
    }

    fn frame_hooks(&self) -> &'static [HookTarget] {
        FRAME_HOOKS
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
            self.nearby.clear();
            self.session = None;
        } else {
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
        }
        WorldSnapshot { frame, local, others: self.others.clone(), markers: self.markers.clone(), creatures: self.nearby.clone(), session: self.session.clone() }
    }

    unsafe fn teleport(&mut self, cx: &Cx, target: &TeleportTarget) -> Result<(), String> {
        let method = self.local.teleport.clone().ok_or("teleport is not available in this game version")?;
        if !cx.unity.is_alive(self.local_transform(cx)) {
            return Err("not in a world".into());
        }
        let mut dest = match target {
            TeleportTarget::Position { position, snap_to_ground } => {
                let mut p = to_unity(*position);
                if *snap_to_ground {
                    p.y = self.ground_height(cx, p).ok_or("could not sample ground height here")? + GROUND_CLEARANCE;
                }
                p
            }
            TeleportTarget::Player { id } => {
                let position = self
                    .others
                    .iter()
                    .find(|o| &o.id == id)
                    .map(|o| o.position)
                    .or_else(|| self.markers.iter().find(|m| &m.id == id).map(|m| m.position))
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
        let instance = if method.is_static { std::ptr::null_mut() } else { self.local_instance(cx) };
        if !method.is_static && instance.is_null() {
            return Err("player instance unavailable".into());
        }
        if self.export_job.is_some() {
            return Err("a world export is scanning caves; try again when it finishes".into());
        }
        if let Some(caves) = &self.caves {
            let systems = caves.systems(cx.rt, cx.unity);
            caves.activate(cx.rt, &systems, caves.containing(cx.rt, &systems, dest))?;
        }
        let mut args = [&mut dest as *mut Vector3 as *mut c_void];
        cx.rt.invoke(&method, instance, &mut args).map(|_| ()).map_err(|e| e.to_string())
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

    unsafe fn solid_mask(&mut self, cx: &Cx, physics: &Physics, underground: bool) -> i32 {
        // The world's static geometry only: players, characters, held items and pickups
        // have layers of their own, and would block every route next to the player. In
        // caves the terrain's collider spans the cave openings overhead (the cave scan
        // leaves it out the same way).
        let mut layers = vec!["Default", "TreeWS", "CullingMedium", "CullingFar", "CullingNone", "Prop", "BasicCollider", "Blocker"];
        if !underground {
            layers.push("Terrain");
        }
        physics.mask_of(cx.rt, &layers)
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
