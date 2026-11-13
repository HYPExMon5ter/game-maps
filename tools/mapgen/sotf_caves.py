"""Render the Sons of the Forest underground map layer from in-game cave scans.

The agent's world export (overlay app: Settings -> Diagnostics -> Export world data)
includes a floor-height scan of every cave and bunker. This draws them, top-down and
aligned with the surface map, into `games/sons-of-the-forest/tiles/caves`: walkable
floors colored by height within their cave (warm at the top, cool at the bottom) with
contour lines every few meters, dark walls around them, transparent everywhere else.

It also writes each cave's walkable floors as a navigation grid (`nav/caves.json` plus
`nav/cave-<area>.bin`, `nav/cave-<area>-water.bin` marking the water and
`nav/cave-<area>-dive.bin` the water under rock), which the app routes on inside caves:
the game's own pathfinding doesn't cover them.

Cells the scan found nothing in are filled from the floor scan `sotf_3d.py` makes of each
cave's collision (`build/floors-*.npz`; the game caps a scan's size and can miss parts
of big caves). Water comes from the cave water surfaces and volumes `sotf_3d.py` writes
(`build/`; run it first). Floors under open water become swimmable at the surface's
height, so routes cross lakes and pools, and the map shows the water. Floors inside a
water volume with rock above are swum along underwater (dives). The climbing ropes and
ziplines from `build/traversal.json` join the floors they lead between (`links` in
`caves.json`).

Usage:
    python -I tools/mapgen/sotf_caves.py --out games/sons-of-the-forest EXPORT.json [EXPORT.json ...]
"""
from __future__ import annotations

import argparse
import base64
import glob
import json
import os
import sys

import numpy as np
from PIL import Image

# Shares the map layout and tile writer with the surface generator (`python -I` leaves
# the script's directory off the import path).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sotf import TERRAIN_ORIGIN, hillshade, write_tiles  # noqa: E402
import mesh3d  # noqa: E402

# World square covered by the map (must match `projection` in game.json).
WORLD_MIN = TERRAIN_ORIGIN[0]
WORLD_SIZE = 4000.0
GRID = 4000  # 1 m cells
IMAGE_SIZE = 8192
NONE = -32768
# A floor needs this much headroom (decimeters) to be walkable; less is the top of
# solid rock under the terrain.
MIN_HEADROOM_DM = 16
# Cells with fewer valid neighbors (of 8) are scan noise.
MIN_NEIGHBORS = 3
WALL_WIDTH = 2  # cells

# Floor colors from the top of a cave to its bottom.
FLOOR_HIGH = np.array((232, 210, 168), np.float32)
FLOOR_MID = np.array((196, 160, 118), np.float32)
FLOOR_LOW = np.array((96, 142, 150), np.float32)
WALL = np.array((24, 19, 17), np.float32)
CONTOUR_STEP = 2.5  # meters
WATER = np.array((70, 140, 196), np.float32)
WATER_OPACITY = 0.6
# Floors this far (meters) under a water surface are in it; deeper is another level
# below. Shallower than SHALLOW is wading: walked as it is.
MAX_DEPTH = 40.0
SHALLOW = 0.4
# Headroom is measured up to this height, so more means open to at least there.
CLEARANCE_CAP_DM = 120
# Underwater, a swimmer needs less room than a walker, and is under once the water is
# over their head.
SWIM_HEADROOM_DM = 8
HEAD_UNDER = 1.7
# A volume with a single face over a cell (no bottom there) reaches down to its lowest
# point; faces this close together are one.
SINGLE_FACE = 0.3
# Where a rope's or zipline's end is, relative to the floor it starts from (meters), and
# how far around it to look for that floor.
ROPE_GRAB = 1.0
ZIPLINE_GRAB = 0.0
# A zipline ride stops this far short of the low anchor (meters, along the line), and
# the rider drops onto the floor under that spot (seen in the game: Cave B's).
ZIPLINE_STOP = 1.5
DROP_REACH = 1.5
# Horizontally: a ledge's edge often has no floor in the scan (too little headroom
# under the overhang, or a lone cell dropped as noise).
LINK_REACH = 5.0
LINK_SLACK = 3.0  # vertically, from where the floor is expected


