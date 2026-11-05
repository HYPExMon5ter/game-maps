//! Kelvin and Virginia: their health and stats (`VailActor`), and for Kelvin the order
//! system behind his notepad (`Sons.Gameplay.Robby`).
//!
//! Orders go through the game's own entry points: `ServerGiveOrders` on the host and
//! `ClientSendOrders` on clients, which raise the same network events the notepad does.

use game_maps_protocol::{CompanionOrderOption, CompanionStat, CompanionStatus};
use unity_runtime::{read_object_array, read_string, Field, ListLayout, ManagedObject, Method, Runtime, Unity};

use crate::provider::to_proto;

const MAX_STATS: usize = 32;
const MAX_ORDERS: usize = 64;

/// `Sons.Gameplay.Robby.OrderType`.
const ORDER_FOLLOW: i32 = 0;
const ORDER_GET: i32 = 1;
const ORDER_BUILD: i32 = 2;
const ORDER_STAY: i32 = 3;
const ORDER_BREAK: i32 = 4;
const ORDER_CLEAR: i32 = 6;
const ORDER_MAINTAIN: i32 = 7;

/// `Sons.Gameplay.Robby.DropLocationType`, for what Kelvin fetches.
const DROPS: &[(i32, &str)] = &[(1, "Bring to me"), (0, "Drop here"), (4, "Fill the log sled"), (3, "Fill holders")];

pub const KELVIN: &str = "kelvin";
const HELP_UP: &str = "help_up";

/// The stats worth showing, as the game names them, and what to call them.
const SHOWN_STATS: &[(&str, &str)] = &[
    ("EnergyStat", "Energy"),
    ("FullnessStat", "Fullness"),
    ("HydrationStat", "Hydration"),
    ("AffectionStat", "Affection"),
    ("MoraleStat", "Morale"),
    ("FearStat", "Fear"),
    ("AngerStat", "Anger"),
];

pub struct Companions {
    get_callbacks: Method,
    health: Method,
    max_health: Method,
    actor_stats: Method,
    /// `VailStatsManager.GetStatsManager()`, from the first stats object seen.
    inner_stats: Option<Method>,
    stats: Method,
    stat_name: Method,
    stat_sample: Method,
    stat_max: Method,
    robby_alive: Method,
    injured: Field,
    follow_transform: Option<Field>,
    /// The order that was running (as an address) when he was told to follow.
    followed_over: Option<usize>,
    active_order: Field,
    order_data: Field,
    /// Members of nested classes, which can only be reached from live objects.
    order_name: Option<Field>,
    order_enabled: Option<Field>,
    running_order: Option<Field>,
    give_server: Method,
    give_client: Method,
    help_up: Method,
    get_orders: Field,
    build_orders: Field,
    clear_orders: Field,
    stay_orders: Field,
}

// Raw object pointers are only dereferenced on the main thread.
unsafe impl Send for Companions {}

