# How Wand (formerly WeMod) implements interactive maps

Research notes from static analysis of the locally installed Wand desktop app
(v12.68.0, `%LOCALAPPDATA%\WeMod\app-12.68.0\resources\app.asar`) and the public
map pages at `wand.com/maps`. Analyzed 2026-11-02.

## Architecture at a glance

```
 ┌──────────────── Wand desktop app (Electron + Aurelia) ────────────────┐
 │                                                                       │
 │  maps-window / mini-map (overlay)                                     │
 │   └─ <iframe src="https://wand.com/maps/{titleSlug}/{mapSlug}?...">   │
 │          ▲  postMessage RPC (rpcType=parent)                          │
 │          │  get_player_coordinates / set_player_coordinates /         │
 │          │  player_location_update / select_pin / ...                 │
 │  PlayerLocationService (polls 1s, push stream preferred)              │
 │   └─ Trainer.getPlayerCoordinates() / setPlayerCoordinates()          │
 │        ├─ new:    Lua "contract" wand.player.location.v1 (gRPC-ish)   │
 │        └─ legacy: trainer variables wm_coord_get/x/y/z/set            │
 └──────────────────────────────┬────────────────────────────────────────┘
                                │ IPC
         TrainerHost_x64.exe → injects TrainerLib/CELib + LuaExecutor +
         ContractRuntimePlugin into the game process
```

The map itself is not part of the desktop app. It is a Next.js + MapLibre web app
served at `wand.com/maps/...`, embedded in an iframe. The desktop app only adds
a live position feed and teleport, through a small RPC bridge.

## 1. The map renderer (web)

- **URL builder** (`app-3d62b112` module `31320`):
  `{mapsBaseUrl}/{lang?}/maps/{titleSlug}/{mapSlug}?hostType=overlay&hostMode=embed&rpcType=parent[&isOverlayMiniMapMode=true]`.
  A `localPreview` mode loads map data from a local URL (`#localMapDataUrl=...`).
  Wand's own map editor uses it.
- **Stack**: Next.js (`/maps/_next/...`) and MapLibre GL.
- **Tiles**: raster TMS pyramid, e.g.
  `https://storage-cdn.wemod.com/maps/<uuid>/{z}/{x}/{y}.webp`, with
  `minZoom 1`, `maxZoom 6`, `tmsEnabled: true`, plus a background color.
- **Pins**: GeoJSON-style entries with `lngLat`, `categoryExternalId`, name,
  description, image, and optional video clip. The SotF categories are Animals,
  Collectibles, Miscellaneous, Plants, Points of Interest, Supplies, and
  Weapons and Armor.
- **Regions**: a GeoJSON `FeatureCollection` of polygons with style properties.
- **Map list API**: `GET https://api.wemod.com/v3/game_maps?title_id=…&locale=…`.
  Each map has a `flags` bitmask:
  `HasGameCoordinates=1`, `SupportsLiveLocation=2`, `SupportsTeleport=4`.
- **Per-user state** (found pins, filters) syncs over
  `wss://…/maps/api/map-preferences/ws`. Settings are also stored locally per
  `mapExternalId` and `titleId`.

## 2. Game ↔ map coordinate transform (module `9554`)

The map image is laid out in Web Mercator "pseudo-geo" space. Each map stores a
linear transform called `playerLocationDeltas` and keeps 2+ reference points as
the source of truth for fitting it:

```json
"playerLocationDeltas": {
  "deltas": {
    "scaleX": 0.00009985002314663676, "scaleY": 0.00009979344088894703,
    "offsetX": -0.7788012495952898,   "offsetY": -0.26588750956676677,
    "gameCoordsMap": { "x": "x", "y": "z", "z": "y" }
  },
  "referencePoints": [
    { "lngLat": {"lat": 41.3788, "lng": 163.2523},
      "gameCoords": {"x": 1813.81, "y": 18.44, "z": 505.53} },
    { "lngLat": {"lat": -81.8731, "lng": -114.0893},
      "gameCoords": {"x": -1268.91, "y": 215.19, "z": -1683.49} }
  ]
},
"supportsTeleport": true
```

(These are the real Sons of the Forest values. Unity is Y-up, so map "north" is
game `z`.)

```
R = 6371000
// game → lngLat
mx = (game[map.x] - offsetX) / scaleX
my = (game[map.y] - offsetY) / scaleY
lng = mx / R * 180/π
lat = atan(sinh(my / R)) * 180/π

// lngLat → game (teleport target; z/height comes from the pin or current player height)
mx = R * lng·π/180
my = R * ln(tan(π/4 + lat·π/360))
game[map.x] = mx * scaleX + offsetX
game[map.y] = my * scaleY + offsetY
```

Other helpers: the haversine distance in map space, the bearing between two
lngLats, and a sign check (`scaleX*scaleY < 0`) that flips left/right for
mirrored maps.

## 3. Live location (desktop side)

- `PlayerLocationService` (module `64472`) is a singleton:
  - Its preferred source is a **push stream** (`onPlayerLocationUpdate`). As a
    fallback it **polls** `getPlayerCoordinates()` every 1 s (cached for
    750 ms). A pushed sample counts as fresh for 2.5 s.
  - It forwards samples to the iframe as `player_location_update`.
  - It derives heading from the camera facing vector when the game provides
    one. Otherwise it averages the last 4 movement deltas, weighted by
    distance.
  - It drops samples that jump ≥100 m at >250 m/s, so teleports and loading
    screens don't corrupt the heading.
  - It also drives the "compass" widget, which shows distance and a 3D arrow
    toward a target pin.
- **Location contract**, embedded in the app as proto source:

```proto
package wand.player.location.v1;
service Location {
  rpc Get(GetRequest) returns (GetResponse);
  rpc Watch(WatchRequest) returns (stream Position);
  rpc Set(SetRequest) returns (SetResponse);       // teleport
}
message Position {
  Vector3 coords = 1;                   // raw game coords
  Vector3 camera_facing_direction = 2;
  double  units_per_meter = 3;          // Unreal default 100, Unity 1
  Vector3 camera_position = 4;
}
```

  The contract is implemented by a **per-game Lua artifact**, downloaded with
  the trainer and run inside the game by `LuaExecutorPlugin` and
  `ContractRuntimePlugin`.
- **Legacy path**: the Cheat-Engine-based trainer exposes magic variables.
  To read, the app writes `wm_coord_get=-1`, the trainer sets it to `1` and
  fills `wm_coord_x/y/z`. To teleport, the app writes `wm_coord_x/y/z` and then
  `wm_coord_set=-1`. Success shows up as `wm_coord_set=1`.

## 4. Teleport flow

The user clicks a pin's "teleport" button. The map converts lngLat to game
coordinates with the inverse transform, then iframe RPC `set_player_coordinates`
→ `trainer.setPlayerCoordinates({x,y,z})` → contract `Set`, or the legacy vars.
The button only appears when the map has `SupportsTeleport` and a trainer is
attached.

## Takeaways for our design

1. **Split it the same way**: a game-agnostic map viewer, plus per-game
   *location providers* behind one small interface (`get`, `watch`, `set`).
   Wand's proto is a good model for that interface.
2. **Per-map data file** = tile source + transform (fitted from reference points)
   + categories + pins + regions + capability flags.
3. **Store the reference points and fit the transform from them.** Don't
   hand-tune the scale and offset values.
4. Wand's tiles, pins, and media are Wand's proprietary content. We need our own
   map imagery and POI data (game-extracted, self-captured, or properly licensed).