def neighbors(mask: np.ndarray) -> np.ndarray:
    """Count of set cells among the 8 neighbors."""
    p = np.pad(mask.astype(np.int8), 1)
    h, w = mask.shape
    return sum(p[1 + dy : 1 + dy + h, 1 + dx : 1 + dx + w] for dy in (-1, 0, 1) for dx in (-1, 0, 1) if dy or dx)


def dilate(mask: np.ndarray, steps: int) -> np.ndarray:
    out = mask
    for _ in range(steps):
        out = out | (neighbors(out) > 0)
    return out


def walkable_floor(scan: dict) -> np.ndarray:
    """Highest floor with headroom per cell (meters, NaN where none), row 0 = south."""
    w, h, layers = scan["width"], scan["height"], scan["layers"]
    floors = np.frombuffer(base64.b64decode(scan["floors"]), "<i2").reshape(layers, h, w)
    room = np.frombuffer(base64.b64decode(scan["clearance"]), "<i2").reshape(layers, h, w)
    out = np.full((h, w), np.nan, np.float32)
    for layer in range(layers - 1, -1, -1):
        ok = (floors[layer] != NONE) & (room[layer] >= MIN_HEADROOM_DM)
        out = np.where(ok, floors[layer] / 10.0, out)
    # Drop speckles, then fill single-cell gaps from their neighbors.
    valid = ~np.isnan(out)
    valid &= neighbors(valid) >= MIN_NEIGHBORS
    out = np.where(valid, out, np.nan)
    gaps = ~valid & (neighbors(valid) >= 5)
    if gaps.any():
        padded = np.pad(np.nan_to_num(out), 1)
        count = np.pad(valid.astype(np.float32), 1)
        total = sum(padded[1 + dy : 1 + dy + h, 1 + dx : 1 + dx + w] for dy in (-1, 0, 1) for dx in (-1, 0, 1) if dy or dx)
        n = sum(count[1 + dy : 1 + dy + h, 1 + dx : 1 + dx + w] for dy in (-1, 0, 1) for dx in (-1, 0, 1) if dy or dx)
        out = np.where(gaps, total / np.maximum(n, 1), out)
    return out


def compose(scans: list[dict]) -> tuple[np.ndarray, np.ndarray]:
    """All caves on the world grid, row 0 = south: floor heights (meters, NaN outside
    caves) and each cell's height within its cave (0 = cave bottom, 1 = top)."""
    world = np.full((GRID, GRID), np.nan, np.float32)
    relative = np.full((GRID, GRID), np.nan, np.float32)
    cell = WORLD_SIZE / GRID
    for scan in scans:
        floor = walkable_floor(scan)
        h, w = floor.shape
        lo, hi = (np.nanpercentile(floor, 2), np.nanpercentile(floor, 98)) if (~np.isnan(floor)).any() else (0.0, 1.0)
        rel = np.clip((floor - lo) / max(hi - lo, 1.0), 0, 1)
        # Cell centers -> world grid indices.
        xs = np.floor((scan["origin_x"] + (np.arange(w) + 0.5) * scan["cell"] - WORLD_MIN) / cell).astype(int)
        zs = np.floor((scan["origin_z"] + (np.arange(h) + 0.5) * scan["cell"] - WORLD_MIN) / cell).astype(int)
        cols, rows = np.meshgrid(xs, zs)
        inside = (cols >= 0) & (cols < GRID) & (rows >= 0) & (rows < GRID) & ~np.isnan(floor)
        r, c, v = rows[inside], cols[inside], floor[inside]
        # Where caves overlap (stacked levels), keep the higher floor.
        current = world[r, c]
        higher = np.isnan(current) | (v > current)
        world[r, c] = np.where(higher, v, current)
        relative[r, c] = np.where(higher, rel[inside], relative[r, c])
    return world, relative