impl Companions {
    pub fn resolve(rt: &Runtime) -> Result<Self, String> {
        let e = |e: unity_runtime::Error| e.to_string();
        let robby = rt.class("Sons.Gameplay", "Robby").map_err(e)?;
        let actor = rt.class("Sons.Ai.Vail", "VailActor").map_err(e)?;
        let manager = rt.class("Sons.StatSystem", "StatsManager").map_err(e)?;
        let stat = rt.class("Sons.StatSystem", "Stat").map_err(e)?;
        let data = rt.class("Sons.Gameplay", "RobbyOrderData").map_err(e)?;
        Ok(Self {
            get_callbacks: rt.method(actor, "GetRobbyCallbacks", 0).map_err(e)?,
            health: rt.method(actor, "GetHealth", 0).map_err(e)?,
            max_health: rt.method(actor, "GetMaxHealth", 0).map_err(e)?,
            actor_stats: rt.method(actor, "GetStatsManager", 0).map_err(e)?,
            inner_stats: rt.class("Sons.Ai.Vail", "VailStatsManager").ok().and_then(|c| rt.method(c, "GetStatsManager", 0).ok()),
            stats: rt.method(manager, "get_Stats", 0).map_err(e)?,
            stat_name: rt.method(stat, "GetName", 0).map_err(e)?,
            stat_sample: rt.method(stat, "Sample", 0).map_err(e)?,
            stat_max: rt.method(stat, "GetMax", 0).map_err(e)?,
            robby_alive: rt.method(robby, "IsAlive", 0).map_err(e)?,
            injured: rt.field(robby, "_injuredState").map_err(e)?,
            follow_transform: rt.field(robby, "_followTransform").ok(),
            followed_over: None,
            active_order: rt.field(robby, "_activeOrder").map_err(e)?,
            order_data: rt.field(robby, "_orderData").map_err(e)?,
            order_name: None,
            order_enabled: None,
            running_order: None,
            give_server: rt.method(robby, "ServerGiveOrders", 4).map_err(e)?,
            give_client: rt.method(robby, "ClientSendOrders", 3).map_err(e)?,
            help_up: rt.method(robby, "LocalPlayerHelpUp", 0).map_err(e)?,
            get_orders: rt.field(data, "_getItemOrders").map_err(e)?,
            build_orders: rt.field(data, "_buildItemOrders").map_err(e)?,
            clear_orders: rt.field(data, "_clearAreaOrders").map_err(e)?,
            stay_orders: rt.field(data, "_stayOrders").map_err(e)?,
        })
    }

    unsafe fn elements(&self, rt: &Runtime, list: *mut ManagedObject, max: usize) -> Vec<*mut ManagedObject> {
        if list.is_null() {
            return Vec::new();
        }
        match ListLayout::of(rt, list) {
            Ok(l) => l.elements(list, max),
            Err(_) => read_object_array(list, max),
        }
    }

    unsafe fn float(&self, rt: &Runtime, method: &Method, this: *mut ManagedObject) -> Option<f32> {
        rt.unbox::<f32>(rt.invoke(method, this, &mut []).ok()?).ok()
    }

    /// Health first, then whatever else the actor's stats hold.
    unsafe fn read_stats(&self, rt: &Runtime, actor: *mut ManagedObject) -> Vec<CompanionStat> {
        let mut stats = Vec::new();
        if let (Some(value), Some(max)) = (self.float(rt, &self.health, actor), self.float(rt, &self.max_health, actor)) {
            stats.push(CompanionStat { name: "Health".into(), value, max });
        }
        let manager = rt.invoke(&self.actor_stats, actor, &mut []).ok().filter(|m| !m.is_null());
        let manager = manager.and_then(|m| match &self.inner_stats {
            Some(inner) => rt.invoke(inner, m, &mut []).ok().filter(|m| !m.is_null()),
            None => Some(m),
        });
        let Some(list) = manager.and_then(|m| rt.invoke(&self.stats, m, &mut []).ok()) else { return stats };
        for s in self.elements(rt, list, MAX_STATS) {
            let name = rt.invoke(&self.stat_name, s, &mut []).ok().and_then(|n| read_string(n));
            let value = self.float(rt, &self.stat_sample, s);
            let max = self.float(rt, &self.stat_max, s);
            let shown = name.and_then(|n| SHOWN_STATS.iter().find(|(id, _)| *id == n));
            if let (Some((_, name)), Some(value), Some(max)) = (shown, value, max) {
                stats.push(CompanionStat { name: (*name).into(), value, max });
            }
        }
        stats
    }

    /// Kelvin's `Robby` script, which is his actor's callbacks.
    unsafe fn robby(&self, rt: &Runtime, unity: &Unity, actor: *mut ManagedObject) -> Option<*mut ManagedObject> {
        if !unity.is_alive(actor) {
            return None;
        }
        rt.invoke(&self.get_callbacks, actor, &mut []).ok().filter(|r| !r.is_null())
    }

