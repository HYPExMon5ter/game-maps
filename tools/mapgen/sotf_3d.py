"""Sons of the Forest cave geometry from the game files: what `sotf_caves.py` needs to
build the caves' routes.

Cave geometry comes from the game's own collision: each cave's collision prefab (an
Addressables bundle, `caves_assets_cave<x>_*.bundle`, named parts such as
CaveCollisionFloorA or CaveCollisionWallsB) and the static colliders of its scene
(layer BasicCollider: rocks placed in caves, and the rooms of the bunkers, which are
mostly box colliders).

Writes to `build/` (not part of the map pack), per cave or bunker: a floor scan of its
collision (`floors-<id>.npz`), its water surfaces (`water-<id>.bin`, mesh format in
`mesh3d.py`), and its climbing ropes, ziplines and water volumes (`traversal.json`,
`water-volumes.npz`).

Usage:
    python -I tools/mapgen/sotf_3d.py --game "D:/SteamLibrary/steamapps/common/Sons Of The Forest" --out games/sons-of-the-forest
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from dataclasses import dataclass

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mesh3d  # noqa: E402
from sotf import TERRAIN_ORIGIN, load_terrain  # noqa: E402
from unity_world import Assets, Hierarchy, bundle_files, read_mesh  # noqa: E402

# Static environment collision.
BASIC_COLLIDER_LAYER = 22
# Scene roots outside the cave (set dressing around the entrances on the surface).
SKIP_ROOTS = re.compile(r"External|ExternalSetDressing", re.I)
# Scene files whose meshes the cave and bunker scenes reference.
SHARED_FILES = ("sharedassets7.assets", "sharedassets11.assets", "sharedassets15.assets", "sharedassets20.assets", "resources.assets")
# Water surfaces in caves: renderers on the Water layer (lakes, pools, streams). Lower
# levels of detail of the same surface are named ...LOD1_, ...LOD2_.
WATER_LAYER = 4
LOWER_LOD = re.compile(r"LOD[1-9]")
# Triangles facing up at least this much are the surface (not a lake's sides).
SURFACE_NORMAL_Y = 0.7
# The water volumes: where the player swims (trigger colliders on the Water layer). Lava
# pools are on that layer too.
WATER_VOLUME = re.compile(r"^(_Lake\w*Zone_|WaterZone|SwimVolume)")
NOT_WATER = re.compile(r"Lava", re.I)
# Climbing ropes hang from a rock (ClimbingRopeRock), with a grab point at each end;
# ziplines span two anchors.
ROPE = "ClimbingRopeRock"
ROPE_ENDS = ("TriggerTop", "TriggerBottom")
ZIPLINE = "ZipLineController"
ZIPLINE_ENDS = ("AnchorA", "AnchorB")
# The in-game floor scan starts this far under the terrain (`cave_scan.rs`).
SCAN_BELOW_GROUND = 0.3
# Floors per cell in the collision scan: as many as the cave grids keep
# (`sotf_caves.NAV_LAYERS`).
SCAN_LAYERS = 4
# Copies placed in a scene are numbered: "ClimbingRopeRock (3)".
COPY_NUMBER = re.compile(r" \(\d+\)$")
# Parts of a cave that the game turns on once it's opened up (Cave H's entrance): what's
# in them is there to use, just not yet.
OPENED_STATE = re.compile(r"Opened$")


def present(h: Hierarchy, tid: int) -> bool:
    """Whether a node is on, or only waits for its cave to be opened."""
    while tid:
        n = h.nodes[tid]
        if not n.active and not OPENED_STATE.search(n.name):
            return False
        tid = n.parent
    return True


@dataclass(frozen=True)
class Group:
    id: str
    name: str
    bundles: tuple[str, ...]  # Addressables bundle name prefixes
    scenes: tuple[int, ...]  # build scene indexes (`level<n>` files)


# Names match the game's cave and bunker pins.
GROUPS = (
    Group("cave-a", "Cave A", ("caves_assets_cavea_",), (15,)),
    Group("cave-b", "Cave B", ("caves_assets_caveb_",), (16,)),
    Group("cave-c", "Cave C", ("caves_assets_cavec_",), (17,)),
    Group("cave-d", "Cave D", ("caves_assets_caved_",), (18,)),
    Group("cave-f", "Cave F", ("caves_assets_cavef_",), (30,)),
    Group("cave-g", "Cave G", ("caves_assets_caveg_",), (31,)),
    Group("cave-h", "Cave H", (), (38,)),
    # The Luxury bunker scene also holds Cave E, the cave system leading to it.
    Group("bunker-luxury", "Bunker Luxury and Cave E", ("caves_assets_cavee_",), (26,)),
    Group("bunker-a", "Bunker A", (), (20,)),
    Group("bunker-b", "Bunker B", (), (21,)),
    Group("bunker-c", "Bunker C", (), (22,)),
    Group("bunker-entertainment", "Bunker Entertainment", (), (23,)),
    Group("bunker-food", "Bunker Food", (), (24,)),
    Group("bunker-residential", "Bunker Residential", (), (25,)),
)
# Not modelled: the snow caves (scene 19, IceCaves). They're single small chambers in
# the island's merged rock collision (level 7, MergedWorldCollision*), whose faces point
# out of the rock: a model of them shows only the outside of boulders.


def collider_parts(h: Hierarchy, layer: int | None, skip_roots: re.Pattern | None) -> tuple[list, list]:
    """World-space (positions, faces) of the enabled, solid mesh colliders below this
    file's roots, and its box colliders (matrix, center, size): (mesh parts, boxes)."""
    a = h.assets
    meshes, boxes = [], []
    seen = set()
    for tid, n in h.nodes.items():
        if layer is not None and n.layer != layer:
            continue
        if skip_roots and skip_roots.search(h.path(tid).split("/")[0]):
            continue
        colliders = [(c, cid) for c, cid in n.components if c in ("MeshCollider", "BoxCollider")]
        if not colliders or not h.active_in_hierarchy(tid):
            continue
        m = h.world(tid)
        for cname, cid in colliders:
            c = a.tt((h.key, cid))
            if c.get("m_IsTrigger") or not c.get("m_Enabled", 1):
                continue
            if cname == "BoxCollider":
                size, center = c["m_Size"], c["m_Center"]
                boxes.append((m, (center["x"], center["y"], center["z"]), (size["x"], size["y"], size["z"])))
                continue
            ref = a.resolve(h.key, c["m_Mesh"])
            # The same prefab can be placed twice at one spot (Cave D nests its halves).
            key = (ref, m.round(2).tobytes())
            if not ref or key in seen:
                continue
            seen.add(key)
            md = read_mesh(a, ref)
            if md is None or not md.submeshes:
                continue
            faces = np.concatenate(md.submeshes) if len(md.submeshes) > 1 else md.submeshes[0]
            meshes.append(mesh3d.transform(md.positions, faces, m))
    return meshes, boxes