def render(world: np.ndarray, relative: np.ndarray, water: np.ndarray | None = None) -> Image.Image:
    floor = ~np.isnan(world)
    heights = np.where(floor, world, 0.0)
    # Relief from the floor itself; fill outside with nearby heights so walls don't
    # read as cliffs in the shading.
    shade = hillshade(np.where(floor, heights, np.nanmean(world) if floor.any() else 0.0), WORLD_SIZE / GRID)
    rgba = np.zeros((GRID, GRID, 4), np.float32)
    if floor.any():
        # Height within each cave: top warm, middle tan, bottom cool, so levels and
        # slopes read at a glance.
        t = np.nan_to_num(relative, nan=0.5)[..., None]
        upper = FLOOR_MID + (FLOOR_HIGH - FLOOR_MID) * np.clip(t * 2 - 1, 0, 1)
        lower = FLOOR_LOW + (FLOOR_MID - FLOOR_LOW) * np.clip(t * 2, 0, 1)
        base = np.where(t >= 0.5, upper, lower)
        lit = base * (0.62 + 0.5 * shade[..., None])
        # Contour lines where the floor crosses a multiple of CONTOUR_STEP.
        level = np.floor(heights / CONTOUR_STEP)
        edge = np.zeros(level.shape, bool)
        edge[:, 1:] |= level[:, 1:] != level[:, :-1]
        edge[1:, :] |= level[1:, :] != level[:-1, :]
        lit = np.where((edge & floor)[..., None], lit * 0.72, lit)
        rgba[floor, :3] = lit[floor]
        rgba[floor, 3] = 255
        # Water over the floor, tinted so it shows where you'd swim.
        if water is not None:
            wet = floor & ~np.isnan(water) & (water > np.where(floor, world, 0) + SHALLOW)
            rgba[wet, :3] = rgba[wet, :3] * (1 - WATER_OPACITY) + WATER * WATER_OPACITY
    wall = dilate(floor, WALL_WIDTH) & ~floor
    rgba[wall, :3] = WALL
    rgba[wall, 3] = 245
    img = Image.fromarray(np.clip(rgba, 0, 255).astype(np.uint8)[::-1], "RGBA")  # north up
    return img.convert("RGBa").resize((IMAGE_SIZE, IMAGE_SIZE), Image.BILINEAR).convert("RGBA")


def load_water(game_dir: str) -> list[np.ndarray]:
    """Every cave water surface `sotf_3d.py` found, as triangles (n, 3 corners, xyz)."""
    out = []
    for path in sorted(glob.glob(os.path.join(game_dir, "build", "water-*.bin"))):
        with open(path, "rb") as f:
            p, _, faces = mesh3d.decode_mesh(f.read())
        out.append(p[faces])
    return out


def rasterize_surfaces(triangles: list[np.ndarray], origin_x: float, origin_z: float, cell: float, width: int, height: int) -> np.ndarray:
    """Water surface height over each cell center (meters, NaN where there's none; the
    highest where surfaces overlap). Rows south first, like the cave grids."""
    return _rasterize(triangles, origin_x, origin_z, cell, width, height, np.fmax)


def _rasterize(triangles: list[np.ndarray], origin_x: float, origin_z: float, cell: float, width: int, height: int, combine) -> np.ndarray:
    """Heights of the triangles over each cell center, `combine`d where several are over
    one (NaN where there's none)."""
    out = np.full((height, width), np.nan, np.float32)
    for tris in triangles:
        for t in tris:
            xs, zs = (t[:, 0] - origin_x) / cell - 0.5, (t[:, 2] - origin_z) / cell - 0.5
            c0, c1 = max(int(np.ceil(xs.min())), 0), min(int(np.floor(xs.max())), width - 1)
            r0, r1 = max(int(np.ceil(zs.min())), 0), min(int(np.floor(zs.max())), height - 1)
            if c0 > c1 or r0 > r1:
                continue
            cs, rs = np.meshgrid(np.arange(c0, c1 + 1), np.arange(r0, r1 + 1))
            # Barycentric test of the cell centers against the triangle (x, z).
            (ax, bx, cx), (az, bz, cz) = xs, zs
            d = (bz - cz) * (ax - cx) + (cx - bx) * (az - cz)
            if abs(d) < 1e-12:
                continue
            l1 = ((bz - cz) * (cs - cx) + (cx - bx) * (rs - cz)) / d
            l2 = ((cz - az) * (cs - cx) + (ax - cx) * (rs - cz)) / d
            inside = (l1 >= -1e-6) & (l2 >= -1e-6) & (1 - l1 - l2 >= -1e-6)
            if not inside.any():
                continue
            y = l1 * t[0, 1] + l2 * t[1, 1] + (1 - l1 - l2) * t[2, 1]
            r, c = rs[inside], cs[inside]
            out[r, c] = combine(out[r, c], y[inside])
    return out