    /// Resolves the order members from a live order.
    unsafe fn resolve_order_members(&mut self, rt: &Runtime, robby: *mut ManagedObject) -> Option<()> {
        if self.order_name.is_some() {
            return Some(());
        }
        let data: *mut ManagedObject = self.order_data.get(robby);
        let first = *self.elements(rt, (!data.is_null()).then(|| self.get_orders.get(data))?, 1).first()?;
        let class = rt.class_of(first)?;
        self.order_name = rt.field(class, "_itemName").ok();
        self.order_enabled = rt.field(class, "_enabled").ok();
        Some(())
    }

    /// What the order in `running` is called.
    unsafe fn activity(&mut self, rt: &Runtime, running: *mut ManagedObject) -> Option<String> {
        if running.is_null() {
            return None;
        }
        let field = match self.running_order {
            Some(f) => f,
            None => *self.running_order.insert(rt.field(rt.class_of(running)?, "_order").ok()?),
        };
        let order: *mut ManagedObject = field.get(running);
        if order.is_null() {
            return None;
        }
        read_string(self.order_name?.get::<*mut ManagedObject>(order)).map(|n| label(&n))
    }

    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn read(&mut self, rt: &Runtime, unity: &Unity, kelvin: *mut ManagedObject, virginia: *mut ManagedObject) -> Vec<CompanionStatus> {
        let mut out = Vec::new();
        if let Some(robby) = self.robby(rt, unity, kelvin) {
            self.resolve_order_members(rt, robby);
            let running: *mut ManagedObject = self.active_order.get(robby);
            // Following isn't an order of its own: the last order stays set meanwhile, and
            // the follow target stays set during orders. So he's following if told to and
            // no order came since (from the notepad, say), or if he has no order at all.
            if self.followed_over.is_some_and(|order| order != running as usize) {
                self.followed_over = None;
            }
            let has_target = self.follow_transform.is_some_and(|f| unity.is_alive(f.get::<*mut ManagedObject>(robby)));
            let following = self.followed_over.is_some() || (running.is_null() && has_target);
            out.push(CompanionStatus {
                id: KELVIN.into(),
                name: "Kelvin".into(),
                position: unity.component_transform(rt, kelvin).and_then(|t| unity.position(rt, t)).ok().map(to_proto),
                alive: rt.invoke(&self.robby_alive, robby, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(true),
                injured: self.injured.get::<i32>(robby) != 0,
                activity: if following { Some("Following".into()) } else { self.activity(rt, running) },
                stats: self.read_stats(rt, kelvin),
            });
        }
        if unity.is_alive(virginia) {
            out.push(CompanionStatus {
                id: "virginia".into(),
                name: "Virginia".into(),
                position: unity.component_transform(rt, virginia).and_then(|t| unity.position(rt, t)).ok().map(to_proto),
                alive: true,
                injured: false,
                activity: None,
                stats: self.read_stats(rt, virginia),
            });
        }
        out
    }

    /// Every order Kelvin takes right now, from the same lists as his notepad.
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn orders(&mut self, rt: &Runtime, unity: &Unity, kelvin: *mut ManagedObject) -> Result<Vec<CompanionOrderOption>, String> {
        let robby = self.robby(rt, unity, kelvin).ok_or("Kelvin isn't around")?;
        self.resolve_order_members(rt, robby).ok_or("Kelvin's orders aren't loaded")?;
        let (Some(name), Some(enabled)) = (self.order_name, self.order_enabled) else { return Err("Kelvin's orders aren't loaded".into()) };
        let data: *mut ManagedObject = self.order_data.get(robby);
        if data.is_null() {
            return Err("Kelvin's orders aren't loaded".into());
        }
        let option = |id: String, group: &str, label: String| CompanionOrderOption { id, group: group.into(), label };
        if self.injured.get::<i32>(robby) != 0 {
            // He takes no orders until he's back on his feet.
            return Ok(vec![option(HELP_UP.into(), "Basics", "Help him up".into())]);
        }
        let mut orders = vec![option("follow".into(), "Basics", "Follow me".into()), option("break".into(), "Basics", "Take a break".into()), option("maintain".into(), "Basics", "Maintain the base".into())];
        for (prefix, group, field) in [("stay", "Stay", self.stay_orders), ("clear", "Clear area", self.clear_orders), ("build", "Build", self.build_orders)] {
            for (i, &o) in self.elements(rt, field.get(data), MAX_ORDERS).iter().enumerate() {
                if enabled.get::<bool>(o) {
                    let text = read_string(name.get::<*mut ManagedObject>(o)).unwrap_or_default();
                    orders.push(option(format!("{prefix}:{i}"), group, label(&text)));
                }
            }
        }
        for (i, &o) in self.elements(rt, self.get_orders.get(data), MAX_ORDERS).iter().enumerate() {
            if !enabled.get::<bool>(o) {
                continue;
            }
            let item = label(&read_string(name.get::<*mut ManagedObject>(o)).unwrap_or_default());
            for (drop, how) in DROPS {
                orders.push(option(format!("get:{i}:{drop}"), &format!("Get {}", item.to_lowercase()), (*how).into()));
            }
        }
        Ok(orders)
    }

    /// Gives Kelvin the order `id` (see [`Companions::orders`]).
    ///
    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn give_order(&mut self, rt: &Runtime, unity: &Unity, kelvin: *mut ManagedObject, player: *mut ManagedObject, id: &str, client: bool) -> Result<(), String> {
        let robby = self.robby(rt, unity, kelvin).ok_or("Kelvin isn't around")?;
        if id == HELP_UP {
            return rt.invoke(&self.help_up, robby, &mut []).map(|_| ()).map_err(|e| e.to_string());
        }
        let (mut order, mut sub, mut sub2) = parse_order(id).ok_or_else(|| format!("unknown order {id}"))?;
        let mut args: Vec<*mut std::ffi::c_void> = vec![&mut order as *mut i32 as _, &mut sub as *mut i32 as _, &mut sub2 as *mut i32 as _];
        let method = if client {
            &self.give_client
        } else {
            args.push(player as _);
            &self.give_server
        };
        rt.invoke(method, robby, &mut args).map_err(|e| e.to_string())?;
        self.followed_over = (id == "follow").then(|| self.active_order.get::<*mut ManagedObject>(robby) as usize);
        Ok(())
    }
}

/// The game's `OrderType` and sub-orders for an order id.
fn parse_order(id: &str) -> Option<(i32, i32, i32)> {
    let mut parts = id.split(':');
    let kind = parts.next()?;
    let mut number = || parts.next()?.parse::<i32>().ok();
    match kind {
        "follow" => Some((ORDER_FOLLOW, 0, 0)),
        "break" => Some((ORDER_BREAK, 0, 0)),
        "maintain" => Some((ORDER_MAINTAIN, 0, 0)),
        "stay" => Some((ORDER_STAY, number()?, 0)),
        "clear" => Some((ORDER_CLEAR, number()?, 0)),
        "build" => Some((ORDER_BUILD, number()?, 0)),
        "get" => {
            let item = number()?;
            Some((ORDER_GET, item, number()?))
        }
        _ => None,
    }
}

/// `PERIMETER_WALL` as "Perimeter wall".
fn label(name: &str) -> String {
    let lower = name.replace('_', " ").to_lowercase();
    let mut chars = lower.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_order_ids() {
        assert_eq!(parse_order("follow"), Some((ORDER_FOLLOW, 0, 0)));
        assert_eq!(parse_order("stay:2"), Some((ORDER_STAY, 2, 0)));
        assert_eq!(parse_order("get:2:4"), Some((ORDER_GET, 2, 4)));
        assert_eq!(parse_order("get:2"), None);
        assert_eq!(parse_order("dance"), None);
    }

    #[test]
    fn labels_read_like_the_notepad() {
        assert_eq!(label("PERIMETER_WALL"), "Perimeter wall");
        assert_eq!(label("5 METERS"), "5 meters");
        assert_eq!(label(""), "");
    }
}
