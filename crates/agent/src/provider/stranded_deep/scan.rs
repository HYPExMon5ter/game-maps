//! Remembers what's in the zones the game has loaded around the player: wrecks, crates,
//! plants, dens and the player's own things. A zone's objects only exist once it's been
//! visited, and only while it's loaded, so the app keeps what was seen (`resources.json`).
//!
//! Everything saved in a zone is a child of its `SaveContainer`. A few dozen children are
//! read per step, so a zone of a few hundred objects is read over a second or two.

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use game_maps_protocol::{Vec3, WorldPin, ZonePins};
use unity_runtime::{Class, ManagedObject, Method, Runtime, WeakRef};

use super::objects;
use crate::provider::{to_proto, Cx};

/// How often a step runs.
const STEP: Duration = Duration::from_millis(100);
/// Children read per step.
const BATCH: i32 = 40;
/// A loaded zone is read again this often, so what's taken or built shows up.
const RESCAN: Duration = Duration::from_secs(20);
/// More than any zone holds: a broken count never runs away.
const MAX_CHILDREN: i32 = 20_000;

pub struct TransformMembers {
    pub child_count: Method,
    pub child: Method,
}

impl TransformMembers {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let transform = rt.class("UnityEngine", "Transform").ok()?;
        Some(Self { child_count: rt.method(transform, "get_childCount", 0).ok()?, child: rt.method(transform, "GetChild", 1).ok()? })
    }
}

/// A loaded zone to read: its index and the transform holding what's saved in it.
pub struct LoadedZone {
    pub index: usize,
    pub container: *mut ManagedObject,
    pub center: Vec3,
}

struct Job {
    zone: usize,
    container: *mut ManagedObject,
    count: i32,
    next: i32,
    found: Vec<(&'static str, &'static str, Vec3)>,
    spawners: Vec<WeakRef>,
}

pub struct Scanner {
    transform: TransformMembers,
    /// `LandAnimalSpawner`: each land animal belongs to one, in its zone.
    spawner_class: Option<Class>,
    job: Option<Job>,
    last_step: Option<Instant>,
    /// When each zone was last read.
    scanned: HashMap<usize, Instant>,
    /// The animal spawners found in each zone. They go when their zone unloads, and stay
    /// in this list until it's read again, so they're weak references.
    spawners: HashMap<usize, Vec<WeakRef>>,
    /// Time spent reading, for the log.
    pub busy: Duration,
    pub steps: u32,
}

impl Scanner {
    pub fn new(transform: TransformMembers, spawner_class: Option<Class>) -> Self {
        Self { transform, spawner_class, job: None, last_step: None, scanned: HashMap::new(), spawners: HashMap::new(), busy: Duration::ZERO, steps: 0 }
    }

    /// Another world: nothing read so far belongs to it.
    pub fn reset(&mut self) {
        self.job = None;
        self.scanned.clear();
        self.spawners.clear();
    }

