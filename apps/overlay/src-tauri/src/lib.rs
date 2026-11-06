//! Game Maps: a tray app with a small launcher window. It detects supported games,
//! loads the agent into them, shows the map overlay, and keeps itself and its map
//! data up to date.

mod analytics;
mod checks;
mod games;
mod injector;
mod launcher;
mod link;
mod map_window;
mod mappacks;
mod overlay;
mod recovery;
mod report;
mod settings;
mod updates;
mod worldmaps;

use std::path::PathBuf;
use std::sync::Arc;

use game_maps_protocol::{CompanionOrderOption, GamePin, GroundPoint, Hotkey, HotkeyAction, PathInfo, PlaceInfo, RouteCheck, TeleportTarget, Vec3, WorldSnapshot};
use tauri::http::{header, Response, StatusCode};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_opener::OpenerExt as _;

use analytics::Analytics;
use games::{AssetRoots, GameEntry};
use link::{AgentLink, LinkConfig, LinkStatus};
use mappacks::{MapPacks, PackStatus};
use overlay::{OverlayMode, OverlayState, PinConfig};
use settings::{AppSettings, HookMode, SettingsState, ZoomKeys};
use updates::{UpdateState, Updates};

/// Passed when started by Windows at login: stay in the tray.
const BACKGROUND_ARG: &str = "--background";
const PROJECT_URL: &str = "https://github.com/HYPExMon5ter/game-maps";
const PRIVACY_URL: &str = "https://github.com/HYPExMon5ter/game-maps#privacy";
const POSTHOG_URL: &str = "https://posthog.com";
const ISSUES_URL: &str = "https://github.com/HYPExMon5ter/game-maps/issues/new/choose";

