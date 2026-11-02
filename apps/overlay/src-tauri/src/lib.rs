//! Game Maps: a tray app with a small launcher window. It detects supported games,
//! loads the agent into them, shows the map overlay, and keeps itself and its map
//! data up to date.

mod checks;
mod games;
mod injector;
mod launcher;
mod link;
mod mappacks;
mod overlay;
mod settings;
mod updates;

use std::path::PathBuf;
use std::sync::Arc;

use game_maps_protocol::{Hotkey, HotkeyAction, TeleportTarget, WorldSnapshot};
use tauri::http::{header, Response, StatusCode};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt as _;

use games::{AssetRoots, GameEntry};
use link::{AgentLink, LinkConfig, LinkStatus};
use mappacks::{MapPacks, PackStatus};
use overlay::{OverlayMode, OverlayState, PinConfig};
use settings::{AppSettings, HookMode, SettingsState};
use updates::{UpdateState, Updates};

/// Passed when started by Windows at login: stay in the tray.
const BACKGROUND_ARG: &str = "--background";

struct Paths {
    /// Bundled game definitions (in development: the repository's `games/`).
    games: PathBuf,
    user: PathBuf,
}

struct Registry(Vec<GameEntry>);

// --- Map overlay -------------------------------------------------------------------

#[tauri::command]
fn get_status(link: State<'_, Arc<AgentLink>>) -> LinkStatus {
    link.status()
}

#[tauri::command]
fn get_last_snapshot(link: State<'_, Arc<AgentLink>>) -> Option<WorldSnapshot> {
    link.last_snapshot()
}

#[tauri::command]
async fn teleport(link: State<'_, Arc<AgentLink>>, target: TeleportTarget) -> Result<(), String> {
    link.inner().clone().teleport(target).await
}

/// Dev tooling: scans the loaded world and saves it as JSON under `exports/`.
/// Returns the file path. `tools/pins` turns exports into a game's pin data.
#[tauri::command]
async fn export_world(link: State<'_, Arc<AgentLink>>, paths: State<'_, Paths>) -> Result<String, String> {
    let export = link.inner().clone().export_world().await?;
    let dir = paths.user.parent().unwrap().join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let file = dir.join(format!("{}-{stamp}.json", export.game));
    let json = serde_json::to_vec_pretty(&export).map_err(|e| e.to_string())?;
    std::fs::write(&file, json).map_err(|e| e.to_string())?;
    log::info!("world export: {} entries -> {}", export.entries.len(), file.display());
    Ok(file.display().to_string())
}

#[tauri::command]
fn get_overlay_mode(app: AppHandle) -> OverlayMode {
    overlay::mode(&app)
}

#[tauri::command]
fn open_map(app: AppHandle) {
    overlay::open(&app);
}

#[tauri::command]
fn close_overlay(app: AppHandle) {
    overlay::close(&app);
}

#[tauri::command]
fn set_pin_config(app: AppHandle, config: PinConfig) {
    overlay::set_pin_config(&app, config);
}

#[tauri::command]
fn list_games(registry: State<'_, Registry>) -> Vec<GameEntry> {
    registry.0.clone()
}

/// Stores a user-supplied pin icon and returns its asset path (`user/icons/<name>`).
#[tauri::command]
fn save_user_icon(paths: State<'_, Paths>, file_name: String, bytes: Vec<u8>) -> Result<String, String> {
    const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;
    if bytes.len() > MAX_ICON_BYTES {
        return Err("icon is larger than 2 MB".into());
    }
    let ext = std::path::Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| matches!(e.as_str(), "png" | "webp" | "jpg" | "jpeg" | "svg"))
        .ok_or("icons must be PNG, WebP, JPEG or SVG")?;
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let dir = paths.user.join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("{hash:016x}.{ext}");
    std::fs::write(dir.join(&name), &bytes).map_err(|e| e.to_string())?;
    Ok(format!("user/icons/{name}"))
}

// --- Launcher ------------------------------------------------------------------------

#[tauri::command]
fn get_app_settings(app: AppHandle) -> AppSettings {
    settings::get(&app)
}

