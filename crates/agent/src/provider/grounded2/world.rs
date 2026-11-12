//! Which world is loaded, the multiplayer session, and the time of day.

use std::sync::Arc;

use game_maps_protocol::{Environment, GameWorld, MultiplayerSession, WorldKind};
use unreal_runtime::{FString, Function, Obj, Property, Unreal};

use crate::unreal::UeCx;

/// `FGuid`
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Guid([u32; 4]);

impl Guid {
    fn is_zero(&self) -> bool {
        self.0 == [0; 4]
    }

    fn text(&self) -> String {
        self.0.iter().map(|p| format!("{p:08x}")).collect()
    }
}

pub struct WorldMembers {
    state_class: Obj,
    /// `InGameGameState.PlaythroughGuid`: the save, the same for everyone playing in it.
    playthrough: Property,
    playthrough_name: Property,
    calendar: Option<Property>,
    hour: Option<Arc<Function>>,
    minute: Option<Arc<Function>>,
    total_hours: Option<Arc<Function>>,
    hours_per_day: Option<Arc<Function>>,
}

impl WorldMembers {
    pub fn resolve(ue: &Unreal) -> unreal_runtime::Result<Self> {
        let state = ue.find_struct("/Script/Maine.InGameGameState")?;
        let calendar_class = ue.find_struct("/Script/Maine.CalendarComponent").ok();
        let f = |name: &str| calendar_class.and_then(|c| ue.function(c, name).ok());
        Ok(Self {
            state_class: state,
            playthrough: ue.property_of(state, "PlaythroughGuid", "Struct<Guid>")?,
            playthrough_name: ue.property_of(state, "PlaythroughName", "Str")?,
            calendar: ue.find_struct("/Script/Maine.SurvivalGameState").ok().and_then(|s| ue.property_of(s, "CalendarComponent", "Object<CalendarComponent>").ok()),
            hour: f("GetHourFloat"),
            minute: f("GetMinute"),
            total_hours: f("GetTotalGameHoursPassed"),
            hours_per_day: f("GetHoursPerDay"),
        })
    }

    /// The loaded save and the session around it. `game_state` is the world's
    /// `InGameGameState`; `players` how many are in the world.
    ///
    /// # Safety
    /// Game thread only.
    pub unsafe fn read(&self, cx: &UeCx, game_state: Obj, context: Obj, players: usize, player_name: Option<String>) -> (Option<GameWorld>, Option<MultiplayerSession>) {
        let ue = &cx.ue;
        if !ue.is_a(game_state, self.state_class) {
            return (None, None);
        }
        let guid: Guid = ue.get(game_state, &self.playthrough);
        if guid.is_zero() {
            return (None, None);
        }
        let name = Some(ue.get::<FString>(game_state, &self.playthrough_name).to_string_lossy()).filter(|n| !n.is_empty());
        let server = cx.is_server(context).unwrap_or(true);
        let standalone = cx.is_standalone(context).unwrap_or(false);
        let kind = world_kind(server, standalone, players);
        let world = GameWorld { id: guid.text(), name, kind };
        // Everyone in the world shares its save's id, and only they know it.
        let session = (kind != WorldKind::SinglePlayer).then(|| MultiplayerSession { id: format!("playthrough:{}", guid.text()), player_name });
        (Some(world), session)
    }

    /// The game's clock.
    ///
    /// # Safety
    /// Game thread only; `game_state` must be the world's `SurvivalGameState`.
    pub unsafe fn environment(&self, cx: &UeCx, game_state: Obj) -> Option<Environment> {
        let calendar = cx.ue.get_obj(game_state, self.calendar.as_ref()?)?;
        let hour: f32 = cx.call_get(calendar, self.hour.as_ref()?)?;
        let minute: i32 = cx.call_get(calendar, self.minute.as_ref()?)?;
        let total: f32 = self.total_hours.as_ref().and_then(|f| cx.call_get(calendar, f)).unwrap_or(0.0);
        let per_day: i32 = self.hours_per_day.as_ref().and_then(|f| cx.call_get(calendar, f)).unwrap_or(24);
        Some(Environment { day: day_number(total, per_day), hour: hour.floor().clamp(0.0, 23.0) as u8, minute: minute.clamp(0, 59) as u8, season: None, weather: None, wind: None })
    }
}

fn world_kind(server: bool, standalone: bool, players: usize) -> WorldKind {
    match (server, standalone || players <= 1) {
        (false, _) => WorldKind::Joined,
        (true, true) => WorldKind::SinglePlayer,
        (true, false) => WorldKind::Hosted,
    }
}

/// The day the clock is on, counting from day 1.
fn day_number(total_hours: f32, hours_per_day: i32) -> Option<u32> {
    (total_hours > 0.0 && hours_per_day > 0).then(|| (total_hours / hours_per_day as f32).floor() as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_hosting_from_joining() {
        assert_eq!(world_kind(true, true, 1), WorldKind::SinglePlayer);
        assert_eq!(world_kind(true, false, 1), WorldKind::SinglePlayer);
        assert_eq!(world_kind(true, false, 3), WorldKind::Hosted);
        assert_eq!(world_kind(false, false, 3), WorldKind::Joined);
    }

    #[test]
    fn counts_days_from_one() {
        assert_eq!(day_number(0.0, 24), None);
        assert_eq!(day_number(5.0, 24), Some(1));
        assert_eq!(day_number(49.5, 24), Some(3));
    }

    #[test]
    fn writes_guids_as_hex() {
        assert_eq!(Guid([1, 0xabcdef, 0, 0xffffffff]).text(), "0000000100abcdef00000000ffffffff");
        assert!(Guid::default().is_zero());
    }
}
