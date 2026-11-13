//! Unreal Engine games: the engine-level parts every Unreal provider shares (the local
//! player, its camera, other players, line traces, navigation), on top of
//! `unreal-runtime`'s name-based access.
//!
//! Unreal positions are centimeters with Z up; the agent works in Unity's convention
//! (meters, Y up, both left-handed). [`to_proto`] and [`from_proto`] convert between the
//! two: Unreal's forward (X), right (Y) and up (Z) become Z, X and Y.

use std::ops::Deref;
use std::sync::{Arc, Mutex};

use game_maps_protocol::{CameraView, PathInfo, Vec3};
use unreal_runtime::{Args, FRotator, FVector, Function, Obj, Property, TArray, Unreal};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetClientRect, GetWindowThreadProcessId, IsWindowVisible};
use windows::core::BOOL;

use crate::engine::Engine;
use crate::walk::{Hit, World};
use unity_runtime::Vector3;

/// Unreal units per meter.
pub const CM: f64 = 100.0;

/// An Unreal position as the agent and the app use positions.
pub fn to_proto(v: FVector) -> Vec3 {
    Vec3::new((v.y / CM) as f32, (v.z / CM) as f32, (v.x / CM) as f32)
}

pub fn from_proto(v: Vec3) -> FVector {
    FVector { x: v.z as f64 * CM, y: v.x as f64 * CM, z: v.y as f64 * CM }
}

/// An Unreal direction, without scaling.
pub fn dir_to_proto(v: FVector) -> Vec3 {
    Vec3::new(v.y as f32, v.z as f32, v.x as f32)
}

fn walk_to_ue(v: Vector3) -> FVector {
    from_proto(Vec3::new(v.x, v.y, v.z))
}

fn ue_to_walk(v: FVector) -> Vector3 {
    let p = to_proto(v);
    Vector3 { x: p.x, y: p.y, z: p.z }
}

/// `EObjectTypeQuery`: the object types route checks collide with, by default the
/// world's static and movable geometry (not pawns, not physics bodies).
pub type ObjectTypes = [u8; 2];
const WORLD_STATIC_AND_DYNAMIC: ObjectTypes = [0, 1];

/// Engine classes and members every Unreal game has.
struct Members {
    game_engine: Obj,
    game_instance: Property,
    local_players: Property,
    player_controller: Property,
    pawn: Property,
    camera_manager: Property,
    camera_cache: Property,
    pov: Property,
    pov_location: Property,
    pov_rotation: Property,
    pov_fov: Property,
    get_location: Arc<Function>,
    get_rotation: Arc<Function>,
    line_trace: Option<Arc<Function>>,
    sphere_trace: Option<Arc<Function>>,
    hit_location: Option<Property>,
    hit_normal: Option<Property>,
    hit_distance: Option<Property>,
    find_path: Option<Arc<Function>>,
    path_points: Option<Property>,
    path_partial: Option<Arc<Function>>,
    get_component: Option<Arc<Function>>,
    loc_text: Option<Arc<Function>>,
    is_server: Option<Arc<Function>>,
    is_standalone: Option<Arc<Function>>,
}

