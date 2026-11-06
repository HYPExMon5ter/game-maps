# Sons of the Forest

Unity 2022.2, IL2CPP (metadata v29), no anti-cheat. Steam app 1326470.

## Map

- Surface terrain: `TerrainData` "Site2Terrain" in `sharedassets7.assets`, used by
  "Site02Terrain Tess" in scene `level7`. Heightmap 4097², 0.9765625 m/sample;
  height scale 752.94; terrain origin (-2000, -19, -2000), so it covers
  x, z ∈ [-2000, 2000].
- Validated against two independent reference points (game position → terrain
  height): errors under 0.1 m.
- Splat maps (3 × RGBA, 2048²) live in `resources.assets`. There are 12 terrain
  layers (sand, dirt, rock, grass, forest ground, …).
- Icons (white glyphs on transparent backgrounds, trimmed to the glyph): the GPS icon
  set (`items_assets_gpslocatoritem_*.bundle`), the cave icon (`uishared_assets_all_*`),
  and every inventory item icon (`<Item>Icon` textures in `resources.assets`, named by
  each item's `ItemUiData._icon`).
- Generator: `tools/mapgen/sotf.py` (`--icons-only` for icons). Tiles go in XYZ tiles
  z3–8, with the island on tile (3,3) at z3 (see `projection` in `game.json`). An
  eighth of the world wide, so 32:9 screens can still center the island.

## Agent members (looked up by name at runtime)

| Use | Member |
|---|---|
| Player position | `TheForest.Utils.LocalPlayer` `<Transform>k__BackingField` / `_transform` |
| Camera direction | `LocalPlayer` `<MainCamTr>k__BackingField` / `_mainCamTr` |
| Cave state | `LocalPlayer.get_IsInCaves()` |
| Teleport | `LocalPlayer.TeleportTo(UnityEngine.Vector3)` |
| Other players | `PlayerTracker._instance` → `AllPlayers` (List) |
| Player name/color | `Sons.Multiplayer.Client.CoopPlayerRemoteSetup` `_cachedPlayerName`, `_cachedPlayerColor` |
| Chat open (suppress hotkey) | `TheForest.UI.Multiplayer.ChatBox` `<IsChatOpen>k__BackingField` |
| Ground height for teleports | `UnityEngine.Terrain.activeTerrain.SampleHeight(Vector3)` |
| Cave systems (teleports, export) | `Sons.Areas.CaveSystem`: `IsInsideExtents(Vector3)`, `DoForcedEnter`, `DoForcedExit`, `GetAreaMask`, `_areaId`; `CaveEntranceManager.get_CurrentAreaMask` |
| Live GPS markers, Kelvin | `Sons.Gameplay.GPS.GPSLocator._activeLocators` (static `List`), `Position()`, `get_IconData()._icon`; Kelvin's tracker has the `RobbyLogo` icon |
| Nearby creatures | `Sons.Ai.Vail.VailActorManager._instance._activeActors` (`List<VailActor>`), `get_TypeId()` / `get_ClassId()` (enum names: Cannibal and Creepy are hostile, Animal is wildlife), `IsDead()`. Kelvin (`Robby`) is left out: his GPS tracker already shows him. Friendly ones (Virginia) are companions in the app: listed with the players, selectable, trackable, and teleport targets |
| Loaded save | `Sons.Gameplay.GameSetup.GameSetupManager.GetSelectedSaveId()` (a `UInt32`, 0 for a new game until it's saved) and `GetSaveGameType()` (`SinglePlayer`, `Multiplayer` when hosting, `MultiplayerClient`; `Default` for an unsaved new game) |
| Multiplayer session | `Sons.Multiplayer.CoopLobbyManager.GetActiveInstance()` → `CoopLobby.get_LobbyId()` (a `CSteamID`), else `GameServerManager.ServerId` on dedicated servers; the player's name from `Steamworks.SteamFriends.GetPersonaName()` |
| Kelvin's condition and orders | `VailActor.GetRobbyCallbacks()` is his `Sons.Gameplay.Robby`: `IsAlive()`, `_injuredState`, `_activeOrder`, `_followTransform`, and the order lists in `_orderData`. Orders go through `ServerGiveOrders(OrderType, Int32, Int32, Transform)` on the host and `ClientSendOrders(OrderType, Int32, Int32)` on clients; the first number is the index in the order's list, the second the `DropLocationType` for fetching. `LocalPlayerHelpUp()` helps him up (it walks the player to him) |
| Companion stats | `VailActor.GetHealth()`, `GetMaxHealth()`; `GetStatsManager()` → `VailStatsManager.GetStatsManager()` → `StatsManager.get_Stats()`, each `Stat` with `GetName()`, `Sample()`, `GetMax()` |
| Time, season, weather | `Sons.Environment.TimeOfDayHolder.GetTimeOfDay()` (`_timeSpan`), `GPSTrackerSystem.GetDaysSurvived()`, `SeasonsManager.get_ActiveSeason()`, `TheForest.World.WeatherSystem.Instance` |
| Player dying | `LocalPlayer.Stats` → `PlayerStats.IsDying()`, `IsDead()` |
| Dropped bag | `TheForest.Player.Actions.PlayerRetrieveDroppedInventoryAction`: `_isInventoryDropped`, `_droppedInventoryPosition`. The game picks the spot, which can be tens of meters from where you died. Found by searching objects, once per world and after a death |
| Route checks, breadcrumbs | `UnityEngine.Physics` rays and sphere sweeps against the world layers (`Default`, `TreeWS`, `Culling*`, `Prop`, `BasicCollider`, `Blocker`, and `Terrain` above ground); player, character, item and trigger layers are left out |
| Item names (export) | `ItemDatabaseManager.ItemById`, `ItemData.get_Name/get_Type/get_UiData`, `ItemUiData.GetTitleLocalized()` |
| Frame hooks | `RenderPipelineManager.DoRenderLoop_Internal`, `EventSystem.Update` (menus only in this game), `Sons.Input.InputSystem.LateUpdate`, `FirstPersonCharacter.Update` |

If a game update renames something, the agent logs which members failed to resolve
(Settings → Diagnostics) and turns off only the affected feature.

## Verified in game (2026-11-02, main menu + new Peaceful game, single player)

- Injection at game start, resolution of all members above, and every frame hook installed.
- Hot-swapping an outdated agent while the game runs, and reconnecting after app restarts.
- M opens and closes the overlay, and focus returns to the game. The overlay draws
  over the borderless game.
- Live position: player Y 385.39 matched the terrain height of 385.39 at the same x/z.
- Teleport with ground snapping, and teleport back to an exact position.
- HUD mini-map over gameplay.

Verified again on 2026-11-05: the agent loads with every system found, and teleporting into
Cave GA reported the player indoors and marked the cave found.

## Pins and points of interest

- Pickups aren't in the scene files. Once a world is loaded, though, every pickup on
  the island exists as a (mostly inactive) `PickUp` component, so a single in-game
  export right after starting a new game captures them all (~2,900, with every
  unique item). Wild plants are the exception: they're spawned procedurally around
  the player, so they're left out of the pins.
- Export: Settings -> Diagnostics -> Export world data (`AppMessage::ExportWorld`).
  It records pickups (item id, name, type flags, title, icon), GPS locators, cave
  triggers, terrain height, and the cave system containing each object.
- `tools/pins/sotf.py` turns exports into `pins.json`: categories from item type flags
  and names, readable names from the localized titles, `caves` layer for anything
  inside a cave system or more than 4 m below the terrain, cave/bunker names from the
  GPS entrance markers (cave triggers share an area id per cave), and each entrance
  lists the notable loot inside.

## Caves and bunkers

- Caves and bunkers are marked found once the player has been inside (`visitInside` in
  `game.json`), from the caves layer the agent reports as `indoors`.
- The game has no map marks a player can place, only GPS locators it places itself, so
  in-game pins aren't possible here.
- Cave layouts: the world export also scans every `CaveSystem` (`cave_scan.rs`). It
  force-enters the cave (its colliders only exist while "inside"), waits 30 frames, then
  casts rays down from just below the terrain on a 1 m grid with terrain, water and
  triggers excluded. Mesh colliders are one-sided, so the rays pass through cave
  ceilings and stop on floors. An upward ray measures each floor's headroom: at least
  1.6 m is a walkable floor, less is the top of solid rock. The scan runs within an
  8 ms per-frame budget (about 30 s for all 14 caves and bunkers), then restores the
  player's cave state. `tools/mapgen/sotf_caves.py` renders the floors (relief-shaded,
  with dark walls) into the transparent `tiles/caves` layer shown in the Underground view.
- Cave bounds come from the cave's extent colliders, or from the objects known to be
  inside it (±40 m).
- Cave interiors only get collision and lighting once the game puts the player in that
  cave (normally via entrance triggers). Teleports to a cave call `DoForcedEnter` on the
  cave system containing the destination, and `DoForcedExit` on any other the player is
  in, before `TeleportTo`. Without it the player falls through and the game rescues them
  to the surface.
- A cave's extents reach past its entrances and up through the ground above it, so being
  within them doesn't mean underground. The app says which layer a teleport is for;
  entering the cave at a surface spot leaves the game in cave mode outside: dark, black
  walls, and the map on Underground.
- Which caves the player is in comes from `CaveEntranceManager.get_CurrentAreaMask` and
  `CaveSystem.GetAreaMask`. `_isActive` on the cave system stays false either way.
  `DoForcedExit` also clears the deep cave state (`IsInDeepCaves`).
- The map follows the player underground using `IsInCaves` or a position more than
  4 m below the terrain.

## Stability (verified 2026-11-02)

- Alt-tab with the map open closes it, leaving focus in the other app. The pinned
  mini-map hides while another app is in front and comes back with the game.
- Hotkey after refocusing works, including quick taps. Keys typed in other apps are
  discarded, so they never trigger the map later.
- Quit to the main menu, start a new world again, then quit with Alt+F4 from the world,
  Exit from the main menu, or by closing the window: all clean (Unity's normal shutdown
  log, exit in 0–6 s). The agent stops main-thread work in
  `Application.Internal_ApplicationQuit`, and its pipe thread isn't attached to the
  runtime, so shutdown never waits on it.
- Closing the game while the map is open hides the overlay.
- Cave systems are looked up per operation, never cached across frames, because
  objects from unloaded scenes may be reclaimed.

## Also verified in game (same session)

- World export and pins (1,813 pins, 14 categories, 638 underground).
- Teleport into Cave C (Shovel) and the Residential bunker (Katana) and back out; the
  map switched to Underground and back on its own, and the game's GPS switched to the
  cave view.
- Kelvin and live GPS markers on the map; Kelvin in the players menu.
- Overlay placement on a 1920x1080 secondary monitor; layout at 1280x720 to 5120x1440.
- Not yet verified: remote players (needs a second player), Virginia (her GPS icon is
  unknown; she'd show as a generic live marker).
