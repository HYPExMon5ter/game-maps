"""MapLibre raster tile pyramids for a square map image placed on one Web Mercator tile.

The map image occupies XYZ tile (tile_x, tile_y) at zoom `tile_zoom`; tiles are written
in that absolute scheme, matching `projection` in each game's game.json.
"""
from __future__ import annotations

import json
import math
import os
import shutil

from PIL import Image

TILE = 256
# Base images can be far larger than PIL's decompression-bomb default.
Image.MAX_IMAGE_PIXELS = None


def resize(img: Image.Image, side: int) -> Image.Image:
    """Resamples a square image; transparency is premultiplied so edges don't darken."""
    if img.mode == "RGBA":
        return img.convert("RGBa").resize((side, side), Image.LANCZOS).convert("RGBA")
    return img.resize((side, side), Image.LANCZOS)


def write_tiles(img: Image.Image, out_dir: str, tile_zoom: int, tile_x: int, tile_y: int, layer: str = "surface") -> tuple[int, int]:
    """Writes the pyramid for `tiles/<layer>` (RGB or RGBA). Returns (min_zoom, max_zoom)."""
    size = img.width
    max_zoom = int(math.log2(size // TILE))
    tiles_dir = os.path.join(out_dir, "tiles", layer)
    if os.path.isdir(tiles_dir):
        shutil.rmtree(tiles_dir)
    level = img
    for z in range(max_zoom, -1, -1):
        n = 2**z
        if level.width != n * TILE:
            level = resize(level, n * TILE)
        abs_z = z + tile_zoom
        for x in range(n):
            col = os.path.join(tiles_dir, str(abs_z), str(tile_x * n + x))
            os.makedirs(col, exist_ok=True)
            for y in range(n):
                tile = level.crop((x * TILE, y * TILE, (x + 1) * TILE, (y + 1) * TILE))
                tile.save(os.path.join(col, f"{tile_y * n + y}.webp"), "WEBP", quality=88, method=4)
    return tile_zoom, max_zoom + tile_zoom


def write_tiles_meta(out_dir: str, layers: dict[str, tuple[int, int]], tile_zoom: int, tile_x: int, tile_y: int) -> None:
    """Records each layer's native zoom range next to the tiles (`tiles/tiles.json`)."""
    meta = {
        layer: {"tileSize": TILE, "minNativeZoom": lo, "maxNativeZoom": hi, "tileZoom": tile_zoom, "tileX": tile_x, "tileY": tile_y}
        for layer, (lo, hi) in layers.items()
    }
    with open(os.path.join(out_dir, "tiles", "tiles.json"), "w") as f:
        json.dump(meta, f, indent=2)