impl Members {
    fn resolve(ue: &Unreal) -> unreal_runtime::Result<Self> {
        let class = |name: &str| ue.find_struct(name);
        let actor = class("/Script/Engine.Actor")?;
        let kismet = class("/Script/Engine.KismetSystemLibrary")?;
        let hit = class("/Script/Engine.HitResult").ok();
        let nav = class("/Script/NavigationSystem.NavigationSystemV1").ok();
        let camera_cache = class("/Script/Engine.CameraCacheEntry")?;
        let view = class("/Script/Engine.MinimalViewInfo")?;
        Ok(Self {
            game_engine: class("/Script/Engine.GameEngine")?,
            game_instance: ue.property_of(class("/Script/Engine.GameEngine")?, "GameInstance", "Object<GameInstance>")?,
            local_players: ue.property_of(class("/Script/Engine.GameInstance")?, "LocalPlayers", "Array<Object<LocalPlayer>>")?,
            player_controller: ue.property_of(class("/Script/Engine.Player")?, "PlayerController", "Object<PlayerController>")?,
            pawn: ue.property_of(class("/Script/Engine.Controller")?, "Pawn", "Object<Pawn>")?,
            camera_manager: ue.property_of(class("/Script/Engine.PlayerController")?, "PlayerCameraManager", "Object<PlayerCameraManager>")?,
            camera_cache: ue.property_of(class("/Script/Engine.PlayerCameraManager")?, "CameraCachePrivate", "Struct<CameraCacheEntry>")?,
            pov: ue.property_of(camera_cache, "POV", "Struct<MinimalViewInfo>")?,
            pov_location: ue.property_of(view, "Location", "Struct<Vector>")?,
            pov_rotation: ue.property_of(view, "Rotation", "Struct<Rotator>")?,
            pov_fov: ue.property_of(view, "FOV", "Float")?,
            get_location: ue.function(actor, "K2_GetActorLocation")?,
            get_rotation: ue.function(actor, "K2_GetActorRotation")?,
            line_trace: ue.function(kismet, "LineTraceSingleForObjects").ok(),
            sphere_trace: ue.function(kismet, "SphereTraceSingleForObjects").ok(),
            hit_location: hit.and_then(|h| ue.property(h, "ImpactPoint").ok()),
            hit_normal: hit.and_then(|h| ue.property(h, "ImpactNormal").ok()),
            hit_distance: hit.and_then(|h| ue.property_of(h, "Distance", "Float").ok()),
            find_path: nav.and_then(|n| ue.function(n, "FindPathToLocationSynchronously").ok()),
            path_points: class("/Script/NavigationSystem.NavigationPath").ok().and_then(|p| ue.property_of(p, "PathPoints", "Array<Struct<Vector>>").ok()),
            path_partial: class("/Script/NavigationSystem.NavigationPath").ok().and_then(|p| ue.function(p, "IsPartial").ok()),
            get_component: ue.function(actor, "GetComponentByClass").ok(),
            loc_text: class("/Script/OEIText.OEITextLibrary").ok().and_then(|c| ue.function(c, "GetLocStringText").ok()),
            is_server: ue.function(kismet, "IsServer").ok(),
            is_standalone: ue.function(kismet, "IsStandalone").ok(),
        })
    }
}

/// The local player's objects this frame.
#[derive(Debug, Clone, Copy)]
pub struct Local {
    pub world: Obj,
    pub controller: Obj,
    pub pawn: Option<Obj>,
}

pub struct UnrealEngine {
    pub ue: Unreal,
    members: Option<Members>,
    /// The engine object, once found.
    engine: Mutex<Option<Obj>>,
    /// Problems resolving engine members, for the log.
    pub notes: Mutex<Vec<String>>,
    /// The engine version, read on the first tick (also proves calls work).
    version: Mutex<Option<String>>,
    /// The game window's aspect ratio, and when it was measured.
    aspect: Mutex<Option<(std::time::Instant, f32)>>,
    /// Localized text already looked up, by string table and id.
    texts: Mutex<std::collections::HashMap<(i32, i32), String>>,
}

/// What Unreal providers work with.
pub struct UeCx<'a> {
    pub engine: &'a UnrealEngine,
}

impl Deref for UeCx<'_> {
    type Target = UnrealEngine;
    fn deref(&self) -> &UnrealEngine {
        self.engine
    }
}

impl UnrealEngine {
    pub fn new(ue: Unreal) -> Self {
        let members = Members::resolve(&ue);
        let mut notes = Vec::new();
        if let Err(e) = &members {
            notes.push(format!("Unreal engine members: {e}"));
        }
        Self { ue, members: members.ok(), engine: Mutex::new(None), notes: Mutex::new(notes), version: Mutex::new(None), texts: Mutex::new(Default::default()), aspect: Mutex::new(None) }
    }

