//! Companions (`Assistant`): how they're doing, and the orders from the game's command menu.
//!
//! Orders go through `Assistant.ExecuteCommand` and `ExecuteGoSomeWhereAt`, the game's own
//! entry points, which send its network calls, so they work on clients too. It's the menu's
//! list, without what needs a target picked in the world (a vehicle, a mounted weapon).

use std::ffi::c_void;

use game_maps_protocol::{CompanionOrderOption, CompanionStat, CompanionStatus, Vec3};
use unity_runtime::{Field, ManagedObject, Method, Runtime, Unity, Vector3};

use crate::provider::to_unity;

/// `AssistantBehaviior`, by name: `ToggleFire` isn't a job, and `Ride` gets them off a vehicle.
const JOBS: &[(&str, &str, &str, &str)] = &[
    ("Follow", "follow", "Basics", "Follow me"),
    ("Standby", "standby", "Basics", "Stand by"),
    ("Rest", "rest", "Basics", "Rest"),
    ("Gather", "gather", "Work", "Scavenge"),
    ("ChopWood", "chop", "Work", "Chop wood"),
    ("Mine", "mine", "Work", "Mine"),
    ("Repair", "repair", "Work", "Repair"),
    ("Charge", "charge", "Combat", "Charge enemies"),
];
const HOLD_FIRE: &str = "hold_fire";
const GET_OFF: &str = "get_off";
/// `go:<x>,<z>`: an order to go to a spot.
const GO_PREFIX: &str = "go:";

/// What they're doing, by `AssistantBehaviior` name, for what the game says.
const ACTIVITIES: &[(&str, &str)] = &[
    ("Standby", "Standing by"),
    ("Gather", "Scavenging"),
    ("ChopWood", "Chopping wood"),
    ("Mine", "Mining"),
    ("Follow", "Following"),
    ("GoSomeWhere", "On the way"),
    ("Ride", "On a vehicle"),
    ("Charge", "Charging enemies"),
    ("Repair", "Repairing"),
    ("OperateWeapon", "On a mounted weapon"),
    ("Rest", "Resting"),
];

