//! Game registry (`games/<id>/game.json`) and the `gamemaps://` asset protocol that
//! serves game data (tiles, icons, pins) and user files to the web UI.

use std::path::{Component, Path, PathBuf};

use game_maps_protocol::{Hotkey, HotkeyAction};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    pub id: String,
    pub process: String,
    #[serde(default)]
    pub default_hotkeys: Vec<Hotkey>,
}

impl GameEntry {
    pub fn hotkeys(&self) -> Vec<Hotkey> {
        if self.default_hotkeys.is_empty() {
            vec![Hotkey { action: HotkeyAction::ToggleMap, vk: 0x4D, ctrl: false, shift: false, alt: false }]
        } else {
            self.default_hotkeys.clone()
        }
    }
}

/// Loads every `games/*/game.json`. Invalid entries are logged and skipped.
pub fn load_registry(games_dir: &Path) -> Vec<GameEntry> {
    let Ok(dirs) = std::fs::read_dir(games_dir) else {
        log::error!("games directory missing: {}", games_dir.display());
        return Vec::new();
    };
    let mut games: Vec<GameEntry> = dirs
        .flatten()
        .filter_map(|d| {
            let path = d.path().join("game.json");
            let text = std::fs::read_to_string(&path).ok()?;
            match serde_json::from_str::<GameEntry>(&text) {
                Ok(g) => Some(g),
                Err(e) => {
                    log::error!("invalid {}: {e}", path.display());
                    None
                }
            }
        })
        .collect();
    games.sort_by(|a, b| a.id.cmp(&b.id));
    games
}

/// Resolves a protocol path (`games/...` or `user/...`) to a file inside the allowed
/// roots. Rejects traversal and absolute components.
pub fn resolve_asset(path: &str, games_dir: &Path, user_dir: &Path) -> Option<PathBuf> {
    let path = path.trim_start_matches('/');
    let (root, rest) = path.split_once('/')?;
    let base = match root {
        "games" => games_dir,
        "user" => user_dir,
        _ => return None,
    };
    let rel = Path::new(rest);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(base.join(rel))
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("webp") => "image/webp",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_inside_roots() {
        let g = Path::new("C:/g");
        let u = Path::new("C:/u");
        assert_eq!(resolve_asset("/games/sotf/tiles/0/0/0.webp", g, u), Some(g.join("sotf/tiles/0/0/0.webp")));
        assert_eq!(resolve_asset("user/icons/a.png", g, u), Some(u.join("icons/a.png")));
        assert_eq!(resolve_asset("games/../secret", g, u), None);
        assert_eq!(resolve_asset("other/x", g, u), None);
        assert_eq!(resolve_asset("games/C:/x", g, u), None);
    }

    #[test]
    fn parses_sotf_game_json() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games");
        let games = load_registry(&path);
        let sotf = games.iter().find(|g| g.id == "sons-of-the-forest").expect("sotf registered");
        assert_eq!(sotf.process, "SonsOfTheForest.exe");
        assert_eq!(sotf.hotkeys()[0].vk, 0x4D);
    }
}