    fn engine_object(&self) -> Option<Obj> {
        let m = self.members.as_ref()?;
        let mut engine = self.engine.lock().unwrap();
        if engine.is_none_or(|e| !self.ue.alive(e)) {
            *engine = self.ue.instances_of(m.game_engine).into_iter().next();
        }
        *engine
    }

    /// The engine's version, read once on the game thread.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn version(&self) -> Option<String> {
        let mut v = self.version.lock().unwrap();
        if v.is_none() {
            *v = Some(self.ue.engine_version().unwrap_or_else(|e| format!("unknown ({e})")));
        }
        v.clone()
    }

    /// The local player's world, controller and pawn.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn local(&self) -> Option<Local> {
        let m = self.members.as_ref()?;
        let engine = self.engine_object()?;
        let instance = self.ue.get_obj(engine, &m.game_instance)?;
        let players: TArray<Obj> = self.ue.get(instance, &m.local_players);
        let player = players.to_vec().into_iter().next().filter(|&p| p != 0 && self.ue.alive(p))?;
        let controller = self.ue.get_obj(player, &m.player_controller)?;
        // An actor's outer is its level, and the level's is the world.
        let world = self.ue.outer(self.ue.outer(controller)?)?;
        let pawn = self.ue.get_obj(controller, &m.pawn);
        Some(Local { world, controller, pawn })
    }

    /// An actor's position.
    ///
    /// # Safety
    /// Game thread only; `actor` must be a live actor.
    pub unsafe fn location(&self, actor: Obj) -> Option<FVector> {
        let m = self.members.as_ref()?;
        let mut args = Args::new(&m.get_location);
        self.ue.call(actor, &mut args).ok()?;
        args.get::<FVector>("ReturnValue").ok()
    }

    /// An actor's rotation.
    ///
    /// # Safety
    /// Game thread only; `actor` must be a live actor.
    pub unsafe fn rotation(&self, actor: Obj) -> Option<FRotator> {
        let m = self.members.as_ref()?;
        let mut args = Args::new(&m.get_rotation);
        self.ue.call(actor, &mut args).ok()?;
        args.get::<FRotator>("ReturnValue").ok()
    }

    /// Calls `f`, which takes no parameters, on `obj` for its return value.
    ///
    /// # Safety
    /// Game thread only; `obj` must be a live instance of `f`'s class, and `T` its return type.
    pub unsafe fn call_get<T: Copy>(&self, obj: Obj, f: &Function) -> Option<T> {
        let mut args = Args::new(f);
        self.ue.call(obj, &mut args).ok()?;
        args.get::<T>("ReturnValue").ok()
    }

    /// The component of `actor` that is a `class`, if it has one.
    ///
    /// # Safety
    /// Game thread only; `actor` must be a live actor.
    pub unsafe fn component(&self, actor: Obj, class: Obj) -> Option<Obj> {
        let f = self.members.as_ref()?.get_component.as_ref()?;
        let mut args = Args::new(f);
        args.set("ComponentClass", class).ok()?;
        self.ue.call(actor, &mut args).ok()?;
        args.get::<Obj>("ReturnValue").ok().filter(|&c| c != 0 && self.ue.alive(c))
    }

    /// The text of an Obsidian `LocString` (string table id, string id). Each distinct
    /// text is looked up once: the game allocates the string it returns.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn loc_text(&self, table: i32, id: i32) -> Option<String> {
        if let Some(t) = self.texts.lock().unwrap().get(&(table, id)) {
            return Some(t.clone());
        }
        let f = self.members.as_ref()?.loc_text.as_ref()?;
        let mut args = Args::new(f);
        args.set("LocString", [table as u32, id as u32, 0, 0]).ok()?;
        self.ue.call(0, &mut args).ok()?;
        let text = args.get::<unreal_runtime::FString>("ReturnValue").ok()?.to_string_lossy();
        self.texts.lock().unwrap().insert((table, id), text.clone());
        Some(text)
    }

    unsafe fn net_check(&self, f: Option<&Arc<Function>>, context: Obj) -> Option<bool> {
        let mut args = Args::new(f?);
        args.set("WorldContextObject", context).ok()?;
        self.ue.call(0, &mut args).ok()?;
        args.get_bool("ReturnValue").ok()
    }

    /// Whether this game runs the world (single player or hosting), not a client of it.
    ///
    /// # Safety
    /// Game thread only; `context` must be a live object in the world.
    pub unsafe fn is_server(&self, context: Obj) -> Option<bool> {
        self.net_check(self.members.as_ref()?.is_server.as_ref(), context)
    }

    /// Whether the world is a single player one (no network at all).
    ///
    /// # Safety
    /// Game thread only; `context` must be a live object in the world.
    pub unsafe fn is_standalone(&self, context: Obj) -> Option<bool> {
        self.net_check(self.members.as_ref()?.is_standalone.as_ref(), context)
    }

    /// The point of view the player sees: position, rotation and horizontal field of view.
    unsafe fn view(&self) -> Option<(FVector, FRotator, f32)> {
        let m = self.members.as_ref()?;
        let local = self.local()?;
        let manager = self.ue.get_obj(local.controller, &m.camera_manager)?;
        let pov = manager + m.camera_cache.offset + m.pov.offset;
        Some((self.ue.get(pov, &m.pov_location), self.ue.get(pov, &m.pov_rotation), self.ue.get(pov, &m.pov_fov)))
    }

    /// The height of the highest solid surface under `at` (agent coordinates), looking
    /// down from `above` meters over it.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn ground_below(&self, at: Vec3, above: f32) -> Option<f32> {
        let line = self.members.as_ref()?.line_trace.as_ref()?;
        let from = from_proto(Vec3::new(at.x, at.y + above, at.z));
        let to = from_proto(Vec3::new(at.x, at.y - above, at.z));
        self.trace(line, from, to, None, &WORLD_STATIC_AND_DYNAMIC).map(|(point, _, _)| to_proto(point).y)
    }

    /// A trace from `from` to `to` against `types`, ignoring the local player.
    unsafe fn trace(&self, function: &Function, from: FVector, to: FVector, radius: Option<f32>, types: &ObjectTypes) -> Option<(FVector, FVector, f32)> {
        let m = self.members.as_ref()?;
        let context = self.local().and_then(|l| l.pawn.or(Some(l.controller)))?;
        let mut args = Args::new(function);
        let types = TArray { data: types.as_ptr(), num: types.len() as i32, max: types.len() as i32 };
        args.set("WorldContextObject", context).ok()?;
        args.set("Start", from).ok()?;
        args.set("End", to).ok()?;
        args.set("ObjectTypes", types).ok()?;
        args.set_bool("bIgnoreSelf", true).ok()?;
        if let Some(r) = radius {
            args.set("Radius", r).ok()?;
        }
        self.ue.call(0, &mut args).ok()?;
        if !args.get_bool("ReturnValue").ok()? {
            return None;
        }
        let hit = args.param_ptr("OutHit").ok()? as usize;
        let (location, normal, distance) = (m.hit_location.as_ref()?, m.hit_normal.as_ref()?, m.hit_distance.as_ref()?);
        Some((self.ue.get(hit, location), self.ue.get(hit, normal), self.ue.get::<f32>(hit, distance)))
    }
}

