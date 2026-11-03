//! App-level settings owned by the backend (`app-settings.json`): how each game is
//! hooked, its map hotkey, the map zoom keys, and whether updates install automatically. Both windows
//! edit them through commands and stay in sync through the [`EVENT`] event.
//! Map/UI preferences live in the web UI's own store.

use std::collections::HashMap;
use std::sync::Mutex;

use game_maps_protocol::{Hotkey, HotkeyAction};
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

/// Keys that zoom the map, the same for every game. Each action has two slots so a
/// keyboard without a numpad still has a key; `None` turns a slot off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoomKeys {
    pub zoom_in: [Option<Hotkey>; 2],
    pub zoom_out: [Option<Hotkey>; 2],
}

impl Default for ZoomKeys {
    /// Numpad + / - and the main row's = / - keys.
    fn default() -> Self {
        let key = |action, vk| Some(Hotkey { action, vk, ctrl: false, shift: false, alt: false });
        Self {
            zoom_in: [key(HotkeyAction::ZoomIn, 0x6B), key(HotkeyAction::ZoomIn, 0xBB)],
            zoom_out: [key(HotkeyAction::ZoomOut, 0x6D), key(HotkeyAction::ZoomOut, 0xBD)],
        }
    }
}

impl ZoomKeys {
    /// Checks that every slot holds the action it belongs to.
    pub fn validate(&self) -> Result<(), String> {
        let wrong = |slots: &[Option<Hotkey>; 2], action| slots.iter().flatten().any(|h| h.action != action);
        if wrong(&self.zoom_in, HotkeyAction::ZoomIn) || wrong(&self.zoom_out, HotkeyAction::ZoomOut) {
            return Err("zoom keys must be bound to their own action".into());
        }
        Ok(())
    }

    fn bindings(&self) -> impl Iterator<Item = &Hotkey> {
        self.zoom_in.iter().chain(&self.zoom_out).flatten()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub hook_modes: HashMap<String, HookMode>,
    /// Per-game override of the key that opens the map; absent uses the game's default.
    pub map_hotkeys: HashMap<String, Hotkey>,
    pub zoom_keys: ZoomKeys,
    /// Key that returns the player to where they were before their last teleport (all
    /// games); `None` turns it off.
    pub back_key: Option<Hotkey>,
    pub auto_update: bool,
}

/// Backspace: rarely bound by games, and easy to reach.
pub fn default_back_key() -> Hotkey {
    Hotkey { action: HotkeyAction::TeleportBack, vk: 0x08, ctrl: false, shift: false, alt: false }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hook_modes: HashMap::new(),
            map_hotkeys: HashMap::new(),
            zoom_keys: ZoomKeys::default(),
            back_key: Some(default_back_key()),
            auto_update: true,
        }
    }
}

impl AppSettings {
    pub fn hook_mode(&self, game_id: &str) -> HookMode {
        self.hook_modes.get(game_id).copied().unwrap_or_default()
    }

    /// Hotkeys sent to the agent for `game`: its map key plus the zoom keys.
    pub fn hotkeys(&self, game: &GameEntry) -> Vec<Hotkey> {
        let mut keys = match self.map_hotkeys.get(&game.id) {
            Some(h) => vec![h.clone()],
            None => game.hotkeys(),
        };
        keys.extend(self.zoom_keys.bindings().cloned());
        keys.extend(self.back_key.clone());
        keys
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

        assert_eq!(settings.hotkeys(&game("a"))[0], k);
        assert_eq!(settings.hotkeys(&game("b"))[0].vk, 77);
    }

    #[test]
    fn zoom_keys_default_to_numpad_and_main_row_and_can_be_turned_off() {
        let mut settings = AppSettings::default();
        let zoom = |s: &AppSettings| {
            s.hotkeys(&game("a")).into_iter().filter(|h| matches!(h.action, HotkeyAction::ZoomIn | HotkeyAction::ZoomOut)).map(|h| h.vk).collect::<Vec<_>>()
        };
        assert_eq!(zoom(&settings), vec![0x6B, 0xBB, 0x6D, 0xBD]);

        settings.zoom_keys.zoom_in[1] = None;
        assert_eq!(zoom(&settings), vec![0x6B, 0x6D, 0xBD]);
    }

    #[test]
    fn teleport_back_key_defaults_to_backspace_and_can_be_turned_off() {
        let back = |s: &AppSettings| s.hotkeys(&game("a")).into_iter().filter(|h| h.action == HotkeyAction::TeleportBack).map(|h| h.vk).collect::<Vec<_>>();
        let mut settings = AppSettings::default();
        assert_eq!(back(&settings), vec![0x08]);
        // Settings saved before the key existed get the default.
        let old: AppSettings = serde_json::from_str(r#"{"hook_modes":{},"auto_update":true}"#).unwrap();
        assert_eq!(back(&old), vec![0x08]);
        settings.back_key = None;
        assert!(back(&settings).is_empty());
    }

    #[test]
    fn zoom_keys_reject_other_actions() {
        let mut keys = ZoomKeys::default();
        assert!(keys.validate().is_ok());
        keys.zoom_out[0] = keys.zoom_in[0].clone();
        assert!(keys.validate().is_err());
    }
}
