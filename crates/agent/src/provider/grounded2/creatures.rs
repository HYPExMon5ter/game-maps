//! Creatures around the player, and their buggy (the tamed creature that follows them).

use std::collections::HashMap;
use std::sync::Arc;

use game_maps_protocol::{CompanionOrderOption, CompanionStat, CompanionStatus, Creature, CreatureKind, Vec3};
use unreal_runtime::{Args, FString, FVector, Function, Obj, Property, TArray, Unreal};

use crate::unreal::{to_proto, UeCx};

/// Creatures farther than this (meters) aren't reported. The game only keeps creatures
/// near players as full actors anyway.
const CREATURE_RANGE: f32 = 150.0;
/// The buggy's id in `CompanionStatus` and companion orders.
pub const BUGGY: &str = "buggy";

/// `ETeamAttitude`
const ATTITUDE_FRIENDLY: u8 = 0;
const ATTITUDE_HOSTILE: u8 = 2;
/// `EHealthState`
const HEALTH_INCAPACITATED: u8 = 1;
const HEALTH_DEAD: u8 = 2;
/// `ELootBagType`
const PLAYER_BACKPACK: u8 = 1;
/// `EActivePetBehaviorType`
const BEHAVIOR_FOLLOW: u8 = 0;
const BEHAVIOR_ROAM: u8 = 1;

pub struct CreatureMembers {
    lod_manager: Property,
    full_actors: Property,
    all_actors: Property,
    loot_bag: Option<Obj>,
    bag_owner: Option<Property>,
    bag_type: Option<Property>,
    creature_lod: Obj,
    location: Arc<Function>,
    /// The real actor of a LOD actor near players, and where its root is: read from
    /// memory, so looking through the creatures around costs no calls into the game.
    full_actor: Option<Property>,
    root: Option<Property>,
    root_location: Option<Property>,
    type_name: Arc<Function>,
    /// What a creature's AI is going after.
    target: Option<Arc<Function>>,
    team_class: Obj,
    relationship: Arc<Function>,
    pet_master_class: Option<Obj>,
    current_pet: Option<Property>,
    pet_health_state: Option<Property>,
    pet_behavior: Option<Property>,
    pet_health: Option<Property>,
    current_health: Option<Arc<Function>>,
    max_health: Option<Arc<Function>>,
    instance_name: Option<Arc<Function>>,
    send_home: Option<Arc<Function>>,
    whistle: Option<Arc<Function>>,
    cycle_behavior: Option<Arc<Function>>,
}

impl CreatureMembers {
    pub fn resolve(ue: &Unreal) -> unreal_runtime::Result<Self> {
        let class = |path: &str| ue.find_struct(path);
        let base = class("/Script/Maine.BaseLODActor")?;
        let character = class("/Script/Maine.CharacterLODActor")?;
        let team = class("/Script/Maine.TeamComponent")?;
        let pet_master = class("/Script/Maine.PetMasterComponent").ok();
        let pet_fn = |name: &str| pet_master.and_then(|p| ue.function(p, name).ok());
        let pet_prop = |name: &str, ty: &str| pet_master.and_then(|p| ue.property_of(p, name, ty).ok());
        let survival_character = class("/Script/Maine.SurvivalCharacter").ok();
        let health = class("/Script/Maine.HealthComponent").ok();
        Ok(Self {
            lod_manager: ue.property_of(class("/Script/Maine.SurvivalGameState")?, "LODActorManagerComponent", "Object<LODActorManagerComponent>")?,
            full_actors: ue.property_of(class("/Script/Maine.LODActorManagerComponent")?, "FullActors", "Array<Object<BaseLODActor>>")?,
            all_actors: ue.property_of(class("/Script/Maine.LODActorManagerComponent")?, "LODActors", "Array<Object<BaseLODActor>>")?,
            loot_bag: class("/Script/Maine.LootBagLODActor").ok(),
            bag_owner: class("/Script/Maine.LootBagLODActor").ok().and_then(|c| ue.property_of(c, "OwnerPlayerGuid", "Struct<Guid>").ok()),
            bag_type: class("/Script/Maine.LootBagLODActor").ok().and_then(|c| ue.property_of(c, "LootBagType", "Enum<ELootBagType>").ok()),
            creature_lod: class("/Script/Maine.CreatureLODActor")?,
            location: ue.function(base, "GetActorLocation")?,
            full_actor: ue.property_of(base, "FullActor", "Object<Actor>").ok(),
            root: class("/Script/Engine.Actor").ok().and_then(|a| ue.property_of(a, "RootComponent", "Object<SceneComponent>").ok()),
            root_location: class("/Script/Engine.SceneComponent").ok().and_then(|c| ue.property_of(c, "RelativeLocation", "Struct<Vector>").ok()),
            type_name: ue.function(character, "GetCharacterTypeName")?,
            target: ue.function(character, "GetBlackboardTargetActor").ok(),
            team_class: team,
            relationship: ue.function(team, "GetRelationshipToLODActor")?,
            pet_master_class: pet_master,
            current_pet: pet_prop("CurrentPetFull", "Object<SurvivalCreature>"),
            pet_health_state: pet_prop("CurrentPetHealthState", "Enum<EHealthState>"),
            pet_behavior: pet_prop("CurrentPetBehavior", "Enum<EActivePetBehaviorType>"),
            pet_health: survival_character.and_then(|c| ue.property_of(c, "HealthComponent", "Object<HealthComponent>").ok()),
            current_health: health.and_then(|h| ue.function(h, "GetCurrentHealth").ok()),
            max_health: health.and_then(|h| ue.function(h, "GetMaxHealth").ok()),
            instance_name: survival_character.and_then(|c| ue.function(c, "GetCharacterInstanceName").ok()),
            send_home: pet_fn("ServerTrySendPetHome"),
            whistle: pet_fn("ServerWhistle"),
            cycle_behavior: pet_fn("ServerCycleActivePetBehavior"),
        })
    }
}

