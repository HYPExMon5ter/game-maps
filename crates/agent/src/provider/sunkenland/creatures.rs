//! Creatures around the player: `WorldScene.code.AllCharactersList`, every live
//! `Character` (players and companions are other classes). `Character.EnemyType` only
//! names animals, mutants and bosses; ordinary enemies are `None`, often with no `Faction`
//! either. So anything alive that isn't in a friendly faction counts as hostile.
//! Enemy ships are `WorldScene.code.EnemyBoatComponents`.

use game_maps_protocol::{Creature, CreatureKind};
use unity_runtime::{Class, Field, ListLayout, ManagedObject, Method, Runtime, Unity, Vector3};

use crate::provider::to_proto;

/// `EnemyType` names we show, and what they are to the player.
const TYPES: &[(&str, CreatureKind)] = &[
    ("Animal", CreatureKind::Animal),
    ("MalinFish", CreatureKind::Animal),
    ("Shark", CreatureKind::Hostile),
    ("Human", CreatureKind::Hostile),
    ("Mutant", CreatureKind::Hostile),
    ("PirateKing", CreatureKind::Hostile),
    ("PirateOnBoatQuest", CreatureKind::Hostile),
    ("PirateBridgeQuest", CreatureKind::Hostile),
    ("SlaveOwner", CreatureKind::Hostile),
    ("Warlord", CreatureKind::Hostile),
];
const MAX_CHARACTERS: usize = 4096;
/// Most creatures reported at once; the map only needs what's close.
const MAX_CREATURES: usize = 64;

pub struct Creatures {
    characters: Field,
    enemy_type: Field,
    transform: Field,
    boats: Option<Field>,
    boat_position: Option<Field>,
    boat_destroyed: Option<Field>,
    faction: Field,
    is_friendly: Field,
    is_dead: Method,
    /// `EnemyType` value, name and kind of each of `TYPES`.
    types: Vec<(i32, &'static str, CreatureKind)>,
    layout: Option<ListLayout>,
}

impl Creatures {
    /// `world_scene`: the `WorldScene` class.
    pub fn resolve(rt: &Runtime, world_scene: Class) -> Option<Self> {
        let character = rt.class("", "Character").ok()?;
        let faction = rt.class("", "Faction").ok()?;
        let enemy_type = rt.class("", "EnemyType").ok()?;
        let types: Vec<_> = TYPES
            .iter()
            .filter_map(|&(name, kind)| {
                let field = rt.field(enemy_type, name).ok().filter(|f| f.is_static)?;
                Some((unsafe { rt.static_value::<i32>(field) }, name, kind))
            })
            .collect();
        (!types.is_empty()).then_some(())?;
        let boat = rt.class("", "Boat").ok();
        Some(Self {
            characters: rt.field(world_scene, "AllCharactersList").ok()?,
            enemy_type: rt.field(character, "EnemyType").ok()?,
            transform: rt.field(character, "myTransform").ok()?,
            boats: rt.field(world_scene, "EnemyBoatComponents").ok(),
            boat_position: boat.and_then(|c| rt.field(c, "TransformPosition").ok()),
            boat_destroyed: boat.and_then(|c| rt.field(c, "Destroied").ok()),
            faction: rt.field(character, "Faction").ok()?,
            is_friendly: rt.field(faction, "IsFriendly").ok()?,
            is_dead: rt.method(character, "get_IsDead", 0).ok()?,
            types,
            layout: None,
        })
    }

    /// How many `EnemyType`s were found, for the log.
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Hostiles and animals within `radius` of `around`, nearest first.
    ///
    /// # Safety
    /// Must run on the game's main thread; `scene` must be the live `WorldScene`.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity, scene: *mut ManagedObject, around: Vector3, radius: f32) -> Result<Vec<Creature>, String> {
        let list: *mut ManagedObject = self.characters.get(scene);
        if list.is_null() {
            return Ok(Vec::new());
        }
        let layout = match self.layout {
            Some(l) => l,
            None => *self.layout.insert(ListLayout::of(rt, list).map_err(|e| e.to_string())?),
        };
        let near = |p: Vector3| {
            let (dx, dy, dz) = (p.x - around.x, p.y - around.y, p.z - around.z);
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            (d <= radius).then_some(d)
        };
        let mut found: Vec<(f32, Creature)> = Vec::new();
        for character in layout.elements(list, MAX_CHARACTERS) {
            if character.is_null() || !unity.is_alive(character) {
                continue;
            }
            let transform: *mut ManagedObject = self.transform.get(character);
            let Ok(p) = unity.position(rt, transform) else { continue };
            let Some(distance) = near(p) else { continue };
            let faction: *mut ManagedObject = self.faction.get(character);
            let friendly = !faction.is_null() && self.is_friendly.get::<bool>(faction);
            let dead = rt.invoke(&self.is_dead, character, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(false);
            let value: i32 = self.enemy_type.get(character);
            if dead || friendly {
                continue;
            }
            let (name, kind) = match self.types.iter().find(|t| t.0 == value) {
                Some(&(_, name, kind)) => (name, kind),
                None => ("Enemy", CreatureKind::Hostile),
            };
            found.push((distance, Creature { id: format!("actor:{:x}", character as usize), kind, name: name.into(), position: to_proto(p) }));
        }
        if let (Some(boats), Some(position)) = (self.boats, self.boat_position) {
            let list: *mut ManagedObject = boats.get(scene);
            if !list.is_null() {
                for boat in layout.elements(list, MAX_CHARACTERS) {
                    if boat.is_null() || !unity.is_alive(boat) || self.boat_destroyed.is_some_and(|f| f.get::<bool>(boat)) {
                        continue;
                    }
                    let p: Vector3 = position.get(boat);
                    if let Some(distance) = near(p) {
                        let creature = Creature { id: format!("ship:{:x}", boat as usize), kind: CreatureKind::Hostile, name: "Enemy ship".into(), position: to_proto(p) };
                        found.push((distance, creature));
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.truncate(MAX_CREATURES);
        let found = found.into_iter().map(|(_, c)| c).collect();
        Ok(found)
    }
}
