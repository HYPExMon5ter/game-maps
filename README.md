<img src="assets/icon.svg" width="64" height="64" alt="">

# Game Maps

An in-game map overlay for PC games: an interactive map with searchable locations,
custom pins, your live position, other players in multiplayer, teleport, and a
tracking compass. Sons of the Forest is the first supported game.

- **No mod loader.** The app loads a small agent into the running game. Nothing in
  the game folder is changed.
- **Works as host or client.** It only reads and moves your own player.
- **Built from game files.** Map imagery, icons and coordinates come from your own
  game install, so they line up exactly with live positions.

## Using it

1. Download `Game Maps_x.y.z_x64-setup.exe` from the
   [latest release](https://github.com/HYPExMon5ter/game-maps/releases/latest) and run it.
   No admin rights needed. Game Maps updates itself after that.
2. Set the game's display mode to **Fullscreen Window** (borderless). The overlay is
   a separate window on top of the game and can't cover exclusive fullscreen. The
   launcher warns you if it detects exclusive fullscreen.
3. Click **Play** in the Game Maps window (or start the game as usual). Game Maps
   connects on its own and downloads the game's map the first time.
4. Press **M** in-game to open or close the map. The key still reaches the game, so
   your GPS opens as usual. You can rebind it in Settings.

The launcher lets you pick, per game, whether Game Maps connects **automatically** or
only when you click **Connect**, and you can **Disconnect** at any time. Turn on
**Start with Windows** to keep it ready in the tray. Closing the window keeps it running
in the tray; quit from the tray icon.

In the map:

- **Right-click** the map to add a pin, teleport there, or copy coordinates.
- **Click a pin** to track it, teleport to it, mark it found, or edit it (custom pins).
  Teleports into caves and bunkers work: the game is switched into that cave first.
- **Surface / Underground** (top bar) switches the view; it follows you into caves.
  Underground shows the cave and bunker layouts over a dimmed island. Pins on the
  other layer stay visible, faded.
- **Players** (top bar) lists other players and Kelvin. You can track them or teleport
  to them.
- Everything the in-game GPS shows is on the map live too (Kelvin, locators you place,
  vehicles that were moved).
- **Picture-in-picture** (top bar) keeps a click-through mini-map on screen while
  you play. A tracked target also shows a compass there.

## Development

Requirements: Rust (stable, MSVC), Bun, and Python 3.12+ for map generation.

```sh
# One-time: generate map tiles and icons from your game install
python -m venv .venv && .venv/Scripts/pip install -r tools/mapgen/requirements.txt
.venv/Scripts/python -I tools/mapgen/sotf.py --game "D:/SteamLibrary/steamapps/common/Sons Of The Forest" --out games/sons-of-the-forest

# Pins and cave layouts: in-game, after starting a NEW game (nothing picked up yet),
# open Settings -> Diagnostics -> Export world data (about 30 s; it scans every cave),
# then:
python -I tools/pins/sotf.py --out games/sons-of-the-forest "%LOCALAPPDATA%/com.gamemaps.overlay/exports/<export>.json"
.venv/Scripts/python -I tools/mapgen/sotf_caves.py --out games/sons-of-the-forest "%LOCALAPPDATA%/com.gamemaps.overlay/exports/<export>.json"

# Run the app (builds the agent first)
cd apps/overlay
bun install
bun run app:dev

# Tests
cargo test --workspace
python -I -m unittest discover -s tools/pins
python -I -m unittest discover -s tools/mappack
cd apps/overlay && bun run test
```

### Publishing

Map data and the app are published separately, both on GitHub Releases. The repository
(or at least its releases) must be public so installed apps can download them.

- **Map data** (tiles, icons, pins): after regenerating a game's data, run
  `python -I tools/mappack/publish.py --game <id>`. It zips the pack, uploads it to the
  `maps` release and updates `maps.json`. Installed apps pick it up within a few hours
  or at their next start. No app release needed.
- **App**: `python -I tools/release.py 0.2.0`, then `git push --follow-tags`. The
  `Release` workflow builds the signed installer and publishes the release, and installed
  apps update themselves when no game is running. It needs the repository secrets
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the updater key
  whose public half is in `tauri.conf.json`). Losing the private key means existing
  installs can no longer update, so back it up.

To build an installer locally: set the two variables above and run `bun run app:build`
in `apps/overlay` (output in `target/release/bundle/nsis/`).

Layout:

| Path | What |
|---|---|
| `crates/protocol` | Messages between the app and the agent |
| `crates/il2cpp` | Name-based access to a Unity IL2CPP runtime from inside the process |
| `crates/agent` | The injected agent: frame hook, pipe server, per-game providers |
| `apps/overlay` | Tauri app: launcher, injector, agent link, overlay window, map packs, updater, React + MapLibre UI |
| `games/<id>` | Per-game definition (`game.json`), pins, and generated tiles/icons |
| `tools/mapgen` | Generators that build map tiles/icons from game files |
| `tools/pins` | Builds `pins.json` from in-game world exports |
| `tools/mappack` | Publishes a game's map data as a downloadable map pack |
| `tools/release.py` | Bumps the version and tags an app release |
| `docs/` | Plan, research notes, per-game notes |

Adding a game: create `games/<id>/game.json` (with `steamAppId` for the Play button)
plus tiles and pins, add the `game.json` to `bundle.resources` in
`apps/overlay/src-tauri/tauri.conf.json`, publish its map pack, and release the app.
The launcher lists it automatically. For live features, add a provider in
`crates/agent/src/provider/`.
