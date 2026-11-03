"""Render the Sons of the Forest surface map from the game's own terrain data.

Reads the island terrain (heightmap + splat maps) straight from the installed game,
renders a shaded-relief map, and cuts it into a MapLibre raster tile pyramid.
Re-run after game updates; nothing is hand-edited.

Usage:
    python -I tools/mapgen/sotf.py --game "D:/SteamLibrary/steamapps/common/Sons Of The Forest" \
        --out games/sons-of-the-forest [--size 8192] [--preview-only | --icons-only]

Coordinate contract (must match `projection` in game.json): the image covers the
terrain square [x0, x0+W] x [z0, z0+W] with north (+z) up; column = x, row 0 = max z.
"""
from __future__ import annotations

import argparse
import math
import os
import sys

import numpy as np
import UnityPy
from PIL import Image, ImageFilter

# Shared tile writer (`python -I` leaves the script's directory off the import path).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tiles  # noqa: E402
from icons import trim_icon, write_icon_index  # noqa: E402

# The map image occupies Web Mercator tile (MAP_TILE_X, MAP_TILE_Y) at zoom MAP_TILE_ZOOM;
# tiles are written in that absolute XYZ scheme. Must match `projection` in game.json.
# One eighth of the world wide, so even 32:9 screens can center the island without
# reaching the world's edge (MapLibre won't pan past it).
MAP_TILE_ZOOM = 3
MAP_TILE_X = 3
MAP_TILE_Y = 3
UNITY_MAX_HEIGHT = 32766.0
SEA_LEVEL = 0.0  # world Y of the ocean surface
# The island terrain object sits at (-2000, -19, -2000) in level7 ("Site02Terrain Tess").
TERRAIN_ORIGIN = (-2000.0, -19.0, -2000.0)

# Terrain layer name fragment -> map color (sRGB). Curated rather than the textures'
# average colors, which are too muddy to read at map scale.
LAYER_COLORS = [
    ("Sand", (214, 200, 160)),
    ("Dirt", (150, 128, 96)),
    ("Rock", (138, 134, 128)),
    ("Grass", (122, 150, 82)),
    ("ForestGround", (72, 102, 58)),
    ("Roots", (82, 104, 62)),
    ("RiverBed", (150, 140, 118)),
    ("SharpRocks", (128, 124, 118)),
    ("GolfGrass", (120, 170, 80)),
    ("BedRock", (120, 116, 110)),
]
DEFAULT_LAYER_COLOR = (110, 130, 85)
SNOW_COLOR = np.array([244, 246, 250], dtype=np.float32)
DEEP_WATER = np.array([46, 92, 128], dtype=np.float32)
SHALLOW_WATER = np.array([110, 170, 190], dtype=np.float32)


def load_terrain(data_dir: str):
    env = UnityPy.load(
        os.path.join(data_dir, "sharedassets7.assets"),
        os.path.join(data_dir, "resources.assets"),
    )
    td_obj = next(o for o in env.objects if o.type.name == "TerrainData" and o.read_typetree()["m_Name"] == "Site2Terrain")
    td = td_obj.read_typetree()
    hm = td["m_Heightmap"]
    res = hm["m_Resolution"]
    heights = np.array(hm["m_Heights"], dtype=np.int16).astype(np.float32).reshape(res, res)
    world_h = TERRAIN_ORIGIN[1] + heights / UNITY_MAX_HEIGHT * hm["m_Scale"]["y"]

    # Resolve splat textures and layers through the asset file's PPtr table.
    sf = td_obj.assets_file

    def deref(ptr):
        try:
            return sf.resolve_pptr(ptr) if hasattr(sf, "resolve_pptr") else None
        except Exception:  # noqa: BLE001
            return None

    by_file = {}
    for f in env.files.values():
        for o in getattr(f, "objects", {}).values():
            by_file.setdefault(os.path.basename(f.name), {})[o.path_id] = o
    externals = [os.path.basename(e.path) for e in sf.externals]

    def lookup(ref):
        fid, pid = ref.get("m_FileID", 0), ref.get("m_PathID", 0)
        name = os.path.basename(sf.name) if fid == 0 else externals[fid - 1]
        return by_file.get(name, {}).get(pid)

    splat = td["m_SplatDatabase"]
    weights = []
    for ref in splat["m_AlphaTextures"]:
        tex = lookup(ref)
        if tex is None:
            raise SystemExit(f"cannot resolve splat texture {ref}")
        img = np.asarray(tex.read().image.convert("RGBA"), dtype=np.float32) / 255.0
        weights.extend(img[..., c] for c in range(4))
    layer_names = []
    for ref in splat["m_TerrainLayers"]:
        layer = lookup(ref)
        layer_names.append(layer.read_typetree()["m_Name"] if layer is not None else "")

    return {
        "heights": world_h,  # [z_row, x_col], row 0 = min z
        "cell": hm["m_Scale"]["x"],
        "weights": weights[: len(layer_names)],
        "layers": layer_names,
    }