def group_collision(a: Assets, g: Group, aa_dir: str, scene_keys: dict[int, str]) -> tuple[list, list]:
    """A cave's collision, from its bundles and scenes: (mesh parts, box colliders)."""
    meshes, boxes = [], []
    sources = [(Hierarchy(a, key), None, None) for prefix in g.bundles for path in bundle_files(aa_dir, prefix) for key in load_bundle(a, path)]
    sources += [(Hierarchy(a, scene_keys[s]), BASIC_COLLIDER_LAYER, SKIP_ROOTS) for s in g.scenes]
    for h, layer, skip in sources:
        m, b = collider_parts(h, layer, skip)
        meshes += m
        boxes += b
    return meshes, boxes


def write_floor_scan(g: Group, p: np.ndarray, f: np.ndarray, terrain: dict, build_dir: str) -> None:
    """The cave's floors and headroom from its full collision, ceilings included, on the
    in-game scan's terms (`build/floors-<id>.npz`): `sotf_caves.py` fills what the scan
    missed from it (the scan caps a cave's size, and can miss parts of big caves). Like
    the scan, only what's under the terrain counts (not the rocks at an entrance), and
    the terrain stops headroom."""
    t0 = time.time()
    scan = mesh3d.floor_scan(p[f].astype(np.float32), layers=SCAN_LAYERS, ground=lambda x, z: ground_height(terrain, x, z), below_ground=SCAN_BELOW_GROUND)
    os.makedirs(build_dir, exist_ok=True)
    np.savez_compressed(os.path.join(build_dir, f"floors-{g.id}.npz"), **scan)
    found = int((scan["floors"] != mesh3d.NO_FLOOR).sum())
    print(f"  floor scan: {scan['width']}x{scan['height']} cells, {found} floors ({time.time() - t0:.0f}s)")


