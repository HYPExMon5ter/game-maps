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
    /// Has an agent provider (live location, teleport); otherwise a static map.
    #[serde(default)]
    pub live_features: bool,
    /// Worlds are generated, so the map comes from the running game, not a map pack.
    #[serde(default)]
    pub world_maps: bool,
    /// Pins and pings can go on the game's own map.
    #[serde(default)]
    pub game_map: bool,
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

/// Files that say a game runs anti-cheat, by the start of their lowercase names.
const ANTI_CHEAT_FILES: &[(&str, &str)] = &[("easyanticheat", "Easy Anti-Cheat"), ("start_protected_game", "Easy Anti-Cheat"), ("battleye", "BattlEye"), ("beservice", "BattlEye")];

/// The anti-cheat in a game's folder, if any. Loading into a game that has one could get
/// the player banned, and a game can gain one in an update, so it's checked every run.
pub fn anti_cheat_in(game_dir: &Path) -> Option<&'static str> {
    std::fs::read_dir(game_dir).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        ANTI_CHEAT_FILES.iter().find(|(start, _)| name.starts_with(start)).map(|(_, product)| *product)
    })
}

/// The Steam build of the game at `exe`, from its `appmanifest_<id>.acf`.
pub fn steam_build(exe: &Path, app_id: u32) -> Option<String> {
    let steamapps = exe.ancestors().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps")))?;
    let manifest = std::fs::read_to_string(steamapps.join(format!("appmanifest_{app_id}.acf"))).ok()?;
    manifest.lines().find_map(|line| {
        let mut parts = line.split('"').filter(|p| !p.trim().is_empty());
        (parts.next()? == "buildid").then(|| parts.next().map(str::to_owned)).flatten()
    })
}

/// Where asset requests are looked up.
pub struct AssetRoots<'a> {
    /// Game definitions bundled with the app (and, in development, all map data).
    pub games: &'a Path,
    /// Installed map packs (`<packs>/<game id>/...`).
    pub packs: &'a Path,
    /// Maps made from the running game (`<worlds>/<game id>/<world id>/...`).
    pub worlds: &'a Path,
    pub user: &'a Path,
}

/// Resolves a protocol path (`games/...`, `worlds/...` or `user/...`) to the files that may serve it,
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
        "worlds" => vec![roots.worlds.join(rel)],
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
        let (g, p, w, u) = (Path::new("C:/g"), Path::new("C:/p"), Path::new("C:/w"), Path::new("C:/u"));
        let roots = AssetRoots { games: g, packs: p, worlds: w, user: u };
        assert_eq!(
            resolve_asset("/games/sotf/tiles/0/0/0.webp", &roots),
            vec![p.join("sotf/tiles/0/0/0.webp"), g.join("sotf/tiles/0/0/0.webp")]
        );
        assert_eq!(resolve_asset("user/icons/a.png", &roots), vec![u.join("icons/a.png")]);
        assert_eq!(resolve_asset("worlds/valheim/world-1/pins.json", &roots), vec![w.join("valheim/world-1/pins.json")]);
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

    #[test]
    fn finds_anti_cheat_in_a_game_folder() {
        let dir = std::env::temp_dir().join(format!("game-maps-ac-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("EasyAntiCheat")).unwrap();
        assert_eq!(anti_cheat_in(&dir), Some("Easy Anti-Cheat"));
        std::fs::remove_dir_all(dir.join("EasyAntiCheat")).unwrap();
        assert_eq!(anti_cheat_in(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_the_steam_build() {
        let root = std::env::temp_dir().join(format!("game-maps-build-{}", std::process::id()));
        let game = root.join("steamapps").join("common").join("Game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(root.join("steamapps/appmanifest_7.acf"), "\"AppState\"\n{\n\t\"buildid\"\t\t\"123\"\n}\n").unwrap();
        assert_eq!(steam_build(&game.join("game.exe"), 7).as_deref(), Some("123"));
        assert_eq!(steam_build(&game.join("game.exe"), 8), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn every_category_is_in_one_group() {
        for text in [include_str!("../../../../games/valheim/game.json"), include_str!("../../../../games/sunkenland/game.json"), include_str!("../../../../games/sons-of-the-forest/game.json")] {
            let game: serde_json::Value = serde_json::from_str(text).unwrap();
            let ids: Vec<&str> = game["categories"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
            let grouped: Vec<&str> = game["groups"].as_array().unwrap().iter().flat_map(|g| g["categories"].as_array().unwrap()).map(|c| c.as_str().unwrap()).collect();
            for id in &grouped {
                assert!(ids.contains(id), "{id} isn't a category");
                assert_eq!(grouped.iter().filter(|g| *g == id).count(), 1, "{id} is in two groups");
            }
            // The player's own pins go in a group the app makes.
            for id in ids.iter().filter(|id| **id != "custom") {
                assert!(grouped.contains(id), "{id} isn't in a group");
            }
        }
    }

    #[test]
    fn parses_valheim_game_json() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games");
        let games = load_registry(&path);
        let game = games.iter().find(|g| g.id == "valheim").expect("valheim registered");
        assert_eq!(game.process, "valheim.exe");
        assert_eq!(game.steam_app_id, Some(892970));
        assert!(game.live_features && game.world_maps);
    }

    #[test]
    fn parses_grounded2_game_json() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games");
        let games = load_registry(&path);
        let game = games.iter().find(|g| g.id == "grounded-2").expect("grounded-2 registered");
        assert_eq!(game.process, "Grounded2Steam-Win64-Shipping.exe");
        assert_eq!(game.steam_app_id, Some(2661300));
        assert!(game.live_features && !game.world_maps && !game.game_map);
    }

    #[test]
    fn parses_sunkenland_game_json() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../games");
        let games = load_registry(&path);
        let game = games.iter().find(|g| g.id == "sunkenland").expect("sunkenland registered");
        assert_eq!(game.process, "Sunkenland.exe");
        assert_eq!(game.steam_app_id, Some(2080690));
        assert!(game.live_features);
    }
}