def layer_color(name: str):
    for frag, color in LAYER_COLORS:
        if frag.lower() in name.lower():
            return color
    return DEFAULT_LAYER_COLOR


def hillshade(h: np.ndarray, cell: float) -> np.ndarray:
    """Multi-directional hillshade in [0, 1]."""
    gy, gx = np.gradient(h, cell)
    slope = np.arctan(np.hypot(gx, gy) * 1.6)
    aspect = np.arctan2(-gx, gy)
    out = np.zeros_like(h)
    for az, alt, w in ((315, 45, 0.5), (270, 50, 0.2), (0, 55, 0.2), (225, 60, 0.1)):
        a, b = math.radians(az), math.radians(alt)
        out += w * (np.sin(b) * np.cos(slope) + np.cos(b) * np.sin(slope) * np.cos(a - aspect))
    return np.clip(out, 0, 1)


def contour_lines(h: np.ndarray, interval: float) -> np.ndarray:
    """One-pixel-wide mask where the height crosses a multiple of `interval`."""
    level = np.floor(h / interval)
    edge = np.zeros(h.shape, dtype=bool)
    edge[:, 1:] |= level[:, 1:] != level[:, :-1]
    edge[1:, :] |= level[1:, :] != level[:-1, :]
    return edge


def render(t, size: int) -> Image.Image:
    h = t["heights"]
    n = h.shape[0]
    base = np.zeros((n, n, 3), dtype=np.float32)
    total = np.zeros((n, n), dtype=np.float32)
    for w, name in zip(t["weights"], t["layers"]):
        w = np.asarray(Image.fromarray(w).resize((n, n), Image.BILINEAR))
        base += w[..., None] * np.array(layer_color(name), dtype=np.float32)
        total += w
    base /= np.maximum(total, 1e-3)[..., None]

    # Snow on the high peaks, faded in over 60 m.
    snow = np.clip((h - 430.0) / 60.0, 0, 1)[..., None]
    base = base * (1 - snow) + SNOW_COLOR * snow

    shade = hillshade(h, t["cell"])
    lit = base * (0.45 + 0.75 * shade[..., None])

    # Ocean with a depth gradient and a soft shoreline.
    depth = np.clip((SEA_LEVEL - h) / 40.0, 0, 1)[..., None]
    water = SHALLOW_WATER * (1 - depth) + DEEP_WATER * depth
    wet = np.clip((SEA_LEVEL + 0.4 - h) / 0.8, 0, 1)[..., None]
    # Fade open water into the deep-water background color toward the image edges,
    # so the map blends into the viewer's background instead of ending in a hard square.
    idx = np.arange(n, dtype=np.float32)
    edge = np.minimum(np.minimum(idx, n - 1 - idx)[:, None], np.minimum(idx, n - 1 - idx)[None, :]) / n
    fade = np.clip(edge / 0.06, 0, 1)[..., None]
    water = water * fade + DEEP_WATER * (1 - fade)
    rgb = lit * (1 - wet) + water * wet

    # Thin contours every 25 m, emphasized every 100 m, on land.
    fine = contour_lines(h, 25.0)
    major = contour_lines(h, 100.0)
    land = (h > SEA_LEVEL)[..., None]
    rgb = np.where(land & fine[..., None], rgb * 0.93, rgb)
    rgb = np.where(land & major[..., None], rgb * 0.82, rgb)

    img = Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8)[::-1])  # north up
    img = img.resize((size, size), Image.LANCZOS)
    return img.filter(ImageFilter.UnsharpMask(radius=1.2, percent=40, threshold=2))