def water_surfaces(h: Hierarchy) -> tuple[np.ndarray, np.ndarray]:
    """The water surfaces of a scene: its Water-layer renderers' upward faces, in world
    space (one level of detail each)."""
    a = h.assets
    parts = []
    for tid, n in h.nodes.items():
        if n.layer != WATER_LAYER or LOWER_LOD.search(n.name) or not h.active_in_hierarchy(tid):
            continue
        renderer = next((cid for cname, cid in n.components if cname == "MeshRenderer"), None)
        mf = h.component(tid, "MeshFilter")
        if renderer is None or mf is None or not a.tt((h.key, renderer)).get("m_Enabled", 1):
            continue
        ref = a.resolve(h.key, mf["m_Mesh"])
        md = read_mesh(a, ref) if ref else None
        if md is None or not md.submeshes:
            continue
        p, f = mesh3d.transform(md.positions, np.concatenate(md.submeshes), h.world(tid))
        a_, b_, c_ = p[f[:, 0]], p[f[:, 1]], p[f[:, 2]]
        normal = np.cross(b_ - a_, c_ - a_)
        length = np.linalg.norm(normal, axis=1)
        flat = np.abs(normal[:, 1]) >= SURFACE_NORMAL_Y * np.maximum(length, 1e-9)
        if flat.any():
            parts.append(mesh3d.drop_unused(p, f[flat]))
    return mesh3d.weld(*mesh3d.merge(parts))


# Water this far outside a cave's collision still belongs to it (meters).
WATER_MARGIN = 2.0


def build_water(a: Assets, g: Group, scene_keys: dict[int, str], lo: np.ndarray, hi: np.ndarray) -> tuple[np.ndarray, np.ndarray] | None:
    """The cave's water surfaces: its scenes' water within the bounds of its collision
    (a scene can hold the sea or a lake outside, near an entrance)."""
    p, f = mesh3d.merge([water_surfaces(Hierarchy(a, scene_keys[s])) for s in g.scenes])
    if not len(f):
        return None
    c = p[f].mean(axis=1)
    inside = ((c >= lo - WATER_MARGIN) & (c <= hi + WATER_MARGIN)).all(axis=1)
    return mesh3d.drop_unused(p, f[inside]) if inside.any() else None


def traversal(h: Hierarchy) -> dict:
    """The scene's climbing ropes ({top, bottom}) and ziplines ({high, low}: they're
    ridden downhill), as world points."""
    ropes, ziplines = [], []
    for tid, n in h.nodes.items():
        name = COPY_NUMBER.sub("", n.name)
        if name not in (ROPE, ZIPLINE) or not present(h, tid):
            continue
        ends = {h.nodes[c].name: [round(float(v), 2) for v in h.world(c)[:3, 3]] for c in n.children if c in h.nodes}
        if name == ROPE and all(e in ends for e in ROPE_ENDS):
            ropes.append({"top": ends[ROPE_ENDS[0]], "bottom": ends[ROPE_ENDS[1]]})
        elif name == ZIPLINE and all(e in ends for e in ZIPLINE_ENDS):
            a, b = (ends[e] for e in ZIPLINE_ENDS)
            high, low = (a, b) if a[1] >= b[1] else (b, a)
            ziplines.append({"high": high, "low": low})
    return {"ropes": ropes, "ziplines": ziplines}


def water_volumes(h: Hierarchy) -> list[np.ndarray]:
    """The scene's water volumes, each as world-space triangles (n, 3 corners, xyz)."""
    a = h.assets
    out = []
    for tid, n in h.nodes.items():
        if n.layer != WATER_LAYER or not WATER_VOLUME.search(n.name) or NOT_WATER.search(n.name) or not h.active_in_hierarchy(tid):
            continue
        collider = h.component(tid, "MeshCollider")
        ref = a.resolve(h.key, collider["m_Mesh"]) if collider and collider.get("m_Enabled", 1) else None
        md = read_mesh(a, ref) if ref else None
        if md is None or not md.submeshes:
            continue
        p, f = mesh3d.transform(md.positions, np.concatenate(md.submeshes), h.world(tid))
        out.append(p[f].astype(np.float32))
    return out


