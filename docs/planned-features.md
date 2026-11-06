# Planned features

Ideas for Game Maps that aren't built yet, split into features for every game and
features for one game. Notes on effort and approach are rough; anything that runs in
the game is measured before it ships (see the frame-time budget in
`docs/architecture.md`).

Status: **Built** (in the next release), **Planned**, **Idea** (worth doing, not designed
yet), **Deferred** (wanted, waiting on something).

## Every game

### Map and navigation

| Feature | Status | Notes |
|---|---|---|
| Per-world data | Built | The agent reports which world or save is loaded (single player, hosting, or joining someone); found items, completion, fog of war, custom pins and the map state are kept per world. Games without detection use one world per game. |
| Fog of war | Built | Only what you've explored is revealed: clouds cover unexplored land and game pins there are hidden (each can be turned off). Recorded from position updates; a bitmap per world. |
| First-run choice | Built | A one-time question sets up fog of war and teleport; both stay in the settings. A tip explains the clouds the first time the map opens. |
| Maps on demand | Built | A game's map downloads only when it's needed: Play, Connect, opening its map, a Download button, or the first run of a game set to connect automatically. |
| Completion tracker | Built | Progress per collectible category with what's left, per world. |
| Map on a second monitor | Built | An always-on, interactive map window on a chosen monitor that follows you live while you play on the main screen. |
| Path history | Built | A trail of where you walked since the app started, on the map and mini-map. |
| Death marker | Built | A pin where you last died, to recover your gear. In Sons of the Forest it moves to your dropped bag, and goes when you pick that up. Death detection is per game (Sons of the Forest so far); the pin is shared code. |
| Pin menu | Built | Right-click a pin, on the map or in a list, to track, edit or delete it, or mark it found. |
| Distinct badges | Built | Every category has its own shape, color and symbol. Symbols ship with the app; a game's `iconAliases` replaces icons from its map pack that don't read on a badge. |
| Nearest unfound | Built | A key that tracks the closest thing not found yet, in the categories shown on the map. |
| Map and mini-map layout | Built | An edit mode to drag the floating map and the mini-map anywhere and resize them by their edges or with sliders. Each game keeps its own mini-map placement. |
| Saved filter sets | Built | Name the categories shown now and switch back in one click ("Looting run", "Story items"), per game. |
| Collection routes | Idea | Visit what's left in a category in an efficient order (nearest-neighbor tour over the route planner). |
| Measure tool | Idea | Right-click to measure straight-line and walking distance between points. |
| Map window follows the game's screen | Idea | Put the map window on whichever monitor the game isn't on, when the game moves. |

### Pins and progress

| Feature | Status | Notes |
|---|---|---|
| Notes on pins | Built | Your own note on any pin, game pins included ("needs the shovel"), kept per world. |
| Screenshots on pins | Idea | An image per pin. |
| Pin timers | Built | A countdown on a pin (resource respawns, crops), with a notice when it's up. |
| Import and export pins | Built | Save a world's pins to a file, or add pins from one, outside multiplayer. |
| Manage worlds | Idea | Rename, merge or delete a world's data from the world picker. |
| Copy pins between worlds | Planned | Comes with per-world data: copy a custom pin to every world, or to one. |
| Share pins by code | Idea | Paste a short code to a friend instead of sending a file. |

### Project

| Feature | Status | Notes |
|---|---|---|
| Usage analytics | Built | Anonymous `app_started`, `game_connected`, `game_session_ended`, `hook_failed` and `app_problem` events to PostHog's free tier, sent from the backend (`src-tauri/src/analytics.rs`) with a random install ID. The launcher asks once, and nothing is sent until the player chooses Share; the README lists everything and Settings → General changes the answer. Downloads come from GitHub's release counts. |

### While playing