/// Reads creatures and the buggy, remembering what doesn't change.
#[derive(Default)]
pub struct Creatures {
    /// Each creature's type name, by LOD actor (looked up once each).
    names: HashMap<Obj, String>,
    /// The buggy's own name, by creature.
    pet_names: HashMap<Obj, String>,
    /// Whether a LOD actor class is a creature's, so each class is looked up once.
    creature_classes: HashMap<Obj, bool>,
}

impl Creatures {
    /// Living creatures within range of `around`, as the player sees them, except `skip`
    /// (a creature's real actor). `players` are the players' characters.
    ///
    /// # Safety
    /// Game thread only; `game_state` must be the world's `SurvivalGameState`.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn nearby(&mut self, cx: &UeCx, m: &CreatureMembers, game_state: Obj, pawn: Obj, players: &[Obj], around: Vec3, skip: Option<Obj>) -> Vec<Creature> {
        let ue = &cx.ue;
        let Some(manager) = ue.get_obj(game_state, &m.lod_manager) else { return Vec::new() };
        let team = cx.component(pawn, m.team_class);
        let actors: TArray<Obj> = ue.get(manager, &m.full_actors);
        let mut out = Vec::new();
        for lod in actors.to_vec() {
            if lod == 0 {
                continue;
            }
            let class = ue.class_of(lod);
            let creature = *self.creature_classes.entry(class).or_insert_with(|| class != 0 && ue.derives(class, m.creature_lod));
            if !creature || !ue.alive(lod) {
                continue;
            }
            if skip.is_some() && m.full_actor.as_ref().and_then(|p| ue.get_obj(lod, p)) == skip {
                continue;
            }
            let Some(position) = position_of(cx, m, lod) else { continue };
            let (dx, dz) = (position.x - around.x, position.z - around.z);
            if dx * dx + dz * dz > CREATURE_RANGE * CREATURE_RANGE {
                continue;
            }
            // Most bugs are neutral until provoked: one going after a player counts as
            // hostile, whatever its team says.
            let target = m.target.as_ref().and_then(|f| cx.call_get::<Obj>(lod, f)).filter(|&t| t != 0);
            let kind = match team.and_then(|t| relationship(cx, &m.relationship, t, lod)) {
                _ if target.is_some_and(|t| players.contains(&t)) => CreatureKind::Hostile,
                Some(ATTITUDE_HOSTILE) => CreatureKind::Hostile,
                Some(ATTITUDE_FRIENDLY) => CreatureKind::Friendly,
                _ => CreatureKind::Animal,
            };
            let name = match self.names.get(&lod) {
                Some(n) => n.clone(),
                None => {
                    let n = cx.call_get::<[i32; 4]>(lod, &m.type_name).and_then(|[t, id, ..]| cx.loc_text(t, id)).unwrap_or_default();
                    self.names.insert(lod, n.clone());
                    n
                }
            };
            out.push(Creature { id: format!("{lod:x}"), kind, name, position });
        }
        // Creatures come and go; forget names of ones no longer around.
        if self.names.len() > 4096 {
            self.names.clear();
        }
        out
    }

    /// The player's buggy, while one is out, and its character.
    ///
    /// # Safety
    /// Game thread only; `pawn` must be the local player's character.
    pub unsafe fn buggy(&mut self, cx: &UeCx, m: &CreatureMembers, pawn: Obj) -> Option<(CompanionStatus, Obj)> {
        let ue = &cx.ue;
        let master = cx.component(pawn, m.pet_master_class?)?;
        let pet = ue.get_obj(master, m.current_pet.as_ref()?)?;
        let state = m.pet_health_state.as_ref().map(|p| ue.get::<u8>(master, p)).unwrap_or(0);
        let behavior = m.pet_behavior.as_ref().map(|p| ue.get::<u8>(master, p));
        let name = match self.pet_names.get(&pet) {
            Some(n) => n.clone(),
            None => {
                let n = m.instance_name.as_ref().and_then(|f| {
                    let mut args = Args::new(f);
                    args.set_bool("bFilter", true).ok()?;
                    ue.call(pet, &mut args).ok()?;
                    Some(args.get::<FString>("ReturnValue").ok()?.to_string_lossy())
                });
                let n = n.filter(|n| !n.is_empty()).unwrap_or_else(|| "Buggy".into());
                self.pet_names.insert(pet, n.clone());
                n
            }
        };
        let health = (|| {
            let component = ue.get_obj(pet, m.pet_health.as_ref()?)?;
            let value: f32 = cx.call_get(component, m.current_health.as_ref()?)?;
            let max: f32 = cx.call_get(component, m.max_health.as_ref()?)?;
            Some(CompanionStat { name: "Health".into(), value, max })
        })();
        let status = CompanionStatus {
            id: BUGGY.into(),
            name,
            position: cx.location(pet).map(to_proto),
            alive: state != HEALTH_DEAD,
            injured: state == HEALTH_INCAPACITATED,
            activity: behavior.map(|b| if b == BEHAVIOR_FOLLOW { "Following you" } else { "Roaming" }.to_owned()),
            stats: health.into_iter().collect(),
        };
        Some((status, pet))
    }

    /// Where the backpack `owner` (a player's `PlayerGuid`) dropped when they died is.
    ///
    /// # Safety
    /// Game thread only; `game_state` must be the world's `SurvivalGameState`.
    pub unsafe fn backpack(&self, cx: &UeCx, m: &CreatureMembers, game_state: Obj, owner: [u32; 4]) -> Option<Vec3> {
        let ue = &cx.ue;
        let (class, owner_prop, type_prop) = (m.loot_bag?, m.bag_owner.as_ref()?, m.bag_type.as_ref()?);
        let manager = ue.get_obj(game_state, &m.lod_manager)?;
        let actors: TArray<Obj> = ue.get(manager, &m.all_actors);
        actors
            .to_vec()
            .into_iter()
            .filter(|&a| a != 0 && ue.class_of(a) == class && ue.alive(a))
            .find(|&a| ue.get::<u8>(a, type_prop) == PLAYER_BACKPACK && ue.get::<[u32; 4]>(a, owner_prop) == owner)
            .and_then(|a| cx.call_get::<FVector>(a, &m.location))
            .map(to_proto)
    }

    /// What the buggy can be told to do.
    pub fn orders(m: &CreatureMembers) -> Vec<CompanionOrderOption> {
        let mut orders = Vec::new();
        let mut add = |available: bool, id: &str, label: &str| {
            if available {
                orders.push(CompanionOrderOption { id: id.into(), group: "Buggy".into(), label: label.into() });
            }
        };
        let behaviors = m.cycle_behavior.is_some() && m.pet_behavior.is_some();
        add(m.whistle.is_some(), "whistle", "Call over");
        add(behaviors, "follow", "Follow me");
        add(behaviors, "roam", "Roam");
        add(m.send_home.is_some(), "home", "Send home");
        orders
    }

    /// Gives the buggy an order from [`Creatures::orders`].
    ///
    /// # Safety
    /// Game thread only; `pawn` must be the local player's character.
    pub unsafe fn order(cx: &UeCx, m: &CreatureMembers, pawn: Obj, order: &str) -> Result<(), String> {
        let ue = &cx.ue;
        let master = m.pet_master_class.and_then(|c| cx.component(pawn, c)).ok_or("you have no buggy out")?;
        let f = match order {
            "whistle" => m.whistle.as_ref(),
            "follow" | "roam" => {
                let pet = m.current_pet.as_ref().and_then(|p| ue.get_obj(master, p));
                if pet.is_none() {
                    return Err("you have no buggy out".into());
                }
                // The game only cycles behaviors, as its own button does. It has two, so
                // one cycle reaches the other.
                let current = m.pet_behavior.as_ref().map(|p| ue.get::<u8>(master, p));
                let wanted = if order == "follow" { BEHAVIOR_FOLLOW } else { BEHAVIOR_ROAM };
                if current == Some(wanted) {
                    return Ok(());
                }
                m.cycle_behavior.as_ref()
            }
            "home" => m.send_home.as_ref(),
            _ => return Err(format!("unknown order {order}")),
        }
        .ok_or("the game doesn't have that order")?;
        let mut args = Args::new(f);
        cx.ue.call(master, &mut args).map_err(|e| e.to_string())
    }
}

/// A LOD actor's position: its real actor's root when it has one (a characters' root
/// isn't attached to anything, so its relative location is where it is), else asked.
unsafe fn position_of(cx: &UeCx, m: &CreatureMembers, lod: Obj) -> Option<Vec3> {
    let ue = &cx.ue;
    let from_root = (|| {
        let actor = ue.get_obj(lod, m.full_actor.as_ref()?)?;
        let root = ue.get_obj(actor, m.root.as_ref()?)?;
        Some(ue.get::<FVector>(root, m.root_location.as_ref()?))
    })();
    from_root.or_else(|| cx.call_get::<FVector>(lod, &m.location)).map(to_proto)
}

unsafe fn relationship(cx: &UeCx, f: &Function, team: Obj, lod: Obj) -> Option<u8> {
    let mut args = Args::new(f);
    args.set("Other", lod).ok()?;
    cx.ue.call(team, &mut args).ok()?;
    args.get::<u8>("ReturnValue").ok()
}