def write_traversal(a: Assets, groups: list[Group], aa_dir: str, scene_keys: dict[int, str], build_dir: str) -> None:
    """What the cave routes need besides the floors (`sotf_caves.py` reads it): ropes and
    ziplines in `build/traversal.json`, water volumes in `build/water-volumes.npz`, both
    by cave (from its scenes and its collision bundles: some ropes are in those). Caves
    not built this time keep what they had."""
    os.makedirs(build_dir, exist_ok=True)
    json_path = os.path.join(build_dir, "traversal.json")
    npz_path = os.path.join(build_dir, "water-volumes.npz")
    links = json.load(open(json_path)) if os.path.exists(json_path) else {}
    volumes = dict(np.load(npz_path)) if os.path.exists(npz_path) else {}
    for g in groups:
        keys = [scene_keys[s] for s in g.scenes] + [k for prefix in g.bundles for path in bundle_files(aa_dir, prefix) for k in load_bundle(a, path)]
        found = {"ropes": [], "ziplines": []}
        water = []
        for key in keys:
            h = Hierarchy(a, key)
            for kind, items in traversal(h).items():
                found[kind] += items
            water += water_volumes(h)
        links[g.id] = found
        volumes = {k: v for k, v in volumes.items() if not k.startswith(f"{g.id}/")}
        volumes.update({f"{g.id}/{i}": v for i, v in enumerate(water)})
    with open(json_path, "w") as f:
        json.dump({g.id: links[g.id] for g in GROUPS if g.id in links}, f, indent=1)
    np.savez_compressed(npz_path, **volumes)
    print(f"traversal: {sum(len(v['ropes']) for v in links.values())} ropes, {sum(len(v['ziplines']) for v in links.values())} ziplines, {len(volumes)} water volumes")


_bundles: dict[str, list[str]] = {}


def load_bundle(a: Assets, path: str) -> list[str]:
    """Serialized files of a bundle, loaded once."""
    if path not in _bundles:
        _bundles[path] = a.load(path)
    return _bundles[path]


def load_scenes(data_dir: str, groups) -> tuple[Assets, dict[int, str]]:
    a = Assets()
    scenes = sorted({s for g in groups for s in g.scenes})
    a.load(*[os.path.join(data_dir, f"level{s}") for s in scenes])
    for name in SHARED_FILES + tuple(f"sharedassets{s}.assets" for s in scenes):
        path = os.path.join(data_dir, name)
        if os.path.exists(path):
            a.load(path)
    return a, {s: f"level{s}" for s in scenes}


def ground_height(t: dict, x: np.ndarray, z: np.ndarray) -> np.ndarray:
    """The terrain's height at world points (its nearest sample)."""
    h = t["heights"]
    r = np.clip(np.round((z - TERRAIN_ORIGIN[2]) / t["cell"]).astype(np.int64), 0, h.shape[0] - 1)
    c = np.clip(np.round((x - TERRAIN_ORIGIN[0]) / t["cell"]).astype(np.int64), 0, h.shape[1] - 1)
    return h[r, c]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--game", required=True, help="Sons Of The Forest install directory")
    ap.add_argument("--out", required=True, help="games/sons-of-the-forest directory")
    ap.add_argument("--only", nargs="*", help="group ids to build (for testing)")
    args = ap.parse_args()

    data_dir = os.path.join(args.game, "SonsOfTheForest_Data")
    aa_dir = os.path.join(data_dir, "StreamingAssets", "aa", "StandaloneWindows64")
    groups = [g for g in GROUPS if not args.only or g.id in args.only]
    t0 = time.time()
    a, scene_keys = load_scenes(data_dir, groups)
    print(f"loaded scenes in {time.time() - t0:.0f}s")
    build_dir = os.path.join(args.out, "build")
    os.makedirs(build_dir, exist_ok=True)
    terrain_data = load_terrain(data_dir)
    for g in groups:
        parts, boxes = group_collision(a, g, aa_dir, scene_keys)
        p, f = mesh3d.merge(parts + [mesh3d.box_triangles(c, s, m) for m, c, s in boxes])
        if not len(f):
            print(f"{g.id}: no collision found, skipped")
            continue
        corners = p[f].reshape(-1, 3)
        lo, hi = corners.min(axis=0), corners.max(axis=0)
        print(f"{g.id}: {len(f)} triangles, y {lo[1]:.0f}..{hi[1]:.0f}")
        write_floor_scan(g, p, f, terrain_data, build_dir)
        water_path = os.path.join(build_dir, f"water-{g.id}.bin")
        water = build_water(a, g, scene_keys, lo, hi)
        if water:
            info = mesh3d.write_mesh(water_path, *water)
            print(f"  water: {info['triangles']} triangles, y {info['min'][1]:.0f}..{info['max'][1]:.0f}")
        elif os.path.exists(water_path):
            os.remove(water_path)
    write_traversal(a, groups, aa_dir, scene_keys, build_dir)

    print(f"done in {time.time() - t0:.0f}s")


if __name__ == "__main__":
    main()
