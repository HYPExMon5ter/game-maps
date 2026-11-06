//! Maps of generated worlds, which no map pack can have. When such a world loads, the
//! agent reads it from the game, and this turns that into what a map pack would hold:
//! `<data>/worlds/<game>/<world>/tiles/{z}/{x}/{y}.jpg` and `pins.json`. The terrain never
//! changes, so it's only read the first time. Pins and `explored.bin` are read again when
//! the map opens: a game can drop locations as the world fills in (Valheim keeps one of
//! the spots a trader could be at once it's placed).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use game_maps_protocol::{WorldPin, ZonePins};
use image::imageops::{self, FilterType};
use image::{ImageBuffer, RgbImage};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::link::AgentLink;
use crate::{Paths, Registry};

/// Tells the windows a world's map files changed.
const READY_EVENT: &str = "world-map-ready";
/// The agent reports the world before the player spawns, and a new world can take minutes.
const READ_TRIES: u32 = 300;
const READ_RETRY: Duration = Duration::from_secs(2);
/// Shortest time between re-reads when the map opens.
const REREAD_INTERVAL: Duration = Duration::from_secs(60);

const TILE_SIZE: u32 = 256;
const JPEG_QUALITY: u8 = 88;
/// The whole map is this tile (see `projection` in the game's `game.json`).
const BASE_ZOOM: u32 = 3;
const BASE_TILE: u32 = 3;
/// Zoom levels below the base; the deepest is 16 tiles across, 2x the game's texture.
const LEVELS: u32 = 5;
/// Written last, so a folder without it has no finished tiles.
const DONE_FILE: &str = "done";

#[derive(Clone, Serialize)]
struct Ready {
    game_id: String,
    world_id: String,
    /// The tiles are new, not only the pins.
    tiles: bool,
}

/// What's been seen in a world, by area (see `Provider::next_zone_pins`).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Remembered {
    /// By `"x,y"`.
    zones: BTreeMap<String, Vec<WorldPin>>,
    #[serde(skip)]
    flush_pending: bool,
}

impl Remembered {
    /// Replaces what `zone` has. Something found stays found, a timer that's running keeps
    /// its end, and what moves is only in one place.
    fn merge(&mut self, zone: ZonePins, now_ms: u64) {
        let key = format!("{},{}", zone.x, zone.y);
        let old = self.zones.remove(&key).unwrap_or_default();
        let mut pins = zone.pins;
        for pin in &mut pins {
            let before = old.iter().find(|p| p.id == pin.id);
            pin.hidden &= before.is_none_or(|p| p.hidden);
            pin.respawn_at = pin.respawn_seconds.map(|s| before.and_then(|p| p.respawn_at).unwrap_or(now_ms + u64::from(s) * 1000));
        }
        for others in self.zones.values_mut() {
            others.retain(|p| !pins.iter().any(|n| n.id == p.id));
        }
        self.zones.retain(|_, pins| !pins.is_empty());
        if !pins.is_empty() {
            self.zones.insert(key, pins);
        }
    }
}

/// By world folder.
static REMEMBERED: LazyLock<Mutex<HashMap<PathBuf, Remembered>>> = LazyLock::new(Mutex::default);

/// New things are saved and shown in batches: pins are slow to rebuild for a few more.
const REMEMBER_DELAY: Duration = Duration::from_secs(10);
const REMEMBERED_FILE: &str = "resources.json";
const HOST_PINS_FILE: &str = "host-pins.json";

/// The agent saw what's in an area of `world`.
pub fn remember(app: &AppHandle, game: &str, world: &str, zone: ZonePins) {
    let Some(dir) = world_dir(&app.state::<Paths>().worlds, game, world) else { return };
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let mut worlds = REMEMBERED.lock().unwrap();
    let known = worlds.entry(dir.clone()).or_insert_with(|| std::fs::read(dir.join(REMEMBERED_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default());
    known.merge(zone, now_ms);
    if std::mem::replace(&mut known.flush_pending, true) {
        return;
    }
    let (app, game, world) = (app.clone(), game.to_owned(), world.to_owned());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REMEMBER_DELAY).await;
        let bytes = {
            let mut worlds = REMEMBERED.lock().unwrap();
            let Some(known) = worlds.get_mut(&dir) else { return };
            known.flush_pending = false;
            serde_json::to_vec(known)
        };
        let saved = bytes.map_err(|e| e.to_string()).and_then(|b| {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join(REMEMBERED_FILE), b).map_err(|e| e.to_string())
        });
        match saved {
            Ok(()) => {
                let _ = app.emit(READY_EVENT, Ready { game_id: game, world_id: world, tiles: false });
            }
            Err(e) => log::warn!("could not save what was seen in {world}: {e}"),
        }
    });
}

