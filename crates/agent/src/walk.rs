//! Checks routes and breadcrumb spots against the world as it is right now, with the
//! game's own colliders: precomputed navigation data can't know that a passage is
//! blocked by a rock, or that one has since been cleared.

use std::time::{Duration, Instant};

use game_maps_protocol::{GroundPoint, RouteCheck};
use unity_runtime::{ManagedArray, Physics, Runtime, Vector3};

/// Rays looking for the floor start this high above the last floor point.
const LIFT: f32 = 1.0;
/// The floor is sampled this often (meters) along each leg.
const SAMPLE: f32 = 0.5;
/// Rises above this between samples are ledges a player can't step up.
const MAX_STEP: f32 = 0.6;
/// Farthest down a floor is looked for below the last one (deeper drops are unknown).
const MAX_DROP: f32 = 3.0;
/// The body sweep: a sphere this high above the floor, this wide. It covers knee to
/// chest height, so small bumps underfoot don't count but rocks and walls do.
const BODY_HEIGHT: f32 = 0.95;
const BODY_RADIUS: f32 = 0.3;
/// Faces whose normal points up at least this much are floors.
const FLOOR_NORMAL_Y: f32 = 0.35;
/// Ground probes look for floors within this distance of each point, up and down.
const PROBE_REACH: f32 = 2.0;
/// Breadcrumbs are seen when the line of sight reaches this close to them.
const SIGHT_SLACK: f32 = 0.4;
/// Breadcrumbs sit this far above the floor when checking line of sight.
const SIGHT_HEIGHT: f32 = 0.3;
/// Routes run cell to cell on a coarse grid; a player steps around rubble and wall bumps
/// beside that line. A leg is blocked only when lines this far to either side (meters)
/// are blocked too: a rock across a passage blocks, one in the middle of it doesn't.
const SIDE_STEP: f32 = 0.45;

const UP: Vector3 = Vector3 { x: 0.0, y: 1.0, z: 0.0 };

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub point: Vector3,
    pub normal_y: f32,
    pub distance: f32,
}

/// The physics queries the checks need.
pub trait World {
    /// First solid hit on the segment from `from` to `to`.
    fn ray(&self, from: Vector3, to: Vector3) -> Option<Hit>;
    /// Every solid hit on the segment from `from` to `to`.
    fn ray_all(&self, from: Vector3, to: Vector3) -> Vec<Hit>;
    /// Whether a sphere of `radius` moving from `from` to `to` hits anything solid.
    fn sweep_blocked(&self, from: Vector3, to: Vector3, radius: f32) -> bool;
}

/// What stops a player on a leg of a route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Blocker {
    /// A rise too high to step up.
    Ledge,
    /// Something solid at body height.
    Obstacle,
}

/// Where a leg is blocked, and by what.
#[derive(Debug, Clone, Copy)]
pub struct Block {
    pub blocker: Blocker,
    pub at: Vector3,
}

/// Which legs between consecutive `points` (on the floor, about a meter apart) a player
/// can't walk now. Stops when `budget` runs out; `checked` tells how far it got.
pub fn check_route(world: &impl World, points: &[Vector3], budget: Duration) -> (RouteCheck, Vec<Block>) {
    let started = Instant::now();
    let mut check = RouteCheck { blocked: Vec::new(), checked: 0 };
    let mut blocks = Vec::new();
    for (i, leg) in points.windows(2).enumerate() {
        if started.elapsed() > budget {
            break;
        }
        if let Some((blocker, at)) = leg_blocker(world, leg[0], leg[1]).filter(|_| sides_blocked(world, leg[0], leg[1])) {
            check.blocked.push(i as u32);
            blocks.push(Block { blocker, at });
        }
        check.checked = i as u32 + 1;
    }
    (check, blocks)
}

