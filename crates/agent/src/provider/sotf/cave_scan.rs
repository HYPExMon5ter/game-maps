//! Cave floor scan (dev tooling): samples every cave's walkable floors on a 1 m grid so
//! the overlay can draw cave layouts.
//!
//! Per cave: force the game into that cave (its collision only exists while the player
//! is "inside"), wait a few frames, then cast rays straight down from just below the
//! terrain surface (terrain excluded). Mesh colliders are one-sided, so a downward ray
//! passes through cave ceilings from above and stops on surfaces facing up: floors. An
//! upward ray from each floor measures its headroom, which tells real floors (open
//! space above) from the tops of solid rock. The work is spread over frames with a
//! time budget so the game keeps running.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use base64::Engine as _;
use game_maps_protocol::{CaveScan, Vec3};
use il2cpp::{Class, ManagedArray, Method, Runtime, Vector3};

use super::caves::{Aabb, Caves};
use super::SonsOfTheForest;
use crate::provider::Cx;

/// Grid cell edge (meters).
const CELL: f32 = 1.0;
/// Floors kept per cell (multi-level caves), highest first.
const LAYERS: usize = 3;
const MAX_HITS: usize = 16;
/// Faces whose normal points up at least this much count as floor.
const FLOOR_NORMAL_Y: f32 = 0.35;
/// Headroom is measured up to this height (decimeters stored, capped).
const MAX_CLEARANCE: f32 = 12.0;
/// Frames to wait after entering a cave before scanning, so its colliders are live.
const SETTLE_FRAMES: u32 = 30;
/// Main-thread time spent scanning per frame.
const FRAME_BUDGET: Duration = Duration::from_millis(8);
/// Safety cap on grid size per cave (cells per side).
const MAX_SIDE: usize = 1200;
const NONE: i16 = i16::MIN;
/// `UnityEngine.QueryTriggerInteraction.Ignore`.
const IGNORE_TRIGGERS: i32 = 1;

struct Physics {
    raycast_all: Method,
    raycast_one: Method,
    sync_transforms: Option<Method>,
    hit_class: Class,
    point: usize,
    normal: usize,
    distance: usize,
    /// Layers for floor rays (terrain and water excluded) and headroom rays.
    floor_mask: i32,
    headroom_mask: i32,
}

impl Physics {
    unsafe fn resolve(rt: &Runtime) -> Result<Self, String> {
        let physics = rt.class("UnityEngine", "Physics").map_err(|e| e.to_string())?;
        let hit_class = rt.class("UnityEngine", "RaycastHit").map_err(|e| e.to_string())?;
        let raycast_all = rt
            .method_with_params(
                physics,
                "RaycastNonAlloc",
                &["UnityEngine.Vector3", "UnityEngine.Vector3", "UnityEngine.RaycastHit[]", "System.Single", "System.Int32", "UnityEngine.QueryTriggerInteraction"],
            )
            .map_err(|e| e.to_string())?;
        let raycast_one = rt
            .method_with_params(
                physics,
                "Raycast",
                &["UnityEngine.Vector3", "UnityEngine.Vector3", "UnityEngine.RaycastHit&", "System.Single", "System.Int32", "UnityEngine.QueryTriggerInteraction"],
            )
            .map_err(|e| e.to_string())?;
        // Field offsets include the 16-byte object header of a boxed value.
        let offset = |name: &str| rt.field(hit_class, name).map(|f| f.offset - 0x10).map_err(|e| e.to_string());
        let layer = |name: &str| layer_index(rt, name);
        let excluded = [layer("Terrain"), layer("Water"), Some(2) /* Ignore Raycast */]
            .into_iter()
            .flatten()
            .fold(0i32, |m, l| m | (1 << l));
        Ok(Self {
            raycast_all,
            raycast_one,
            sync_transforms: rt.method(physics, "SyncTransforms", 0).ok(),
            hit_class,
            point: offset("m_Point")?,
            normal: offset("m_Normal")?,
            distance: offset("m_Distance")?,
            floor_mask: !excluded,
            headroom_mask: !(1 << 2),
        })
    }
}

unsafe fn layer_index(rt: &Runtime, name: &str) -> Option<i32> {
    let class = rt.class("UnityEngine", "LayerMask").ok()?;
    let m = rt.method_with_params(class, "NameToLayer", &["System.String"]).ok()?;
    let mut args = [rt.new_string(name) as *mut c_void];
    rt.unbox::<i32>(rt.invoke(&m, std::ptr::null_mut(), &mut args).ok()?).ok().filter(|&l| (0..32).contains(&l))
}

#[derive(Clone, Copy)]
struct Target {
    area: i32,
    bounds: Aabb,
    width: usize,
    height: usize,
}

enum Phase {
    Settling { frames: u32 },
    Scanning { next_cell: usize },
}

