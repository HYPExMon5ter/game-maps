# Game Maps: architecture & plan

An in-game map overlay for several games: an interactive map with pins and search,
plus live player location and teleport where a game provider supports them.
Sons of the Forest (SotF) is the first game.

Background research: [research/wand-maps.md](research/wand-maps.md).

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Overlay | Tauri v2 transparent, always-on-top window over the game. The game runs in borderless / "Fullscreen Window" mode. | Simplest way to get a full web UI (MapLibre). It costs the game almost nothing to render, unlike hooking the game's swap chain. |
| Game integration | Native agent DLL (Rust), injected by the app at runtime. It talks to Unity's IL2CPP runtime API by **name**. | No mod loader, and the game folder is never modified. Works as a non-host client. Name-based lookups survive most game updates. The same agent core works for any Unity IL2CPP game. |
| Main-thread work | Detours on a few per-frame methods (engine render loop, uGUI, plus game-specific ones). Work runs once per frame and does almost nothing. | Unity APIs must be called on the main thread. One cheap call per frame keeps lag minimal. |
| IPC | Named pipe `\\.\pipe\game-maps-agent-<pid>`, newline-delimited JSON, versioned protocol. Position is pushed only when it changes, capped at 30 Hz. | Local only, no firewall prompt, trivial cost at this message size. |
| Map renderer | MapLibre GL JS. Raster tile pyramid. Game ↔ map conversion via a fitted affine transform in Web-Mercator space (same model Wand uses). | Proven, GPU-accelerated, handles 10k+ pins with clustering. |
| Map data | Pulled from the game by agent dev tools: a top-down orthographic capture builds the tiles, `VirtualPickupManager` and world scans give pickups, `GPSLocator` gives POIs. User pins are stored locally. | "Use game assets when possible." Live positions and pins share one coordinate system by construction. |
| Hotkey | The agent watches for the configured key (default **M**) only while the game is focused, and doesn't consume it, so the in-game GPS still opens. | No global hook, no conflict when typing outside the game. |

## Repository layout

```
crates/
  protocol/        shared message types (app <-> agent)
  il2cpp/          IL2CPP runtime bindings: resolve exports, find class/method/field, invoke
  agent/           injected cdylib: pipe server, frame hook, game providers (sotf, …)
apps/overlay/      Tauri v2 app (Rust backend + React/TS/MapLibre frontend)
  src-tauri/       injector, agent connection, window control, game data loading
games/
  sons-of-the-forest/
    game.json      process name, capabilities, coordinate transform, layers, categories
    pins.json      generated pins (from game export)
    tiles/         generated raster tiles
docs/
```

## Game plugin model

`games/<id>/game.json` declares what a game supports:

- `process`: executable name used for detection.
- `capabilities`: `liveLocation`, `teleport`, `hotkey` (only available when a
  provider for that game is compiled into the agent).
- `maps[]`: each map (surface, caves, …) has a tile source, zoom range, bounds, and
  `transform` (reference points plus fitted `scale`/`offset`/`axes`).
- `categories[]`: id, name, icon, color, default visibility.

A game with only `game.json` + tiles + pins works as a static map. Adding a provider
to the agent enables live location and teleport.

## Phases

1. **Foundation**: workspace, protocol, IL2CPP bindings, agent skeleton (inject,
   frame hook, pipe), overlay shell (window, hotkey toggle, MapLibre with a test map).
2. **SotF live**: position, heading, and cave state streaming; teleport via
   `LocalPlayer.TeleportTo`; M hotkey; compass/track widget. Other players in
   multiplayer (`PlayerTracker.AllPlayers` + `CoopPlayerRemoteSetup` name/color)
   shown on the map, with "teleport to player" and "track player".
3. **SotF data**: dev export of pickups and POIs, top-down capture to build the tile
   pyramid, transform fitting.
4. **UX**: search, filters, found tracking, custom pins with custom icons, opacity
   and size controls, pinned mini-map mode, settings.
5. **Hardening**: game-update resilience checks, reconnect and re-inject, packaging.

## Status (2026-11-02)

- Phases 1–4: done and verified in-game (single player), except remote players
  (needs multiplayer).
- Map: generated from the game's terrain, with the game's own GPS and item icons.
- Pins: 1,813 from an in-game world export (`tools/pins`), surface and underground,
  with cave/bunker names and contents. Live GPS markers (Kelvin, placed locators,
  moved vehicles) stream from the game.
- Teleport works into and out of caves and bunkers. The Underground view draws the
  cave and bunker layouts, scanned in-game.
- Alt-tab, main-menu exits and game shutdown are handled; no freezes in testing.
- Overlay covers the game's monitor exactly and adapts to any size, 720p to 32:9.
- Next: multiplayer test, packaging (NSIS build), lakes/rivers on the map.

## Verification

- Unit tests: protocol round-trips, transform fitting and conversion, search.
- Agent smoke test: a small CLI injects into a running process and checks that the
  handshake, class resolution, and frame ticks work (main menu is enough).
- In-game checklist for the user: position accuracy, teleport (host and client),
  hotkey, caves.
