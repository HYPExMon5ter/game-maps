# Stranded Deep

Unity 2021.2, **Mono** (game code in `Stranded_Deep_Data/Managed/Assembly-CSharp.dll`, with
`Beam.Game` and the game modes in `Assembly-CSharp-firstpass.dll`), Steam app 313120. No
anti-cheat. The game is `Stranded_Deep.exe`; `Stranded_Deep_x64.exe` is only the launcher
that picks the graphics API and starts it. Tested on Direct3D 11.

The provider is `crates/agent/src/provider/stranded_deep.rs`.

## World

A world is 49 zones placed from its seed (`World.CreateWorldZonePoints`): points at least
500 m apart in a 4375 m square around the origin. Zone 0, the starting island, is at the
origin. Four zones are the game's own maps (`Data/Default Maps`): three boss arenas and the
carrier; the rest are generated islands. Between zones the sea has no floor.

`game.json` projects the map onto 4800 m around the origin. Each zone has a `Terrain` of
250 m with a 257-sample heightmap, positioned 100 m below the sea and 150 m tall, and every
zone's heights are applied when the world loads, visited or not. The agent reads them all
with `TerrainData.GetHeights` (about 10 ms for the whole world, once) and
`provider/stranded_deep/terrain.rs` draws 4096 pixels per side from them, with hillshade:
sand at the shore, then grass, forest and bare rock on cliffs and rocky islands, and
shallow water lighter over reefs. `biomes.bin` has land, shallows (to 6 m deep), open sea
(to 60 m) and deep sea.

The sea is at y = 0. `StrandedWorld.GetHeight` is the ground anywhere, loaded or not.

## World and session ids

- Single player and hosting: `slot<n>-<seed>`, from `Options.GeneralSettings.LastSaveSlotUsed`
  and `StrandedWorld.LOCAL_WORLD_SEED` (the server peer's `WorldSeed` when hosting). A
  single-player and a hosted game in the same slot share the slot's world, so they share
  the map.
- Joined: `joined-<seed>-<hash of the host's name>`.
- The session is `BoltMatchmaking.CurrentSession`'s token (`MPExtensions.GetToken`), whose
  `SessionId` is a GUID the host makes and every client receives.

## Places

Islands are named by three terms (`MAP_NAME_PREFIX_3 MAP_NAME_STEM_4 MAP_NAME_SUFFIX_2`),
localized through `Localization.GetLanguageHandler().Localize`. An island's pin is the
land nearest the zone's middle.

The arenas are always the same maps, checked in the game: `INTERNAL_MISSION_0` is the Great
Abaia, `_1` Megalodon, `_2` Lusca the Great, and `_3` the carrier.

`Zone.HasVisited` is the game's own record of islands landed on, so the snapshot's `visited`
marks those pins found and `explored.bin` clears the fog around them, from before Game Maps
too.

## Remembered things

A zone's objects are only generated the first time it loads, and only exist while it's
loaded (`Zone.Loaded`). Everything saved in a zone is a child of its `SaveContainer`, named
after its prefab (`SHIPWRECK_3A(Clone)`). `provider/stranded_deep/scan.rs` reads 40
children every 100 ms, so a zone of a few hundred objects takes a second or two at about
60–170 µs a step, and each loaded zone again every 20 seconds. `objects.rs` sorts the
prefab names; things of a kind close together are one pin. The full list of prefab names is
in `resources.assets`:

```sh
grep -a -o -E "Prefabs/[A-Za-z0-9_/ ]+" Stranded_Deep_Data/resources.assets | sort -u
```

## Creatures

Searching the scene for creatures (`FindObjectsOfType`) took 4 ms, too much twice a second,
so they come from what the game keeps:

- Sharks, whales and marlin register in `PiscusInspectionManager.piscuses`.
- Each land animal belongs to a `LandAnimalSpawner` (`_activeCreature`) in its zone; the
  scanner keeps the spawners it passes.
- Bosses aren't on either list: while the player is in an arena, the scene is searched for
  one every 3 seconds until it's found.

That's about 45 µs a read. Whales, marlin and land crabs are animals, sea creatures that
can't attack are too, and with the world's wildlife set to `Passive` (`Peer._wildlife`)
everything but bosses is.

Objects found once and kept (the world, the time, the boss, the spawners) are held as weak
GC handles (`unity_runtime::WeakRef`). A raw pointer kept across frames outlived a spawner
whose zone unloaded, and reading it crashed the game.

## Teleport

The game's own `DeveloperMode.Teleport` sets the position and turns on fly mode. The
provider does what loading a save does instead: `Movement.ResetChecks()` (off rafts,
ladders and ledges), `_fallStartLevel` set to the destination so the arrival isn't a fall,
the position set, then `Physics.SyncTransforms`. Over water it arrives at the surface.

Refused while in a vehicle, while knocked out, and while `Player.CurrentMissionGoal` is
set: `MissionControllerBase` only ends a fight when it sees the player 80 m away inside the
arena's zone, so after a teleport the fight would never end. Tested into all three arenas,
onto the carrier, onto islands, into open sea and over a wreck 15 m down.

## Time and weather

`GameTime._hours` and `_minutes`; `GameCalendar._daysElapsed` is the days survived.
`AtmosphereStorm._currentWeatherEvent` is a `WeatherEvent` named `Weather Event - Rain`,
shown while it's `_hasStarted`. Wind is left out: it has a direction (`Beam.Wind`), but
sails push the raft forward whatever it is.

## Death

Being knocked out (`Statistics._isKnockedOut`) is dying: in co-op a partner can revive you,
and with everyone down it's game over. There's nothing dropped to mark.

## The game's own map

There isn't one, and no ping, so `gameMap` is off.

## Input

Running in the background isn't set: an unfocused single-player game stops updating, which
the app sees as a hung game for a moment. Hosted games keep running.

## Testing

- Saves are in `%USERPROFILE%\AppData\LocalLow\Beam Team Games\Stranded Deep\Data\Slot0..3`
  (menu slots 1 to 4); `Options.json` has the slot in use. A new random world asks before
  replacing the slot's save.
- Synthetic clicks work in the menus on the main monitor. Return at the title screen opens
  the menu; pressed twice it goes on into **New Game** for the current slot.
- The intro can't be skipped: walk forward in the plane and it plays through to the raft.
- Wildlife **Passive** keeps animals from attacking; bosses still kill.
- Crash stacks in `Player.log` resolve with `cdb -z target/release/game_maps_agent.dll -y
  target/release -c "ln game_maps_agent+0x<offset>"`.
- `STRANDED_DEEP_DIR=<game> cargo test -p game-maps-agent stranded_deep -- --ignored`
  resolves every member against the game's assemblies.