/// Keeps the locations a world's host shared, which a joining player's game doesn't have.
pub fn save_host_pins(app: &AppHandle, game: &str, world: &str, pins: &[WorldPin]) -> Result<(), String> {
    let dir = world_dir(&app.state::<Paths>().worlds, game, world).ok_or("unusable world id")?;
    let bytes = serde_json::to_vec(&PinFile { version: 1, pins }).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(HOST_PINS_FILE), bytes).map_err(|e| e.to_string())?;
    let _ = app.emit(READY_EVENT, Ready { game_id: game.to_owned(), world_id: world.to_owned(), tiles: false });
    Ok(())
}

#[derive(Serialize)]
struct PinFile<'a> {
    version: u32,
    pins: &'a [WorldPin],
}

/// Worlds being read, so two reads never write the same files.
static REFRESHING: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Mutex::default);

/// A world's folder. World ids come from the game, so only plain ones are used.
fn world_dir(root: &Path, game: &str, world: &str) -> Option<PathBuf> {
    let plain = !world.is_empty() && world.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| root.join(game).join(world))
}

/// Reads the loaded world's map, in the background.
pub fn refresh_soon(app: &AppHandle, game: &str, world: &str) {
    let Some(dir) = world_dir(&app.state::<Paths>().worlds, game, world) else {
        log::warn!("world map: unusable world id {world:?}");
        return;
    };
    if !REFRESHING.lock().unwrap().insert(dir.clone()) {
        return;
    }
    let (app, game, world) = (app.clone(), game.to_owned(), world.to_owned());
    tauri::async_runtime::spawn(async move {
        let result = refresh(&app, &dir).await;
        REFRESHING.lock().unwrap().remove(&dir);
        match result {
            Ok(tiles) => {
                let _ = app.emit(READY_EVENT, Ready { game_id: game, world_id: world, tiles });
            }
            Err(e) => log::warn!("world map {world}: {e}"),
        }
    });
}

/// The full map opened: catches up with what changed in the world, now and then.
pub fn map_opened(app: &AppHandle) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let link = app.state::<Arc<AgentLink>>();
    let Some((game, world)) = link.status().game_id.zip(link.world_id()) else { return };
    if !app.state::<Registry>().0.iter().any(|g| g.id == game && g.world_maps) {
        return;
    }
    let mut last = LAST.lock().unwrap();
    if last.is_none_or(|t| t.elapsed() >= REREAD_INTERVAL) {
        *last = Some(Instant::now());
        refresh_soon(app, &game, &world);
    }
}

/// Returns whether the tiles were made.
async fn refresh(app: &AppHandle, dir: &Path) -> Result<bool, String> {
    let link = app.state::<Arc<AgentLink>>().inner().clone();
    let needs_terrain = !dir.join("tiles").join(DONE_FILE).is_file();
    let mut tries = 0;
    let map = loop {
        match link.read_world_map(dir.to_string_lossy().into_owned(), needs_terrain).await {
            Ok(map) => break map,
            Err(e) if tries + 1 >= READ_TRIES => return Err(e),
            Err(_) => {
                tries += 1;
                tokio::time::sleep(READ_RETRY).await;
            }
        }
    };
    log::info!("world map: {} pins read into {}", map.pins.len(), dir.display());
    let dir = dir.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let pins = serde_json::to_vec(&PinFile { version: 1, pins: &map.pins }).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("pins.json"), pins).map_err(|e| e.to_string())?;
        if needs_terrain {
            build_tiles(&dir, map.size)?;
        }
        Ok::<bool, String>(needs_terrain)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cuts `terrain.rgb` into tiles, then removes it.
fn build_tiles(dir: &Path, size: u32) -> Result<(), String> {
    let started = std::time::Instant::now();
    let raw = dir.join("terrain.rgb");
    let bytes = std::fs::read(&raw).map_err(|e| format!("could not read the terrain: {e}"))?;
    let terrain: RgbImage = ImageBuffer::from_raw(size, size, bytes).ok_or("the terrain has the wrong size")?;
    let tiles = dir.join("tiles");
    let _ = std::fs::remove_dir_all(&tiles);
    for level in 0..LEVELS {
        let across = 1u32 << level;
        let side = TILE_SIZE * across;
        let image = if side == size { terrain.clone() } else { imageops::resize(&terrain, side, side, if side > size { FilterType::CatmullRom } else { FilterType::Triangle }) };
        write_level(&tiles, &image, BASE_ZOOM + level, across)?;
    }
    std::fs::write(tiles.join(DONE_FILE), []).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(raw);
    log::info!("world map: tiles built in {:.1} s", started.elapsed().as_secs_f32());
    Ok(())
}