struct Paths {
    /// Bundled game definitions (in development: the repository's `games/`).
    games: PathBuf,
    user: PathBuf,
    /// Maps made from the running game (see `worldmaps`).
    worlds: PathBuf,
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
async fn teleport(app: AppHandle, link: State<'_, Arc<AgentLink>>, target: TeleportTarget) -> Result<(), String> {
    link.inner().clone().teleport(&app, target).await
}

/// A walking route from the player to `to` on the game's navigation mesh.
#[tauri::command]
async fn find_path(link: State<'_, Arc<AgentLink>>, to: Vec3) -> Result<PathInfo, String> {
    let result = link.inner().clone().find_path(to).await;
    if let Err(e) = &result {
        // Routes are requested every 1.5 s: log each distinct failure once.
        static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let mut last = LAST.lock().unwrap();
        if last.as_deref() != Some(e.as_str()) {
            log::info!("route unavailable: {e}");
            *last = Some(e.clone());
        }
    }
    result
}

/// Which legs of a route (floor points about a meter apart) can't be walked right now.
#[tauri::command]
async fn check_route(link: State<'_, Arc<AgentLink>>, points: Vec<Vec3>) -> Result<RouteCheck, String> {
    link.inner().clone().check_route(points).await
}

/// The floor under points near the player, and whether the game camera sees them.
#[tauri::command]
async fn probe_ground(link: State<'_, Arc<AgentLink>>, points: Vec<Vec3>) -> Result<Vec<GroundPoint>, String> {
    link.inner().clone().probe_ground(points).await
}

/// What the game says about the place at `position`.
#[tauri::command]
async fn place_info(link: State<'_, Arc<AgentLink>>, position: Vec3) -> Result<PlaceInfo, String> {
    link.inner().clone().place_info(position).await
}

/// What a companion can be told to do right now.
#[tauri::command]
async fn companion_orders(link: State<'_, Arc<AgentLink>>, companion: String) -> Result<Vec<CompanionOrderOption>, String> {
    link.inner().clone().companion_orders(companion).await
}

/// Tells a companion to do something (an id from `companion_orders`).
#[tauri::command]
async fn companion_order(link: State<'_, Arc<AgentLink>>, companion: String, order: String) -> Result<(), String> {
    link.inner().clone().companion_order(companion, order).await
}

/// Returns the player to where they were before their last teleport.
#[tauri::command]
async fn teleport_back(link: State<'_, Arc<AgentLink>>) -> Result<(), String> {
    link.inner().clone().teleport_back().await
}

/// How many teleports "teleport back" can undo.
#[tauri::command]
fn teleport_back_steps(link: State<'_, Arc<AgentLink>>) -> usize {
    link.teleport_back_steps()
}

/// Sets the teleport back key (all games); `None` turns it off.
#[tauri::command]
fn set_back_key(app: AppHandle, link: State<'_, Arc<AgentLink>>, registry: State<'_, Registry>, key: Option<Hotkey>) -> Result<AppSettings, String> {
    if key.as_ref().is_some_and(|h| h.action != HotkeyAction::TeleportBack) {
        return Err("the key must be bound to teleport back".into());
    }
    let saved = settings::update(&app, |s| s.back_key = key)?;
    if let Some(game) = link.status().game_id.and_then(|id| registry.0.iter().find(|g| g.id == id)) {
        link.apply_hotkeys(&game.id, saved.hotkeys(game));
    }
    Ok(saved)
}

/// Sets the key of an action the map handles itself (all games); `None` turns it off.
#[tauri::command]
fn set_action_key(app: AppHandle, link: State<'_, Arc<AgentLink>>, registry: State<'_, Registry>, action: HotkeyAction, key: Option<Hotkey>) -> Result<AppSettings, String> {
    if key.as_ref().is_some_and(|h| h.action != action) {
        return Err("the key must be bound to its own action".into());
    }
    let saved = settings::update(&app, |s| {
        if let Some(slot) = s.action_keys.slot(action) {
            *slot = key;
        }
    })?;
    if let Some(game) = link.status().game_id.and_then(|id| registry.0.iter().find(|g| g.id == id)) {
        link.apply_hotkeys(&game.id, saved.hotkeys(game));
    }
    Ok(saved)
}

/// Asks where to save `contents`, then writes it. False when the user cancels.
async fn save_as(app: &AppHandle, name: &str, kind: (&str, &str), contents: String) -> Result<bool, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_file_name(name).add_filter(kind.0, &[kind.1]).save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.map_err(|e| e.to_string())? else { return Ok(false) };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(path, contents).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
async fn save_text_file(app: AppHandle, name: String, contents: String) -> Result<bool, String> {
    save_as(&app, &name, ("Pins", "json"), contents).await
}

/// Saves a problem report (see [`report`]). `ui` is what the asking window knows.
#[tauri::command]
async fn save_report(app: AppHandle, ui: String) -> Result<bool, String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let saved = save_as(&app, &format!("game-maps-report-{stamp}.txt"), ("Text", "txt"), report::build(&app, &ui)).await?;
    if saved {
        log::info!("problem report saved");
    }
    Ok(saved)
}

