//! App-level settings owned by the backend (`app-settings.json`): how each game is
//! hooked, its map hotkey, and whether updates install automatically. Both windows
//! edit them through commands and stay in sync through the [`EVENT`] event.
//! Map/UI preferences live in the web UI's own store.

use std::collections::HashMap;
use std::sync::Mutex;

use game_maps_protocol::Hotkey;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

use crate::games::GameEntry;

const STORE_FILE: &str = "app-settings.json";
const STORE_KEY: &str = "settings";
pub const EVENT: &str = "app-settings";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookMode {
    /// Connect to the game as soon as it starts.
    #[default]
    Auto,
    /// Only connect when the user clicks Connect.
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub hook_modes: HashMap<String, HookMode>,
    /// Per-game override of the key that opens the map; absent uses the game's default.
    pub map_hotkeys: HashMap<String, Hotkey>,
    pub auto_update: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { hook_modes: HashMap::new(), map_hotkeys: HashMap::new(), auto_update: true }
    }
}

impl AppSettings {
    pub fn hook_mode(&self, game_id: &str) -> HookMode {
        self.hook_modes.get(game_id).copied().unwrap_or_default()
    }

    /// Hotkeys sent to the agent for `game`.
    pub fn hotkeys(&self, game: &GameEntry) -> Vec<Hotkey> {
        match self.map_hotkeys.get(&game.id) {
            Some(h) => vec![h.clone()],
            None => game.hotkeys(),
        }
    }
}

pub struct SettingsState(Mutex<AppSettings>);

impl SettingsState {
    pub fn load(app: &AppHandle) -> Self {
        let saved = app
            .store(STORE_FILE)
            .ok()
            .and_then(|s| s.get(STORE_KEY))
            .and_then(|v| match serde_json::from_value(v) {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("ignoring invalid {STORE_FILE}: {e}");
                    None
                }
            })
            .unwrap_or_default();
        Self(Mutex::new(saved))
    }
}

pub fn get(app: &AppHandle) -> AppSettings {
    app.state::<SettingsState>().0.lock().unwrap().clone()
}

/// Applies `change` and saves the result.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut AppSettings)) -> Result<AppSettings, String> {
    let next = {
        let state = app.state::<SettingsState>();
        let mut current = state.0.lock().unwrap();
        change(&mut current);
        current.clone()
    };
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(STORE_KEY, serde_json::to_value(&next).map_err(|e| e.to_string())?);
    store.save().map_err(|e| e.to_string())?;
    let _ = app.emit(EVENT, &next);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use game_maps_protocol::HotkeyAction;

    use super::*;

    fn game(id: &str) -> GameEntry {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": id, "process": "x.exe",
            "defaultHotkeys": [{ "action": "toggle_map", "vk": 77 }]
        }))
        .unwrap()
    }

    #[test]
    fn hotkeys_are_per_game_with_default_fallback() {
        // Settings saved before per-game hotkeys existed still load.
        let mut settings: AppSettings = serde_json::from_str(r#"{"hook_modes":{},"auto_update":false}"#).unwrap();
        let k = Hotkey { action: HotkeyAction::ToggleMap, vk: 0x4B, ctrl: true, shift: false, alt: false };
        settings.map_hotkeys.insert("a".into(), k.clone());

        assert_eq!(settings.hotkeys(&game("a")), vec![k]);
        assert_eq!(settings.hotkeys(&game("b"))[0].vk, 77);
    }
}
