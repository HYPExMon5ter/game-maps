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
  layers (sand, dirt, rock, grass, forest ground, …), some only named `Layer8` and so
  on, so colors go by each layer's diffuse texture name. UnityPy returns images top row
  first while the heightmap's row 0 is min z, so splat maps are flipped to match.
- The ocean surface is at y = 14 (`Atmosphere/CrestOcean` in `level7`), not 0. Ground
  below that only counts as sea if it connects to the map's edge.
- Rivers and lakes are meshes under `Streams` and `Lakes` in `Site02StreamsAndLakes`
  (build scene 13, `level13`). Their terrain is carved, so they don't show in the
  heightmap. Materials containing `TerrainBlend` are moss on the banks, not water.
- Validated against data the render doesn't use: all 80 `WorldUnderWater` spawns are in
  water, 99% of the 2,181 `WaterStimuli_*` markers are within 4 m of it, and there's
  next to no ocean inside the `SonsOceanOccluder` boxes.
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

## Cave water

- Water in caves and bunkers is on the Water layer (4) of their scenes: flat surface
  meshes (`Lake Polygon`, `_LakeCave<x>LOD0_`, Cave D's `LakeAGroupNoFreeze`, stream
  surfaces; lower levels of detail are `...LOD1_`, `...LOD2_`) and volumes
  (`_LakeCave<x>Zone_`, `WaterZone`, `SwimVolume`). `sotf_3d.py` keeps the surfaces'
  upward faces within each cave's collision bounds as `build/water-<id>.bin`.
- The cave scan's floor rays skip the Water layer, so lake beds are scanned. `sotf_caves.py`
  rasterizes the surfaces onto each cave grid: a floor up to 40 m under a surface with
  open water above it becomes a swim node at the surface's height (wading depth, under
  0.4 m, is walked as it is), marked in `nav/cave-<area>-water.bin` (a bit per node).
  Routes cross water at the surface and may drop into it from any height.
- Jumping into water added about 11% more reachable floor to Cave B, little elsewhere.
  What cut caves into parts was ropes and underwater passages (below).

## Ropes, ziplines and dives

- Climbing ropes are `ClimbingRopeRock` objects (numbered copies: `ClimbingRopeRock (3)`)
  in the cave scenes and some in the Addressables cave bundles, with `TriggerTop` and
  `TriggerBottom` grab points. Ziplines are `ZipLineController` with `AnchorA` and
  `AnchorB`. Off ones are left out, except under a part the game turns on once a cave
  is opened (`CaveHExternalOpened`). 19 ropes and 2 ziplines in all (2026-11-10).
- `sotf_3d.py` writes them, and the water volumes as triangles, to `build/` (by cave;
  not in the map pack). `sotf_caves.py` snaps each rope end to the nearest floor within
  5 m across and 3 m up or down (a ledge's edge often has no floor in the scan), and
  lists the node pairs as `links` in `caves.json`. All 19 ropes link.
- Verified in game (2026-11-10): hold E at a rope's bottom to grab it (a tap doesn't),
  then W climbs about 1.2 m/s; it lets go on the ledge at the top. A zipline only offers
  its prompt (a tap of E) at the high end: there's none at the low end, and the rope is
  out of reach from the floor there. The ride stops about 1.5 m short of the low anchor
  and drops the rider onto the floor under that spot, which is where the link lands
  (Cave B's: the game put the player within 0.1 m of it).
- Dives: floors inside a water volume, with the water at least 1.7 m over them and rock
  in between (else they're open water, swum at the top), marked in
  `nav/cave-<area>-dive.bin`. A volume whose column has only its top face reaches down to
  its lowest point. Underwater, routes swim up and down freely; a meter under water
  costs the same as one walked, plus 2 m for going under (so routes don't bob in and
  out). Teleporting to a dive node in Cave F put the player under water.
- On 150 random trips per cave, ropes, ziplines and dives made 24 more routable in
  Cave B, 34 in Cave F and 19 in Cave G. Searches average under 10 ms, 76 ms at worst,
  in the worker.
- The in-game walk check skips climbed, ridden and swum legs: it walks the floor and
  would call them blocked.

## Cave collision

- `sotf_3d.py` reads each cave's collision from the game files. Caves A–G each have a
  collision prefab in Addressables, `caves_assets_cave<x>_*.bundle`, with parts named
  `CaveCollision<Kind><Letter>` (Floor, Walls, Ceiling, Rocks, Flowstone, Cliffs) already
  in world coordinates. Cave D nests
  its halves twice (`CaveDCollisionAddressable/CaveDCollisionAAddressable` and
  `CaveDCollisionAAddressable` at the root), so parts are deduplicated by mesh and matrix.
- The cave and bunker scenes add their static colliders on layer 22 (`BasicCollider`),
  minus their `*External*` roots (set dressing around the entrances). Bunkers are mostly
  box colliders, Cave H has no prefab and is only scene colliders, and the Luxury
  bunker's scene (`BunkerLuxury`, level 26) also holds Cave E. Their meshes live in
  sharedassets 7, 11, 15 and 20 and resources.assets.

## Filling the cave scans from collision

- The in-game scan caps a cave at 1200 cells a side (Cave D is 1642 m across: its east
  end was cut off) and found almost nothing in parts of `CaveD_Expansion`. `sotf_3d.py`
  scans each cave's full collision offline the same way (`mesh3d.floor_scan`: surfaces
  facing up are floors; headroom up to the first surface above facing down, or the
  terrain; ceilings included; nothing above the terrain), to `build/floors-<id>.npz`.
- `sotf_caves.py` grows each scan to that collision's extent and adds the floors it has
  that the game's scan doesn't.
- Both scans kept three floors per cell (four now, below). Keeping the highest three cut caves apart
  (found 2026-11-10): where tunnels stack, the outer tops of rocks buried in the ground
  (one-sided collision: nothing inside solid ground stops a ray, so they look like
  floors open to the sky) and the tops of rock between tunnels took the slots, and
  dropped the tunnel under them. Cave D was eight big pieces routes couldn't join,
  Cave G three, Cave H four. Now the offline scan looks at eight surfaces per cell and
  keeps (`mesh3d.pick_floors`) floors with room to stand and a ceiling first, then
  floors open above, then the rest; the room over a floor stops at the next floor above
  it too. The in-game scan still keeps its highest three; the merge applies the same
  rule to both.
- Collision is made of pieces, and rays fall through the slits where two meet, which a
  player walks over. The offline scan bridges gaps of up to 2 cells between floors with
  room to stand at the same height (within 0.5 m), where the gap has a ceiling over it
  at about their headroom: solid rock between two tunnels has none (collision faces
  into the tunnels), so it isn't bridged.
- Bunker stairs are box colliders with a negative height (`7.5 x -0.2 x 2.6`). Unity
  takes a box's size as its absolute value; read as is, the box came out inside out, its
  top facing down, and no flight of stairs was a floor (fixed 2026-11-12).
- The cave grids keep four floors per cell (`NAV_LAYERS`; the in-game scan keeps three).
  Bunker Residential's stairwells stack four flights, and Cave D's main entrance joins
  the rest of the cave by two ropes, one above the other at (206, 164..212, -676): with
  three floors, a rope end's floor fell out and the rope linked nothing. Where a cell
  has more floors than it keeps, those with room to stand (2 m) come before those with
  only crouching room (the slab under the next flight up has 1.6 m).
- Checked in game (2026-11-12): a route from Cave D's main cave to Artifact Piece D
  climbs both ropes, and the walk check passes it. The two tunnels that cross near
  (201, 160, -675) don't join there: between them is a 2 m hole down to the tunnel at
  142 m (a teleport onto the narrow ledge at 159 m dropped the player through it).
- Pieces routes still can't join (2026-11-12), none with a pin in it except one: Cave
  D's straight tube from (-28, 341, -648) down to (272, 232, -504), deep under the
  mountain and unlit, which a player slides down (it's steeper than they can stand on),
  and a few small pieces near it; Bunker Residential's entrance and exit rooms, whose
  tunnels to the bunker aren't in its collision files (they're in the game, the
  in-game scan has part of them); Bunker C's buried roof; and Bunker Entertainment's
  32 m² ledge with the Hang Glider Launcher blueprint.
- Cave routes that can't reach their target (it's under water, or up a climb) go to the
  nearest walkable spot instead, marked incomplete; the map draws the rest of the way as
  a faint dotted line.

