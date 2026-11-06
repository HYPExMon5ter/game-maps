//! Saves the map of a world that only exists at runtime (see `Provider::read_world_map`).
//!
//! Reading has to happen on the game's main thread, but drawing the picture and writing
//! it takes longer than a frame, so that runs on a thread of its own, which touches no
//! managed objects.

use std::path::Path;
use std::sync::Mutex;
use std::thread::JoinHandle;

use game_maps_protocol::{WorldMap, WorldPin};

/// What a provider read from the game, ready to leave the main thread.
pub struct WorldMapCapture {
    /// Pixels per side of the terrain.
    pub size: u32,
    pub pins: Vec<WorldPin>,
    /// Bit per fog cell, in the app's fog layout (see `lib/fog.ts`).
    pub explored: Vec<u8>,
    /// Draws the terrain.
    pub terrain: Option<Box<dyn FnOnce() -> Terrain + Send>>,
}

pub struct Terrain {
    /// 8-bit RGB, row 0 is the map's north edge.
    pub image: Vec<u8>,
    /// A biome id per pixel of the game's own map, row 0 in the south (see `lib/biomes.ts`).
    pub biomes: Vec<u8>,
}

/// Running saves: their code goes away with the agent, so it waits for them.
static SAVES: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

/// Writes `capture` into `dir` in the background, then calls `done`.
pub fn save(capture: WorldMapCapture, dir: String, done: impl FnOnce(Result<WorldMap, String>) + Send + 'static) {
    let handle = std::thread::spawn(move || done(write(capture, Path::new(&dir))));
    let mut saves = SAVES.lock().unwrap();
    saves.retain(|s| !s.is_finished());
    saves.push(handle);
}

fn write(capture: WorldMapCapture, dir: &Path) -> Result<WorldMap, String> {
    let failed = |e: std::io::Error| format!("could not save the map to {}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(failed)?;
    std::fs::write(dir.join("explored.bin"), &capture.explored).map_err(failed)?;
    if let Some(draw) = capture.terrain {
        let terrain = draw();
        std::fs::write(dir.join("biomes.bin"), terrain.biomes).map_err(failed)?;
        std::fs::write(dir.join("terrain.rgb"), terrain.image).map_err(failed)?;
    }
    Ok(WorldMap { size: capture.size, pins: capture.pins })
}

/// Blocks until every running save has finished.
pub fn wait_for_saves() {
    for save in std::mem::take(&mut *SAVES.lock().unwrap()) {
        let _ = save.join();
    }
}
