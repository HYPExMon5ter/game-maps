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
 │  (IL2CPP/Mono by name)   │  pipe    │  map packs, updater, settings        │
 └──────────────────────────┘          │ Web UI (apps/overlay/src)            │
                                       │  overlay window: MapLibre map, HUD   │
                                       │  launcher window                     │
                                       └──────────────────────────────────────┘
                     GitHub Releases: app installers + latest.json, `maps` (map packs)

 Other players' apps ◄── WebRTC data channels (direct) ──► this app
                     (found through public Nostr relays; see "Playing together")
```

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Overlay | Tauri v2 transparent, always-on-top window over the game. The game runs in borderless / "Fullscreen Window" mode. | Simplest way to get a full web UI (MapLibre). It costs the game almost nothing to render, unlike hooking the game's swap chain. |
| Game integration | Native agent DLL (Rust), injected by the app at runtime. It talks to Unity's scripting runtime by **name**, through IL2CPP's or Mono's exported API (`crates/unity-runtime`). | No mod loader, and the game folder is never modified. Works as a non-host client. Name-based lookups survive most game updates. The same agent core and provider style work for any Unity game, IL2CPP or Mono. |
| Main-thread work | Detours on a few per-frame methods (engine render loop, uGUI, plus game-specific ones). Work runs once per frame and does almost nothing. | Unity APIs must be called on the main thread. One cheap call per frame keeps lag minimal. |
| IPC | Named pipe `\\.\pipe\game-maps-agent-<pid>`, newline-delimited JSON, versioned protocol. Position is pushed only when it changes, capped at 30 Hz, or 2 Hz with nothing on screen. A whole `WorldSnapshot` goes out on entering a world or session; after that `SnapshotUpdate`s carry what moved, with the marker and creature lists only when they changed. | Local only, no firewall prompt, trivial cost at this message size. |
| Map renderer | MapLibre GL JS. Raster tile pyramid. Game ↔ map conversion via a fitted affine transform in Web-Mercator space. | Proven, GPU-accelerated, handles 10k+ pins with clustering. |
| Map data | Generated from the game's own files: tiles are rendered from the terrain data or, for worlds built from meshes, drawn top-down on the GPU (`tools/mapgen`); pins come from an in-game world export or straight from the scene files (`tools/pins`). User pins are stored locally. | Uses the game's own data, so live positions and pins share one coordinate system by construction. |
| Distribution | Map data ships as per-game map packs on a `maps` GitHub release, separate from the app. The app ships only `game.json`. A pack downloads only when the player needs it (Play, Connect, opening the map, a download button, or a game set to connect automatically starting); packs they have stay up to date. | Map fixes reach users without an app release, the installer stays small, and nobody downloads maps for games they don't play. |
| Updates | Tauri updater against signed GitHub releases. Installs only while no game is running. | Never interrupts a session; signed updates can't be tampered with in transit. |
| Routes | Surface routes come from the game's own pathfinding (Unity NavMesh, or a game's own, e.g. the A* Pathfinding Project in Sons of the Forest). Caves the game can't route in use grids from the map pack (`nav/`), searched in a web worker, and each route is then walked against the game's colliders as they are now (`crates/agent/src/walk.rs`); blocked spots are routed around for a while. | Precomputed data can't know that a rock blocks a passage, or that it's been cleared. The check costs about 1–2 ms per route. |
| Settings | One settings panel (`apps/overlay/src/settings/`) in both windows. Each setting is an entry in `registry.tsx` (category, search words, per game or for all games, control), so the panel's categories and search cover new settings without more UI work. UI settings are saved with the user data and sent to the other window when they change; the backend owns keys, connection modes and updates (`app-settings.json`) and announces changes itself. | Settings stay findable as they grow, and either window shows the same values. |
| Worlds | The agent reports which world or save is loaded (`WorldSnapshot.world`), and found items, custom pins, map state and fog of war are kept per world (`apps/overlay/src/state/worlds.ts`). Games that don't report one use a default world, which is where everything lived before; the first world detected takes its data over. A new game without a save id yet is a provisional world whose data moves to the save when it's first saved. | Saves, hosted and joined worlds don't mix up each other's progress, and games without detection keep working as before. |
| Failure handling | Failures degrade instead of stopping the app. A crashed page reloads; if WebView2's browser process dies, the app restarts (at most 3 times in 10 minutes). UI errors retry in place, then reload the page (at most 3 times in 10 minutes). Connection failures retry with backoff (5 s up to a minute), injecting again if the agent is gone. The agent is injected only once the game's window has been up for 3 seconds: earlier can hang the game's startup. An agent silent for 20 seconds (a hung game) clears the live data and reconnects, and everything comes back by itself. With nobody connected and friends around, the play-together room is joined again (30 s up to 5 minutes), and when the network returns. Failed map downloads retry after 2 minutes. While the agent isn't working, the app polls the map key itself, so the map still opens over the game as a plain map. Save files are checked at startup: a corrupt one is moved aside and its last good backup restored. Panics, UI errors and map errors go to the log, repeats counted rather than written, and the last few log files are kept. Settings → Advanced → Report a problem saves logs and setup to one file, without the home folder, account name or session ids (a session id is its room's password, so it's never logged); agent tick panics are caught and ticks go on, until 10 of them turn live features off. Frame hooks fall back to ticking from the message pump. | The map is useful even when live features aren't, and nothing needs a manual restart. |
| Usage analytics | The backend posts a few anonymous events straight to PostHog's capture API (`src-tauri/src/analytics.rs`), with no SDK. Nothing is sent until the player chooses Share in the launcher, and development builds never send. What happens during play adds up into one event per game run, failures count once and only as fixed categories, and the random install ID lives in its own `analytics.json`, outside the settings that go into problem reports. | Enough to see what's used and what breaks while staying within the free plan, with nothing identifying and no error text that could hold a file path. |
| Map state | Tracking, selection, search, layer and view are saved per world with the user data and restored whenever that world's map loads. | The map is as it was left across restarts, reconnects and game sessions. |
| Fog of war | A bitmap per world over the map's rectangle (8 m cells, ~31 KB for Sons of the Forest), revealed within a set distance of the player on the surface (100 m by default, 25–300 m), saved every 5 seconds, drawn as one image layer of generated clouds that thin into wisps at the edge of what's explored (`lib/clouds.ts`, `lib/fogCanvas.ts`). The clouds are still: moving them would mean redrawing the whole image every frame, including on the mini-map while the game runs. By default it covers unexplored land and hides game pins there (on the map, in search and in progress lists; the pin filter refreshes every 2 s while exploring). Own, found and tracked pins and live markers always show. | No spoilers by default, each part can be turned off, and nothing runs per frame in the game. |
| Map window | An ordinary window (`src-tauri/src/map_window.rs`) running the same UI in its "window" view on a monitor of the user's choice. The overlay stays in charge of recording (worlds, fog, found items) and multiplayer; the windows keep pins, found items, tracking and fog in step through events. | A second screen shows the map while playing without a second copy of anything that writes. |
| Playing together | Copies of the app in the same multiplayer session connect peer to peer over WebRTC ([Trystero](https://github.com/dmotz/trystero)), finding each other through public Nostr relays. The room and its password come from the game session (e.g. the Steam lobby), so only players in it can join, and the relays see an opaque topic and encrypted offers. | Nothing to host or pay for, works for any game whose session can be read, and on by default without exposing anything outside the session. |
| Hotkeys | The agent watches for the configured keys (map: default **M**; zoom: **Num +/-** and **=/-**) only while the game is focused. While the app is connected, the game doesn't see the map hotkey through Unity's legacy `Input` API, so a game map on the same key stays closed; without the app it works as usual. While the full map is open it has focus and handles the same keys itself, and the game sees no keyboard or mouse input (`crates/agent/src/focus.rs`). | No global hook, no conflict when typing outside the game, and only one map opens. |

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

A game whose worlds are generated sets `worldMaps`: no map pack can have them, so each
world's map comes from the running game. The first time a world loads the agent draws it
(`Provider::read_world_map`, on a thread of its own after one read on the main thread) into
`<data>/worlds/<game>/<world>/`, where `src-tauri/src/worldmaps.rs` cuts it into tiles
and saves its pins. They're served under `worlds/...`, so the world's map stays available
with the game closed. Pins and explored areas are read again as the map opens.

A game that sets `gameMap` and whose provider reports `Capabilities::game_map` also gets the
player's pins and pings on its own map (`Provider::set_game_pins`, `Provider::game_ping`),
so players without Game Maps see them, and those players' pings come back as markers.

A game with only `game.json`, tiles and pins works as a static map. A provider in
`crates/agent/src/provider/` adds live location, teleport, other players and the
in-game hotkey. The agent reports which of these it supports when it connects.
Providers can also report creatures around the player and the multiplayer session
(which turns on playing together).

## Playing together

`apps/overlay/src/lib/sync/` connects the app to others in the same multiplayer
session. The agent reports the session in each snapshot (`WorldSnapshot.session`);
while **Play together** is on, the app joins the room for `<game>/<session>`.
Messages (`messages.ts`, validated on arrival) are:

- `hello`: version and player name. Peers on another version are ignored.
- `track`: "track this with me". The receiver gets a card to accept; it tracks the
  map pin when their map has it, else the spot.
- `ping`: a spot shown on everyone's map for 30 seconds.
- `pins`: the sender's shared custom pins, shown under **Friends' pins** while they're
  connected.

Peers behind strict NATs may fail to connect directly; there's no relay server for
that case. The connection check (`lib/sync/connectionCheck.ts`) tells players which side
to look at.
