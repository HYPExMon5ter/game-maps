//! Game Maps overlay app: tray app that detects supported games, loads the agent into
//! them, and shows the map overlay.

mod games;
mod injector;
mod link;
mod overlay;

use std::path::PathBuf;
use std::sync::Arc;

use game_maps_protocol::{Hotkey, TeleportTarget, WorldSnapshot};
use tauri::http::{header, Response, StatusCode};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State};

use link::{AgentLink, LinkConfig, LinkStatus};
use overlay::{OverlayMode, OverlayState, PinConfig};

struct Paths {
    games: PathBuf,
    user: PathBuf,
}

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
fn set_hotkeys(link: State<'_, Arc<AgentLink>>, hotkeys: Vec<Hotkey>) {
    link.set_hotkeys(hotkeys);
}

#[tauri::command]
fn get_overlay_mode(app: AppHandle) -> OverlayMode {
    overlay::mode(&app)
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
fn list_games(paths: State<'_, Paths>) -> Vec<String> {
    games::load_registry(&paths.games).into_iter().map(|g| g.id).collect()
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

fn serve_asset(paths: &Paths, uri_path: &str) -> Response<Vec<u8>> {
    let decoded = percent_decode(uri_path);
    let file = games::resolve_asset(&decoded, &paths.games, &paths.user);
    let builder = Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    match file.and_then(|f| std::fs::read(&f).ok().map(|b| (f, b))) {
        Some((f, bytes)) => builder
            .header(header::CONTENT_TYPE, games::content_type(&f))
            .header(header::CACHE_CONTROL, "max-age=3600")
            .body(bytes)
            .unwrap(),
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| overlay::open(app)))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(OverlayState::default())
        .register_asynchronous_uri_scheme_protocol("gamemaps", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                let paths = app.state::<Paths>();
                responder.respond(serve_asset(&paths, &path));
            });
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) && window.label() == overlay::WINDOW_LABEL {
                overlay::enforce_placement(window.app_handle());
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let (paths, agent_dll) = resolve_paths(&handle);
            log::info!("games: {}, agent: {}", paths.games.display(), agent_dll.display());
            let games = games::load_registry(&paths.games);
            let staging_dir = paths.user.parent().unwrap().join("agent");
            app.manage(paths);
            let link = AgentLink::start(handle.clone(), LinkConfig { games, agent_dll, staging_dir });
            app.manage(link);
            overlay::watch_focus(handle.clone());

            let open_item = MenuItem::with_id(app, "open", "Open map", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Game Maps", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("Game Maps")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => overlay::open(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Launched by the user: show the map. `--background` starts in the tray only.
            if !std::env::args().any(|a| a == "--background") {
                overlay::open(&handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_last_snapshot,
            teleport,
            export_world,
            set_hotkeys,
            get_overlay_mode,
            close_overlay,
            set_pin_config,
            list_games,
            save_user_icon,
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