/// Opens the project's page for reporting a problem.
#[tauri::command]
fn open_issue_page(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(ISSUES_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Opens the project's page.
#[tauri::command]
fn open_project_page(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(PROJECT_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Opens the list of what the usage counts contain.
#[tauri::command]
fn open_privacy_page(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(PRIVACY_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Opens the site of the service that receives the usage counts.
#[tauri::command]
fn open_posthog_page(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(POSTHOG_URL, None::<&str>).map_err(|e| e.to_string())
}

/// Dev tooling: scans the loaded world and saves it as JSON under `exports/`.
/// Returns the file path. `tools/pins` turns exports into a game's pin data.
#[tauri::command]
fn set_game_pins(link: State<'_, Arc<AgentLink>>, pins: Vec<GamePin>) -> Result<(), String> {
    link.set_game_pins(pins)
}

#[tauri::command]
fn set_game_options(link: State<'_, Arc<AgentLink>>, options: std::collections::BTreeMap<String, bool>) -> Result<(), String> {
    link.set_game_options(options)
}

#[tauri::command]
fn save_host_pins(app: AppHandle, link: State<'_, Arc<AgentLink>>, pins: Vec<game_maps_protocol::WorldPin>) -> Result<(), String> {
    let (game, world) = link.status().game_id.zip(link.world_id()).ok_or("not in a world")?;
    worldmaps::save_host_pins(&app, &game, &world, &pins)
}

#[tauri::command]
fn game_ping(link: State<'_, Arc<AgentLink>>, position: Vec3) -> Result<(), String> {
    link.game_ping(position)
}

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

/// Opens `game_id`'s map. A running game's map stays shown over any other.
#[tauri::command]
fn open_map(app: AppHandle, game_id: String) {
    mappacks::request(&app, &game_id);
    let _ = app.emit("show-game", game_id);
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

#[tauri::command]
fn set_all_hook_modes(app: AppHandle, link: State<'_, Arc<AgentLink>>, mode: HookMode) -> Result<AppSettings, String> {
    let saved = settings::update(&app, |s| s.set_all_hook_modes(mode))?;
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

/// Sets the map zoom keys (all games), or with `None` restores the defaults.
#[tauri::command]
fn set_zoom_keys(app: AppHandle, link: State<'_, Arc<AgentLink>>, registry: State<'_, Registry>, keys: Option<ZoomKeys>) -> Result<AppSettings, String> {
    let keys = keys.unwrap_or_default();
    keys.validate()?;
    let saved = settings::update(&app, |s| s.zoom_keys = keys)?;
    if let Some(game) = link.status().game_id.and_then(|id| registry.0.iter().find(|g| g.id == id)) {
        link.apply_hotkeys(&game.id, saved.hotkeys(game));
    }
    Ok(saved)
}

#[tauri::command]
fn set_auto_update(app: AppHandle, enabled: bool) -> Result<AppSettings, String> {
    settings::update(&app, |s| s.auto_update = enabled)
}

/// Sets whether usage counts are sent.
#[tauri::command]
fn set_analytics(app: AppHandle, enabled: bool) -> Result<AppSettings, String> {
    let saved = settings::update(&app, |s| s.analytics = Some(enabled))?;
    analytics::app_started(&app);
    Ok(saved)
}

/// A feature used during play, for the session's usage counts.
#[tauri::command]
fn count_feature(app: AppHandle, feature: analytics::Feature) {
    analytics::count(&app, feature);
}

/// An error in a window's UI, for the log.
#[tauri::command]
fn log_ui_error(window: tauri::Window, message: String) {
    log::error!("[ui:{}] {message}", window.label());
}

/// Something that happened in a window's UI, for the log.
#[tauri::command]
fn log_ui_info(window: tauri::Window, message: String) {
    log::info!("[ui:{}] {message}", window.label());
}

/// Monitors the map window can be put on. Async: monitor and window calls wait for the
/// main thread, where synchronous commands run.
#[tauri::command]
async fn list_monitors(app: AppHandle) -> Vec<map_window::MonitorInfo> {
    map_window::monitors(&app)
}

/// Keeps the map window open on `monitor` (by name), or closes it (`None`).
#[tauri::command]
async fn set_map_window(app: AppHandle, monitor: Option<String>) -> Result<AppSettings, String> {
    map_window::apply(&app, monitor.as_deref())?;
    settings::update(&app, |s| s.map_window = monitor)
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
fn connect_game(app: AppHandle, link: State<'_, Arc<AgentLink>>) -> Result<(), String> {
    link.request_connect()?;
    if let Some(id) = link.status().game_id {
        mappacks::request(&app, &id);
    }
    Ok(())
}

#[tauri::command]
fn disconnect_game(link: State<'_, Arc<AgentLink>>) -> Result<(), String> {
    link.disconnect()
}

#[tauri::command]
fn launch_game(app: AppHandle, registry: State<'_, Registry>, game_id: String) -> Result<(), String> {
    let game = registry.0.iter().find(|g| g.id == game_id).ok_or("unknown game")?;
    let app_id = game.steam_app_id.ok_or("this game can't be started from Game Maps")?;
    mappacks::request(&app, &game_id);
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
    (Paths { games, user, worlds: data.join("worlds") }, agent)
}

fn serve_asset(app: &AppHandle, uri_path: &str) -> Response<Vec<u8>> {
    let paths = app.state::<Paths>();
    let packs = app.state::<MapPacks>();
    let roots = AssetRoots { games: &paths.games, packs: packs.dir(), worlds: &paths.worlds, user: &paths.user };
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
    recovery::log_panics();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| launcher::show(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                // The session a problem happened in may not be the newest file.
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
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
            map_window::WINDOW_LABEL => map_window::on_window_event(window, event),
            _ => {}
        })
        .setup(|app| {
            let handle = app.handle().clone();
            recovery::protect_save_files(&handle);
            for window in handle.webview_windows().values() {
                recovery::watch_webview(window);
            }
            let (paths, agent_dll) = resolve_paths(&handle);
            log::info!("Game Maps {} - games: {}, agent: {}", env!("CARGO_PKG_VERSION"), paths.games.display(), agent_dll.display());
            let registry = games::load_registry(&paths.games);
            let data_dir = paths.user.parent().unwrap().to_owned();
            // Games whose maps come from the running game have no pack to get.
            let ids: Vec<String> = registry.iter().filter(|g| !g.world_maps).map(|g| g.id.clone()).collect();
            app.manage(MapPacks::new(data_dir.join("maps"), &paths.games, &ids));
            app.manage(SettingsState::load(&handle));
            app.manage(Analytics::new());
            analytics::app_started(&handle);
            app.manage(paths);
            // A game just started: pick up a map published since the last check, and its
            // map if there is none yet and the player wants the app connecting to it.
            // Before the link starts, so a game that's already running counts too.
            let h = handle.clone();
            app.listen_any("agent-status", move |event| {
                let status = serde_json::from_str::<serde_json::Value>(event.payload()).unwrap_or_default();
                let started = matches!(status.get("state").and_then(|v| v.as_str()), Some("connecting" | "ready"));
                if started {
                    mappacks::sync_soon(&h);
                    if let Some(id) = status.get("game_id").and_then(|v| v.as_str()) {
                        if settings::get(&h).hook_mode(id) == HookMode::Auto {
                            mappacks::request(&h, id);
                        }
                    }
                }
            });

            let link = AgentLink::start(handle.clone(), LinkConfig { games: registry.clone(), agent_dll, staging_dir: data_dir.join("agent") });
            app.manage(link);
            app.manage(Registry(registry));

            overlay::watch_focus(handle.clone());
            checks::start_fullscreen_watch(handle.clone());
            mappacks::start_background(handle.clone());
            updates::start_background(handle.clone());

            map_window::restore(&handle);
            build_tray(app)?;
            if !std::env::args().any(|a| a == BACKGROUND_ARG) {
                launcher::show(&handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_last_snapshot,
            set_game_pins,
            set_game_options,
            save_host_pins,
            game_ping,
            teleport,
            teleport_back,
            log_ui_error,
            log_ui_info,
            list_monitors,
            set_map_window,
            find_path,
            check_route,
            probe_ground,
            companion_orders,
            place_info,
            companion_order,
            teleport_back_steps,
            set_back_key,
            set_action_key,
            save_text_file,
            save_report,
            open_issue_page,
            open_project_page,
            open_privacy_page,
            open_posthog_page,
            export_world,
            get_overlay_mode,
            open_map,
            close_overlay,
            set_pin_config,
            list_games,
            save_user_icon,
            get_app_settings,
            set_hook_mode,
            set_all_hook_modes,
            set_map_hotkey,
            set_zoom_keys,
            set_auto_update,
            set_analytics,
            count_feature,
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
