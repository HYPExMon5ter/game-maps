"""Render the Sunkenland map from the game's own files.

The world is one scene (`level2`): three terrains (seabed and the larger islands) plus
50 locations. Each location's buildings are streamed at runtime from an Addressables
prefab (`LocationStaticStreamer.streamedVisualPrefab`), so they're read from the
location visuals bundle and placed under their location, exactly as the game does.
Everything is drawn top-down on the GPU (see `topdown.py`) with its own textures, then
composited with the sea: seabed seen through water that darkens with depth.

Usage:
    python -I tools/mapgen/sunkenland.py --game "D:/SteamLibrary/steamapps/common/Sunkenland" \
        --out games/sunkenland [--size 16384] [--preview-only | --icons-only]
    python -I tools/mapgen/sunkenland.py --game ... --export-world world.json

`--export-world` writes every point of interest in the world scene (locations, traders,
research items, ore, ...) for `tools/pins/sunkenland.py`; the game places them in the
scene rather than spawning them, so no in-game export is needed.

Coordinate contract (must match `projection` in game.json): the image covers
[MIN_EAST, MIN_EAST + SPAN] x [MIN_NORTH, MIN_NORTH + SPAN] (x east, z north), north up.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys
import time

import numpy as np
from PIL import Image

# Shared helpers (`python -I` leaves the script's directory off the import path).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tiles  # noqa: E402
from icons import trim_icon, write_icon_index  # noqa: E402
from topdown import TEX_SIZE, TopDownRenderer  # noqa: E402
from unity_world import (  # noqa: E402
    Assets,
    Hierarchy,
    addressables_catalog,
    bundle_files,
    collect_renderers,
    read_material,
    read_mesh,
    read_texture_rgba,
    trs_matrix,
)

MAP_TILE_ZOOM, MAP_TILE_X, MAP_TILE_Y = 3, 3, 3
# 6 km square around the playable world (the game's own map covers x -2750..250,
# z -4000..2000; terrains and locations span x -2650..350, z -3850..1260).
MIN_EAST, MIN_NORTH, SPAN = -4200.0, -4300.0, 6000.0
UNITY_MAX_HEIGHT = 32766.0

SCENE = "level2"
DATA_FILES = ("level2", "sharedassets0.assets", "sharedassets1.assets", "sharedassets2.assets", "resources.assets", "globalgamemanagers.assets")
# Scene subtrees drawn on the map (the rest is UI, the player rig and cutscenes).
WORLD_ROOTS = ("Locations", "Shelter Island Location", "Scene/AirDropPos")
VISUALS_BUNDLE = "locationvisuals_assets_all_"
BUILTIN_BUNDLE = "unitybuiltinshaders"
STRINGS_BUNDLES = ("localization-string-tables-english(en)_assets_all", "localization-assets-shared_assets_all")

# World export: script class -> entry kind. Everything is a scene object; the streamed
# location prefabs are visuals only.
EXPORT_KINDS = {
    "Location": "location",
    "Trader": "trader",
    "NPC": "npc",
    "PrisonCage": "prison",
    "AssistantRescue": "companion",
    "LearningResource": "research",
    "VeinOfOre": "ore_vein",
    "CollectableByToolHit": "mineable",
    "FishSpawner": "fish",
    "ItemSpawner": "forage",
    "AirDropPlatform": "airdrop",
    "Fragments": "map_fragment",
    "Chest": "chest",
    "SupplyCrate": "supply_crate",
    "RadiationZone": "radiation",
}
# Glyph sprites exported for category icons and the custom pin picker, besides every
# location's own map icon and the game's `icon ...` UI glyphs.
EXTRA_ICONS = ("Island", "ICON RESEARCH", "Cage", "Box-Outline-Icon", "Fragment 1")
MAX_ICON_SIZE = 512

TERRAIN_STEP = 1  # heightmap samples per terrain mesh vertex
CHUNK = 2048  # output pixels per GPU render

# Terrain colors (sRGB), curated for readability at map scale.
SEABED = np.array([194, 178, 140], dtype=np.float32)
SAND = np.array([214, 196, 150], dtype=np.float32)
GREEN = np.array([92, 112, 62], dtype=np.float32)
ROCK = np.array([118, 112, 104], dtype=np.float32)
DEEP_WATER = np.array([22, 58, 92], dtype=np.float32)
SHALLOW_WATER = np.array([48, 150, 160], dtype=np.float32)
EMPTY_DEPTH = 300.0  # treated as open ocean where nothing was drawn


def load_assets(game: str) -> tuple[Assets, str]:
    data = os.path.join(game, "Sunkenland_Data")
    aa = os.path.join(data, "StreamingAssets", "aa")
    assets = Assets()
    assets.load(*[os.path.join(data, f) for f in DATA_FILES])
    # Only the bundle the catalog references (older builds can leave stale copies).
    _, ids = addressables_catalog(os.path.join(aa, "catalog.json"))
    platform = os.path.join(aa, "StandaloneWindows64")
    visuals = next(os.path.join(platform, os.path.basename(i.replace("\\", "/"))) for i in ids if VISUALS_BUNDLE in i)
    visuals_key = assets.load(visuals)[0]
    assets.load(*[f for f in bundle_files(platform, "") if BUILTIN_BUNDLE in f])
    assets.use_game_scripts(game)
    return assets, visuals_key


class Collector:
    """Turns renderers into GPU geometry, caching meshes, materials and textures."""

    def __init__(self, assets: Assets, renderer: TopDownRenderer):
        self.assets, self.r = assets, renderer
        self.meshes: dict = {}
        self.materials: dict = {}
        self.triangles = 0

    def material(self, ref):
        if ref is None:
            return (-1, (0.55, 0.55, 0.55, 1.0), None, (1.0, 1.0, 0.0, 0.0))
        if ref not in self.materials:
            m = read_material(self.assets, ref)
            if m.transparent:
                self.materials[ref] = None
            else:
                layer = -1
                if m.texture:
                    layer = self.r.texture_layer(m.texture, read_texture_rgba(self.assets, m.texture, TEX_SIZE))
                self.materials[ref] = (layer, m.color, m.alpha_clip, m.tiling)
        return self.materials[ref]

    def add(self, renderers):
        for rd in renderers:
            if rd.mesh not in self.meshes:
                try:
                    self.meshes[rd.mesh] = read_mesh(self.assets, rd.mesh)
                except Exception as e:  # unreadable mesh: skip it, keep the map
                    print(f"  skipped mesh {rd.mesh}: {e}")
                    self.meshes[rd.mesh] = None
            md = self.meshes[rd.mesh]
            if md is None:
                continue
            pos = (md.positions.astype(np.float64) @ rd.matrix[:3, :3].T + rd.matrix[:3, 3]).astype(np.float32)
            for i, tri in enumerate(md.submeshes):
                mat = self.material(rd.materials[min(i, len(rd.materials) - 1)] if rd.materials else None)
                if mat is None or not len(tri):
                    continue
                layer, tint, clip, (sx, sy, ox, oy) = mat
                uv = md.uv * np.array([sx, sy], np.float32) + np.array([ox, oy], np.float32) if md.uv is not None else None
                self.r.add(pos, uv, tri, layer, tint, clip)
                self.triangles += len(tri)


def find_path(h: Hierarchy, path: str) -> int | None:
    parts = path.split("/")
    level = h.roots()
    tid = None
    for name in parts:
        tid = next((t for t in level if h.nodes[t].name == name), None)
        if tid is None:
            return None
        level = h.nodes[tid].children
    return tid


def location_visuals(assets: Assets, scene: Hierarchy, visuals_key: str, catalog: dict) -> tuple[list[tuple[int, int]], Hierarchy]:
    """(location transform, prefab root transform in the visuals bundle) per streamer."""
    bundle = next(o for o in assets.objects[visuals_key].values() if o.type.name == "AssetBundle").read_typetree()
    container = {path: info["asset"]["m_PathID"] for path, info in bundle["m_Container"]}
    prefabs = Hierarchy(assets, visuals_key)
    out = []
    for tid, node in scene.nodes.items():
        for cname, cid in node.components:
            if cname != "MonoBehaviour":
                continue
            b = assets.behaviour((SCENE, cid))
            if not b or b[0] != "LocationStaticStreamer" or b[1] is None:
                continue
            guid = b[1]["streamedVisualPrefab"]["m_AssetGUID"]
            path = next((p for p in catalog.get(guid, []) if p in container), None)
            if path is None:
                print(f"  {scene.path(tid)}: streamed visual {guid} not in the bundle")
                continue
            out.append((tid, prefabs.by_go[container[path]]))
    return out, prefabs


def terrain_color(height: np.ndarray, cell: float, sea: float) -> np.ndarray:
    """RGBA color map (row 0 = south) from height above the sea and slope: seabed and
    beach sand, vegetation inland, rock on steep slopes. The terrains' splat maps are
    shared between terrains and don't match their shapes, so they aren't used."""
    gz, gx = np.gradient(height, cell)
    slope = np.clip((np.hypot(gx, gz) - 0.45) / 0.5, 0, 1)[..., None]
    above = (height - sea)[..., None]
    beach = np.clip(1 - (above - 1.5) / 2.5, 0, 1)
    color = GREEN * (1 - beach) + SAND * beach
    color = color * (1 - slope) + ROCK * slope
    color = np.where(above < 0, SEABED, color)
    out = np.empty(height.shape + (4,), np.uint8)
    out[..., :3] = np.clip(color, 0, 255)
    out[..., 3] = 255
    return out


def add_terrains(assets: Assets, scene: Hierarchy, collector: Collector, renderer: TopDownRenderer, sea: float) -> int:
    count = 0
    for tid, node in scene.nodes.items():
        terr = scene.component(tid, "Terrain")
        if not terr or not scene.active_in_hierarchy(tid):
            continue
        (owner, _), td = assets.deref(SCENE, terr["m_TerrainData"])
        hm = td["m_Heightmap"]
        res, scale = hm["m_Resolution"], hm["m_Scale"]
        origin = scene.world(tid)[:3, 3]
        full = origin[1] + np.asarray(hm["m_Heights"], dtype=np.float32).reshape(res, res) / UNITY_MAX_HEIGHT * scale["y"]
        heights = full[::TERRAIN_STEP, ::TERRAIN_STEP]
        n = heights.shape[0]
        size_x, size_z = (res - 1) * scale["x"], (res - 1) * scale["z"]
        gz, gx = np.meshgrid(np.linspace(0, 1, n), np.linspace(0, 1, n), indexing="ij")
        pos = np.stack([origin[0] + gx * size_x, heights, origin[2] + gz * size_z], -1).reshape(-1, 3).astype(np.float32)
        uv = np.stack([gx, gz], -1).reshape(-1, 2).astype(np.float32)
        i = np.arange(n * n, dtype=np.uint32).reshape(n, n)
        a, b, c, d = i[:-1, :-1], i[:-1, 1:], i[1:, :-1], i[1:, 1:]
        tris = np.concatenate([np.stack([a, c, b], -1).reshape(-1, 3), np.stack([b, c, d], -1).reshape(-1, 3)])
        renderer.add_textured(pos, uv, tris, terrain_color(full, scale["x"], sea))
        count += 1
        add_trees(assets, owner, td, origin, (size_x, scale["y"], size_z), collector)
    return count


def add_trees(assets: Assets, owner: str, td: dict, origin, size, collector: Collector) -> None:
    dd = td["m_DetailDatabase"]
    prototypes = []
    hierarchies: dict[str, Hierarchy] = {}
    for p in dd["m_TreePrototypes"]:
        ref = assets.resolve(owner, p["prefab"])
        if not ref:
            prototypes.append(None)
            continue
        if ref[0] not in hierarchies:
            hierarchies[ref[0]] = Hierarchy(assets, ref[0])
        h = hierarchies[ref[0]]
        prototypes.append(collect_renderers(h, h.by_go[ref[1]], root=np.eye(4)))
    for t in dd["m_TreeInstances"]:
        proto = prototypes[t["index"]] if t["index"] < len(prototypes) else None
        if not proto:
            continue
        p = t["position"]
        world = (origin[0] + p["x"] * size[0], origin[1] + p["y"] * size[1], origin[2] + p["z"] * size[2])
        rot = t.get("rotation", 0.0)
        q = (0.0, math.sin(rot / 2), 0.0, math.cos(rot / 2))
        m = trs_matrix(world, q, (t["widthScale"], t["heightScale"], t["widthScale"]))
        for rd in proto:
            collector.add([type(rd)(rd.mesh, rd.materials, m @ rd.matrix, rd.path, rd.layer)])


def sea_level(assets: Assets, scene: Hierarchy) -> float:
    """World y of the ocean surface: `WorldScene.oceanObject`'s height, which the game
    copies into `oceanObjectY` at startup."""
    for tid, node in scene.nodes.items():
        for cname, cid in node.components:
            if cname != "MonoBehaviour":
                continue
            b = assets.behaviour((SCENE, cid))
            if b and b[0] == "WorldScene" and b[1]:
                ocean = assets.resolve(SCENE, b[1]["oceanObject"])
                return float(scene.world(scene.by_go[ocean[1]])[1, 3])
    raise RuntimeError("WorldScene.oceanObject not found")


def build_scene(game: str) -> tuple[TopDownRenderer, float]:
    t0 = time.time()
    assets, visuals_key = load_assets(game)
    print(f"loaded game files in {time.time() - t0:.0f} s")
    scene = Hierarchy(assets, SCENE)
    sea = sea_level(assets, scene)
    print(f"sea level: y = {sea:.2f}")
    renderer = TopDownRenderer(height_range=(-600.0, 600.0))
    collector = Collector(assets, renderer)

    for path in WORLD_ROOTS:
        tid = find_path(scene, path)
        if tid is None:
            print(f"  missing scene object {path}")
            continue
        if scene.active_in_hierarchy(tid):
            collector.add(collect_renderers(scene, tid))
    print(f"scene: {collector.triangles:,} triangles")

    catalog, _ = addressables_catalog(os.path.join(game, "Sunkenland_Data", "StreamingAssets", "aa", "catalog.json"))
    visuals, prefabs = location_visuals(assets, scene, visuals_key, catalog)
    for loc, root in visuals:
        # InstantiateAsync(parent) keeps the prefab root's local transform under the location.
        collector.add(collect_renderers(prefabs, root, root=scene.world(loc)))
    print(f"+ {len(visuals)} streamed location visuals: {collector.triangles:,} triangles")

    terrains = add_terrains(assets, scene, collector, renderer, sea)
    print(f"+ {terrains} terrains and their trees: {collector.triangles:,} triangles, {len(renderer.layers)} textures")
    renderer.build()
    print(f"built GPU scene in {time.time() - t0:.0f} s")
    return renderer, sea


def composite(rgb: np.ndarray, h: np.ndarray, sea: float) -> np.ndarray:
    """Sea over the rendered world: the seabed shows through shallow water and fades
    into deep blue with depth; a light rim marks the shoreline."""
    h = np.where(np.isnan(h), sea - EMPTY_DEPTH, h)
    lit = np.clip((rgb.astype(np.float32) / 255.0) ** 0.8 * 1.12, 0, 1) * 255.0
    depth = np.clip(sea - h, 0, None)[..., None]
    water = SHALLOW_WATER + (DEEP_WATER - SHALLOW_WATER) * np.clip(depth / 70.0, 0, 1)
    visible = np.exp(-depth / 10.0)
    under = lit * visible * 0.85 + water * (1 - visible * 0.85)
    wet = (h < sea)[..., None]
    out = np.where(wet, under, lit)
    rim = ((depth[..., 0] > 0) & (depth[..., 0] < 0.6))[..., None]
    out = np.where(rim, out * 0.6 + 255 * 0.4, out)
    return np.clip(out, 0, 255).astype(np.uint8)


def render_map(renderer: TopDownRenderer, sea: float, size: int, supersample: int) -> Image.Image:
    image = np.empty((size, size, 3), np.uint8)
    mpp = SPAN / size
    chunk = min(CHUNK, size)
    max_north = MIN_NORTH + SPAN
    for cy in range(size // chunk):
        for cx in range(size // chunk):
            px = chunk * supersample
            rgb, h = renderer.render(MIN_EAST + cx * chunk * mpp, max_north - (cy + 1) * chunk * mpp, chunk * mpp, chunk * mpp, px, px)
            tile = composite(rgb, h, sea)
            if supersample > 1:
                tile = np.asarray(Image.fromarray(tile).resize((chunk, chunk), Image.LANCZOS))
            image[cy * chunk : (cy + 1) * chunk, cx * chunk : (cx + 1) * chunk] = tile
        print(f"  rendered row {cy + 1}/{size // chunk}")
    return Image.fromarray(image)


def load_strings(assets: Assets, game: str) -> dict[int, str]:
    """English localized strings by key id."""
    platform = os.path.join(game, "Sunkenland_Data", "StreamingAssets", "aa", "StandaloneWindows64")
    keys = []
    for prefix in STRINGS_BUNDLES:
        keys += assets.load(*bundle_files(platform, prefix))
    strings: dict[int, str] = {}
    for key in keys:
        for pid, o in assets.objects[key].items():
            if o.type.name != "MonoBehaviour":
                continue
            b = assets.behaviour((key, pid))
            if b and b[1] and "m_TableData" in b[1]:
                for e in b[1]["m_TableData"]:
                    strings[e["m_Id"]] = e["m_Localized"]
    return strings


def localized(strings: dict[int, str], ref) -> str | None:
    if not isinstance(ref, dict):
        return None
    key = ref.get("m_TableEntryReference", {}).get("m_KeyId")
    text = strings.get(key) if key else None
    return text.strip() if text and text.strip() else None


def object_name(assets: Assets, owner: str, pptr) -> str | None:
    """GameObject name behind a PPtr to a GameObject or component (prefabs, items)."""
    ref = assets.resolve(owner, pptr) if isinstance(pptr, dict) else None
    if not ref:
        return None
    obj = assets.obj(ref)
    if obj.type.name == "GameObject":
        return assets.tt(ref).get("m_Name")
    # Components: only the base fields are needed (scripts may lack type trees).
    go = obj.read(check_read=False).m_GameObject
    found = assets.deref(ref[0], {"m_FileID": go.m_FileID, "m_PathID": go.m_PathID})
    return found[1].get("m_Name") if found else None


def clean_name(name: str) -> str:
    """`Mine Copper New_3` -> `Mine Copper New`; `ornamental fish_9` -> `Ornamental fish`."""
    name = re.sub(r"[ _]?\(?\d+\)?$", "", name.replace("_LOD0", "")).replace("_", " ").strip()
    return name[:1].upper() + name[1:]


def export_world(game: str) -> dict:
    """Every point of interest in the world scene, in the agent's export entry format."""
    assets, _ = load_assets(game)
    scene = Hierarchy(assets, SCENE)
    sea = sea_level(assets, scene)
    strings = load_strings(assets, game)

    behaviours: dict[int, dict[str, dict]] = {}  # transform -> class -> fields
    for tid, node in scene.nodes.items():
        for cname, cid in node.components:
            if cname == "MonoBehaviour":
                b = assets.behaviour((SCENE, cid))
                if b:  # fields may be unreadable; the class alone still identifies objects
                    behaviours.setdefault(tid, {})[b[0]] = b[1] or {}

    # Display name of each location, and which location every object belongs to.
    location_names: dict[int, str] = {}
    for tid, classes in behaviours.items():
        loc = classes.get("Location")
        if loc:
            location_names[tid] = localized(strings, loc.get("localizedName")) or loc["locationName"]
    location_of: dict[int, str] = {}
    for tid, name in location_names.items():
        for t in scene.descendants(tid):
            location_of[t] = name

    def position(tid: int) -> dict:
        x, y, z = scene.world(tid)[:3, 3]
        return {"x": round(float(x), 2), "y": round(float(y), 2), "z": round(float(z), 2)}

    def ancestor_with(tid: int, cls: str) -> int | None:
        while tid:
            if cls in behaviours.get(tid, {}):
                return tid
            tid = scene.nodes[tid].parent
        return None

    entries = []
    for tid, classes in behaviours.items():
        for cls, kind in EXPORT_KINDS.items():
            f = classes.get(cls)
            if f is None:
                continue
            node = scene.nodes[tid]
            props: dict[str, str] = {}
            name = clean_name(node.name)
            pos_tid = tid
            if kind == "location":
                name = location_names[tid]
                props["internal_name"] = f["locationName"]
                props["level"] = str(f.get("level", 0))
                props["blueprints"] = str(len(f.get("blueprints") or []))
                desc = localized(strings, f.get("localizedDescription"))
                if desc:
                    props["description"] = desc
                icon = assets.deref(SCENE, f.get("locationIcon", {}))
                if icon:
                    props["icon"] = icon[1]["m_Name"]
                # The game's own map places the location at its map locator.
                locator = assets.resolve(SCENE, f.get("mapLocator", {}))
                if locator and locator[1] in scene.nodes:
                    pos_tid = locator[1]
            elif kind in ("npc", "trader"):
                npc = classes.get("NPC")
                name = (localized(strings, npc.get("NPCName")) if npc else None) or name
                if kind == "npc" and ("Trader" in classes or "Slaver" in classes):
                    continue  # covered by the trader and prison entries
            elif kind == "prison":
                props["captives"] = str(len(f.get("slavers") or []))
            elif kind == "companion":
                owner = ancestor_with(tid, "Assistant")
                if owner:
                    name, pos_tid = clean_name(scene.nodes[owner].name), owner
            elif kind == "research":
                taught = [object_name(assets, SCENE, r) for r in (f.get("LearnedPiece") or []) + (f.get("LearnedItem") or [])]
                taught = sorted({clean_name(t) for t in taught if t})
                if taught:
                    props["teaches"] = ", ".join(taught)
            elif kind == "ore_vein" and node.parent:
                name = clean_name(scene.nodes[node.parent].name)
            elif kind == "chest":
                props["level"] = str(f.get("itemsLevel", 0))
            elif kind == "radiation":
                props["radius"] = str(round(f.get("Radius", 0.0), 1))
            if tid in location_of:
                props["location"] = location_of[tid]
            entries.append({"kind": kind, "name": name, "position": position(pos_tid), "active": scene.active_in_hierarchy(tid), "props": props})
    return {"game": "sunkenland", "sea_level": round(sea, 3), "entries": entries}