/// Walks the floor from `a` to `b` in short steps: blocked by a ledge too high to step
/// up, or by anything solid at body height.
fn leg_blocker(world: &impl World, a: Vector3, b: Vector3) -> Option<(Blocker, Vector3)> {
    let mut prev = Vector3 { y: floor_below(world, a).unwrap_or(a.y), ..a };
    let steps = (horizontal(a, b) / SAMPLE).ceil().max(1.0) as usize;
    for k in 1..=steps {
        let q = lerp(a, b, k as f32 / steps as f32);
        // No floor found: not loaded or a deep drop; trust the route's height.
        let y = floor_below(world, Vector3 { y: prev.y, ..q }).unwrap_or(q.y);
        if y - prev.y > MAX_STEP {
            return Some((Blocker::Ledge, q));
        }
        let next = Vector3 { y, ..q };
        // Level at the higher of the two floors: stepping off a ledge moves forward, then
        // falls (a diagonal would clip the ledge's edge).
        let body = prev.y.max(y) + BODY_HEIGHT;
        if world.sweep_blocked(Vector3 { y: body, ..prev }, Vector3 { y: body, ..next }, BODY_RADIUS) {
            return Some((Blocker::Obstacle, next));
        }
        prev = next;
    }
    None
}

/// Whether lines `SIDE_STEP` to the left and right of the leg are both blocked.
fn sides_blocked(world: &impl World, a: Vector3, b: Vector3) -> bool {
    let length = horizontal(a, b);
    if length < 1e-4 {
        return true;
    }
    let side = Vector3 { x: -(b.z - a.z) / length * SIDE_STEP, y: 0.0, z: (b.x - a.x) / length * SIDE_STEP };
    [side, scale(side, -1.0)].iter().all(|&o| leg_blocker(world, add(a, o), add(b, o)).is_some())
}

/// Height of the first surface below `LIFT` above `p`, down to `MAX_DROP` below it.
fn floor_below(world: &impl World, p: Vector3) -> Option<f32> {
    world.ray(add(p, scale(UP, LIFT)), add(p, scale(UP, -MAX_DROP))).map(|h| h.point.y)
}

/// For each point: the floor nearest it, and whether `camera` can see it.
pub fn probe_ground(world: &impl World, points: &[Vector3], camera: Option<Vector3>) -> Vec<GroundPoint> {
    points
        .iter()
        .map(|&p| {
            let y = world
                .ray_all(add(p, scale(UP, PROBE_REACH)), add(p, scale(UP, -PROBE_REACH)))
                .into_iter()
                .filter(|h| h.normal_y >= FLOOR_NORMAL_Y)
                .map(|h| h.point.y)
                .min_by(|a, b| (a - p.y).abs().total_cmp(&(b - p.y).abs()));
            let spot = Vector3 { y: y.unwrap_or(p.y) + SIGHT_HEIGHT, ..p };
            let visible = camera.is_some_and(|c| {
                let length = len(sub(spot, c));
                world.ray(c, spot).is_none_or(|h| h.distance >= length - SIGHT_SLACK)
            });
            GroundPoint { y, visible }
        })
        .collect()
}

/// [`World`] backed by the game's physics, for one main-thread request.
pub struct PhysicsWorld<'a> {
    pub rt: &'a Runtime,
    pub physics: &'a Physics,
    /// Layers that are solid for these checks.
    pub mask: i32,
    pub hits: ManagedArray,
}

impl World for PhysicsWorld<'_> {
    fn ray(&self, from: Vector3, to: Vector3) -> Option<Hit> {
        let (dir, length) = direction(from, to)?;
        unsafe { self.physics.raycast(self.rt, from, dir, length, self.mask) }.map(to_hit)
    }

    fn ray_all(&self, from: Vector3, to: Vector3) -> Vec<Hit> {
        let Some((dir, length)) = direction(from, to) else { return Vec::new() };
        unsafe { self.physics.raycast_all(self.rt, from, dir, length, self.mask, &self.hits) }
            .map(|hits| hits.into_iter().map(to_hit).collect())
            .unwrap_or_default()
    }

    fn sweep_blocked(&self, from: Vector3, to: Vector3, radius: f32) -> bool {
        let Some((dir, length)) = direction(from, to) else { return false };
        unsafe { self.physics.sphere_cast(self.rt, from, radius, dir, length, self.mask) }.is_some()
    }
}