struct Behaviors {
    /// `(enum value, order id, group, label)`.
    jobs: Vec<(i32, &'static str, &'static str, &'static str)>,
    activities: Vec<(i32, &'static str)>,
    toggle_fire: i32,
    ride: i32,
}

pub struct Companions {
    recruited: Method,
    dead: Method,
    health: Method,
    max_health: Field,
    bag_full: Option<Method>,
    on_vehicle: Option<Method>,
    behavior_part: Field,
    behavior: Method,
    following: Option<Method>,
    work_unavailable: Option<Method>,
    combat_part: Option<Field>,
    hold_fire: Option<Method>,
    execute: Method,
    go_to: Method,
    behaviors: Behaviors,
}

/// The value of a member of an enum, through its static field.
unsafe fn enum_value(rt: &Runtime, class: unity_runtime::Class, name: &str) -> Option<i32> {
    let field = rt.field(class, name).ok().filter(|f| f.is_static)?;
    Some(rt.static_value::<i32>(field))
}

impl Companions {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let assistant = rt.class("", "Assistant").ok()?;
        let behavior = rt.class("", "AssistantBehavior").ok()?;
        let values = rt.class("", "AssistantBehaviior").ok()?;
        let combat = rt.class("", "AssistantCombat").ok();
        let behaviors = unsafe {
            Behaviors {
                jobs: JOBS.iter().filter_map(|&(name, id, group, label)| Some((enum_value(rt, values, name)?, id, group, label))).collect(),
                activities: ACTIVITIES.iter().filter_map(|&(name, text)| Some((enum_value(rt, values, name)?, text))).collect(),
                toggle_fire: enum_value(rt, values, "ToggleFire")?,
                ride: enum_value(rt, values, "Ride")?,
            }
        };
        Some(Self {
            recruited: rt.method(assistant, "get_IsRecruited", 0).ok()?,
            dead: rt.method(assistant, "get_IsDead", 0).ok()?,
            health: rt.method(assistant, "get_Health", 0).ok()?,
            max_health: rt.field(assistant, "MaxHealth").ok()?,
            bag_full: rt.method(assistant, "get_IsBagFullWorkPaused", 0).ok(),
            on_vehicle: rt.method(assistant, "get_IsOnVehicle", 0).ok(),
            behavior_part: rt.field(assistant, "BehaviorPart").ok()?,
            behavior: rt.method(behavior, "get_AssistantBehaviior", 0).ok()?,
            following: rt.method(behavior, "get_IsFollowingPlayer", 0).ok(),
            work_unavailable: rt.method(behavior, "get_IsWorkUnavailable", 0).ok(),
            combat_part: rt.field(assistant, "CombatPart").ok(),
            hold_fire: combat.and_then(|c| rt.method(c, "get_HoldFire", 0).ok()),
            execute: rt.method_with_params(assistant, "ExecuteCommand", &["AssistantBehaviior", "UnityEngine.Vector3"]).ok()?,
            go_to: rt.method_with_params(assistant, "ExecuteGoSomeWhereAt", &["UnityEngine.Vector3"]).ok()?,
            behaviors,
        })
    }

    unsafe fn flag(rt: &Runtime, method: Option<&Method>, this: *mut ManagedObject) -> bool {
        method.is_some_and(|m| rt.invoke(m, this, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(false))
    }

    pub unsafe fn is_recruited(&self, rt: &Runtime, assistant: *mut ManagedObject) -> bool {
        Self::flag(rt, Some(&self.recruited), assistant)
    }

    unsafe fn holding_fire(&self, rt: &Runtime, unity: &Unity, assistant: *mut ManagedObject) -> bool {
        let Some(field) = self.combat_part else { return false };
        let combat: *mut ManagedObject = field.get(assistant);
        unity.is_alive(combat) && Self::flag(rt, self.hold_fire.as_ref(), combat)
    }

    unsafe fn activity(&self, rt: &Runtime, unity: &Unity, assistant: *mut ManagedObject) -> Option<String> {
        let part: *mut ManagedObject = self.behavior_part.get(assistant);
        if !unity.is_alive(part) {
            return None;
        }
        let current: i32 = rt.unbox(rt.invoke(&self.behavior, part, &mut []).ok()?).ok()?;
        let following = Self::flag(rt, self.following.as_ref(), part);
        let text = if following { "Following" } else { self.behaviors.activities.iter().find(|(v, _)| *v == current)?.1 };
        let mut notes = vec![text.to_owned()];
        if Self::flag(rt, self.work_unavailable.as_ref(), part) {
            notes.push("nothing to do here".to_owned());
        }
        if Self::flag(rt, self.bag_full.as_ref(), assistant) {
            notes.push("bag full".to_owned());
        }
        if self.holding_fire(rt, unity, assistant) {
            notes.push("holding fire".to_owned());
        }
        Some(notes.join(", "))
    }

    /// How a recruited companion is doing.
    ///
    /// # Safety
    /// Must run on the game's main thread, with a live `Assistant`.
    pub unsafe fn status(&self, rt: &Runtime, unity: &Unity, assistant: *mut ManagedObject, id: String, name: String, position: Option<Vec3>) -> CompanionStatus {
        let dead = Self::flag(rt, Some(&self.dead), assistant);
        let health = rt.invoke(&self.health, assistant, &mut []).and_then(|b| rt.unbox::<i32>(b)).unwrap_or(0);
        let max: i32 = self.max_health.get(assistant);
        let stats = vec![CompanionStat { name: "Health".into(), value: health as f32, max: max.max(1) as f32 }];
        CompanionStatus { id, name, position, alive: !dead, injured: false, activity: self.activity(rt, unity, assistant), stats }
    }

    /// What they can be told to do now.
    ///
    /// # Safety
    /// Must run on the game's main thread, with a live `Assistant`.
    pub unsafe fn orders(&self, rt: &Runtime, unity: &Unity, assistant: *mut ManagedObject) -> Result<Vec<CompanionOrderOption>, String> {
        if !self.is_recruited(rt, assistant) {
            return Err("they haven't joined you yet".into());
        }
        if Self::flag(rt, Some(&self.dead), assistant) {
            return Ok(Vec::new());
        }
        let option = |id: &str, group: &str, label: &str| CompanionOrderOption { id: id.to_owned(), group: group.to_owned(), label: label.to_owned() };
        let mut orders: Vec<_> = self.behaviors.jobs.iter().map(|(_, id, group, label)| option(id, group, label)).collect();
        orders.push(option(HOLD_FIRE, "Combat", if self.holding_fire(rt, unity, assistant) { "Open fire" } else { "Hold fire" }));
        if Self::flag(rt, self.on_vehicle.as_ref(), assistant) {
            orders.push(option(GET_OFF, "Basics", "Get off the vehicle"));
        }
        Ok(orders)
    }

    /// Gives an order. `ground`: the height to stand at above a spot.
    ///
    /// # Safety
    /// Must run on the game's main thread, with a live `Assistant`.
    pub unsafe fn order(&self, rt: &Runtime, assistant: *mut ManagedObject, order: &str, ground: impl FnOnce(f32, f32) -> Result<f32, String>) -> Result<(), String> {
        if !self.is_recruited(rt, assistant) {
            return Err("they haven't joined you yet".into());
        }
        if let Some((x, z)) = parse_spot(order) {
            let mut at = to_unity(Vec3::new(x, ground(x, z)?, z));
            let mut args = [&mut at as *mut Vector3 as *mut c_void];
            return rt.invoke(&self.go_to, assistant, &mut args).map(|_| ()).map_err(|e| e.to_string());
        }
        let behavior = match order {
            HOLD_FIRE => self.behaviors.toggle_fire,
            GET_OFF => self.behaviors.ride,
            _ => self.behaviors.jobs.iter().find(|(_, id, ..)| *id == order).ok_or("unknown order")?.0,
        };
        let (mut behavior, mut at) = (behavior, Vector3 { x: 0.0, y: 0.0, z: 0.0 });
        let mut args = [&mut behavior as *mut i32 as *mut c_void, &mut at as *mut Vector3 as *mut c_void];
        rt.invoke(&self.execute, assistant, &mut args).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// The spot in a `go:<x>,<z>` order.
fn parse_spot(order: &str) -> Option<(f32, f32)> {
    let (x, z) = order.strip_prefix(GO_PREFIX)?.split_once(',')?;
    let (x, z) = (x.parse::<f32>().ok()?, z.parse::<f32>().ok()?);
    (x.is_finite() && z.is_finite()).then_some((x, z))
}

#[cfg(test)]
mod tests {
    use super::parse_spot;

    #[test]
    fn reads_a_spot_from_an_order() {
        assert_eq!(parse_spot("go:10.5,-3"), Some((10.5, -3.0)));
        assert_eq!(parse_spot("go:1"), None);
        assert_eq!(parse_spot("go:NaN,3"), None);
        assert_eq!(parse_spot("follow"), None);
    }
}
