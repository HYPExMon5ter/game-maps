//! The world around the player: `Sons.Environment.TimeOfDayHolder.GetTimeOfDay()`,
//! `GPSTrackerSystem.GetDaysSurvived()`, `SeasonsManager.get_ActiveSeason()` and
//! `TheForest.World.WeatherSystem.Instance`.

use game_maps_protocol::Environment;
use unity_runtime::{Field, ManagedObject, Method, Runtime, Unity};

/// `TimeSpan` ticks per minute.
const TICKS_PER_MINUTE: i64 = 600_000_000;
const MINUTES_PER_DAY: i64 = 24 * 60;

pub struct EnvironmentMembers {
    time_of_day: Method,
    time_span: Field,
    days_survived: Option<Method>,
    season: Option<Method>,
    weather: Option<WeatherMembers>,
}

struct WeatherMembers {
    instance: Field,
    is_raining: Method,
    is_snowing: Method,
    rain_type: Option<Method>,
}

impl EnvironmentMembers {
    pub fn resolve(rt: &Runtime) -> Option<Self> {
        let holder = rt.class("Sons.Environment", "TimeOfDayHolder").ok()?;
        let time = rt.class("Sons.Environment", "TimeOfDay").ok()?;
        let weather = (|| {
            let w = rt.class("TheForest.World", "WeatherSystem").ok()?;
            Some(WeatherMembers {
                instance: rt.field(w, "Instance").ok().filter(|f| f.is_static)?,
                is_raining: rt.method(w, "get_IsRaining", 0).ok()?,
                is_snowing: rt.method(w, "get_IsSnowing", 0).ok()?,
                rain_type: rt.method(w, "get_CurrentRainType", 0).ok(),
            })
        })();
        Some(Self {
            time_of_day: rt.method(holder, "GetTimeOfDay", 0).ok().filter(|m| m.is_static)?,
            time_span: rt.field(time, "_timeSpan").ok()?,
            days_survived: rt.class("Sons.Gameplay.GPS", "GPSTrackerSystem").ok().and_then(|c| rt.method(c, "GetDaysSurvived", 0).ok()).filter(|m| m.is_static),
            season: rt.class("Sons.Atmosphere", "SeasonsManager").ok().and_then(|c| rt.method(c, "get_ActiveSeason", 0).ok()).filter(|m| m.is_static),
            weather,
        })
    }

    /// # Safety
    /// Must run on the game's main thread.
    pub unsafe fn read(&self, rt: &Runtime, unity: &Unity) -> Option<Environment> {
        let time = rt.invoke(&self.time_of_day, std::ptr::null_mut(), &mut []).ok().filter(|t| !t.is_null())?;
        let minutes = self.time_span.get::<i64>(time) / TICKS_PER_MINUTE;
        let of_day = minutes.rem_euclid(MINUTES_PER_DAY);
        let day = self
            .days_survived
            .as_ref()
            .and_then(|m| rt.invoke(m, std::ptr::null_mut(), &mut []).ok())
            .and_then(|b| rt.unbox_integer(b))
            .map(|d| d.max(0) as u32);
        let season = self.season.as_ref().and_then(|m| rt.enum_name(rt.invoke(m, std::ptr::null_mut(), &mut []).ok()?));
        Some(Environment { day, hour: (of_day / 60) as u8, minute: (of_day % 60) as u8, season, weather: self.weather(rt, unity) })
    }

    unsafe fn weather(&self, rt: &Runtime, unity: &Unity) -> Option<String> {
        let w = self.weather.as_ref()?;
        let instance: *mut ManagedObject = rt.static_value(w.instance);
        if !unity.is_alive(instance) {
            return None;
        }
        let flag = |m: &Method| rt.invoke(m, instance, &mut []).and_then(|b| rt.unbox::<bool>(b)).unwrap_or(false);
        if flag(&w.is_snowing) {
            return Some("Snow".into());
        }
        if !flag(&w.is_raining) {
            return Some("Clear".into());
        }
        let kind = w.rain_type.as_ref().and_then(|m| rt.enum_name(rt.invoke(m, instance, &mut []).ok()?));
        Some(match kind.as_deref() {
            Some("Light") => "Light rain".into(),
            Some("Medium") => "Rain".into(),
            Some("Heavy") => "Heavy rain".into(),
            _ => "Rain".into(),
        })
    }
}
