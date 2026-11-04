# Sunkenland

Unity 2021.3, **Mono** (managed assemblies in `Sunkenland_Data/Managed`), HDRP, Photon
Fusion multiplayer, no anti-cheat. Steam app 2080690.

## World

- One scene, `level2`. The world is open sea with an archipelago of small islands, a
  western island and a large southern island.
- Sea level is `WorldScene.oceanObject`'s height, **y = 28.59** (not 0). The game copies
  it into `WorldScene.oceanObjectY` at startup.
- Three terrains (`Biome3_1 1`, `Biome3_1 3`, `Biome3_2`) form the seabed and the larger
  islands. Their splat maps are shared between terrains and don't match the terrain
  shapes, so the map colors terrain by height and slope instead.
- The 50 `Location`s (`Locations/Level N/<name>`) hold loot, NPCs and gameplay objects in
  the scene. Their buildings are streamed at runtime: `LocationStaticStreamer` instantiates
  the Addressables prefab `streamedVisualPrefab` under the location
  (`InstantiateAsync(parent)`), from the `locationvisuals_assets_all_*` bundle listed in
  `StreamingAssets/aa/catalog.json`. Older builds leave stale copies of that bundle next
  to it; only the one the catalog lists is current.
- The game's own map (`UIMap`) is a grid with icons, with no map art. Its world rectangle
  is south-west corner (-2750, -4000), size 3000 x 6000 (x east, z north).

## Map

- `tools/mapgen/sunkenland.py` draws the world top-down on the GPU (moderngl): every
  enabled MeshRenderer under `Locations`, `Shelter Island Location` and
  `Scene/AirDropPos` (LOD 0 only), the streamed location prefabs, the terrains and their
  trees and kelp, each with its own base color texture. About 70M triangles and 650
  textures. It then composites the sea: the seabed shows through shallow water and fades
  to deep blue with depth.
- The map square is x -4200..1800, z -4300..1700 (6 km), on tile (3, 3) at z3; the base
  image is 16384 px (0.37 m per pixel, native zoom z3–z9).
- Icons: each location's map icon (`Location.locationIcon`, white glyphs) and the
  game's `icon …` UI glyphs.

## Pins