def dive_floors(floors: np.ndarray, room: np.ndarray, under: np.ndarray, volumes: list[np.ndarray], origin_x: float, origin_z: float, cell: float) -> np.ndarray:
    """Which nodes (layers x rows x columns) are swum underwater: floors inside a water
    volume, deep enough to be under, that aren't open water (`under`: swum at the top)."""
    layers, h, w = floors.shape
    has = (floors != NONE) & (room >= SWIM_HEADROOM_DM) & ~under
    y = floors / 10.0
    dive = np.zeros(floors.shape, bool)
    for tris in volumes:
        top = _rasterize([tris], origin_x, origin_z, cell, w, h, np.fmax)
        if np.isnan(top).all():
            continue
        bottom = _rasterize([tris], origin_x, origin_z, cell, w, h, np.fmin)
        bottom = np.where(top - bottom < SINGLE_FACE, tris[..., 1].min(), bottom)
        dive |= has & (y + HEAD_UNDER <= top[None]) & (y >= bottom[None] - HEAD_UNDER)
    return dive


def snap(floors: np.ndarray, scan: dict, p, below: float) -> int | None:
    """Node of the floor `below` meters under the world point `p` (within LINK_REACH
    across and LINK_SLACK up or down), nearest first; None if there's none."""
    layers, h, w = floors.shape
    cell = scan["cell"]
    c0, r0 = (p[0] - scan["origin_x"]) / cell - 0.5, (p[2] - scan["origin_z"]) / cell - 0.5
    reach = int(np.ceil(LINK_REACH / cell))
    best = None
    for r in range(int(round(r0)) - reach, int(round(r0)) + reach + 1):
        for c in range(int(round(c0)) - reach, int(round(c0)) + reach + 1):
            if not (0 <= r < h and 0 <= c < w):
                continue
            across = np.hypot(r - r0, c - c0) * cell
            if across > LINK_REACH:
                continue
            for layer in range(layers):
                f = floors[layer, r, c]
                dy = abs(f / 10.0 - (p[1] - below))
                if f == NONE or dy > LINK_SLACK:
                    continue
                score = across + dy
                if best is None or score < best[0]:
                    best = (score, (layer * h + r) * w + c)
    return best[1] if best else None


def drop_onto(floors: np.ndarray, scan: dict, p) -> int | None:
    """Node of the floor someone at the world point `p` drops onto: the highest one under
    them, the nearest across first (within DROP_REACH, else LINK_REACH)."""
    layers, h, w = floors.shape
    cell = scan["cell"]
    c0, r0 = (p[0] - scan["origin_x"]) / cell - 0.5, (p[2] - scan["origin_z"]) / cell - 0.5
    for reach in (DROP_REACH, LINK_REACH):
        best = None
        n = int(np.ceil(reach / cell))
        for r in range(int(round(r0)) - n, int(round(r0)) + n + 1):
            for c in range(int(round(c0)) - n, int(round(c0)) + n + 1):
                across = np.hypot(r - r0, c - c0) * cell
                if not (0 <= r < h and 0 <= c < w) or across > reach:
                    continue
                for layer in range(layers):
                    f = floors[layer, r, c]
                    if f == NONE or f / 10.0 >= p[1]:
                        continue
                    key = (round(across), -f)
                    if best is None or key < best[0]:
                        best = (key, (layer * h + r) * w + c)
        if best:
            return best[1]
    return None


def links(floors: np.ndarray, scan: dict, traversal: dict) -> list[dict]:
    """The ropes (climbed both ways) and ziplines (ridden downhill) whose ends are both
    on this grid, as node pairs."""
    out = []
    for rope in traversal["ropes"]:
        top, bottom = snap(floors, scan, rope["top"], ROPE_GRAB), snap(floors, scan, rope["bottom"], ROPE_GRAB)
        if top is not None and bottom is not None and top != bottom:
            out += [{"from": bottom, "to": top, "kind": "rope"}, {"from": top, "to": bottom, "kind": "rope"}]
    for line in traversal["ziplines"]:
        top, bottom = np.array(line["high"], float), np.array(line["low"], float)
        stop = bottom + (top - bottom) / max(np.linalg.norm(top - bottom), 1e-6) * ZIPLINE_STOP
        high, low = snap(floors, scan, line["high"], ZIPLINE_GRAB), drop_onto(floors, scan, stop)
        if high is not None and low is not None and high != low:
            out.append({"from": high, "to": low, "kind": "zipline"})
    return out


