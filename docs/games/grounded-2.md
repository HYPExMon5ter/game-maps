# Grounded 2

Unreal Engine 5.7 (game version 0.6.0.5), Steam app 2661300, process
`Grounded2Steam-Win64-Shipping.exe`. The game's code module is `Maine`, as in Grounded 1.
No anti-cheat. Provider: `crates/agent/src/provider/grounded2.rs`.

## Engine access

The first Unreal game, so it brought `crates/unreal-runtime`: name-based access to the
engine's reflection data, found without signatures (see the crate docs). Things that differ
from older UE versions and that the runtime handles:

- Object table entries keep the object 8 bytes in (a 64-bit flags and reference count
  comes first), and chunks are allocated ahead of use.
- Enums keep their names and values in two arrays (`UEnum::Names` split).
- Array properties keep their flags before the element property.

The agent ticks from the message pump on the game thread (`hook::enable_pump_ticks`),
which runs once per frame before the world ticks. Positions are converted to the agent's
convention in `crates/agent/src/unreal.rs` (meters, Y up: east = Unreal Y, north = Unreal X).

A development build of the agent writes `%TEMP%\game-maps-unreal-types.txt` (every class,
struct and enum, with members) and `%TEMP%\game-maps-unreal.usmap` (type mappings), when
the mappings are older than the game. Both take a few minutes; they're what names here
were looked up in.

## World

`InGameGameState.PlaythroughGuid` is the save, and the same for everyone playing in it, so
it's both the world id and the play-together session id (`playthrough:<guid>`).
`PlaythroughName` is the save's name. Single player, hosting or joining comes from
`KismetSystemLibrary.IsServer` / `IsStandalone` and the number of players.

The clock is `SurvivalGameState.CalendarComponent` (`GetHourFloat`, `GetMinute`, and
`GetTotalGameHoursPassed` / `GetHoursPerDay` for the day).

## Map