fn to_hit(h: unity_runtime::RayHit) -> Hit {
    Hit { point: h.point, normal_y: h.normal.y, distance: h.distance }
}

fn direction(from: Vector3, to: Vector3) -> Option<(Vector3, f32)> {
    let d = sub(to, from);
    let length = len(d);
    (length > 1e-4).then(|| (scale(d, 1.0 / length), length))
}

fn add(a: Vector3, b: Vector3) -> Vector3 {
    Vector3 { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z }
}

fn sub(a: Vector3, b: Vector3) -> Vector3 {
    Vector3 { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}

fn scale(a: Vector3, s: f32) -> Vector3 {
    Vector3 { x: a.x * s, y: a.y * s, z: a.z * s }
}

fn len(a: Vector3) -> f32 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

fn horizontal(a: Vector3, b: Vector3) -> f32 {
    (b.x - a.x).hypot(b.z - a.z)
}

fn lerp(a: Vector3, b: Vector3, t: f32) -> Vector3 {
    add(a, scale(sub(b, a), t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A floor `height(x)` along x (unbounded in z) with solid boxes standing on it.
    struct Fake {
        height: fn(f32) -> Option<f32>,
        boxes: Vec<(Vector3, Vector3)>,
    }

    impl Fake {
        fn solid(&self, p: Vector3) -> bool {
            self.boxes.iter().any(|(lo, hi)| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && p.z >= lo.z && p.z <= hi.z)
                || (self.height)(p.x).is_some_and(|h| p.y <= h)
        }

        /// Marches along the segment; the first solid point is the hit.
        fn march(&self, from: Vector3, to: Vector3, mut keep: impl FnMut(Hit) -> bool) {
            let length = len(sub(to, from));
            let steps = (length / 0.01).ceil() as usize;
            let mut inside = self.solid(from);
            for k in 1..=steps {
                let p = lerp(from, to, k as f32 / steps as f32);
                let solid = self.solid(p);
                if solid && !inside {
                    let on_box = self.boxes.iter().any(|(lo, hi)| p.y <= hi.y && p.y >= hi.y - 0.02 && p.x >= lo.x && p.x <= hi.x);
                    let normal_y = if on_box || (self.height)(p.x).is_some_and(|h| p.y >= h - 0.02) { 1.0 } else { 0.0 };
                    if !keep(Hit { point: p, normal_y, distance: length * k as f32 / steps as f32 }) {
                        return;
                    }
                }
                inside = solid;
            }
        }
    }

    impl World for Fake {
        fn ray(&self, from: Vector3, to: Vector3) -> Option<Hit> {
            let mut first = None;
            self.march(from, to, |h| {
                first = Some(h);
                false
            });
            first
        }

        fn ray_all(&self, from: Vector3, to: Vector3) -> Vec<Hit> {
            let mut all = Vec::new();
            self.march(from, to, |h| {
                all.push(h);
                true
            });
            all
        }

        fn sweep_blocked(&self, from: Vector3, to: Vector3, radius: f32) -> bool {
            // Like Unity, ignores what the sphere already touches where it starts: only
            // the side facing the motion can hit anything.
            let d = sub(to, from);
            let steps = (len(d) / 0.05).ceil().max(1.0) as usize;
            let ring = [(0.0, -1.0, 0.0), (1.0, 0.0, 0.0), (-1.0, 0.0, 0.0), (0.0, 0.0, 1.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0)];
            let leading: Vec<Vector3> = ring.iter().map(|&(x, y, z)| Vector3 { x, y, z }).filter(|o| o.x * d.x + o.y * d.y + o.z * d.z > 0.0).collect();
            (1..=steps).any(|k| {
                let c = lerp(from, to, k as f32 / steps as f32);
                leading.iter().any(|&o| self.solid(add(c, scale(o, radius))))
            })
        }
    }

    fn v(x: f32, y: f32) -> Vector3 {
        Vector3 { x, y, z: 0.0 }
    }

    /// Route points a meter apart along x (cell centers, like the cave grids), on the floor.
    fn route(world: &Fake, to: i32) -> Vec<Vector3> {
        (0..=to).map(|x| x as f32 + 0.5).map(|x| v(x, (world.height)(x).unwrap_or(0.0))).collect()
    }

    fn blocked(world: &Fake, to: i32) -> Vec<u32> {
        check_route(world, &route(world, to), Duration::from_secs(5)).0.blocked
    }

    #[test]
    fn open_floor_is_walkable() {
        let flat = Fake { height: |_| Some(0.0), boxes: vec![] };
        assert!(blocked(&flat, 6).is_empty());
    }

    #[test]
    fn slopes_and_small_bumps_are_walkable() {
        let slope = Fake { height: |x| Some(x * 0.9), boxes: vec![] };
        assert!(blocked(&slope, 6).is_empty());
        let bump = Fake { height: |_| Some(0.0), boxes: vec![(v(2.2, 0.0), v(2.6, 0.25))] };
        assert!(blocked(&bump, 6).is_empty());
    }

    #[test]
    fn rocks_and_ledges_block() {
        let rock = Fake { height: |_| Some(0.0), boxes: vec![(Vector3 { x: 2.8, y: 0.0, z: -3.0 }, Vector3 { x: 3.3, y: 1.1, z: 3.0 })] };
        assert!(blocked(&rock, 6).contains(&2), "{:?}", blocked(&rock, 6));
        let ledge = Fake { height: |x| Some(if x < 3.0 { 0.0 } else { 1.5 }), boxes: vec![] };
        assert_eq!(blocked(&ledge, 6), vec![2]);
        let (_, blocks) = check_route(&ledge, &route(&ledge, 6), Duration::from_secs(5));
        assert_eq!(blocks[0].blocker, Blocker::Ledge);
        assert!((blocks[0].at.x - 3.0).abs() < 0.01);
        // Dropping down a ledge is fine.
        let down = Fake { height: |x| Some(if x < 3.0 { 1.5 } else { 0.0 }), boxes: vec![] };
        assert!(blocked(&down, 6).is_empty(), "{:?}", blocked(&down, 6));
    }

    #[test]
    fn rocks_a_player_can_step_around_do_not_block() {
        let pebble = Fake { height: |_| Some(0.0), boxes: vec![(Vector3 { x: 2.8, y: 0.0, z: -0.15 }, Vector3 { x: 3.3, y: 1.1, z: 0.15 })] };
        assert!(blocked(&pebble, 6).is_empty());
    }

    #[test]
    fn missing_floor_is_not_a_block() {
        let none = Fake { height: |_| None, boxes: vec![] };
        assert!(blocked(&none, 4).is_empty());
    }

    #[test]
    fn budget_limits_the_check() {
        let flat = Fake { height: |_| Some(0.0), boxes: vec![] };
        let (check, _) = check_route(&flat, &route(&flat, 6), Duration::ZERO);
        assert!(check.checked <= 1);
    }

    #[test]
    fn probes_snap_to_the_floor_and_see_past_nothing_solid() {
        let wall = Fake { height: |x| Some(x * 0.1), boxes: vec![(Vector3 { x: 4.0, y: 0.0, z: -9.0 }, Vector3 { x: 4.5, y: 5.0, z: 9.0 })] };
        let camera = Some(v(0.0, 1.6));
        let probes = probe_ground(&wall, &[v(2.0, 1.0), v(6.0, 0.0)], camera);
        assert!((probes[0].y.unwrap() - 0.2).abs() < 0.05, "{probes:?}");
        assert!(probes[0].visible);
        assert!((probes[1].y.unwrap() - 0.6).abs() < 0.05);
        assert!(!probes[1].visible, "the wall hides it");
        assert!(!probe_ground(&wall, &[v(1.0, 0.0)], None)[0].visible);
    }
}