def load_floor_scans(game_dir: str) -> list[dict]:
    """The floor scans `sotf_3d.py` made from each cave's collision (`build/floors-*.npz`)."""
    out = []
    for path in sorted(glob.glob(os.path.join(game_dir, "build", "floors-*.npz"))):
        with np.load(path) as d:
            out.append({k: (d[k] if d[k].ndim else d[k].item()) for k in d.files})
    return out


# A collision floor this close to one the game found (decimeters) is the same floor.
SAME_FLOOR_DM = 5
# Floors kept per cell in the cave grids. The game's scan keeps three, but a stairwell in
# Bunker Residential stacks four flights, and with three the lowest one fell out.
NAV_LAYERS = 4


def fill_from_collision(scan: dict, collision: list[dict]) -> tuple[dict, int]:
    """The in-game scan, grown to the extent of the collision scan that has the most
    floor within it (the game caps a scan's size), with the floors that scan has and the
    game's doesn't added (see `mesh3d.pick_floors`: the game keeps a few floors per
    cell, and the tops of rock between stacked tunnels can crowd a lower tunnel out; the
    game's scan also starts lower near cave entrances), keeping `NAV_LAYERS` floors per
    cell. Also how many were added."""
    scanned, cell = scan["layers"], scan["cell"]
    layers = max(scanned, NAV_LAYERS)
    h, w = scan["height"], scan["width"]
    ox, oz = scan["origin_x"], scan["origin_z"]

    def floor_cells_inside(c):
        xs = c["origin_x"] + (np.arange(c["width"]) + 0.5) * c["cell"]
        zs = c["origin_z"] + (np.arange(c["height"]) + 0.5) * c["cell"]
        inside = ((zs >= oz) & (zs <= oz + h * cell))[:, None] & ((xs >= ox) & (xs <= ox + w * cell))[None]
        return int(((c["floors"] != NONE).any(0) & inside).sum())

    best = max(collision, key=floor_cells_inside, default=None)
    if best is None or floor_cells_inside(best) == 0:
        return scan, 0
    # Grow on the scan's own grid, by whole cells.
    grow_w = int(max(np.ceil((ox - best["origin_x"]) / cell), 0))
    grow_s = int(max(np.ceil((oz - best["origin_z"]) / cell), 0))
    grow_e = int(max(np.ceil((best["origin_x"] + best["width"] * best["cell"] - (ox + w * cell)) / cell), 0))
    grow_n = int(max(np.ceil((best["origin_z"] + best["height"] * best["cell"] - (oz + h * cell)) / cell), 0))
    W, H = w + grow_w + grow_e, h + grow_s + grow_n
    OX, OZ = ox - grow_w * cell, oz - grow_s * cell
    floors = np.full((layers, H, W), NONE, np.int16)
    room = np.full((layers, H, W), NONE, np.int16)
    floors[:scanned, grow_s : grow_s + h, grow_w : grow_w + w] = np.frombuffer(base64.b64decode(scan["floors"]), "<i2").reshape(scanned, h, w)
    room[:scanned, grow_s : grow_s + h, grow_w : grow_w + w] = np.frombuffer(base64.b64decode(scan["clearance"]), "<i2").reshape(scanned, h, w)
    # The collision scan's cell under each cell center.
    cc = np.floor((OX + (np.arange(W) + 0.5) * cell - best["origin_x"]) / best["cell"]).astype(np.int64)
    rr = np.floor((OZ + (np.arange(H) + 0.5) * cell - best["origin_z"]) / best["cell"]).astype(np.int64)
    ok_c, ok_r = (cc >= 0) & (cc < best["width"]), (rr >= 0) & (rr < best["height"])
    cs, rs = np.meshgrid(np.clip(cc, 0, best["width"] - 1), np.clip(rr, 0, best["height"] - 1))
    inside = ok_r[:, None] & ok_c[None]
    their_floors = np.where(inside, best["floors"][:, rs, cs], NONE)
    their_room = np.where(inside, best["clearance"][:, rs, cs], NONE)
    has = floors != NONE
    known = np.zeros(their_floors.shape, bool)
    for layer in range(layers):
        known |= has[layer] & (np.abs(their_floors.astype(np.int32) - floors[layer]) <= SAME_FLOOR_DM)
    new = (their_floors != NONE) & ~known
    picked_floors, picked_room = mesh3d.pick_floors(
        np.concatenate([floors, np.where(new, their_floors, NONE)]),
        np.concatenate([room, np.where(new, their_room, NONE)]),
        layers,
    )
    # The game's floors still there (the collision's differ from them), and so what's new.
    survived = (has[:, None] & (floors[:, None] == picked_floors[None])).any(axis=1)
    added = int((picked_floors != NONE).sum() - survived.sum())
    encode = lambda a: base64.b64encode(a.astype("<i2").tobytes()).decode()  # noqa: E731
    grown = {**scan, "origin_x": OX, "origin_z": OZ, "width": W, "height": H, "layers": layers, "floors": encode(picked_floors), "clearance": encode(picked_room)}
    return grown, added