/// The size of this process's Unreal window, for the camera's aspect ratio.
fn window_aspect() -> Option<f32> {
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut class = [0u16; 32];
        let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
        if pid == GetCurrentProcessId() && IsWindowVisible(hwnd).as_bool() && String::from_utf16_lossy(&class[..len]) == "UnrealWindow" {
            *(data.0 as *mut HWND) = hwnd;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut hwnd = HWND::default();
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut hwnd as *mut HWND as isize));
        if hwnd.is_invalid() {
            return None;
        }
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).ok()?;
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        (w > 0 && h > 0).then(|| w as f32 / h as f32)
    }
}

impl Engine for UnrealEngine {
    type Cx<'a> = UeCx<'a>;
    type Solid = ObjectTypes;

    fn cx(&self) -> UeCx<'_> {
        UeCx { engine: self }
    }

    fn frame_count(&self) -> Option<i64> {
        // Ticks come from the message pump, at most once per pump interval.
        None
    }

    unsafe fn camera(&self) -> Option<CameraView> {
        let (location, rotation, fov_x) = self.view()?;
        let aspect = {
            // Finding the window means going through every window: once a second is enough.
            let mut cached = self.aspect.lock().unwrap();
            match *cached {
                Some((at, a)) if at.elapsed() < std::time::Duration::from_secs(1) => a,
                _ => {
                    let a = window_aspect().unwrap_or(16.0 / 9.0);
                    *cached = Some((std::time::Instant::now(), a));
                    a
                }
            }
        };
        // Unreal keeps the horizontal field of view; the app wants the vertical one.
        let fov_y = 2.0 * ((fov_x.to_radians() / 2.0).tan() / aspect).atan().to_degrees();
        Some(CameraView { position: to_proto(location), forward: dir_to_proto(rotation.forward()), up: dir_to_proto(rotation.up()), fov_y, aspect })
    }

    unsafe fn find_path(cx: &UeCx, from: Vec3, to: Vec3) -> Result<PathInfo, String> {
        let m = cx.members.as_ref().ok_or("the engine's members were not found")?;
        let (f, points, partial) = (m.find_path.as_ref(), m.path_points.as_ref(), m.path_partial.as_ref());
        let (Some(f), Some(points), Some(partial)) = (f, points, partial) else { return Err("this game has no navigation system".into()) };
        let local = cx.local().ok_or("not in a world")?;
        let mut args = Args::new(f);
        args.set("WorldContextObject", local.controller).map_err(|e| e.to_string())?;
        args.set("PathStart", from_proto(from)).map_err(|e| e.to_string())?;
        args.set("PathEnd", from_proto(to)).map_err(|e| e.to_string())?;
        cx.ue.call(0, &mut args).map_err(|e| e.to_string())?;
        let path: Obj = args.get("ReturnValue").map_err(|e| e.to_string())?;
        if path == 0 || !cx.ue.alive(path) {
            return Err("no route between the two points".into());
        }
        let corners: TArray<FVector> = cx.ue.get(path, points);
        let corners: Vec<Vec3> = corners.to_vec().into_iter().map(to_proto).collect();
        if corners.len() < 2 {
            return Err("no route between the two points".into());
        }
        let mut args = Args::new(partial);
        cx.ue.call(path, &mut args).map_err(|e| e.to_string())?;
        let complete = !args.get_bool("ReturnValue").unwrap_or(true);
        Ok(PathInfo { corners, complete })
    }

    unsafe fn default_solid(_cx: &UeCx) -> Result<ObjectTypes, String> {
        Ok(WORLD_STATIC_AND_DYNAMIC)
    }

    unsafe fn with_world<T>(&self, types: ObjectTypes, f: &mut dyn FnMut(&dyn World) -> T) -> Result<T, String> {
        let m = self.members.as_ref().ok_or("the engine's members were not found")?;
        let (Some(line), Some(sphere)) = (m.line_trace.as_ref(), m.sphere_trace.as_ref()) else { return Err("this game has no traces".into()) };
        Ok(f(&TraceWorld { engine: self, line, sphere, types }))
    }

    unsafe fn describe_solid(&self, types: [ObjectTypes; 2]) -> String {
        format!("object types {types:?}")
    }
}