#[tauri::command]
fn set_hook_mode(app: AppHandle, link: State<'_, Arc<AgentLink>>, game_id: String, mode: HookMode) -> Result<AppSettings, String> {
    let saved = settings::update(&app, |s| {
        s.hook_modes.insert(game_id, mode);
    })?;
    link.wake();
    Ok(saved)
}

/// Sets (or with `None`, resets to the game's default) the key that opens a game's map.
#[tauri::command]
fn set_map_hotkey(
    app: AppHandle,
    link: State<'_, Arc<AgentLink>>,
    registry: State<'_, Registry>,
    game_id: String,
    hotkey: Option<Hotkey>,
) -> Result<AppSettings, String> {
    let game = registry.0.iter().find(|g| g.id == game_id).ok_or("unknown game")?;
    if hotkey.as_ref().is_some_and(|h| h.action != HotkeyAction::ToggleMap) {
        return Err("only the map hotkey can be set".into());
    }
    let saved = settings::update(&app, |s| match hotkey {
        Some(h) => {
            s.map_hotkeys.insert(game_id.clone(), h);
        }
        None => {
            s.map_hotkeys.remove(&game_id);
        }
    })?;
    link.apply_hotkeys(&game_id, saved.hotkeys(game));
    Ok(saved)
}

#[tauri::command]
fn set_auto_update(app: AppHandle, enabled: bool) -> Result<AppSettings, String> {
    settings::update(&app, |s| s.auto_update = enabled)
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let autolaunch = app.autolaunch();
    let result = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    result.map_err(|e| e.to_string())?;
    Ok(autolaunch.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
fn connect_game(link: State<'_, Arc<AgentLink>>) -> Result<(), String> {
    link.request_connect()
}

#[tauri::command]
fn disconnect_game(link: State<'_, Arc<AgentLink>>) -> Result<(), String> {
    link.disconnect()
}

#[tauri::command]
fn launch_game(app: AppHandle, registry: State<'_, Registry>, game_id: String) -> Result<(), String> {
    let game = registry.0.iter().find(|g| g.id == game_id).ok_or("unknown game")?;
    let app_id = game.steam_app_id.ok_or("this game can't be started from Game Maps")?;
    app.opener().open_url(format!("steam://rungameid/{app_id}"), None::<&str>).map_err(|_| "Could not start Steam. Is it installed?".to_string())
}

#[tauri::command]
fn get_setup_checks(app: AppHandle) -> checks::SetupChecks {
    checks::current(&app)
}

#[tauri::command]
fn get_map_packs(packs: State<'_, MapPacks>) -> Vec<PackStatus> {
    packs.statuses()
}

#[tauri::command]
async fn install_map_pack(app: AppHandle, packs: State<'_, MapPacks>, game_id: String) -> Result<(), String> {
    if packs.statuses().iter().any(|s| s.game_id == game_id && s.available.is_none()) {
        packs.refresh(&app).await?;
    }
    packs.install(&app, &game_id).await
}

#[tauri::command]
fn get_update_state(updates: State<'_, Updates>) -> UpdateState {
    updates.state()
}

#[tauri::command]
async fn check_for_updates(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    updates.check(&app, true).await
}

#[tauri::command]
async fn install_update(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    updates.install(&app).await
}

#[tauri::command]
fn open_logs_folder(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

// --- Setup ---------------------------------------------------------------------------

fn resolve_paths(app: &AppHandle) -> (Paths, PathBuf) {
    let data = app.path().app_local_data_dir().expect("app data dir");
    let user = data.join("user");
    let (games, agent) = if cfg!(debug_assertions) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let agent = ["release", "debug"]
            .iter()
            .map(|p| root.join("target").join(p).join("game_maps_agent.dll"))
            .filter(|p| p.exists())
            .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .unwrap_or_else(|| root.join("target/release/game_maps_agent.dll"));
        (root.join("games"), agent)
    } else {
        let res = app.path().resource_dir().expect("resource dir");
        (res.join("games"), res.join("agent").join("game_maps_agent.dll"))
    };
    (Paths { games, user }, agent)
}

fn serve_asset(app: &AppHandle, uri_path: &str) -> Response<Vec<u8>> {
    let paths = app.state::<Paths>();
    let packs = app.state::<MapPacks>();
    let roots = AssetRoots { games: &paths.games, packs: packs.dir(), user: &paths.user };
    let decoded = percent_decode(uri_path);
    let found = games::resolve_asset(&decoded, &roots).into_iter().find_map(|f| std::fs::read(&f).ok().map(|b| (f, b)));
    // Map packs can be replaced while the app runs, so nothing is cached by the webview
    // (MapLibre keeps decoded tiles in memory anyway).
    let builder = Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*").header(header::CACHE_CONTROL, "no-cache");
    match found {
        Some((f, bytes)) => builder.header(header::CONTENT_TYPE, games::content_type(&f)).body(bytes).unwrap(),
        None => builder.status(StatusCode::NOT_FOUND).body(Vec::new()).unwrap(),
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let launcher_item = MenuItem::with_id(app, "launcher", "Open Game Maps", true, None::<&str>)?;
    let map_item = MenuItem::with_id(app, "map", "Open map", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Game Maps", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&launcher_item, &map_item, &quit_item])?;
    TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Game Maps")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "launcher" => launcher::show(app),
            "map" => overlay::open(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                launcher::show(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| launcher::show(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::Builder::new().args([BACKGROUND_ARG]).build())
        .manage(OverlayState::default())
        .manage(Updates::default())
        .manage(checks::FullscreenWatch::default())
        .register_asynchronous_uri_scheme_protocol("gamemaps", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || responder.respond(serve_asset(&app, &path)));
        })
        .on_window_event(|window, event| match window.label() {
            overlay::WINDOW_LABEL if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) => {
                overlay::enforce_placement(window.app_handle());
            }
            launcher::WINDOW_LABEL => launcher::on_window_event(window, event),
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let (paths, agent_dll) = resolve_paths(&handle);
            log::info!("Game Maps {} - games: {}, agent: {}", env!("CARGO_PKG_VERSION"), paths.games.display(), agent_dll.display());
            let registry = games::load_registry(&paths.games);
            let data_dir = paths.user.parent().unwrap().to_owned();
            let ids: Vec<String> = registry.iter().map(|g| g.id.clone()).collect();
            app.manage(MapPacks::new(data_dir.join("maps"), &paths.games, &ids));
            app.manage(SettingsState::load(&handle));
            app.manage(paths);
            let link = AgentLink::start(handle.clone(), LinkConfig { games: registry.clone(), agent_dll, staging_dir: data_dir.join("agent") });
            app.manage(link);
            app.manage(Registry(registry));

            overlay::watch_focus(handle.clone());
            checks::start_fullscreen_watch(handle.clone());
            mappacks::start_background(handle.clone());
            updates::start_background(handle.clone());

            // A game just started: pick up a map published since the last check.
            let h = handle.clone();
            app.listen_any("agent-status", move |event| {
                let status = serde_json::from_str::<serde_json::Value>(event.payload()).unwrap_or_default();
                let started = matches!(status.get("state").and_then(|v| v.as_str()), Some("connecting" | "ready"));
                if started {
                    mappacks::sync_soon(&h);
                }
            });

            build_tray(app)?;
            if !std::env::args().any(|a| a == BACKGROUND_ARG) {
                launcher::show(&handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_last_snapshot,
            teleport,
            export_world,
            get_overlay_mode,
            open_map,
            close_overlay,
            set_pin_config,
            list_games,
            save_user_icon,
            get_app_settings,
            set_hook_mode,
            set_map_hotkey,
            set_auto_update,
            get_autostart,
            set_autostart,
            connect_game,
            disconnect_game,
            launch_game,
            get_setup_checks,
            get_map_packs,
            install_map_pack,
            get_update_state,
            check_for_updates,
            install_update,
            open_logs_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Game Maps");
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("/games/a%20b/c.png"), "/games/a b/c.png");
        assert_eq!(percent_decode("/x%2"), "/x%2");
        assert_eq!(percent_decode("/plain"), "/plain");
    }
}