    /// Animal spawners in the zones read so far; some may be gone since (check they're alive).
    pub fn spawners(&self) -> impl Iterator<Item = &WeakRef> + '_ {
        self.spawners.values().flatten()
    }

    /// One step of reading. `loaded` lists the zones the game has loaded now; the one
    /// nearest `around` that's due goes first.
    ///
    /// # Safety
    /// Main thread only; `loaded` must hold live transforms.
    pub unsafe fn next(&mut self, cx: &Cx, loaded: &[LoadedZone], around: Vec3) -> Option<ZonePins> {
        let now = Instant::now();
        if self.last_step.is_some_and(|t| now - t < STEP) {
            return None;
        }
        self.last_step = Some(now);
        if self.job.as_ref().is_some_and(|j| !loaded.iter().any(|z| z.index == j.zone && z.container == j.container)) {
            // Unloaded while being read: start over next time it loads.
            self.job = None;
        }
        if self.job.is_none() {
            let due = loaded
                .iter()
                .filter(|z| self.scanned.get(&z.index).is_none_or(|t| now - *t >= RESCAN))
                .min_by(|a, b| distance(a.center, around).total_cmp(&distance(b.center, around)))?;
            let count = self.child_count(cx, due.container)?;
            self.job = Some(Job { zone: due.index, container: due.container, count: count.clamp(0, MAX_CHILDREN), next: 0, found: Vec::new(), spawners: Vec::new() });
        }
        let started = Instant::now();
        let finished = self.step(cx);
        self.busy += started.elapsed();
        self.steps += 1;
        if !finished {
            return None;
        }
        let job = self.job.take()?;
        self.scanned.insert(job.zone, now);
        self.spawners.insert(job.zone, job.spawners);
        Some(ZonePins { x: job.zone as i32, y: 0, pins: pins(job.found) })
    }

    unsafe fn child_count(&self, cx: &Cx, container: *mut ManagedObject) -> Option<i32> {
        if !cx.unity.is_alive(container) {
            return None;
        }
        cx.rt.unbox::<i32>(cx.rt.invoke(&self.transform.child_count, container, &mut []).ok()?).ok()
    }

    /// Reads the next batch; true once the zone is done.
    unsafe fn step(&mut self, cx: &Cx) -> bool {
        let Some(job) = self.job.as_mut() else { return true };
        if !cx.unity.is_alive(job.container) {
            return true;
        }
        let end = (job.next + BATCH).min(job.count);
        while job.next < end {
            let mut index = job.next;
            job.next += 1;
            let mut args = [&mut index as *mut i32 as *mut c_void];
            // Out of range if something went away meanwhile: what's been read is kept.
            let Ok(child) = cx.rt.invoke(&self.transform.child, job.container, &mut args) else { return true };
            let Some(name) = cx.unity.object_name(cx.rt, child) else { continue };
            if let Some(class) = self.spawner_class.filter(|_| objects::is_spawner(&name)) {
                let spawner = cx.unity.component_game_object(cx.rt, child).ok().and_then(|go| cx.unity.get_component(cx.rt, go, class).ok().flatten());
                job.spawners.extend(spawner.and_then(|s| cx.rt.weak_ref(s)));
            }
            let Some((category, kind)) = objects::classify(&name) else { continue };
            let Ok(position) = cx.unity.position(cx.rt, child) else { continue };
            job.found.push((category, kind, to_proto(position)));
        }
        job.next >= job.count
    }
}

fn distance(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).hypot(a.z - b.z)
}

/// One pin per group of things of a kind close together.
fn pins(found: Vec<(&'static str, &'static str, Vec3)>) -> Vec<WorldPin> {
    let mut groups: Vec<(&'static str, &'static str, Vec<Vec3>)> = Vec::new();
    for (category, kind, at) in found {
        let reach = objects::cluster_meters(category);
        match groups.iter_mut().find(|(c, k, members)| *c == category && *k == kind && members.iter().any(|m| distance(*m, at) <= reach)) {
            Some((_, _, members)) => members.push(at),
            None => groups.push((category, kind, vec![at])),
        }
    }
    groups
        .into_iter()
        .map(|(category, kind, members)| {
            let n = members.len() as f32;
            let center = Vec3::new(members.iter().map(|m| m.x).sum::<f32>() / n, members.iter().map(|m| m.y).fold(f32::MIN, f32::max), members.iter().map(|m| m.z).sum::<f32>() / n);
            // Named by the member furthest west, which stays put when others are taken.
            let anchor = members.iter().copied().min_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z))).unwrap_or(center);
            let slug = kind.to_ascii_lowercase().replace(' ', "-");
            WorldPin { id: format!("{slug}@{:.0},{:.0}", anchor.x, anchor.z), category: category.to_owned(), name: kind.to_owned(), position: center, hidden: false, respawn_seconds: None, respawn_at: None }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn things_of_a_kind_close_together_are_one_pin() {
        let found = vec![
            ("food", "Potato", Vec3::new(0.0, 1.0, 0.0)),
            ("food", "Potato", Vec3::new(5.0, 2.0, 0.0)),
            ("food", "Potato", Vec3::new(200.0, 1.0, 0.0)),
            ("food", "Yucca", Vec3::new(1.0, 1.0, 0.0)),
        ];
        let pins = pins(found);
        assert_eq!(pins.len(), 3);
        let field = pins.iter().find(|p| p.name == "Potato" && p.position.x < 100.0).unwrap();
        assert_eq!(field.position, Vec3::new(2.5, 2.0, 0.0));
        assert_eq!(field.id, "potato@0,0");
    }

    #[test]
    fn a_pin_keeps_its_id_when_a_member_goes() {
        let both = pins(vec![("food", "Potato", Vec3::new(0.0, 1.0, 0.0)), ("food", "Potato", Vec3::new(5.0, 1.0, 0.0))]);
        let one = pins(vec![("food", "Potato", Vec3::new(0.0, 1.0, 0.0))]);
        assert_eq!(both[0].id, one[0].id);
    }
}