Everything is placed in the scene, so pins come straight from the game files:
`--export-world` reads the script fields of scene objects (via the game's own assemblies)
and `tools/pins/sunkenland.py` turns that into `pins.json`.

| Category | Source |
|---|---|
| Locations | `Location` (localized name and description, level, blueprint count, map icon; placed at `mapLocator` like the game's map) |
| Traders | `Trader`, plus NPCs that aren't traders or captives |
| Companions | `AssistantRescue` (Freya, Leia, Ronan; inactive until reached) |
| Captives | `PrisonCage` |
| Research & Blueprints | `LearningResource` (what each one teaches) |
| Ore & Mining | `VeinOfOre` (auto-miner spots), `CollectableByToolHit` (iron, copper, sulfur, anatase) |
| Fish, Plants & Foraging | `FishSpawner`, `ItemSpawner` |
| Containers | `Chest`, split into valuables (safes, military and large containers, stashes) and the rest |
| Airdrop platforms, map fragments, supply crates, radiation | `AirDropPlatform`, `Fragments`, `SupplyCrate`, `RadiationZone` |

Objects inactive in the scene are left out (random traders, unused templates), except
companions. Pins below sea level note their depth.

## Agent members (looked up by name at runtime)

| Use | Member |
|---|---|
| Player position and heading | `FPSPlayer.code` (static) → `myTransform` |
| Teleport | `FPSRigidBodyWalker.code.ResetParent()` (leave a boat deck), then `Transform.set_position` on `FPSPlayer.code.myTransform`, as the game does for respawns, beds and teleport gates |
| Teleport ground height | `Physics.Raycast` straight down (triggers ignored), never below `WorldScene.oceanObjectY` |
| Teleport guards | `Global.code.player` (`PlayerCharacter`): `IsDead`, `drivingObject`, `seatObject`, `CurHelicopter` |
| Other players | `WorldScene.code.allPlayerDummies` (includes the local player's `PlayerCharacter.playerDummy`), `PlayerDummy.myTransform`, `get_CharacterName()` / `cache_CharacterName` |
| Companions | `WorldScene.code.Assistants`, `Assistant.TransformPosition`, `AssistantName` (Fusion `NetworkString`) / `DefaultName` |
| Loaded world | `Mainframe.code.get_SaveManager()`: `get_IsCurrentWorldValid()`, `get_CurrentWorldGuid()`, `get_CurrentWorldName()`; when joining, `Mainframe.code.get_WorldManager()`: `get_WorldIdentifier()`, `get_WorldName()` |
| Nearby creatures | `WorldScene.code.AllCharactersList` (`Character`; players and companions are other classes): `myTransform`, `get_IsDead()`, `Faction.IsFriendly`. `EnemyType` only names animals, sharks, mutants and bosses; ordinary enemies are `None`, often with no `Faction`, so anything alive and not friendly counts as hostile. Only characters near the player are spawned |
| Enemy ships | `WorldScene.code.EnemyBoatComponents` (`Boat`): `TransformPosition`, `Destroied` |
| Placed flags | `Buoy` objects (`FindObjectsOfType`, every 10 s, or 5 minutes if the search is slow; about 57 ms in a late-game world; no list holds them): `get_IsAI()` (the game's own), `get_BuoyName()`, `transformPosition`, `M_MarkerType` |
| Map marks | `Global.code.uiCompass.get_MapMarks()`, a Fusion `NetworkDictionary`, walked through its enumerator |
| Multiplayer session | `Fusion.NetworkRunner.get_Instances()`: the running runner's `get_IsSinglePlayer()`, `get_IsClient()`, `get_SessionInfo().get_Name()`; the player's name from `Steamworks.SteamFriends.GetPersonaName()` |
| Boats, helicopters | `WorldScene.code.PlayerBoatComponents` (`TransformPosition`, `DisplayName`, `Destroied`), `Global.code.HelicopterControllers` |
| Chat or console open (suppress hotkey) | `Global.code.uiCombat.uiInGameTextChat.<IsChatInputActive>k__BackingField`, `Mainframe.code.quantumConsole.<IsActive>k__BackingField` |
| Frame hooks | `RenderPipelineManager.DoRenderLoop_Internal` (HDRP), `Global.Update`, `FPSPlayer.Update`. These are JIT-compiled, and launches can leave no free memory near that code for a detour (`MH_ERROR_MEMORY_ALLOC`); the agent then ticks from its `PeekMessage` hook on the game's main thread (`crates/agent/src/hook.rs`) |

The agent's Mono backend (`crates/unity-runtime/src/mono.rs`) uses the embedding API of
`mono-2.0-bdwgc.dll`; frame hooks detour the JIT-compiled code of those methods.

## Focus and input (`crates/agent/src/focus.rs`, all games)

- The game minimizes its Fullscreen Window as soon as it loses focus, so opening the map
  minimized it and the overlay closed with it. The agent skips the game's own minimize
  requests (`user32!ShowWindow`) while a Game Maps window is in front. Alt-tabbing to any
  other app still minimizes.
- Unity's legacy `Input` API (which the game uses) keeps reading the keyboard and mouse in
  the background. Its native functions (`GetKey*`, `GetButton*`, `GetMouseButton*`,
  `GetAxis*`, `anyKey*`) report nothing pressed while a Game Maps window is in front.
- Teleports to points more than 3 m below the sea surface (sunken locations, underwater
  loot) land on the surface right above.
- The game's own map key is also M; while Game Maps is connected the game doesn't see
  the map hotkey, so only the overlay opens.

## Verified in game (2026-11-02, build 26376704, single player, Fullscreen Window 3440x1440)

- Injection, hot-swapping the agent while the game runs, all members and frame hooks,
  16 guarded Input functions.
- M opens the map over the game; the game stays fullscreen behind it and gets focus back
  when the map closes.
- Live position on the map, search, pin details with localized descriptions.
- Teleport to a map point (onto the southern island, onto the sea surface) and to a
  location pin (Airport, 95 m deep: lands on the surface above it).
- Not yet verified: other players and boats or helicopters (none in this save), chat
  detection.