def load_traversal(game_dir: str) -> tuple[dict, list[np.ndarray]]:
    """Every rope and zipline ({ropes, ziplines}) and water volume `sotf_3d.py` found."""
    build = os.path.join(game_dir, "build")
    found = {"ropes": [], "ziplines": []}
    path = os.path.join(build, "traversal.json")
    if os.path.exists(path):
        with open(path) as f:
            for cave in json.load(f).values():
                for kind in found:
                    found[kind] += cave[kind]
    path = os.path.join(build, "water-volumes.npz")
    volumes = list(np.load(path).values()) if os.path.exists(path) else []
    return found, volumes


def swim_floors(floors: np.ndarray, room: np.ndarray, surface: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Floors (layers x rows x columns, decimeters) with those under open water raised
    to the surface, where a player swims; the rest walkable as before (enough headroom),
    or NONE. Also which nodes are water."""
    top = np.where(np.isnan(surface), np.nan, surface * 10)[None]
    has = floors != NONE
    depth = top - floors
    # Open up to the surface: the headroom reaches it (the water itself may stop the
    # measurement), or was too tall to measure.
    open_above = (floors + room >= top - SHALLOW * 10) | (room >= CLEARANCE_CAP_DM)
    under = has & (depth > SHALLOW * 10) & (depth <= MAX_DEPTH * 10) & open_above
    walkable = has & (room >= MIN_HEADROOM_DM)
    out = np.where(walkable, floors, NONE)
    return np.where(under, np.round(np.nan_to_num(top)), out).astype("<i2"), under


def write_bits(path: str, bits: np.ndarray) -> bool:
    """One bit per node (node order, lowest bit first), if any is set; else no file."""
    if bits.any():
        np.packbits(bits.reshape(-1), bitorder="little").tofile(path)
        return True
    if os.path.exists(path):
        os.remove(path)
    return False


def cave_grids(scan: dict, water: list[np.ndarray], traversal: dict, volumes: list[np.ndarray]) -> dict:
    """A cave's routing grids from its scan: `floors` (layers x rows x columns, decimeters,
    NONE where a cell isn't walkable), which nodes are swum at the surface (`swim`) and
    underwater (`dive`), the ropes and ziplines between nodes (`links`), and the water's
    height per cell (`water_level`, meters, NaN where there's none: the surface, or the
    top of the water volume over a dive)."""
    w, h, layers = scan["width"], scan["height"], scan["layers"]
    ox, oz, cell = scan["origin_x"], scan["origin_z"], scan["cell"]
    floors = np.frombuffer(base64.b64decode(scan["floors"]), "<i2").reshape(layers, h, w)
    room = np.frombuffer(base64.b64decode(scan["clearance"]), "<i2").reshape(layers, h, w)
    surface = rasterize_surfaces(water, ox, oz, cell, w, h)
    walkable, swim = swim_floors(floors, room, surface)
    dive = dive_floors(floors, room, swim, volumes, ox, oz, cell)
    walkable = np.where(dive, floors, walkable).astype("<i2")
    level = surface
    if dive.any():
        volume_top = _rasterize(volumes, ox, oz, cell, w, h, np.fmax)
        level = np.where(np.isnan(surface) & dive.any(axis=0), volume_top, surface)
    return {
        "scan": scan,
        "floors": walkable,
        "swim": swim,
        "dive": dive,
        "links": links(walkable, scan, traversal),
        "water_level": level,
    }


def write_nav(caves: list[dict], out_dir: str) -> None:
    """Each cave's walkable floors for routing in the app: `nav/cave-<area>.bin` holds
    int16 floor heights in decimeters, layers x rows (south first) x columns, with
    NONE where a cell isn't walkable; `nav/caves.json` lists the grids, with the ropes
    and ziplines between their nodes."""
    nav_dir = os.path.join(out_dir, "nav")
    os.makedirs(nav_dir, exist_ok=True)
    index = []
    for cave in caves:
        scan, walkable, swim, dive, cave_links = cave["scan"], cave["floors"], cave["swim"], cave["dive"], cave["links"]
        name = f"cave-{scan['area']}.bin"
        walkable.tofile(os.path.join(nav_dir, name))
        # Water nodes: routes may drop into water from any height, and swim up and
        # down freely underwater (dive nodes).
        water_file = f"cave-{scan['area']}-water.bin"
        dive_file = f"cave-{scan['area']}-dive.bin"
        has_water = write_bits(os.path.join(nav_dir, water_file), swim | dive)
        has_dive = write_bits(os.path.join(nav_dir, dive_file), dive)
        index.append({
            "area": scan["area"],
            "originX": scan["origin_x"],
            "originZ": scan["origin_z"],
            "cell": scan["cell"],
            "width": scan["width"],
            "height": scan["height"],
            "layers": scan["layers"],
            "file": name,
            **({"water": water_file} if has_water else {}),
            **({"dive": dive_file} if has_dive else {}),
            **({"links": cave_links} if cave_links else {}),
        })
        print(f"  cave {scan['area']}: {int(swim.sum())} swim, {int(dive.sum())} dive nodes, {sum(link['kind'] == 'rope' for link in cave_links) // 2} ropes, {sum(link['kind'] == 'zipline' for link in cave_links)} ziplines")
    with open(os.path.join(nav_dir, "caves.json"), "w") as f:
        json.dump({"version": 1, "caves": index}, f, indent=1)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, help="games/sons-of-the-forest directory")
    ap.add_argument("exports", nargs="+", help="world export JSON files with cave scans")
    args = ap.parse_args()

    scans: dict[int, dict] = {}
    for path in args.exports:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        for scan in data.get("caves", []):
            scans[scan["area"]] = scan  # later exports win
    if not scans:
        print("no cave scans in the given exports", file=sys.stderr)
        return 1

    water = load_water(args.out)
    if not water:
        print("no cave water (build/water-*.bin): run sotf_3d.py first for swimming and water", file=sys.stderr)
    traversal, volumes = load_traversal(args.out)
    if not volumes:
        print("no ropes, ziplines or water volumes (build/): run sotf_3d.py first for them", file=sys.stderr)
    collision = load_floor_scans(args.out)
    if not collision:
        print("no collision floor scans (build/floors-*.npz): run sotf_3d.py first to fill the scans' gaps", file=sys.stderr)
    for area, scan in scans.items():
        scans[area], filled = fill_from_collision(scan, collision)
        if filled:
            print(f"  cave {area}: {filled} floors added from collision ({scan['width']}x{scan['height']} -> {scans[area]['width']}x{scans[area]['height']})")
    world, relative = compose(list(scans.values()))
    cell = WORLD_SIZE / GRID
    img = render(world, relative, rasterize_surfaces(water, WORLD_MIN, WORLD_MIN, cell, GRID, GRID) if water else None)
    caves = [cave_grids(scan, water, traversal, volumes) for scan in scans.values()]
    write_nav(caves, args.out)
    img.resize((1024, 1024), Image.BILINEAR).save(os.path.join(args.out, "preview-caves.png"))
    min_zoom, max_zoom = write_tiles(img, args.out, "caves")
    print(f"rendered {len(scans)} caves ({int((~np.isnan(world)).sum())} m2 of floor), tiles z{min_zoom}..{max_zoom}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