| Feature | Status | Notes |
|---|---|---|
| Danger alert | Built | Edge-of-screen indicators toward hostile creatures within a set distance (games with creature tracking). |
| Pings in the game | Built | Friends' map pings drawn over the game like the waypoint, for 30 seconds. |
| Quick-mark key | Built | A key that drops a pin where you stand without opening the map. |
| Mini-map only in a world | Built | The mini-map and compass stay off at menus and with no game running; friends' requests still show. |
| Overlay follows the game | Built | The mini-map and everything drawn over the game move with it to another monitor. |
| Streamer mode | Idea | Hide coordinates, session details and player names when capturing. |
| Controller support | Idea | Open, pan and zoom the map with a gamepad. |
| Session recap | Idea | Distance walked, items found and deaths when you quit. |

### Playing together

| Feature | Status | Notes |
|---|---|---|
| Shared fog of war and found items | Built | Optionally merge friends' explored areas and found collectibles in the same world. Not tested with a second player yet. |
| Who's connected | Built | The players list shows who is connected through Game Maps, and who needs to update. |
| Track requests over the game | Built | Requests and what friends share show over the game even with the mini-map off, and a key accepts them. |
| Friends over the game | Built | Other players' names and distances over the game. Not tested with a second player yet. |
| Map window on a second monitor | Built | Mirrors the overlay's connection: friends' pins, requests, pings and Share all work there. |
| Connection check | Built | Tests the servers that find friends and your network type, says how each friend is connected, and who isn't and why. Connections stay direct: there's no relay, so two strict networks can't link. |
| Regroup key | Idea | One press asks everyone to track you. |
| In-game pins and pings | Built (Valheim) | Your pins go on the game's own map and your pings use the game's own ping, so players without Game Maps see them too (`Capabilities.game_map`), and theirs show in Game Maps. Wanted in every game that has a map or pings: see each game's section. |

## Valheim

| Feature | Status | Notes |
|---|---|---|
| Map from the running game | Built | Worlds are generated, so each one's map, pins and explored areas are read from the game the first time it's loaded. |
| In-game pins and pings | Built | Pins go on the game's map and are shared through a cartography table; pings reach everyone in the world. |
| Resource filters | Built | Tar pits, dragon eggs, giant remains and so on each have their own category and icon. |
| Possible trader spots | Built | A trader's spots read "possible spot" until the game places one and drops the rest. |
| Remembered resources | Built | Ore, plants, ships and carts are remembered as they load near the player, one 64 m area every 200 ms (35 µs on average, measured). |
| Silver without the wishbone | Built | Off by default: silver only shows once found with a wishbone on. |
| Regrowth timers | Built | Off by default: a timer on picked berries and plants. |
| Cleared dungeons | Built | Marked found once the player has been inside. |
| Boss progress | Built | A checklist of bosses and what each unlocks. The Deep North boss isn't listed yet: its key isn't known. |
| Portals | Built | With their names; a host has every portal in the world. |
| Biome tools | Built | The biome under the cursor, and the way to the nearest of each. They respect fog of war. |
| Host's locations for friends | Built | A host's locations are sent to players who join. Not tested with a second player yet. |
| Teleport with ores | Built | Off by default: teleports refuse what portals would. |
| Wind on the mini-map | Built | An arrow where the wind blows, which replaces the game's mini-map. |
| Game's mini-map hidden | Built | While the app's mini-map is on; back when it's off or the app disconnects. |
| Ships and carts from anywhere | Deferred | Left out for lag: they're only known where the player has been, and finding them all means reading every object in the world, millions on an old world, on the main thread. Portals don't need it because the game lists them apart. |
| Resources before you get there | Deferred | Left out: the game places them as an area generates. Predicting them from the seed would mean copying its placement rules, which would be slow and break with game updates. |
| Connected portals | Idea | Show which portals are linked, not only their names. |
| Rested and weather alerts | Idea | A notice when a storm or event is coming. |

## Sons of the Forest

