//! Map packs: per-game map data (tiles, icons, pins) downloaded from GitHub Releases,
//! so map fixes and new map data ship without an app update.
//!
//! `maps.json` on the `maps` release lists the current pack for each game. A pack is
//! a zip whose root mirrors `games/<id>/` (minus `game.json`, which ships with the
//! app) and is installed into `<app data>/maps/<id>/`, next to a `pack.json` recording
//! its version. `tools/mappack/publish.py` builds and uploads packs.
//!
//! Every supported game's pack is installed and kept on the latest published version:
//! checked at startup, every 30 minutes, and when a game starts.
//!
//! A game can also publish optional parts ([`EXTRAS`]) as packs of their own, listed under
//! its pack's `extras`. They're downloaded only when the player asks (the 3D view's models
//! are bigger than the rest of the map), into `<app data>/maps/extras/<part>/<id>/`, and
//! kept up to date from then on.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::analytics::{self, Problem};

pub const MANIFEST_URL: &str = "https://github.com/HYPExMon5ter/game-maps/releases/download/maps/maps.json";

/// Where the pack list comes from. Development builds can test packs from elsewhere
/// (`GAME_MAPS_MANIFEST_URL`, e.g. a local server for `target/mappacks`).
fn manifest_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("GAME_MAPS_MANIFEST_URL") {
        return url;
    }
    MANIFEST_URL.to_owned()
}
/// Marks a finished install; written last, so a folder without it is incomplete.
const PACK_FILE: &str = "pack.json";
pub const EVENT: &str = "map-packs";
/// How often published maps are checked while the app runs.
const CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Sooner after a failure: a player without a map shouldn't wait out a dropped connection.
const RETRY_INTERVAL: Duration = Duration::from_secs(2 * 60);
/// How many times to try for a map the player asked for: about an hour.
const REQUEST_TRIES: u32 = 30;
const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// A game start re-checks unless the last check is more recent than this.
const GAME_START_MAX_AGE: Duration = Duration::from_secs(2 * 60);

