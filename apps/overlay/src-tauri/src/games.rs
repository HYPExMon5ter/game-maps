//! Game registry (`games/<id>/game.json`) and the `gamemaps://` asset protocol that
//! serves game data (tiles, icons, pins) and user files to the web UI.
//!
//! Game definitions ship with the app. Map data (tiles, icons, pins) comes from map
//! packs downloaded into the user's data folder (see `mappacks`); in development it is
//! read from the repository's `games/` folder instead.

use std::path::{Component, Path, PathBuf};

use game_maps_protocol::{Hotkey, HotkeyAction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    pub id: String,
    pub name: String,
    pub process: String,
    /// Lets the launcher start the game through Steam.
    #[serde(default)]
    pub steam_app_id: Option<u32>,
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

/// Where asset requests are looked up.
pub struct AssetRoots<'a> {
    /// Game definitions bundled with the app (and, in development, all map data).
    pub games: &'a Path,
    /// Installed map packs (`<packs>/<game id>/...`).
    pub packs: &'a Path,
    pub user: &'a Path,
}

/// Resolves a protocol path (`games/...` or `user/...`) to the files that may serve it,
/// in priority order. Rejects traversal and absolute components.
pub fn resolve_asset(path: &str, roots: &AssetRoots) -> Vec<PathBuf> {
    let path = path.trim_start_matches('/');
    let Some((root, rest)) = path.split_once('/') else { return Vec::new() };
    let rel = Path::new(rest);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Vec::new();
    }
    match root {
        // A downloaded map pack wins over bundled/development data.
        "games" => vec![roots.packs.join(rel), roots.games.join(rel)],
        "user" => vec![roots.user.join(rel)],
        _ => Vec::new(),
    }
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
        let (g, p, u) = (Path::new("C:/g"), Path::new("C:/p"), Path::new("C:/u"));
        let roots = AssetRoots { games: g, packs: p, user: u };
        assert_eq!(
            resolve_asset("/games/sotf/tiles/0/0/0.webp", &roots),
            vec![p.join("sotf/tiles/0/0/0.webp"), g.join("sotf/tiles/0/0/0.webp")]
        );
        assert_eq!(resolve_asset("user/icons/a.png", &roots), vec![u.join("icons/a.png")]);
        assert!(resolve_asset("games/../secret", &roots).is_empty());
        assert!(resolve_asset("other/x", &roots).is_empty());
        assert!(resolve_asset("games/C:/x", &roots).is_empty());
    }

    #[test]
    fn parses_sotf_game_json() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games");
        let games = load_registry(&path);
        let sotf = games.iter().find(|g| g.id == "sons-of-the-forest").expect("sotf registered");
        assert_eq!(sotf.process, "SonsOfTheForest.exe");
        assert_eq!(sotf.steam_app_id, Some(1326470));
        assert_eq!(sotf.hotkeys()[0].vk, 0x4D);
    }
}
