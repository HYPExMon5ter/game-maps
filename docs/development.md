# Development

## Requirements

- Windows 10/11
- [Rust](https://rustup.rs) (stable, MSVC toolchain)
- [Bun](https://bun.sh)
- Python 3.12+ (map and pin generation only)
- [GitHub CLI](https://cli.github.com), logged in (publishing only)

## Run the app

```sh
cd apps/overlay
bun install
bun run app:dev      # builds the agent DLL, then starts the app with hot reload
```

In development builds, map data is read from `games/<id>/` in the repository instead
of downloaded map packs, and the app never updates itself.

## Checks

These are the same checks [CI](../.github/workflows/ci.yml) runs on every pull request:

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python -I -m unittest discover -s tools/pins
python -I -m unittest discover -s tools/mappack
cd apps/overlay && bun run lint && bun run build && bun run test
```

## Repository layout

| Path | What |
|---|---|
| `apps/overlay/src-tauri` | Tauri backend: game detection, injector, agent link, overlay window, map packs, updater, settings |
| `apps/overlay/src` | React UI: the map overlay (`App.tsx`, `map/`, `ui/`) and the launcher window (`launcher/`) |
| `crates/agent` | The agent loaded into the game: frame hook, pipe server, per-game providers |
| `crates/il2cpp` | Name-based access to a Unity IL2CPP runtime from inside the process |
| `crates/protocol` | Messages between the app and the agent (versioned) |
| `games/<id>` | Per-game definition (`game.json`), pins, and generated tiles and icons |
| `tools/mapgen` | Builds map tiles and icons from game files |
| `tools/pins` | Builds `pins.json` from in-game world exports |
| `tools/mappack` | Publishes a game's map data |
| `tools/release.py`, `tools/version.py` | App releases (see [Releasing](releasing.md)) |

The [architecture notes](architecture.md) explain the main design decisions, and
`docs/games/<id>.md` documents each game's internals.

## Map data

Tiles, icons and pins are generated from your own game install. They're excluded
from git and published as map packs (see [Releasing](releasing.md)).

```sh
python -m venv .venv
.venv/Scripts/pip install -r tools/mapgen/requirements.txt

# Tiles and icons from the game files
.venv/Scripts/python -I tools/mapgen/sotf.py --game "D:/SteamLibrary/steamapps/common/Sons Of The Forest" --out games/sons-of-the-forest
```

Pins and cave layouts come from an in-game export. Start a **new** game (so nothing
has been picked up yet), open the map's **Settings → Diagnostics → Export world
data** (about 30 seconds; it scans every cave), then:

```sh
python -I tools/pins/sotf.py --out games/sons-of-the-forest "%LOCALAPPDATA%/com.gamemaps.overlay/exports/<export>.json"
.venv/Scripts/python -I tools/mapgen/sotf_caves.py --out games/sons-of-the-forest "%LOCALAPPDATA%/com.gamemaps.overlay/exports/<export>.json"
```

Re-run these after a game update that moves things, then publish the map.

## Adding a game

1. Create `games/<id>/game.json`. Copy the Sons of the Forest one and adjust:
   `process` (the executable name), `steamAppId` (for the Play button),
   `defaultHotkeys`, map projection and categories.
2. Generate tiles, icons and `pins.json` into `games/<id>/`.
3. Add the `game.json` to `bundle.resources` in `apps/overlay/src-tauri/tauri.conf.json`.
4. For live location and teleport, add a provider in `crates/agent/src/provider/`.
   Without one, the game works as a static map.
5. Publish its map pack and release the app. The launcher lists the game automatically.
