//! Creatures around the player: the AI actors the game has active
//! (`Sons.Ai.Vail.VailActorManager._instance._activeActors`), which is everything within
//! its own simulation distance. Each actor's type (`VailActorTypeId`: "Fingers",
//! "Deer", …) and class (`VailActorClassId`: Cannibal, Creepy, Animal, Other) tell what
//! it is to the player.

use std::collections::HashMap;

use game_maps_protocol::{Creature, CreatureKind};
use unity_runtime::{Field, ListLayout, ManagedObject, Method, Runtime, Unity, Vector3, WeakRef};

use crate::provider::to_proto;

/// Upper bound on actors read per refresh (the game keeps well under this active).
const MAX_ACTORS: usize = 512;
/// Types left out: Kelvin is already on the map through his GPS tracker, and the rest
/// aren't creatures.
const SKIPPED_TYPES: &[&str] = &["Robby", "PlayerRobby", "Helicopter", "None"];
/// Types on the player's side whatever their class.
const FRIENDLY_TYPES: &[&str] = &["Virginia"];

pub struct Creatures {
    instance: Field,
    active: Field,
    type_id: Method,
    class_id: Method,
    is_dead: Method,
    list_layout: Option<ListLayout>,
    /// Companions' actors from the last read, while they're active. Weak references: the
    /// game can drop an actor at any time, and a raw pointer kept across frames would then
    /// point at whatever took its place.
    kelvin: Option<WeakRef>,
    virginia: Option<WeakRef>,
    /// Enum names by value (they never change; reading them allocates).
    type_names: HashMap<i32, Option<String>>,
    class_names: HashMap<i32, Option<String>>,
}

// Only used on the main thread.
unsafe impl Send for Creatures {}

/// What a weak reference points at now: null when there's none, or the object is gone.
unsafe fn target(actor: &Option<WeakRef>) -> *mut ManagedObject {
    actor.as_ref().map_or(std::ptr::null_mut(), |a| a.get())
}

impl Creatures {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let manager = rt.class("Sons.Ai.Vail", "VailActorManager").ok()?;
        let actor = rt.class("Sons.Ai.Vail", "VailActor").ok()?;
        Some(Self {
            instance: rt.field(manager, "_instance").ok().filter(|f| f.is_static)?,
            active: rt.field(manager, "_activeActors").ok()?,
            type_id: rt.method(actor, "get_TypeId", 0).ok()?,
            class_id: rt.method(actor, "get_ClassId", 0).ok()?,
            is_dead: rt.method(actor, "IsDead", 0).ok()?,
            list_layout: None,
            kelvin: None,
            virginia: None,
            type_names: HashMap::new(),
            class_names: HashMap::new(),
        })
    }

    /// Drops the companions' actors: they don't outlive the world they're in.
    pub fn forget_companions(&mut self) {
        self.kelvin = None;
        self.virginia = None;
    }

    /// Kelvin's actor as last read: null when he isn't active, or has gone since. Still to
    /// be checked with `is_alive`, and only good for this frame.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn kelvin(&self) -> *mut ManagedObject {
        target(&self.kelvin)
    }

    /// Virginia's actor (see [`Creatures::kelvin`]).
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn virginia(&self) -> *mut ManagedObject {
        target(&self.virginia)
    }

    /// Living creatures within `radius` of `around`.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity, around: Vector3, radius: f32) -> Result<Vec<Creature>, String> {
        self.forget_companions();
        let manager: *mut ManagedObject = rt.static_value(self.instance);
        if !unity.is_alive(manager) {
            return Ok(Vec::new());
        }
        let list: *mut ManagedObject = self.active.get(manager);
        if list.is_null() {
            return Ok(Vec::new());
        }
        let layout = match self.list_layout {
            Some(l) => l,
            None => *self.list_layout.insert(ListLayout::of(rt, list).map_err(|e| format!("active actors are stored in an unsupported list: {e}"))?),
        };
        let mut creatures = Vec::new();
        for actor in layout.elements(list, MAX_ACTORS) {
            if !unity.is_alive(actor) {
                continue;
            }
            // Before the distance check: companions matter wherever they are.
            let type_name = enum_name(rt, actor, &self.type_id, &mut self.type_names);
            match type_name.as_deref() {
                Some("Robby") => self.kelvin = rt.weak_ref(actor),
                Some("Virginia") => self.virginia = rt.weak_ref(actor),
                _ => {}
            }
            let Ok(position) = unity.component_transform(rt, actor).and_then(|t| unity.position(rt, t)) else { continue };
            let (dx, dz) = (position.x - around.x, position.z - around.z);
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            if rt.invoke(&self.is_dead, actor, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(true) {
                continue;
            }
            let Some(name) = type_name else { continue };
            if SKIPPED_TYPES.contains(&name.as_str()) {
                continue;
            }
            let kind = if FRIENDLY_TYPES.contains(&name.as_str()) {
                CreatureKind::Friendly
            } else {
                match enum_name(rt, actor, &self.class_id, &mut self.class_names).as_deref() {
                    Some("Cannibal" | "Creepy") => CreatureKind::Hostile,
                    Some("Animal") => CreatureKind::Animal,
                    _ => continue,
                }
            };
            creatures.push(Creature { id: format!("actor:{:x}", actor as usize), kind, name, position: to_proto(position) });
        }
        Ok(creatures)
    }
}

/// The name of the enum value `getter` returns for `actor`, cached by value.
unsafe fn enum_name(rt: &Runtime, actor: *mut ManagedObject, getter: &Method, cache: &mut HashMap<i32, Option<String>>) -> Option<String> {
    let boxed = rt.invoke(getter, actor, &mut []).ok()?;
    let value: i32 = rt.unbox(boxed).ok()?;
    cache.entry(value).or_insert_with(|| rt.enum_name(boxed)).clone()
}