pub struct CaveScanJob {
    physics: Physics,
    targets: Vec<Target>,
    current: usize,
    phase: Phase,
    floors: Vec<i16>,
    clearance: Vec<i16>,
    /// Cave the player was in before the scan (restored afterwards).
    player_area: Option<i32>,
    pub results: Vec<CaveScan>,
    pub notes: Vec<String>,
}

impl CaveScanJob {
    /// `fallback` gives bounds for caves whose extents aren't colliders (from the
    /// positions of objects known to be inside them).
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn start(caves: &Caves, cx: &Cx, player: Option<Vector3>, fallback: impl Fn(i32) -> Option<Aabb>) -> Result<Self, String> {
        let physics = Physics::resolve(cx.rt)?;
        let systems = caves.systems(cx.rt, cx.unity);
        let player_area = player.and_then(|p| caves.containing(cx.rt, &systems, p)).map(|s| caves.area(s));
        let mut targets = Vec::new();
        let mut notes = Vec::new();
        for &s in &systems {
            let area = caves.area(s);
            let Some(bounds) = caves.extent_bounds(cx.rt, cx.unity, s).or_else(|| fallback(area)) else {
                notes.push(format!("cave {area}: no bounds, skipped"));
                continue;
            };
            let width = (((bounds.1.x - bounds.0.x) / CELL).ceil() as usize).clamp(1, MAX_SIDE);
            let height = (((bounds.1.z - bounds.0.z) / CELL).ceil() as usize).clamp(1, MAX_SIDE);
            notes.push(format!("cave {area}: {width}x{height} cells, y {:.0}..{:.0}", bounds.0.y, bounds.1.y));
            targets.push(Target { area, bounds, width, height });
        }
        Ok(Self {
            physics,
            targets,
            current: 0,
            phase: Phase::Settling { frames: SETTLE_FRAMES },
            floors: Vec::new(),
            clearance: Vec::new(),
            player_area,
            results: Vec::new(),
            notes,
        })
    }

    /// Advances the scan. Returns `true` once every cave is done and the player's cave
    /// state is restored.
    ///
    /// # Safety
    /// Main thread only.
    pub unsafe fn poll(&mut self, game: &SonsOfTheForest, caves: &Caves, cx: &Cx) -> Result<bool, String> {
        let Some(&target) = self.targets.get(self.current) else {
            let systems = caves.systems(cx.rt, cx.unity);
            let restore = self.player_area.and_then(|a| systems.iter().copied().find(|&s| caves.area(s) == a));
            caves.activate(cx.rt, &systems, restore)?;
            return Ok(true);
        };
        match self.phase {
            Phase::Settling { frames } => {
                if frames == SETTLE_FRAMES {
                    let systems = caves.systems(cx.rt, cx.unity);
                    let system = systems.iter().copied().find(|&s| caves.area(s) == target.area);
                    caves.activate(cx.rt, &systems, system)?;
                }
                self.phase = if frames == 0 {
                    if let Some(sync) = &self.physics.sync_transforms {
                        let _ = cx.rt.invoke(sync, std::ptr::null_mut(), &mut []);
                    }
                    let cells = target.width * target.height;
                    self.floors = vec![NONE; cells * LAYERS];
                    self.clearance = vec![NONE; cells * LAYERS];
                    Phase::Scanning { next_cell: 0 }
                } else {
                    Phase::Settling { frames: frames - 1 }
                };
            }
            Phase::Scanning { next_cell } => {
                let started = Instant::now();
                let cells = target.width * target.height;
                let hits = cx.rt.new_array(self.physics.hit_class, MAX_HITS).map_err(|e| e.to_string())?;
                let mut cell = next_cell;
                while cell < cells && started.elapsed() < FRAME_BUDGET {
                    self.scan_cell(game, cx, &target, cell, &hits);
                    cell += 1;
                }
                if cell < cells {
                    self.phase = Phase::Scanning { next_cell: cell };
                } else {
                    self.finish_cave();
                    self.current += 1;
                    self.phase = Phase::Settling { frames: SETTLE_FRAMES };
                }
            }
        }
        Ok(false)
    }