/// Route checks against the game's collision, through `KismetSystemLibrary` traces.
struct TraceWorld<'a> {
    engine: &'a UnrealEngine,
    line: &'a Function,
    sphere: &'a Function,
    types: ObjectTypes,
}

/// Most surfaces a ray along one line can report (see [`World::ray_all`]).
const MAX_HITS: usize = 8;
/// How far past a hit the next trace starts, meters.
const PAST_HIT: f32 = 0.02;

impl TraceWorld<'_> {
    fn hit(&self, from: Vector3, to: Vector3) -> Option<Hit> {
        let (point, normal, distance) = unsafe { self.engine.trace(self.line, walk_to_ue(from), walk_to_ue(to), None, &self.types) }?;
        Some(Hit { point: ue_to_walk(point), normal_y: dir_to_proto(normal).y, distance: distance / CM as f32 })
    }
}

impl World for TraceWorld<'_> {
    fn ray(&self, from: Vector3, to: Vector3) -> Option<Hit> {
        self.hit(from, to)
    }

    fn ray_all(&self, from: Vector3, to: Vector3) -> Vec<Hit> {
        let length = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2) + (to.z - from.z).powi(2)).sqrt();
        if length < 1e-4 {
            return Vec::new();
        }
        let dir = Vector3 { x: (to.x - from.x) / length, y: (to.y - from.y) / length, z: (to.z - from.z) / length };
        let mut hits = Vec::new();
        let mut travelled = 0.0;
        while hits.len() < MAX_HITS {
            let start = Vector3 { x: from.x + dir.x * travelled, y: from.y + dir.y * travelled, z: from.z + dir.z * travelled };
            let Some(mut hit) = self.hit(start, to) else { break };
            travelled += hit.distance + PAST_HIT;
            hit.distance = travelled - PAST_HIT;
            hits.push(hit);
            if travelled >= length {
                break;
            }
        }
        hits
    }

    fn sweep_blocked(&self, from: Vector3, to: Vector3, radius: f32) -> bool {
        unsafe { self.engine.trace(self.sphere, walk_to_ue(from), walk_to_ue(to), Some(radius * CM as f32), &self.types) }.is_some()
    }
}