def write_tiles(img: Image.Image, out_dir: str, layer: str = "surface") -> tuple[int, int]:
    """Writes the pyramid for `tiles/<layer>` in this map's tile placement."""
    return tiles.write_tiles(img, out_dir, MAP_TILE_ZOOM, MAP_TILE_X, MAP_TILE_Y, layer)


# Icon sources, all white glyphs on transparent backgrounds:
# - GPS map icons (bunkers, golf carts, radios, …) in the GPS locator item bundle;
# - the cave icon in the shared UI bundle;
# - inventory item icons (`<Item>Icon`, referenced by each item's UI data) in resources.assets.
GPS_BUNDLE_PREFIX = "items_assets_gpslocatoritem_"
UI_BUNDLE_PREFIX = "uishared_assets_all_"
UI_ICONS = {"Cave", "CaveOutline"}
MAX_ICON_SIZE = 512


def export_icons(game_dir: str, out_dir: str) -> int:
    """Exports the game's map and item icons as PNGs, plus an index of their names."""
    data_dir = os.path.join(game_dir, "SonsOfTheForest_Data")
    bundles = os.path.join(data_dir, "StreamingAssets", "aa", "StandaloneWindows64")
    icons_dir = os.path.join(out_dir, "icons", "game")
    os.makedirs(icons_dir, exist_ok=True)

    sources = []
    for name in os.listdir(bundles):
        if name.startswith(GPS_BUNDLE_PREFIX):
            sources.append((os.path.join(bundles, name), lambda n: True))
        elif name.startswith(UI_BUNDLE_PREFIX):
            sources.append((os.path.join(bundles, name), lambda n: n in UI_ICONS))
    sources.append((os.path.join(data_dir, "resources.assets"), lambda n: n.endswith("Icon")))

    count = 0
    for path, wanted in sources:
        for obj in UnityPy.load(path).objects:
            if obj.type.name != "Texture2D" or not wanted(obj.peek_name()):
                continue
            tex = obj.read()
            if tex.m_Width > MAX_ICON_SIZE or tex.m_Height > MAX_ICON_SIZE:
                continue
            trim_icon(tex.image).save(os.path.join(icons_dir, f"{tex.m_Name}.png"))
            count += 1
    write_icon_index(icons_dir)
    return count


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, help="Sons Of The Forest install directory")
    ap.add_argument("--out", required=True, help="games/sons-of-the-forest directory")
    ap.add_argument("--size", type=int, default=8192)
    ap.add_argument("--preview-only", action="store_true")
    ap.add_argument("--icons-only", action="store_true")
    args = ap.parse_args()

    data_dir = os.path.join(args.game, "SonsOfTheForest_Data")
    if not args.preview_only:
        print(f"exported {export_icons(args.game, args.out)} game icons")
    if args.icons_only:
        return
    t = load_terrain(data_dir)
    img = render(t, args.size)
    os.makedirs(args.out, exist_ok=True)
    img.resize((1024, 1024), Image.LANCZOS).save(os.path.join(args.out, "preview.png"))
    if args.preview_only:
        print("wrote preview.png")
        return
    min_zoom, max_zoom = write_tiles(img, args.out)
    tiles.write_tiles_meta(args.out, {"surface": (min_zoom, max_zoom)}, MAP_TILE_ZOOM, MAP_TILE_X, MAP_TILE_Y)
    print(f"wrote tiles z{min_zoom}..{max_zoom}")


if __name__ == "__main__":
    main()
