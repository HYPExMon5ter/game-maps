# Architecture

Game Maps is an in-game map overlay for several games: an interactive map with pins
and search, plus live player location and teleport where a game provider supports
them. This page records the main design decisions and how the pieces fit together.

## Components

```
 Game process                          Game Maps app (Tauri)
 ┌──────────────────────────┐          ┌──────────────────────────────────────┐
 │ agent DLL (crates/agent) │  named   │ Rust backend (apps/overlay/src-tauri)│
 │  frame hook → provider   │◄────────►│  detect game, inject, agent link     │
 │  (il2cpp by name)        │  pipe    │  map packs, updater, settings        │
 └──────────────────────────┘          │ Web UI (apps/overlay/src)            │
                                       │  overlay window: MapLibre map, HUD   │
                                       │  launcher window                     │
                                       └──────────────────────────────────────┘
                     GitHub Releases: app installers + latest.json, `maps` (map packs)
```

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Overlay | Tauri v2 transparent, always-on-top window over the game. The game runs in borderless / "Fullscreen Window" mode. | Simplest way to get a full web UI (MapLibre). It costs the game almost nothing to render, unlike hooking the game's swap chain. |
| Game integration | Native agent DLL (Rust), injected by the app at runtime. It talks to Unity's IL2CPP runtime API by **name**. | No mod loader, and the game folder is never modified. Works as a non-host client. Name-based lookups survive most game updates. The same agent core works for any Unity IL2CPP game. |
| Main-thread work | Detours on a few per-frame methods (engine render loop, uGUI, plus game-specific ones). Work runs once per frame and does almost nothing. | Unity APIs must be called on the main thread. One cheap call per frame keeps lag minimal. |
| IPC | Named pipe `\\.\pipe\game-maps-agent-<pid>`, newline-delimited JSON, versioned protocol. Position is pushed only when it changes, capped at 30 Hz. | Local only, no firewall prompt, trivial cost at this message size. |
| Map renderer | MapLibre GL JS. Raster tile pyramid. Game ↔ map conversion via a fitted affine transform in Web-Mercator space. | Proven, GPU-accelerated, handles 10k+ pins with clustering. |
| Map data | Generated from the game's own files: tiles are rendered from the terrain data (`tools/mapgen`), pins come from an in-game world export (`tools/pins`). User pins are stored locally. | Uses the game's own data, so live positions and pins share one coordinate system by construction. |
| Distribution | Map data ships as per-game map packs on a `maps` GitHub release, separate from the app. The app ships only `game.json`. | Map fixes reach users without an app release, and the installer stays small. |
| Updates | Tauri updater against signed GitHub releases. Installs only while no game is running. | Never interrupts a session; signed updates can't be tampered with in transit. |
| Hotkeys | The agent watches for the configured keys (map: default **M**; zoom: **Num +/-** and **=/-**) only while the game is focused, and doesn't consume them, so the in-game GPS still opens. While the full map is open it has focus and handles the same keys itself. | No global hook, no conflict when typing outside the game. |

## Game model

Each game is a folder `games/<id>/`. `game.json` ships with the app; everything else
in the folder is generated and published as a map pack.

`game.json` declares:

- `id`, `name`, `process` (executable name used for detection), `steamAppId`
  (optional, for the launcher's Play button).
- `defaultHotkeys`: the key that opens the map, until the user picks another.
- `liveFeatures`: whether the game has a provider (live location, teleport). The
  launcher's **Live** / **Map only** filter uses it before the game is running.
- `maps[]`: each map has its `tiles` (and optional per-layer `layerTiles`, e.g. caves)
  and a `projection` that maps world coordinates onto the tile grid.
- `categories[]`: id, name, icon, color, default visibility.

A game with only `game.json`, tiles and pins works as a static map. A provider in
`crates/agent/src/provider/` adds live location, teleport, other players and the
in-game hotkey. The agent reports which of these it supports when it connects.
