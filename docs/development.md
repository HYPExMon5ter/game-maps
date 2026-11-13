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
of downloaded map packs, the app never updates itself, and no usage counts are sent.

To try playing together without a multiplayer game, set a room in the overlay's dev
tools console (focus the open map, **Ctrl+Shift+I**):
`localStorage.setItem('game-maps:sync-room', 'any-name')`, then reload. Another copy
of the app (or a page using Trystero with app id `game-maps-overlay`, password and
room `<game id>/any-name`) in the same room connects to it. Remove the item when
done.

## Checks

These are the same checks [CI](../.github/workflows/ci.yml) runs once on every pull request (and again before every release):

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python -I -m unittest discover -s tools/pins
python -I -m unittest discover -s tools/mappack
.venv/Scripts/python -I -m unittest discover -s tools/mapgen   # see "Map data" for the venv
cd apps/overlay && bun run lint && bun run build && bun run test
```

## Repository layout

| Path | What |
|---|---|
| `apps/overlay/src-tauri` | Tauri backend: game detection, injector, agent link, overlay window, map packs, updater, settings |
| `apps/overlay/src` | React UI: the map overlay (`App.tsx`, `map/`, `ui/`) and the launcher window (`launcher/`) |
| `crates/agent` | The agent loaded into the game: frame hook, pipe server, per-game providers |
| `crates/unity-runtime` | Name-based access to a Unity scripting runtime (IL2CPP or Mono) from inside the process |
| `crates/protocol` | Messages between the app and the agent (versioned) |
| `games/<id>` | Per-game definition (`game.json`), pins, and generated tiles and icons |
| `tools/mapgen` | Builds map tiles and icons from game files |
| `tools/pins` | Builds `pins.json` from in-game world exports |
| `tools/mappack` | Publishes a game's map data, and lists the maps a game update left behind |
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
# The caves' collision, water, ropes and ziplines (build/, about 2 minutes)
.venv/Scripts/python -I tools/mapgen/sotf_3d.py --game "D:/SteamLibrary/steamapps/common/Sons Of The Forest" --out games/sons-of-the-forest
```

Pins and cave layouts come from an in-game export. Start a **new** game (so nothing
has been picked up yet), open **Settings → Advanced → Export world data** (about 30 seconds; it scans every cave), then:

```sh
python -I tools/pins/sotf.py --out games/sons-of-the-forest "%LOCALAPPDATA%/com.gamemaps.overlay/exports/<export>.json"
# After sotf_3d.py: the cave grids and the cave layer (about 2 minutes)
.venv/Scripts/python -I tools/mapgen/sotf_caves.py --out games/sons-of-the-forest "%LOCALAPPDATA%/com.gamemaps.overlay/exports/<export>.json"
```

Re-run these after a game update that moves things, then publish the map.

### Sunkenland

Everything comes from the game files; no in-game step. The map is drawn on the GPU
(moderngl, any OpenGL 4.3 GPU) and takes a few minutes.

```sh
.venv/Scripts/python -I tools/mapgen/sunkenland.py --game "D:/SteamLibrary/steamapps/common/Sunkenland" --out games/sunkenland
.venv/Scripts/python -I tools/mapgen/sunkenland.py --game "D:/SteamLibrary/steamapps/common/Sunkenland" --export-world world.json
python -I tools/pins/sunkenland.py --out games/sunkenland world.json
```

Add `--preview-only` for a quick 2048 px `preview.png`.

### Valheim

Nothing to generate or publish: worlds come from a seed, so each map and its pins are read
from the running game. See [Valheim](games/valheim.md#world).

### Stranded Deep

Nothing to generate or publish either: each world's islands are placed from its seed, and
the map and pins come from the running game. See [Stranded Deep](games/stranded-deep.md#world).

## Adding a game

1. Create `games/<id>/game.json`. Copy the Sons of the Forest one and adjust:
   `process` (the executable name), `steamAppId` (for the Play button),
   `defaultHotkeys`, map projection, categories and `groups`. Every category goes in a
   group; set `split` on one to list it by pin name. Set `liveFeatures` to `false` until the
   game has a provider (step 4).
2. Generate tiles, icons and `pins.json` into `games/<id>/`. For a game with generated
   worlds, set `worldMaps` in `game.json` and implement `Provider::read_world_map` instead.
3. Add the `game.json` to `bundle.resources` in `apps/overlay/src-tauri/tauri.conf.json`.
4. For live location and teleport, add a provider in `crates/agent/src/provider/` and
   register it in `provider::detect`. IL2CPP and Mono games use the same runtime API.
   Without one, the game works as a static map.
5. Publish its map pack and release the app. The launcher lists the game automatically.
6. Add its page to `docs/supported-games/`, with screenshots in `docs/images/<id>/`, and
   add it to [Supported games](../README.md#supported-games) and the GitHub topics. Don't
   put the page under `games/<id>/`: map packs include what's in there.
