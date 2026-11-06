# Valheim

Unity, **Mono** (managed assemblies in `valheim_Data/Managed`, game code in
`assembly_valheim.dll`), Steam app 892970. No anti-cheat. Runs on Direct3D 11, or Vulkan
with `-force-vulkan`; the overlay works on both.

## World

Worlds are generated from a seed, so no map pack can have them. `game.json` sets
`worldMaps`: the launcher doesn't look for a pack, and each world's map comes from the
running game the first time it's loaded (`crates/agent/src/provider/valheim.rs`,
`apps/overlay/src-tauri/src/worldmaps.rs`).

- The world is a disc of radius 10500 m around (0, 0). `Minimap` keeps it in square
  textures of 2048 pixels at 12 m each, 24576 m a side: `game.json` projects the map onto
  exactly that, and the agent refuses other sizes.
- `Minimap.m_mapTexture` is the biome colors, `m_forestMaskTexture` a mask (red: forest,
  blue: Ashlands lava and sea), `m_heightTexture` the height (16-bit floats). Row 0 is the
  south. They exist once `m_hasGenerated` is set. `provider/valheim/terrain.rs` turns them
  into a picture: the game does this in a shader we don't use.
- Sea level is 30 m. Swamps sit around it, so swamp a little under it is still drawn as
  swamp, or it comes out as speckles.
- `m_explored` and `m_exploredOthers` are `BitArray`s of a bit per texture pixel, the same
  layout as the app's fog at 12 m cells, so what the player explored before Game Maps
  merges straight in.

## Locations

`ZoneSystem.GetLocationList()` has every location, but only on the host. A client only
gets `m_locationIcons`: bosses, traders and the like. So a player who joins someone else's
world gets the map, but only those pins.

A host sends its locations to players who join, through Play Together (`locations` in
`lib/sync/messages.ts`), so they get the same pins.

`provider/valheim/locations.rs` sorts prefabs into pin categories. To list every prefab:

```sh
grep -a -o -E "[A-Za-z0-9_/ .()-]+\.prefab" valheim_Data/StreamingAssets/SoftRef/manifest | grep -i location
```

Each trader has about 10 possible spots. When one is placed the game removes the others
(`m_unique`), so location counts drop during play: the app reads pins again when the map
opens, at most once a minute.

A new world reports itself minutes before its locations exist, so nothing is read until
the player has spawned.

## Dungeons

Interiors are about 5000 m up, above the entrance, so the map shows the player at the right
spot and only `y` says they're inside. `Character.InInterior()` is true above 3000 m.

Pins are the entrances. Teleporting into and out of interiors was tested on Direct3D 11 and
Vulkan for Burial Chambers, Sunken Crypts, Troll and Frost Caves, Infested Mines, the
Howling Cavern, Smouldering Tomb, Charred Fortress and Fortress Ruins: the player arrives
where they should, the game counts them as sheltered, and they can leave by pin or map
click. A door's real destination is `Teleport.GetTeleportPoint()`, a meter past its target:
on the target itself the player is outside the room.

## Teleport

`Player.TeleportTo(pos, rot, distantTeleport: true)` is what portals call: it fades, waits
for the area to load, and lands safely. It refuses during its cooldown and while another
teleport runs, and the game's timer stops while single player is paused.

Height comes from `WorldGenerator.GetHeight`, which works anywhere, loaded or not.
Positions in dungeons keep their height. Teleports are refused while dead or attached to a
ship or seat.

## Multiplayer

`ZNet.m_world` has `m_uid`, `m_seed` and `m_name` on clients too, so the world id and the
session (which only players in the world can know) are the same for everyone.

Players come from `Player.GetAllPlayers()` when loaded, and `ZNet.GetPlayerList()` otherwise,
which only has a position for players who chose to share theirs.

## The game's own map

Pins placed in the app go on the game's map with `Minimap.AddPin`, saved with the character
like any other pin. Other players only get them through a cartography table, which is how
Valheim shares pins. `Chat.SendPing` is the game's ping: it reaches everyone in the world,
with or without Game Maps.

Going the other way, the game's pins, beds, boss locations and pings show as in-game
markers. `PinType.Death` pins are graves.

## Input

The game reads input through Unity's Input System, not the legacy `Input` API. The agent
hides the map key from window messages and raw input, which is enough to keep the game's
own map closed. When testing, synthetic mouse clicks don't reach Valheim's menus, but keys do.

The game's mini-map is hidden while the app's is on, through `Minimap.m_smallRoot`, which
is also where its wind arrow lives: the app's mini-map has its own, from `EnvMan`. The game
shows its mini-map again by itself, so this is applied once a second, and undone when the
app leaves (`Provider::app_left`).

## Remembered resources

Ore, plants, ships and carts aren't locations: the game only places them as an area
generates. `provider/valheim/scan.rs` reads `ZDOMan.m_objectsBySector` around the player, one
64 m sector every 200 ms, with field reads only. Plants and portals need calls into the game
for their state. That cost about 35 µs on average and at most 0.5 ms while exploring. The
app keeps the result by area in `resources.json`, so what's mined away goes from the map.

Things of a kind within 24 m are one pin. Silver is `hidden` until the player passes it
with a wishbone on, and stays found. Portals are kept apart from other objects
(`ZDOMan.GetPortals()`), and a host has all of them.

Prefab names can be listed from `ZNetScene.m_prefabs` at runtime: most aren't in the manifest.

## Testing

Console cheats need `confirmcheats`, which flags the character for good
(`PlayerProfile.m_usedCheats`): use a throwaway one. Don't move or resize the game's window
before its main menu is up: Unity saves it as a 1x1 resolution, and the next launch hangs.

## Routes

The game only builds navigation around what's loaded, so a route could only reach a few
hundred meters. `find_path` is refused and the app shows the straight line and compass.