## Teleports into caves

- A cave's collision streams in a few frames after the game switches it on, so a
  teleport straight into one could land before its floor and fall through (it killed
  the player once, from the surface into Cave F). The agent now holds the player at the
  spot each frame until there's floor within 4 m under it (up to 5 s), then sets them
  down once more.

## 3D view

- The cave and bunker models are drawn from the routing grids (`tools/mapgen/cave_model.py`,
  run by `sotf_caves.py`), not from the rock: the floors you can stand on as one smooth
  surface, a 1.4 m wall where the cave ends, the face of each step between floors, and no
  roof. Seen from above they read as open-topped tunnels, and routes always lie on them.
  The first models were the collision itself, simplified; they read as lumpy rock, and
  the simplification lost narrow passages (replaced 2026-11-10).
- What's drawn: every piece of floor joined to at least 30 m² of others by walking (a
  step of up to 1.2 m), swimming, a rope or a zipline. Smaller pieces are the tops of
  rocks and props. In a cell, a floor less than 3 m above another is a rock top, left
  out; gaps of up to 8 cells are filled. Outlines and bumps are smoothed by averaging
  with neighbors. Swum cells are drawn as water (`3d/<id>-water.bin`, at the surface, or
  at the top of the water volume over a dive), with no wall along a shore.
- Walls face into the cave only. The viewer draws front faces only, so the walls between
  the camera and a passage aren't drawn and never hide it.
- Sizes: about 22 MB for all 14 (Cave D 7.3 MB, 359k triangles). With the terrain tiles
  they're the map's 3D pack, 13.7 MB zipped, which the app downloads only when the
  player first picks 3D (`tools/mappack/publish.py`, `EXTRAS` in `mappacks.rs`).
  They're not simplified: quadric simplification moved the outlines away from the walls.
- Some caves are still several pieces routes can't join (see "Filling the cave scans
  from collision"). The model shows them all.
- The snow caves (`IceCaves`, level 19) have no model. Their own scene holds only a few
  details (about 10 m across); the chambers are part of the island's merged rock
  collision (`MergedWorldCollision*` in level 7), whose faces point out of the rock, so
  a model cut from it shows only the outside of boulders (tried 2026-11-10).
- There's one model per area the game loads (`CurrentAreaMask` bit), which holds all of
  a cave you can reach from inside it: the Luxury bunker's area includes Cave E. Each is
  named after the cave or bunker whose collision bounds hold most of its floors
  (`build/groups.json`), and its id is that cave's (`cave-a`, `bunker-food`, …), not the
  area id.
- Terrain elevation tiles are the island heightmap at 4 m per pixel (zoom 3 to 5), about
  0.8 MB. MapLibre scales heights by the latitude in the middle of the view, and the map's
  tile spans about 0° to 41°, so the app recomputes the true-scale exaggeration whenever
  the view settles (when it changes by more than 2%).
- The 3D view draws the rest of an incomplete route as a thinner, fainter line.

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