The paintings the game's own map uses: `T_UI_AugustaMainMap` for the park, and
`T_UI_AugustaAbyss` for the Abyss, the region under it, which the game shows as a map of
its own. Their world rectangles are in the level list: `UI_MapPanel`'s entry for
`Augusta_Main`, and the game instance's entry tagged `Level.Abyss`. Positions map linearly
onto them: u from Unreal X, v from Unreal Y. (`MapComponent.WorldMapXMin` and friends are
left over from Grounded 1 and don't match.) The two don't overlap, so
`tools/mapgen/grounded2.py` puts both on one canvas at their real places, turned north-up.

Places under the park (the facility under the cooler, the labs under the outposts) share
the park's painting: there's no separate underground layer yet.

## Pins

Points of interest and a few landmark actors come from the level files
(`tools/mapgen/grounded2-extract`): `BP_POI_Actor` actors with their
`PointOfInterestDataAsset` (name and category: Wonder, FieldStation, Lab, MIXR),
teleporters, BURG.L stations, resource analyzers and comm arrays. Names are Obsidian
`LocString`s; the string table ids are assigned in the game's code, and the few the tools
need were read from the running game (`OEITextLibrary.GetLocStringText`) and are written
in `Program.cs`.

Everything else comes from a world export (Settings → Advanced → Export world data, in a
new game). Grounded 2 keeps whatever can be picked up, harvested or fought as LOD actors
(`BaseLODActor`): light objects for the whole world, from the start of a game, that become
real actors only near a player. About 17,000 in a new world:

| LOD class | What | How it's told apart |
|---|---|---|
| `SpawnedItemLODActor` | Items lying in the world | `GetItem()` → `Item.ItemDataRowHandle` |
| `HarvestNodeLODActor` | Resource nodes, dig spots, milk nuggets | Lite data (below) |
| `ScienceCollectableLODActor` | Raw science | |
| `LootBagLODActor` | Loot bags, dead scientists, player backpacks | Lite data, `LootBagType`, `OwnerPlayerGuid` |
| `ColorCollectibleLODActor` | Paint | |
| `CreatureLODActor` | Creatures | `GetCharacterTypeName()` |

What a LOD actor stands for is its lite data asset (`ActorLiteData`), which it doesn't
expose. `IsFullActorTypeLite(asset)` says whether it's a given one, so the export tests
the assets made for the actor's LOD class (`ActorLiteData.LODActorClass`). The asset's
`LootComponentLiteData.LootData` lists what a node drops, the same data the game's
resource finder uses (`bShowLootInResourceFinder`).

The export takes about 2 seconds with a release build of the agent (12 minutes with a
debug build: use release).

## Teleport

`Actor.K2_TeleportTo` on the player's character, single player or hosting. A spot whose
ground isn't loaded yet (World Partition streams around the player) can't be traced: the
player is moved there in flying movement mode, held until a line trace finds the ground
(10 s at most), then set on it, or sent back if it never loads.

A client can't move itself: the host would put it back. It calls
`SurvivalPlayerCharacter.ServerRequestTeleport(DestLocation, DestRotation)`, the server call
the game's own teleporters use. Not tested with a second player yet.

## Live

- **Other players**: `GameStateBase.PlayerArray` → `PawnPrivate`, `PlayerNamePrivate`.
- **Creatures**: `SurvivalGameState.LODActorManagerComponent.FullActors` (the LOD actors
  that are real actors, around players), within 150 m. Positions are read from the real
  actor's root component, not asked for: there can be a thousand of them. Most bugs are
  neutral to the player's team until provoked, so a creature counts as hostile when its AI
  is going after a player (`CharacterLODActor.GetBlackboardTargetActor`) or its team
  relationship (`TeamComponent.GetRelationshipToLODActor`) is hostile.
- **Buggy**: the player's `PetMasterComponent` (`CurrentPetFull`, health state, behavior).
  Orders: call over (`ServerWhistle`), follow me and roam, and send home
  (`ServerTrySendPetHome`). Follow me and roam are the game's only two behaviors
  (`EActivePetBehaviorType`); it can only cycle them (`ServerCycleActivePetBehavior`), so
  an order cycles once unless the buggy already does it.
- **Death and backpack**: dead players have no character until they respawn, so the last
  snapshot is kept with `dying`. The backpack is the `DroppedBackpack` HUD marker for the
  player, or a `LootBagLODActor` of type `PlayerBackpack` with their `PlayerGuid`.
- **Markers**: `HUDMarkerManagerComponent` holds every marker the game's map shows. Other
  players' waypoints and marker buildings come through as markers.
- **Pings**: `SurvivalPlayerController.ServerAddWaypoint`: the player's own waypoint, which
  everyone in the world sees. The game has one waypoint per player, so the player's pins
  can't all go on its map (`game_map` is off).
- **Chat**: `ChatBoxWidget.IsChatBoxOpen` keeps hotkeys from firing while typing. The
  widget is looked for a slice of the object table per tick. A world hosted for
  multiplayer has one even with nobody else in it; single player has none.

## Routes

Unreal's navigation system (`NavigationSystemV1.FindPathToLocationSynchronously`), retried
from the ground when a pin or map click is off the navigation mesh. Route checks and ground
probes use `KismetSystemLibrary` line and sphere traces against world static and dynamic
objects.

## Cost

Measured walking in the park with a release agent: a tick takes 0.04 ms at the median,
0.6 ms at the 95th percentile and about 1.6 ms at the 99th (snapshots at 30 Hz, the slow
reads every half second).

## Testing

A throwaway save, `testworld` (Mild, then Medium), made through the menus; synthetic clicks
reach them. Tested in the game: live location, camera, teleport (including into unloaded
ground), map alignment, world detection, clock, creatures (hostile ones too, for the
danger alert), the buggy and its orders, waypoint pings, routes, route checks, death state,
the dropped backpack, chat (hosting alone) and the world export. Not tested: anything that
needs a second player, such as a client's teleport.