/// Writes `image`, `across` tiles per side, at `zoom`.
fn write_level(tiles: &Path, image: &RgbImage, zoom: u32, across: u32) -> Result<(), String> {
    for column in 0..across {
        let x = BASE_TILE * across + column;
        let folder = tiles.join(zoom.to_string()).join(x.to_string());
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        for row in 0..across {
            let y = BASE_TILE * across + row;
            let tile = imageops::crop_imm(image, column * TILE_SIZE, row * TILE_SIZE, TILE_SIZE, TILE_SIZE).to_image();
            let file = std::fs::File::create(folder.join(format!("{y}.jpg"))).map_err(|e| e.to_string())?;
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), JPEG_QUALITY);
            encoder.encode_image(&tile).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(id: &str, hidden: bool, respawn_seconds: Option<u32>) -> WorldPin {
        WorldPin { id: id.into(), category: "x".into(), name: "x".into(), position: game_maps_protocol::Vec3::ZERO, hidden, respawn_seconds, respawn_at: None }
    }

    fn zone(x: i32, pins: Vec<WorldPin>) -> ZonePins {
        ZonePins { x, y: 0, pins }
    }

    #[test]
    fn an_area_replaces_what_it_had() {
        let mut r = Remembered::default();
        r.merge(zone(1, vec![pin("a", false, None), pin("b", false, None)]), 0);
        r.merge(zone(1, vec![pin("b", false, None)]), 0);
        assert_eq!(r.zones["1,0"].len(), 1);
        r.merge(zone(1, vec![]), 0);
        assert!(r.zones.is_empty());
    }

    #[test]
    fn something_found_stays_found() {
        let mut r = Remembered::default();
        r.merge(zone(1, vec![pin("silver", false, None)]), 0);
        r.merge(zone(1, vec![pin("silver", true, None)]), 0);
        assert!(!r.zones["1,0"][0].hidden);
    }

    #[test]
    fn what_moves_is_only_in_one_place() {
        let mut r = Remembered::default();
        r.merge(zone(1, vec![pin("ship", false, None)]), 0);
        r.merge(zone(2, vec![pin("ship", false, None)]), 0);
        assert!(!r.zones.contains_key("1,0"));
        assert_eq!(r.zones["2,0"].len(), 1);
    }

    #[test]
    fn a_running_timer_keeps_its_end() {
        let mut r = Remembered::default();
        r.merge(zone(1, vec![pin("berries", false, Some(60))]), 1000);
        assert_eq!(r.zones["1,0"][0].respawn_at, Some(61000));
        r.merge(zone(1, vec![pin("berries", false, Some(50))]), 11000);
        assert_eq!(r.zones["1,0"][0].respawn_at, Some(61000));
        r.merge(zone(1, vec![pin("berries", false, None)]), 70000);
        assert_eq!(r.zones["1,0"][0].respawn_at, None);
    }

    #[test]
    fn only_plain_world_ids_make_folders() {
        let root = Path::new("C:/w");
        assert_eq!(world_dir(root, "valheim", "world--123"), Some(root.join("valheim").join("world--123")));
        assert_eq!(world_dir(root, "valheim", "../secret"), None);
        assert_eq!(world_dir(root, "valheim", "a:b"), None);
        assert_eq!(world_dir(root, "valheim", ""), None);
    }

    #[test]
    fn cuts_every_zoom_level_with_north_on_top() {
        let dir = std::env::temp_dir().join(format!("game-maps-tiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let size = 512u32;
        // The north half is white, the south half black.
        let mut bytes = vec![0u8; (size * size * 3) as usize];
        bytes[..(size * size * 3 / 2) as usize].fill(255);
        std::fs::write(dir.join("terrain.rgb"), bytes).unwrap();

        build_tiles(&dir, size).unwrap();

        let tiles = dir.join("tiles");
        assert!(tiles.join(DONE_FILE).is_file());
        assert!(!dir.join("terrain.rgb").exists());
        assert!(tiles.join("3/3/3.jpg").is_file());
        // The deepest level starts at tile (3 * 16, 3 * 16).
        let north = image::open(tiles.join("7/48/48.jpg")).unwrap().to_rgb8();
        let south = image::open(tiles.join("7/48/63.jpg")).unwrap().to_rgb8();
        assert!(north.get_pixel(5, 5).0[0] > 200);
        assert!(south.get_pixel(5, 5).0[0] < 50);
        assert_eq!(std::fs::read_dir(tiles.join("7")).unwrap().count(), 16);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
