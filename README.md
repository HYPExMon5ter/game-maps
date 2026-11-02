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

Map data and the app are published separately, both on GitHub Releases, and installed
apps pick up both on their own. The repository must be public so they can download them.

**Map update** (no app release needed). After regenerating a game's tiles, icons or pins
(see above):

```sh
python -I tools/mappack/publish.py sons-of-the-forest   # or --all
```

It uploads the pack to the `maps` release and points `maps.json` at it. Every installed
app downloads it within 30 minutes, or right away when the game starts. A map that is
open on screen switches over when it's closed. Unchanged maps are skipped, so you can
run `--all` at any time.

**App update** (code changes, including live features like positioning). Merge your
changes into `master`, then:

```sh
python -I tools/release.py                      # patch: 0.1.0 -> 0.1.1
python -I tools/release.py minor "What changed"  # notes appear in the update prompt
```

Or open the **Actions** tab on GitHub, pick **Release** and press **Run workflow**. The
workflow tests, bumps the version, builds and signs the installer, and publishes it as
the latest release (about 15 minutes). Installed apps find it within 30 minutes. With
automatic updates on, they install it as soon as no game is running. Otherwise they ask
first, and the launcher offers it too.

Friends always install from
[the latest release](https://github.com/HYPExMon5ter/game-maps/releases/latest).

**One-time setup** for app releases: the workflow signs updates with the key created for
this project, which lives in `~/.tauri/` on the machine that set it up. Add it as two
repository secrets:

```powershell
gh secret set TAURI_SIGNING_PRIVATE_KEY --body (Get-Content ~/.tauri/game-maps.key -Raw).Trim()
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body (Get-Content ~/.tauri/game-maps.key.password -Raw).Trim()
```

Back up both files somewhere safe. Installed apps only accept updates signed with this
key (its public half is in `tauri.conf.json`), so losing it means nobody can update
without reinstalling.

CI (`.github/workflows/ci.yml`) runs the tests, lints and clippy on every pull request
and push to `master`.

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
| `tools/release.py` | Publishes a new app version (starts the Release workflow) |
| `tools/version.py` | Sets the app version in every file that records it |
| `docs/` | Plan, research notes, per-game notes |

Adding a game: create `games/<id>/game.json` (with `steamAppId` for the Play button)
plus tiles and pins, add the `game.json` to `bundle.resources` in
`apps/overlay/src-tauri/tauri.conf.json`, publish its map pack, and release the app.
The launcher lists it automatically. For live features, add a provider in
`crates/agent/src/provider/`.
