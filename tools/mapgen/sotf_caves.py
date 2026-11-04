"""Render the Sons of the Forest underground map layer from in-game cave scans.

The agent's world export (overlay app: Settings -> Diagnostics -> Export world data)
includes a floor-height scan of every cave and bunker. This draws them, top-down and
aligned with the surface map, into `games/sons-of-the-forest/tiles/caves`: walkable
floors colored by height within their cave (warm at the top, cool at the bottom) with
contour lines every few meters, dark walls around them, transparent everywhere else.

It also writes each cave's walkable floors as a navigation grid (`nav/caves.json` plus
`nav/cave-<area>.bin`), which the app routes on inside caves: the game's own
pathfinding doesn't cover them.

Usage:
    python -I tools/mapgen/sotf_caves.py --out games/sons-of-the-forest EXPORT.json [EXPORT.json ...]
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import sys

import numpy as np
from PIL import Image

# Shares the map layout and tile writer with the surface generator (`python -I` leaves
# the script's directory off the import path).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from sotf import TERRAIN_ORIGIN, hillshade, write_tiles  # noqa: E402

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


def render(world: np.ndarray, relative: np.ndarray) -> Image.Image:
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
    wall = dilate(floor, WALL_WIDTH) & ~floor
    rgba[wall, :3] = WALL
    rgba[wall, 3] = 245
    img = Image.fromarray(np.clip(rgba, 0, 255).astype(np.uint8)[::-1], "RGBA")  # north up
    return img.convert("RGBa").resize((IMAGE_SIZE, IMAGE_SIZE), Image.BILINEAR).convert("RGBA")


def write_nav(scans: list[dict], out_dir: str) -> None:
    """Each cave's walkable floors for routing in the app: `nav/cave-<area>.bin` holds
    int16 floor heights in decimeters, layers x rows (south first) x columns, with
    NONE where a cell isn't walkable; `nav/caves.json` lists the grids."""
    nav_dir = os.path.join(out_dir, "nav")
    os.makedirs(nav_dir, exist_ok=True)
    index = []
    for scan in scans:
        w, h, layers = scan["width"], scan["height"], scan["layers"]
        floors = np.frombuffer(base64.b64decode(scan["floors"]), "<i2").reshape(layers, h, w)
        room = np.frombuffer(base64.b64decode(scan["clearance"]), "<i2").reshape(layers, h, w)
        walkable = np.where((floors != NONE) & (room >= MIN_HEADROOM_DM), floors, NONE).astype("<i2")
        name = f"cave-{scan['area']}.bin"
        walkable.tofile(os.path.join(nav_dir, name))
        index.append({
            "area": scan["area"],
            "originX": scan["origin_x"],
            "originZ": scan["origin_z"],
            "cell": scan["cell"],
            "width": w,
            "height": h,
            "layers": layers,
            "file": name,
        })
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

    world, relative = compose(list(scans.values()))
    img = render(world, relative)
    write_nav(list(scans.values()), args.out)
    img.resize((1024, 1024), Image.BILINEAR).save(os.path.join(args.out, "preview-caves.png"))
    min_zoom, max_zoom = write_tiles(img, args.out, "caves")
    print(f"rendered {len(scans)} caves ({int((~np.isnan(world)).sum())} m2 of floor), tiles z{min_zoom}..{max_zoom}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
