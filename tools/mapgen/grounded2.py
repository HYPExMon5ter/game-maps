"""Grounded 2 map tiles from the game's own map paintings.

`tools/mapgen/grounded2-extract` reads the paintings and their world bounds out of the
game files: the park, and the Abyss, the region under it, which the game shows as a map
of its own. They don't overlap, so here they go on one canvas at their real places, then
get cut into tiles; `game.json`'s projection is checked against the bounds.

The paintings are drawn with Unreal's +X to the right and +Y down; turned a quarter to
the left they're north-up with east = +Y, which is how the agent reports positions
(east = x, north = z, in meters; see `crates/agent/src/unreal.rs`).

Usage:
    .venv/Scripts/python -I tools/mapgen/grounded2.py --data <extract out dir> --out games/grounded-2
"""
from __future__ import annotations

import argparse
import json
import os
import sys

from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from tiles import write_tiles, write_tiles_meta  # noqa: E402

TILE_ZOOM, TILE_X, TILE_Y = 3, 3, 3
CM = 100.0
# Pixels per side of the combined map. The park painting covers about 60% of it, a little
# above its own resolution; the Abyss's comes out at about half of its own.
CANVAS = 8192


def union(bounds: list[dict]) -> dict:
    """The square (Unreal centimeters) that holds every map's rectangle, from its
    smallest corner."""
    min_x = min(b["minX"] for b in bounds)
    min_y = min(b["minY"] for b in bounds)
    side = max(max(b["maxX"] for b in bounds) - min_x, max(b["maxY"] for b in bounds) - min_y)
    return {"minX": min_x, "minY": min_y, "maxX": min_x + side, "maxY": min_y + side}


def projection(square: dict) -> dict:
    """`game.json` projection for a world square (Unreal centimeters)."""
    return {
        "east": "x",
        "north": "z",
        "up": "y",
        "minEast": round(square["minY"] / CM, 2),
        "maxEast": round(square["maxY"] / CM, 2),
        "minNorth": round(square["minX"] / CM, 2),
        "maxNorth": round(square["maxX"] / CM, 2),
        "tileZoom": TILE_ZOOM,
        "tileX": TILE_X,
        "tileY": TILE_Y,
    }


def placement(square: dict, bounds: dict, size: int) -> tuple[int, int, int, int]:
    """Where a map with `bounds` goes on a `size` canvas of `square`: left, top, width,
    height in pixels (X to the right, Y down, as the paintings are drawn)."""
    scale = size / (square["maxX"] - square["minX"])
    return (
        round((bounds["minX"] - square["minX"]) * scale),
        round((bounds["minY"] - square["minY"]) * scale),
        round((bounds["maxX"] - bounds["minX"]) * scale),
        round((bounds["maxY"] - bounds["minY"]) * scale),
    )


def compose(paintings: list[tuple[Image.Image, dict]], size: int) -> Image.Image:
    """All paintings on one transparent canvas, turned north-up."""
    square = union([b for _, b in paintings])
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    for image, bounds in paintings:
        left, top, width, height = placement(square, bounds, size)
        canvas.alpha_composite(image.convert("RGBA").resize((width, height), Image.LANCZOS), (left, top))
    return canvas.rotate(90, expand=True)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True, help="output folder of grounded2-extract (map.png, abyss.png, data.json)")
    ap.add_argument("--out", required=True, help="games/grounded-2")
    args = ap.parse_args()

    with open(os.path.join(args.data, "data.json"), encoding="utf-8-sig") as f:
        data = json.load(f)
    paintings = [
        (Image.open(os.path.join(args.data, "map.png")), data["mapBounds"]),
        (Image.open(os.path.join(args.data, "abyss.png")), data["abyssBounds"]),
    ]
    expected = projection(union([b for _, b in paintings]))
    with open(os.path.join(args.out, "game.json"), encoding="utf-8") as f:
        game = json.load(f)
    actual = game["maps"][0]["projection"]
    if any(actual.get(k) != v for k, v in expected.items()):
        sys.exit(f"game.json projection doesn't match the game's map bounds; expected:\n{json.dumps(expected, indent=2)}")

    image = compose(paintings, CANVAS)
    zooms = write_tiles(image, args.out, TILE_ZOOM, TILE_X, TILE_Y, layer="surface")
    write_tiles_meta(args.out, {"surface": zooms}, TILE_ZOOM, TILE_X, TILE_Y)
    preview = Image.new("RGB", image.size, game["maps"][0]["background"])
    preview.paste(image, mask=image.split()[3])
    preview.resize((2048, 2048), Image.LANCZOS).save(os.path.join(args.out, "preview.png"))
    print(f"tiles: zoom {zooms[0]}-{zooms[1]}")


if __name__ == "__main__":
    main()
