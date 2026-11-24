//! Each world's explored areas (fog of war), one file per world:
//! `<data>/fog/<game>/<name>.bin`, and `<data>/fog/<game>/unsaved/<name>.bin` for new games
//! not saved yet. `<name>` is the SHA-256 of the world's id: ids come from the game and can
//! hold anything, and Windows file names ignore case.
//!
//! The windows own the contents (see `lib/fog` in the UI); this only keeps the bytes, whole
//! or not at all. They were in `user-data.json` once, which then had to be rewritten in full
//! every few seconds of exploring.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::State;

use crate::files::write_atomic;
use crate::{percent_decode, Paths, Registry};

/// World ids of new games that haven't been saved (see `isUnsaved` in the UI).
const UNSAVED_PREFIX: &str = "unsaved:";
const UNSAVED_DIR: &str = "unsaved";
/// The largest fog there is: 2048 x 2048 cells, a bit each, and a header.
const MAX_BYTES: usize = 2048 * 2048 / 8 + 1024;

/// One at a time, so a save can't land after the delete that was meant to follow it.
static FILES: Mutex<()> = Mutex::new(());

fn fog_file(root: &Path, game: &str, world: &str) -> PathBuf {
    let name: String = Sha256::digest(world.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let dir = root.join(game);
    let dir = if world.starts_with(UNSAVED_PREFIX) { dir.join(UNSAVED_DIR) } else { dir };
    dir.join(format!("{name}.bin"))
}

/// Only games the app knows have fog: the id becomes a folder's name.
fn known_game(registry: &Registry, game: &str) -> Result<(), String> {
    registry.0.iter().any(|g| g.id == game).then_some(()).ok_or_else(|| format!("unknown game {game:?}"))
}

fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// A world's saved fog; empty when it has none.
#[tauri::command(async)]
pub fn load_fog(paths: State<'_, Paths>, registry: State<'_, Registry>, game: String, world: String) -> Result<Response, String> {
    known_game(&registry, &game)?;
    let _files = FILES.lock().unwrap();
    match std::fs::read(fog_file(&paths.fog, &game, &world)) {
        Ok(bytes) => Ok(Response::new(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Response::new(Vec::new())),
        Err(e) => Err(e.to_string()),
    }
}

/// Saves a world's fog: the request's body, with the game and the world (percent-encoded)
/// in its `game` and `world` headers.
#[tauri::command(async)]
pub fn save_fog(paths: State<'_, Paths>, registry: State<'_, Registry>, request: Request<'_>) -> Result<(), String> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(percent_decode).ok_or_else(|| format!("missing {name}"));
    let (game, world) = (header("game")?, header("world")?);
    known_game(&registry, &game)?;
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => std::borrow::Cow::Borrowed(bytes.as_slice()),
        // How bytes arrive when the web view has had to fall back to its slower way of calling.
        InvokeBody::Json(json) => std::borrow::Cow::Owned(serde_json::from_value::<Vec<u8>>(json.clone()).map_err(|_| "the fog must be sent as bytes")?),
    };
    if bytes.len() > MAX_BYTES {
        return Err("the fog is too large".into());
    }
    let _files = FILES.lock().unwrap();
    write_atomic(&fog_file(&paths.fog, &game, &world), &bytes).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn delete_fog(paths: State<'_, Paths>, registry: State<'_, Registry>, game: String, world: String) -> Result<(), String> {
    known_game(&registry, &game)?;
    let _files = FILES.lock().unwrap();
    remove(&fog_file(&paths.fog, &game, &world))
}

/// Forgets the fog of every new game of `game` that was never saved.
#[tauri::command(async)]
pub fn delete_unsaved_fog(paths: State<'_, Paths>, registry: State<'_, Registry>, game: String) -> Result<(), String> {
    known_game(&registry, &game)?;
    let _files = FILES.lock().unwrap();
    match std::fs::remove_dir_all(paths.fog.join(&game).join(UNSAVED_DIR)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worlds_that_differ_only_in_case_get_their_own_files() {
        let root = Path::new("C:/fog");
        let (upper, lower) = (fog_file(root, "sunkenland", "joined:Bay"), fog_file(root, "sunkenland", "joined:bay"));
        assert!(!upper.to_string_lossy().eq_ignore_ascii_case(&lower.to_string_lossy()));
        assert_eq!(upper.parent(), Some(root.join("sunkenland").as_path()));
        assert_eq!(upper.file_name().unwrap().len(), 64 + ".bin".len());
    }

    #[test]
    fn unsaved_worlds_are_kept_apart() {
        let root = Path::new("C:/fog");
        assert_eq!(fog_file(root, "sotf", "unsaved:12-1").parent(), Some(root.join("sotf").join(UNSAVED_DIR).as_path()));
        assert_eq!(fog_file(root, "sotf", "sp:12").parent(), Some(root.join("sotf").as_path()));
    }
}