| Feature | Status | Notes |
|---|---|---|
| World detection | Built | `GameSetupManager.GetSelectedSaveId()` and `GetSaveGameType()` (single player, hosting, client). |
| Companions | Built | Kelvin's and Virginia's health and stats on their card; Kelvin takes every order from his notepad, from the map. |
| "Where's my stuff?" | Idea | Search the contents of your storage and get the container on the map. Needs research into how storage is saved and loaded. |
| Progress checklist | Built | Keycards, shovel, rebreather, armor and other key items, linked to their pins. |
| Time, day and weather | Built | Readout of the in-game time, day count, season and weather. |
| Your structures | Idea | Mark the bases and shelters you've built automatically. |
| Cave prep | Idea | A cave's pin shows the gear you need and whether you have it. Having it needs inventory reading. |
| Kelvin on the mini-map | Idea | His position, and a warning when he's hurt or starving. |
| Low-stat alerts | Idea | A notice when Kelvin's food or water gets low. |
| Seasonal hints | Idea | Which food and plants are around in the current season. |
| In-game pins and pings | Idea | Put pins on the GPS for players without Game Maps, if the game lets a client add locators. Needs research into how GPS locators are placed and shared. |

## Sunkenland

| Feature | Status | Notes |
|---|---|---|
| World detection | Built | `Mainframe.SaveManager.CurrentWorldGuid` / `CurrentWorldName`; clients use the world identifier from the session. |
| Creature tracking | Built | Nearby enemies, enemy ships, sharks, animals and bosses on the map, with the danger alert. Ordinary enemies show as "Enemy": their faction's name is localized and not read yet. |
| Playing together | Built | Sessions come from Photon Fusion; not tested with a second player yet. |
| Boat and base markers | Idea | Your boat and bases on the map. |
| Depth and oxygen | Idea | A readout while diving. |
| Cleared areas | Idea | Wrecks and loot areas you've emptied, like collectibles marked found. |
| Fragment and research tracker | Idea | A checklist like the Sons of the Forest one, from the Research and Map Fragments pins. |
| Loot respawn timers | Idea | Start a pin timer when you empty a spot, if loot respawns. |
| Trader finder | Idea | What each trader buys and sells, on their pin. |
| Fuel range ring | Idea | How far your boat can go on its current fuel. Needs research into reading the fuel level. |
| In-game pins and pings | Idea | Put pins on the game's map as map marks, which are already read, so players without Game Maps see them. Needs research into whether marks are shared. |

## Other games

Ranked by players, how much of Game Maps already applies, and whether the owner has
played them. Player numbers are 30-day averages on Steam from November 2026. Anything with
anti-cheat stays out: injecting into it and teleporting could get players banned.

| Game | Average players | Engine | Status | Notes |
|---|---|---|---|---|
| Schedule I | 10k | Unity IL2CPP | Idea | One handmade map on a runtime that's already supported. Quick to add. |
| Palworld | 37k | Unreal | Idea | 1.0 launched July 2026. Needs an Unreal agent; Wand already has live location and teleport here. |
| Satisfactory | 16k | Unreal | Idea | Comes much cheaper with the Unreal agent. |
| 7 Days to Die | 21k | Unity | Idea | Wand has a map with no live features. It ships with Easy Anti-Cheat, which has to be off for this to work: check that first. |
| Core Keeper | 8k | Unity Mono | Idea | Generated worlds, so it can use the same runtime map as Valheim. |
| Subnautica 2 | 2k, 467k at launch | Unreal | Idea | Worth it when an update brings players back. |
| Grounded 2 | 4k | Unreal | Idea | With the Unreal agent. |
| The Forest | 5k | Unity | Idea | Same audience as Sons of the Forest, and caves make a map useful. |
| Raft | 7k | Unity | Idea | |
| Project Zomboid | 46k | Java | Deferred | Needs a third agent type, and its servers may reject teleports. A strong candidate later. |
| Enshrouded | 41k | Custom | Deferred | Nothing to look up by name, so live features would break with every patch. A static map is possible. |
