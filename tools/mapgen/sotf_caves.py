"""Render the Sons of the Forest underground map layer from in-game cave scans.

The agent's world export (overlay app: Settings -> Diagnostics -> Export world data)
includes a floor-height scan of every cave and bunker. This draws them, top-down and
aligned with the surface map, into `games/sons-of-the-forest/tiles/caves`: walkable
floors shaded by their relief, dark walls around them, transparent everywhere else.

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

FLOOR_LIGHT = np.array((214, 196, 166), np.float32)
FLOOR_DEEP = np.array((150, 128, 104), np.float32)
WALL = np.array((34, 27, 24), np.float32)


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


def compose(scans: list[dict]) -> np.ndarray:
    """All caves on the world grid (meters, NaN outside caves), row 0 = south."""
    world = np.full((GRID, GRID), np.nan, np.float32)
    cell = WORLD_SIZE / GRID
    for scan in scans:
        floor = walkable_floor(scan)
        h, w = floor.shape
        # Cell centers -> world grid indices.
        xs = np.floor((scan["origin_x"] + (np.arange(w) + 0.5) * scan["cell"] - WORLD_MIN) / cell).astype(int)
        zs = np.floor((scan["origin_z"] + (np.arange(h) + 0.5) * scan["cell"] - WORLD_MIN) / cell).astype(int)
        cols, rows = np.meshgrid(xs, zs)
        inside = (cols >= 0) & (cols < GRID) & (rows >= 0) & (rows < GRID) & ~np.isnan(floor)
        r, c, v = rows[inside], cols[inside], floor[inside]
        # Where caves overlap (stacked levels), keep the higher floor.
        current = world[r, c]
        world[r, c] = np.where(np.isnan(current) | (v > current), v, current)
    return world


def render(world: np.ndarray) -> Image.Image:
    floor = ~np.isnan(world)
    heights = np.where(floor, world, 0.0)
    # Relief from the floor itself; fill outside with nearby heights so walls don't
    # read as cliffs in the shading.
    shade = hillshade(np.where(floor, heights, np.nanmean(world) if floor.any() else 0.0), WORLD_SIZE / GRID)
    rgba = np.zeros((GRID, GRID, 4), np.float32)
    if floor.any():
        lo, hi = np.nanpercentile(world, 2), np.nanpercentile(world, 98)
        depth = np.clip((hi - heights) / max(hi - lo, 1.0), 0, 1)[..., None]
        base = FLOOR_LIGHT * (1 - depth) + FLOOR_DEEP * depth
        lit = base * (0.6 + 0.55 * shade[..., None])
        rgba[floor, :3] = lit[floor]
        rgba[floor, 3] = 255
    wall = dilate(floor, WALL_WIDTH) & ~floor
    rgba[wall, :3] = WALL
    rgba[wall, 3] = 235
    img = Image.fromarray(np.clip(rgba, 0, 255).astype(np.uint8)[::-1], "RGBA")  # north up
    return img.convert("RGBa").resize((IMAGE_SIZE, IMAGE_SIZE), Image.BILINEAR).convert("RGBA")


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

    world = compose(list(scans.values()))
    img = render(world)
    img.resize((1024, 1024), Image.BILINEAR).save(os.path.join(args.out, "preview-caves.png"))
    min_zoom, max_zoom = write_tiles(img, args.out, "caves")
    print(f"rendered {len(scans)} caves ({int((~np.isnan(world)).sum())} m2 of floor), tiles z{min_zoom}..{max_zoom}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
