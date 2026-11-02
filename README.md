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

1. Set the game's display mode to **Fullscreen Window** (borderless). The overlay is
   a separate window on top of the game and can't cover exclusive fullscreen.
2. Start Game Maps (it lives in the system tray), then start the game.
3. Press **M** in-game to open or close the map. The key still reaches the game, so
   your GPS opens as usual. You can rebind it in Settings.

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
cd apps/overlay && bun run test
```

Layout:

| Path | What |
|---|---|
| `crates/protocol` | Messages between the app and the agent |
| `crates/il2cpp` | Name-based access to a Unity IL2CPP runtime from inside the process |
| `crates/agent` | The injected agent: frame hook, pipe server, per-game providers |
| `apps/overlay` | Tauri app: injector, agent link, overlay window, React + MapLibre UI |
| `games/<id>` | Per-game definition (`game.json`), pins, and generated tiles/icons |
| `tools/mapgen` | Generators that build map tiles/icons from game files |
| `tools/pins` | Builds `pins.json` from in-game world exports |
| `docs/` | Plan, research notes, per-game notes |

Adding a game: create `games/<id>/game.json` plus tiles and pins, and the static
map works. For live features, add a provider in `crates/agent/src/provider/`.