/// Optional parts of a map, each the name of the folder it fills in the game's folder.
pub const EXTRAS: &[&str] = &["3d"];

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    packs: HashMap<String, RemotePack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePack {
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// Download size in bytes.
    pub size: u64,
    /// The game's Steam build the map was made from.
    #[serde(default, rename = "gameBuild", skip_serializing_if = "Option::is_none")]
    pub game_build: Option<String>,
    /// The optional parts published for this map, by name (see [`EXTRAS`]).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extras: HashMap<String, RemotePack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledPack {
    version: String,
    #[serde(default)]
    game_build: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackStatus {
    pub game_id: String,
    /// Installed pack version; `"development"` when map data comes from the repository.
    pub installed: Option<String>,
    pub available: Option<RemotePack>,
    pub downloading: Option<Progress>,
    pub error: Option<String>,
    /// The optional parts (see [`EXTRAS`]); what's published of them is in `available`.
    pub extras: HashMap<String, ExtraStatus>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExtraStatus {
    /// Installed version, or `"development"`; none until the player asks for it.
    pub installed: Option<String>,
    pub downloading: Option<Progress>,
    pub error: Option<String>,
}

/// A map pack or one of its optional parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Map,
    Extra(&'static str),
}

impl Part {
    fn extra(name: &str) -> Option<Self> {
        EXTRAS.iter().find(|e| **e == name).map(|e| Part::Extra(e))
    }

    /// The folder this part's game folders are installed in.
    fn dir(self, packs: &Path) -> PathBuf {
        match self {
            Part::Map => packs.to_path_buf(),
            Part::Extra(name) => packs.join("extras").join(name),
        }
    }

    /// Key for this part of a game's map in failure counts.
    fn key(self, id: &str) -> String {
        match self {
            Part::Map => id.to_owned(),
            Part::Extra(name) => format!("{id}/{name}"),
        }
    }

    fn remote(self, status: &PackStatus) -> Option<RemotePack> {
        let map = status.available.as_ref()?;
        match self {
            Part::Map => Some(map.clone()),
            Part::Extra(name) => map.extras.get(name).cloned(),
        }
    }

    /// The installed version, download and error of this part in a game's status.
    fn fields(self, status: &mut PackStatus) -> (&mut Option<String>, &mut Option<Progress>, &mut Option<String>) {
        match self {
            Part::Map => (&mut status.installed, &mut status.downloading, &mut status.error),
            Part::Extra(name) => {
                let e = status.extras.entry(name.to_owned()).or_default();
                (&mut e.installed, &mut e.downloading, &mut e.error)
            }
        }
    }
}

impl PackStatus {
    pub fn update_available(&self) -> bool {
        match (&self.installed, &self.available) {
            (Some(i), Some(a)) => i != "development" && *i != a.version,
            _ => false,
        }
    }
}

pub struct MapPacks {
    dir: PathBuf,
    client: reqwest::Client,
    statuses: Mutex<HashMap<String, PackStatus>>,
    last_refresh: Mutex<Option<Instant>>,
    /// Games the player needs the map of (see [`request`]): nothing else gets downloaded.
    requested: Mutex<HashSet<String>>,
    /// Games a [`request`] is working on right now.
    requesting: Mutex<HashSet<String>>,
    /// Failed downloads in a row by game, and when to try again.
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

/// Wait before trying a failed download again: 2 minutes, doubling up to 6 hours.
fn failure_delay(tries: u32) -> Duration {
    RETRY_INTERVAL.saturating_mul(1u32 << tries.min(10)).min(MAX_RETRY_INTERVAL)
}

/// Whether a background sync should download this pack: updates for maps the player
/// has, and maps they asked for. Other games' maps stay on the server.
fn needs_download(status: &PackStatus, requested: bool) -> bool {
    status.downloading.is_none() && status.available.is_some() && (status.update_available() || (status.installed.is_none() && requested))
}

/// Whether a background sync should update an optional part: only one the player has
/// (they ask for it first, see [`MapPacks::install_extra`]).
fn extra_needs_update(status: &PackStatus, name: &'static str) -> bool {
    let Some(extra) = status.extras.get(name) else { return false };
    let Some(remote) = Part::Extra(name).remote(status) else { return false };
    extra.downloading.is_none() && extra.installed.as_deref().is_some_and(|v| v != "development" && v != remote.version)
}

/// Removes what an interrupted download left behind.
fn remove_leftovers(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
        let removed = if name.ends_with(".staging") {
            std::fs::remove_dir_all(&path)
        } else if name.ends_with(".part") {
            std::fs::remove_file(&path)
        } else if let Some(pack) = name.strip_suffix(".old") {
            // A swap that was cut off: the old pack is the one to keep if the new one
            // didn't make it.
            let target = dir.join(pack);
            if target.exists() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::rename(&path, target)
            }
        } else {
            continue;
        };
        if let Err(e) = removed {
            log::warn!("could not remove {}: {e}", path.display());
        }
    }
}

impl MapPacks {
    /// `bundled`: the app's game folders; map data found there (development) counts as installed.
    pub fn new(dir: PathBuf, bundled: &Path, game_ids: &[String]) -> Self {
        remove_leftovers(&dir);
        for name in EXTRAS {
            remove_leftovers(&Part::Extra(name).dir(&dir));
        }
        let statuses = game_ids
            .iter()
            .map(|id| {
                let installed = read_installed(&dir, id)
                    .map(|p| p.version)
                    .or_else(|| bundled.join(id).join("tiles").is_dir().then(|| "development".to_owned()));
                let extras = EXTRAS
                    .iter()
                    .map(|name| {
                        let installed = read_installed(&Part::Extra(name).dir(&dir), id)
                            .map(|p| p.version)
                            .or_else(|| bundled.join(id).join(name).is_dir().then(|| "development".to_owned()));
                        (name.to_string(), ExtraStatus { installed, ..Default::default() })
                    })
                    .collect();
                (id.clone(), PackStatus { game_id: id.clone(), installed, available: None, downloading: None, error: None, extras })
            })
            .collect();
        let client = reqwest::Client::builder()
            .user_agent(concat!("GameMaps/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            // Between reads, not for the whole download: a stalled server mustn't hang it.
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("http client");
        Self {
            dir,
            client,
            statuses: Mutex::new(statuses),
            last_refresh: Mutex::new(None),
            requested: Mutex::new(HashSet::new()),
            requesting: Mutex::new(HashSet::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The game build the installed map of `id` was made from, if it says.
    pub fn installed_build(&self, id: &str) -> Option<String> {
        read_installed(&self.dir, id)?.game_build
    }

    pub fn statuses(&self) -> Vec<PackStatus> {
        let mut list: Vec<_> = self.statuses.lock().unwrap().values().cloned().collect();
        list.sort_by(|a, b| a.game_id.cmp(&b.game_id));
        list
    }

    fn edit(&self, app: &AppHandle, id: &str, f: impl FnOnce(&mut PackStatus)) {
        if let Some(s) = self.statuses.lock().unwrap().get_mut(id) {
            f(s);
        }
        let _ = app.emit(EVENT, self.statuses());
    }

    /// Fetches the pack list.
    pub async fn refresh(&self, app: &AppHandle) -> Result<(), String> {
        let manifest: Manifest = async {
            let res = self.client.get(manifest_url()).send().await?.error_for_status()?;
            res.json().await
        }
        .await
        .map_err(|e| format!("could not check for map updates: {e}"))?;
        {
            let mut statuses = self.statuses.lock().unwrap();
            for (id, status) in statuses.iter_mut() {
                status.available = manifest.packs.get(id).cloned();
            }
        }
        *self.last_refresh.lock().unwrap() = Some(Instant::now());
        let _ = app.emit(EVENT, self.statuses());
        Ok(())
    }

    /// Fetches the pack list (unless it was fetched within `max_age`), then updates the
    /// maps the player has and gets the ones they asked for.
    /// Returns whether everything worked.
    pub async fn sync(&self, app: &AppHandle, max_age: Duration) -> bool {
        let fresh = self.last_refresh.lock().unwrap().is_some_and(|t| t.elapsed() < max_age);
        if !fresh {
            if let Err(e) = self.refresh(app).await {
                log::info!("{e}");
                return false;
            }
        }
        let mut ok = true;
        let requested = self.requested.lock().unwrap().clone();
        let mut wanted: Vec<(String, Part)> = Vec::new();
        for s in self.statuses() {
            if needs_download(&s, requested.contains(&s.game_id)) {
                wanted.push((s.game_id.clone(), Part::Map));
            }
            for &name in EXTRAS {
                if extra_needs_update(&s, name) {
                    wanted.push((s.game_id.clone(), Part::Extra(name)));
                }
            }
        }
        for (id, part) in wanted {
            let key = part.key(&id);
            // A pack that keeps failing shouldn't be downloaded in full every time.
            if self.failures.lock().unwrap().get(&key).is_some_and(|(_, next)| Instant::now() < *next) {
                ok = false;
                continue;
            }
            match self.install_part(app, &id, part).await {
                Ok(()) => {
                    self.failures.lock().unwrap().remove(&key);
                }
                Err(e) => {
                    log::warn!("map pack {key}: {e}");
                    ok = false;
                    let mut failures = self.failures.lock().unwrap();
                    let tries = failures.get(&key).map_or(0, |(n, _)| n + 1);
                    failures.insert(key, (tries, Instant::now() + failure_delay(tries)));
                }
            }
        }
        ok
    }

    /// Downloads, verifies and installs the current pack for `id`.
    pub async fn install(&self, app: &AppHandle, id: &str) -> Result<(), String> {
        self.install_part(app, id, Part::Map).await
    }

    /// Downloads, verifies and installs an optional part of `id`'s map (see [`EXTRAS`]),
    /// which is then kept up to date with the rest.
    pub async fn install_extra(&self, app: &AppHandle, id: &str, name: &str) -> Result<(), String> {
        let part = Part::extra(name).ok_or_else(|| format!("unknown map part {name}"))?;
        self.install_part(app, id, part).await
    }

    async fn install_part(&self, app: &AppHandle, id: &str, part: Part) -> Result<(), String> {
        let remote = {
            let mut statuses = self.statuses.lock().unwrap();
            let status = statuses.get_mut(id).ok_or("unknown game")?;
            let remote = part.remote(status);
            let (_, downloading, error) = part.fields(status);
            if downloading.is_some() {
                return Err("already downloading".into());
            }
            let remote = remote.ok_or("nothing is published for this map yet")?;
            *downloading = Some(Progress { received: 0, total: remote.size });
            *error = None;
            remote
        };
        let _ = app.emit(EVENT, self.statuses());
        log::info!("installing map pack {} {}", part.key(id), remote.version);
        let result = self.download_and_extract(app, id, part, &remote).await;
        self.edit(app, id, |s| {
            let (installed, downloading, error) = part.fields(s);
            *downloading = None;
            match &result {
                Ok(()) => *installed = Some(remote.version.clone()),
                Err(e) => *error = Some(e.clone()),
            }
        });
        match (&result, part) {
            (Ok(()), Part::Map) => {
                let _ = app.emit("map-pack-installed", id);
            }
            (Ok(()), Part::Extra(name)) => {
                let _ = app.emit("map-extra-installed", ExtraInstalled { game_id: id.to_owned(), extra: name.to_owned() });
            }
            (Err(_), _) => analytics::problem(app, Problem::MapDownload),
        }
        result
    }

    async fn download_and_extract(&self, app: &AppHandle, id: &str, part: Part, remote: &RemotePack) -> Result<(), String> {
        let dir = part.dir(&self.dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let zip_path = dir.join(format!("{id}-{stamp}.zip.part"));
        let staging = dir.join(format!("{id}-{stamp}.staging"));
        let result = async {
            self.download(app, id, part, remote, &zip_path).await?;
            let (zip, out) = (zip_path.clone(), staging.clone());
            let (version, build) = (remote.version.clone(), remote.game_build.clone());
            tauri::async_runtime::spawn_blocking(move || extract(&zip, &out, &version, build.as_deref()))
                .await
                .map_err(|e| e.to_string())??;
            let (from, to) = (staging.clone(), dir.join(id));
            tauri::async_runtime::spawn_blocking(move || swap_into_place(&from, &to)).await.map_err(|e| e.to_string())?
        }
        .await;
        let _ = std::fs::remove_file(&zip_path);
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    async fn download(&self, app: &AppHandle, id: &str, part: Part, remote: &RemotePack, to: &Path) -> Result<(), String> {
        let err = |e: reqwest::Error| format!("download failed: {e}");
        let mut res = self.client.get(&remote.url).send().await.map_err(err)?.error_for_status().map_err(err)?;
        let total = res.content_length().unwrap_or(remote.size);
        let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let (mut received, mut last_emit) = (0u64, 0u64);
        while let Some(chunk) = res.chunk().await.map_err(err)? {
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            hasher.update(&chunk);
            received += chunk.len() as u64;
            // About one progress event per percent.
            if received - last_emit >= total / 100 || received == total {
                last_emit = received;
                self.edit(app, id, |s| *part.fields(s).1 = Some(Progress { received, total }));
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !digest.eq_ignore_ascii_case(&remote.sha256) {
            return Err("the downloaded map was corrupted; please try again".into());
        }
        Ok(())
    }
}

/// Sent when an optional part of a map has been installed or updated.
#[derive(Debug, Clone, Serialize)]
struct ExtraInstalled {
    game_id: String,
    extra: String,
}

fn read_installed(dir: &Path, id: &str) -> Option<InstalledPack> {
    let text = std::fs::read_to_string(dir.join(id).join(PACK_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Extracts a pack into `out` (zip entries are confined to it) and marks it complete.
fn extract(zip: &Path, out: &Path, version: &str, game_build: Option<&str>) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("invalid map pack: {e}"))?;
    archive.extract(out).map_err(|e| format!("could not unpack the map: {e}"))?;
    let marker = serde_json::to_vec(&InstalledPack { version: version.to_owned(), game_build: game_build.map(str::to_owned) }).map_err(|e| e.to_string())?;
    std::fs::write(out.join(PACK_FILE), marker).map_err(|e| e.to_string())
}

/// Replaces `target` with `staging`. The old folder is renamed aside first, so a
/// failure leaves the previous pack in place.
fn swap_into_place(staging: &Path, target: &Path) -> Result<(), String> {
    let old = target.with_extension("old");
    let _ = std::fs::remove_dir_all(&old);
    if target.exists() {
        retry(|| std::fs::rename(target, &old)).map_err(|e| format!("could not replace the old map: {e}"))?;
    }
    if let Err(e) = retry(|| std::fs::rename(staging, target)) {
        let _ = std::fs::rename(&old, target);
        return Err(format!("could not install the map: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Folder renames fail while another process briefly holds a file open (antivirus
/// scans, the map reading a tile), so try a few times.
fn retry(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match op() {
            Ok(()) => return Ok(()),
            Err(e) if attempt >= 10 => return Err(e),
            Err(_) => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// Checks for pack updates at startup and every few hours, installing updates to
/// packs the user already has.
pub fn start_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let ok = app.state::<MapPacks>().sync(&app, Duration::ZERO).await;
            tokio::time::sleep(if ok { CHECK_INTERVAL } else { RETRY_INTERVAL }).await;
        }
    });
}

/// Checks right away when a game starts, so a map published minutes ago is there.
pub fn sync_soon(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { app.state::<MapPacks>().sync(&app, GAME_START_MAX_AGE).await });
}

/// The player needs `id`'s map: gets it unless it's already here, retrying through a
/// dropped connection.
pub fn request(app: &AppHandle, id: &str) {
    let packs = app.state::<MapPacks>();
    if packs.statuses().iter().any(|s| s.game_id == id && s.installed.is_some()) {
        return;
    }
    if !packs.requesting.lock().unwrap().insert(id.to_owned()) {
        return;
    }
    log::info!("map pack {id}: needed, getting it");
    packs.requested.lock().unwrap().insert(id.to_owned());
    let (app, id) = (app.clone(), id.to_owned());
    tauri::async_runtime::spawn(async move {
        let packs = app.state::<MapPacks>();
        for _ in 0..REQUEST_TRIES {
            if packs.sync(&app, GAME_START_MAX_AGE).await {
                break;
            }
            tokio::time::sleep(RETRY_INTERVAL).await;
        }
        packs.requesting.lock().unwrap().remove(&id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(installed: Option<&str>, available: Option<&str>) -> PackStatus {
        let remote = |v: &str| RemotePack { version: v.into(), url: String::new(), sha256: String::new(), size: 1, game_build: None, extras: HashMap::new() };
        PackStatus { game_id: "game".into(), installed: installed.map(Into::into), available: available.map(remote), downloading: None, error: None, extras: HashMap::new() }
    }

    /// A map with a published 3D part at `published`, and the player's at `installed`.
    fn with_3d(installed: Option<&str>, published: &str) -> PackStatus {
        let mut status = pack(Some("1"), Some("1"));
        let extra = RemotePack { version: published.into(), url: String::new(), sha256: String::new(), size: 1, game_build: None, extras: HashMap::new() };
        status.available.as_mut().unwrap().extras.insert("3d".into(), extra);
        status.extras.insert("3d".into(), ExtraStatus { installed: installed.map(Into::into), ..Default::default() });
        status
    }

    #[test]
    fn optional_parts_wait_for_the_player() {
        assert!(!extra_needs_update(&with_3d(None, "1"), "3d"));
    }

    #[test]
    fn optional_parts_the_player_has_stay_up_to_date() {
        assert!(extra_needs_update(&with_3d(Some("1"), "2"), "3d"));
        assert!(!extra_needs_update(&with_3d(Some("2"), "2"), "3d"));
        assert!(!extra_needs_update(&with_3d(Some("development"), "2"), "3d"));
    }

    #[test]
    fn manifests_without_optional_parts_still_load() {
        let m: Manifest = serde_json::from_str(r#"{"packs": {"g": {"version": "1", "url": "u", "sha256": "s", "size": 3}}}"#).unwrap();
        assert!(m.packs["g"].extras.is_empty());
        let m: Manifest =
            serde_json::from_str(r#"{"packs": {"g": {"version": "1", "url": "u", "sha256": "s", "size": 3, "extras": {"3d": {"version": "2", "url": "v", "sha256": "t", "size": 9}}}}}"#).unwrap();
        assert_eq!(m.packs["g"].extras["3d"].size, 9);
    }

    #[test]
    fn optional_parts_install_beside_the_maps() {
        assert_eq!(Part::Extra("3d").dir(Path::new("m")), Path::new("m").join("extras").join("3d"));
        assert_eq!(Part::Map.dir(Path::new("m")), Path::new("m"));
        assert_eq!(Part::extra("3d"), Some(Part::Extra("3d")));
        assert_eq!(Part::extra("nope"), None);
    }

    #[test]
    fn maps_nobody_asked_for_stay_on_the_server() {
        assert!(!needs_download(&pack(None, Some("1")), false));
        assert!(needs_download(&pack(None, Some("1")), true));
    }

    #[test]
    fn maps_the_player_has_stay_up_to_date() {
        assert!(needs_download(&pack(Some("1"), Some("2")), false));
        assert!(!needs_download(&pack(Some("2"), Some("2")), false));
        assert!(!needs_download(&pack(Some("development"), Some("2")), false));
    }

    #[test]
    fn nothing_to_download_without_a_published_map() {
        assert!(!needs_download(&pack(None, None), true));
    }

    #[test]
    fn leftovers_of_an_interrupted_download_are_removed() {
        let dir = temp_dir("leftovers");
        std::fs::create_dir_all(dir.join("game")).unwrap();
        std::fs::create_dir_all(dir.join("game-1.staging")).unwrap();
        std::fs::write(dir.join("game-1.zip.part"), b"x").unwrap();
        std::fs::create_dir_all(dir.join("game.old")).unwrap();
        remove_leftovers(&dir);
        assert!(dir.join("game").exists());
        assert!(!dir.join("game-1.staging").exists());
        assert!(!dir.join("game-1.zip.part").exists());
        assert!(!dir.join("game.old").exists());
    }

    #[test]
    fn an_interrupted_swap_keeps_the_old_pack() {
        let dir = temp_dir("interrupted-swap");
        std::fs::create_dir_all(dir.join("game.old")).unwrap();
        remove_leftovers(&dir);
        assert!(dir.join("game").exists());
        assert!(!dir.join("game.old").exists());
    }

    #[test]
    fn failed_downloads_wait_longer_each_time() {
        assert_eq!(failure_delay(0), Duration::from_secs(120));
        assert_eq!(failure_delay(1), Duration::from_secs(240));
        assert_eq!(failure_delay(30), Duration::from_secs(6 * 60 * 60));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gm-packs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extracts_and_replaces_pack() {
        let dir = temp_dir("swap");
        let zip = dir.join("p.zip");
        write_zip(&zip, &[("pins.json", b"{}"), ("tiles/3/3/3.webp", b"x")]);
        let target = dir.join("game");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("stale.txt"), b"old").unwrap();

        let staging = dir.join("staging");
        extract(&zip, &staging, "2", None).unwrap();
        swap_into_place(&staging, &target).unwrap();

        assert!(target.join("tiles/3/3/3.webp").exists());
        assert!(!target.join("stale.txt").exists(), "old pack is replaced, not merged");
        assert_eq!(read_installed(&dir, "game").unwrap().version, "2");
        assert!(!dir.join("game.old").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_entries_outside_the_pack() {
        let dir = temp_dir("slip");
        let zip = dir.join("evil.zip");
        write_zip(&zip, &[("../escape.txt", b"x")]);
        let _ = extract(&zip, &dir.join("out"), "1", None);
        assert!(!dir.join("escape.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn update_available_only_for_installed_packs() {
        let remote = RemotePack { version: "2".into(), url: String::new(), sha256: String::new(), size: 0, game_build: None, extras: HashMap::new() };
        let status = |installed: Option<&str>| PackStatus {
            game_id: "g".into(),
            installed: installed.map(Into::into),
            available: Some(remote.clone()),
            downloading: None,
            error: None,
            extras: HashMap::new(),
        };
        assert!(status(Some("1")).update_available());
        assert!(!status(Some("2")).update_available());
        assert!(!status(None).update_available());
        assert!(!status(Some("development")).update_available());
    }
}