def export_icons(game: str, out_dir: str) -> int:
    """Exports location map icons and the game's UI glyphs as PNGs, plus an index."""
    assets, _ = load_assets(game)
    scene = Hierarchy(assets, SCENE)
    wanted: dict = {}
    for tid, node in scene.nodes.items():
        for cname, cid in node.components:
            if cname != "MonoBehaviour":
                continue
            b = assets.behaviour((SCENE, cid))
            if b and b[0] == "Location" and b[1]:
                ref = assets.resolve(SCENE, b[1].get("locationIcon", {}))
                if ref:
                    wanted[ref] = None
    for key, objects in assets.objects.items():
        for pid, o in objects.items():
            if o.type.name == "Sprite":
                name = o.peek_name() or ""
                if name in EXTRA_ICONS or name.lower().startswith("icon "):
                    wanted[(key, pid)] = None
    icons_dir = os.path.join(out_dir, "icons", "game")
    os.makedirs(icons_dir, exist_ok=True)
    count = 0
    for ref in wanted:
        sprite = assets.obj(ref).read()
        if sprite.m_Rect.width > MAX_ICON_SIZE or sprite.m_Rect.height > MAX_ICON_SIZE:
            continue
        trim_icon(sprite.image).save(os.path.join(icons_dir, f"{sprite.m_Name}.png"))
        count += 1
    write_icon_index(icons_dir)
    return count


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, help="Sunkenland install directory")
    ap.add_argument("--out", required=True, help="games/sunkenland directory")
    ap.add_argument("--size", type=int, default=16384, help="base image size in pixels (power of two)")
    ap.add_argument("--supersample", type=int, default=2)
    ap.add_argument("--preview-only", action="store_true")
    ap.add_argument("--icons-only", action="store_true", help="only export icons")
    ap.add_argument("--export-world", metavar="PATH", help="only write the world export (input of tools/pins/sunkenland.py)")
    args = ap.parse_args()

    if args.export_world:
        export = export_world(args.game)
        with open(args.export_world, "w", encoding="utf-8") as f:
            json.dump(export, f, indent=1, ensure_ascii=False)
        print(f"wrote {len(export['entries'])} entries to {args.export_world}")
        return
    if not args.preview_only:
        print(f"exported {export_icons(args.game, args.out)} game icons")
    if args.icons_only:
        return

    renderer, sea = build_scene(args.game)
    size = 2048 if args.preview_only else args.size
    img = render_map(renderer, sea, size, args.supersample)
    os.makedirs(args.out, exist_ok=True)
    img.resize((1024, 1024), Image.LANCZOS).save(os.path.join(args.out, "preview.png"))
    if args.preview_only:
        print("wrote preview.png")
        return
    lo, hi = tiles.write_tiles(img, args.out, MAP_TILE_ZOOM, MAP_TILE_X, MAP_TILE_Y)
    tiles.write_tiles_meta(args.out, {"surface": (lo, hi)}, MAP_TILE_ZOOM, MAP_TILE_X, MAP_TILE_Y)
    print(f"wrote tiles z{lo}..{hi}")


if __name__ == "__main__":
    main()