    unsafe fn scan_cell(&mut self, game: &SonsOfTheForest, cx: &Cx, t: &Target, cell: usize, hits: &ManagedArray) {
        let (col, row) = (cell % t.width, cell / t.width);
        let x = t.bounds.0.x + (col as f32 + 0.5) * CELL;
        let z = t.bounds.0.z + (row as f32 + 0.5) * CELL;
        let bottom = t.bounds.0.y - 2.0;
        // Start just below the terrain (or at the cave's top if lower).
        let top = game
            .ground_height(cx, Vector3 { x, y: 0.0, z })
            .map_or(t.bounds.1.y, |g| (g - 0.3).min(t.bounds.1.y + 2.0));
        if top <= bottom {
            return;
        }

        let rt = cx.rt;
        let p = &self.physics;
        let mut origin = Vector3 { x, y: top, z };
        let mut down = Vector3 { x: 0.0, y: -1.0, z: 0.0 };
        let mut dist = top - bottom;
        let (mut mask, mut triggers) = (p.floor_mask, IGNORE_TRIGGERS);
        let mut args = [
            &mut origin as *mut Vector3 as *mut c_void,
            &mut down as *mut Vector3 as *mut c_void,
            hits.object as *mut c_void,
            &mut dist as *mut f32 as *mut c_void,
            &mut mask as *mut i32 as *mut c_void,
            &mut triggers as *mut i32 as *mut c_void,
        ];
        let Ok(count) = rt.invoke(&p.raycast_all, std::ptr::null_mut(), &mut args).and_then(|b| rt.unbox::<i32>(b)) else { return };

        let mut floors: Vec<f32> = Vec::with_capacity(MAX_HITS);
        for i in 0..(count.max(0) as usize).min(hits.len) {
            let e = hits.element(i);
            let point = std::ptr::read_unaligned(e.add(p.point) as *const Vector3);
            let normal = std::ptr::read_unaligned(e.add(p.normal) as *const Vector3);
            if normal.y >= FLOOR_NORMAL_Y && point.y.is_finite() {
                floors.push(point.y);
            }
        }
        floors.sort_by(|a, b| b.total_cmp(a));
        floors.dedup_by(|a, b| (*a - *b).abs() < 0.3);

        let cells = t.width * t.height;
        for (layer, &y) in floors.iter().take(LAYERS).enumerate() {
            self.floors[layer * cells + cell] = to_dm(y);
            self.clearance[layer * cells + cell] = to_dm(self.headroom(cx, Vector3 { x, y: y + 0.1, z }));
        }
    }

    /// Free height above a floor point (up to `MAX_CLEARANCE`), terrain included.
    unsafe fn headroom(&self, cx: &Cx, from: Vector3) -> f32 {
        let rt = cx.rt;
        let p = &self.physics;
        let mut origin = from;
        let mut up = Vector3 { x: 0.0, y: 1.0, z: 0.0 };
        let mut hit = [0u8; 64];
        let mut dist = MAX_CLEARANCE;
        let (mut mask, mut triggers) = (p.headroom_mask, IGNORE_TRIGGERS);
        let mut args = [
            &mut origin as *mut Vector3 as *mut c_void,
            &mut up as *mut Vector3 as *mut c_void,
            hit.as_mut_ptr() as *mut c_void,
            &mut dist as *mut f32 as *mut c_void,
            &mut mask as *mut i32 as *mut c_void,
            &mut triggers as *mut i32 as *mut c_void,
        ];
        match rt.invoke(&p.raycast_one, std::ptr::null_mut(), &mut args).and_then(|b| rt.unbox::<bool>(b)) {
            Ok(true) => std::ptr::read_unaligned(hit.as_ptr().add(p.distance) as *const f32) + 0.1,
            _ => MAX_CLEARANCE,
        }
    }

    fn finish_cave(&mut self) {
        let t = &self.targets[self.current];
        let encode = |plane: &[i16]| {
            let bytes: Vec<u8> = plane.iter().flat_map(|v| v.to_le_bytes()).collect();
            base64::engine::general_purpose::STANDARD.encode(bytes)
        };
        let found = self.floors.iter().filter(|&&v| v != NONE).count();
        self.notes.push(format!("cave {}: {found} floor samples", t.area));
        self.results.push(CaveScan {
            area: t.area,
            origin_x: t.bounds.0.x,
            origin_z: t.bounds.0.z,
            cell: CELL,
            width: t.width as u32,
            height: t.height as u32,
            layers: LAYERS as u32,
            floors: encode(&self.floors),
            clearance: encode(&self.clearance),
        });
        self.floors = Vec::new();
        self.clearance = Vec::new();
    }
}

fn to_dm(v: f32) -> i16 {
    (v * 10.0).round().clamp(i16::MIN as f32 + 1.0, i16::MAX as f32) as i16
}

/// Bounds around a set of points (fallback when a cave's extents aren't colliders).
pub fn bounds_around(points: impl IntoIterator<Item = Vec3>, margin: f32) -> Option<Aabb> {
    let mut it = points.into_iter();
    let first = it.next()?;
    let (mut lo, mut hi) = (first, first);
    for p in it {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    Some((
        Vector3 { x: lo.x - margin, y: lo.y - margin, z: lo.z - margin },
        Vector3 { x: hi.x + margin, y: hi.y + margin, z: hi.z + margin },
    ))
}