/// Writes every type the game has loaded to `%TEMP%\game-maps-unreal-types.txt`, for
/// finding names to use in a provider, and the type mappings the map generator reads the
/// game's files with to `%TEMP%\game-maps-unreal.usmap`. Development builds only, and
/// only when the mappings are older than the game: it takes minutes.
#[cfg(debug_assertions)]
pub fn dump_types(ue: &Unreal) {
    let modified = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mappings = modified(&std::env::temp_dir().join("game-maps-unreal.usmap"));
    let game = std::env::current_exe().ok().and_then(|exe| modified(&exe));
    if mappings.is_some_and(|m| game.is_some_and(|g| m > g)) {
        return;
    }
    let path = std::env::temp_dir().join("game-maps-unreal-types.txt");
    let started = std::time::Instant::now();
    let result = std::fs::File::create(&path).and_then(|f| {
        let mut out = std::io::BufWriter::new(f);
        unsafe { ue.dump(&mut out) }?;
        std::io::Write::flush(&mut out)
    });
    crate::filelog::write(&format!("type dump to {}: {result:?} in {:.1} s", path.display(), started.elapsed().as_secs_f32()));
    let path = std::env::temp_dir().join("game-maps-unreal.usmap");
    let result = unsafe { ue.usmap() }.map_err(|e| e.to_string()).and_then(|m| std::fs::write(&path, m).map_err(|e| e.to_string()));
    crate::filelog::write(&format!("type mappings to {}: {result:?}", path.display()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unreal_positions_to_the_agents_convention() {
        // 1 m forward (X), 2 m right (Y), 3 m up (Z) in Unreal.
        let p = to_proto(FVector { x: 100.0, y: 200.0, z: 300.0 });
        assert_eq!((p.x, p.y, p.z), (2.0, 3.0, 1.0));
        assert_eq!(from_proto(p), FVector { x: 100.0, y: 200.0, z: 300.0 });
    }

    #[test]
    fn keeps_handedness() {
        // Unreal: forward X × right Y = up Z, as Unity's forward Z, right X and up Y.
        let (f, r, u) = (dir_to_proto(FVector { x: 1.0, ..Default::default() }), dir_to_proto(FVector { y: 1.0, ..Default::default() }), dir_to_proto(FVector { z: 1.0, ..Default::default() }));
        assert_eq!((f.z, r.x, u.y), (1.0, 1.0, 1.0));
    }
}
