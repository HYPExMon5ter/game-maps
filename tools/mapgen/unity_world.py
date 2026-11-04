"""Reading Unity scenes and asset bundles for map rendering: cross-file references,
world transforms, renderers, meshes (decoded with numpy) and material colors.

Game-agnostic; game scripts (e.g. `sunkenland.py`) decide which objects to render.
Everything is read with UnityPy typetrees, so it works for any Unity version UnityPy
supports, as long as the files carry type trees (scenes and bundles always do for
built-in classes such as Transform, MeshFilter, Mesh and Material).
"""
from __future__ import annotations

import base64
import json
import ntpath
import os
import struct
from dataclasses import dataclass, field

import numpy as np
import UnityPy
from UnityPy.helpers.ResourceReader import get_resource_data

# Vertex attribute channels (Unity 2018.1+ `VertexAttribute` order).
CH_POSITION, CH_NORMAL, CH_UV0 = 0, 1, 4
# `VertexAttributeFormat` -> (numpy dtype, normalize divisor or None).
VERTEX_FORMATS = {
    0: (np.float32, None),
    1: (np.float16, None),
    2: (np.uint8, 255.0),
    3: (np.int8, 127.0),
    4: (np.uint16, 65535.0),
    5: (np.int16, 32767.0),
    6: (np.uint8, None),
    7: (np.int8, None),
    8: (np.uint16, None),
    9: (np.int16, None),
    10: (np.uint32, None),
    11: (np.int32, None),
}
TOPOLOGY_TRIANGLES = 0


def file_key(path: str) -> str:
    """Normalized name a serialized file is referenced by (`externals` paths are
    `sharedassets0.assets`, `archive:/CAB-x/CAB-x`, `Library/unity default resources`, …)."""
    return ntpath.basename(path.replace("/", "\\")).lower()


class Assets:
    """Loaded serialized files with PPtr resolution across them."""

    def __init__(self):
        self.files: dict[str, object] = {}  # key -> SerializedFile
        self.objects: dict[str, dict[int, object]] = {}
        self._tt: dict[tuple[str, int], dict] = {}
        self._generator = None
        self._scripts: dict[tuple[str, int], tuple[str, str]] = {}

    def use_game_scripts(self, game_dir: str) -> None:
        """Enables reading MonoBehaviours (script fields) using the game's managed
        assemblies (Mono) or IL2CPP metadata, via TypeTreeGeneratorAPI."""
        from UnityPy.helpers.TypeTreeGenerator import TypeTreeGenerator

        version = next(iter(self.files.values())).unity_version
        self._generator = TypeTreeGenerator(version)
        self._generator.load_local_game(game_dir)

    def script(self, owner: str, behaviour: dict) -> tuple[str, str] | None:
        """(assembly, full class name) of a MonoBehaviour typetree (base fields)."""
        ref = self.resolve(owner, behaviour.get("m_Script", {}))
        if not ref:
            return None
        if ref not in self._scripts:
            s = self.tt(ref)
            name = f"{s['m_Namespace']}.{s['m_ClassName']}" if s.get("m_Namespace") else s["m_ClassName"]
            self._scripts[ref] = (s["m_AssemblyName"], name)
        return self._scripts[ref]

    def behaviour(self, ref: tuple[str, int]) -> tuple[str, dict] | None:
        """(class name, full typetree including script fields) of a MonoBehaviour."""
        key = ("mb", ref)
        if key in self._tt:
            return self._tt[key]
        raw = self.obj(ref).read(check_read=False)
        info = self.script(ref[0], {"m_Script": {"m_FileID": raw.m_Script.m_FileID, "m_PathID": raw.m_Script.m_PathID}})
        result = None
        if info and self._generator is not None:
            try:
                nodes = self._generator.get_nodes_up(info[0], info[1])
                result = (info[1], self.obj(ref).read_typetree(nodes))
            except Exception:
                result = (info[1], None)
        elif info:
            result = (info[1], None)
        self._tt[key] = result
        return result

    def load(self, *paths: str) -> list[str]:
        """Loads files or bundles; returns the keys of the serialized files added."""
        added = []
        for path in paths:
            env = UnityPy.load(path)
            for sf in _serialized_files(env):
                key = file_key(sf.name)
                if key in self.files:
                    continue
                self.files[key] = sf
                self.objects[key] = dict(sf.objects)
                added.append(key)
        return added

    def resolve(self, owner: str, pptr: dict) -> tuple[str, int] | None:
        """(file key, path id) a PPtr in `owner` points to, if that file is loaded."""
        fid, pid = pptr.get("m_FileID", 0), pptr.get("m_PathID", 0)
        if not pid:
            return None
        if fid == 0:
            key = owner
        else:
            externals = self.files[owner].externals
            if fid - 1 >= len(externals):
                return None
            key = file_key(externals[fid - 1].path)
        return (key, pid) if pid in self.objects.get(key, {}) else None

    def obj(self, ref: tuple[str, int]):
        return self.objects[ref[0]][ref[1]]

    def tt(self, ref: tuple[str, int]) -> dict:
        """Typetree of an object, cached."""
        if ref not in self._tt:
            self._tt[ref] = self.obj(ref).read_typetree()
        return self._tt[ref]

    def deref(self, owner: str, pptr: dict) -> tuple[tuple[str, int], dict] | None:
        ref = self.resolve(owner, pptr)
        return (ref, self.tt(ref)) if ref else None


def _serialized_files(env):
    from UnityPy.files import SerializedFile

    out = []

    def walk(f):
        if isinstance(f, SerializedFile):
            out.append(f)
        for child in (getattr(f, "files", None) or {}).values():
            walk(child)

    for f in env.files.values():
        walk(f)
    return out


# --- transforms --------------------------------------------------------------------


def trs_matrix(pos, rot, scale) -> np.ndarray:
    """4x4 matrix from Unity position (x,y,z), rotation quaternion (x,y,z,w), scale."""
    x, y, z, w = rot
    r = np.array(
        [
            [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
        ],
        dtype=np.float64,
    )
    m = np.eye(4)
    m[:3, :3] = r * np.asarray(scale, dtype=np.float64)[None, :]
    m[:3, 3] = pos
    return m


def local_matrix(t: dict) -> np.ndarray:
    p, r, s = t["m_LocalPosition"], t["m_LocalRotation"], t["m_LocalScale"]
    return trs_matrix((p["x"], p["y"], p["z"]), (r["x"], r["y"], r["z"], r["w"]), (s["x"], s["y"], s["z"]))


@dataclass
class Node:
    """A GameObject with its transform, inside one serialized file."""

    file: str
    go: int
    transform: int
    name: str
    active: bool
    layer: int
    parent: int | None
    children: list[int]
    components: list[tuple[str, int]]  # (class name, path id) in the same file


class Hierarchy:
    """GameObject/Transform tree of one serialized file (a scene or a bundle CAB)."""

    def __init__(self, assets: Assets, key: str):
        self.assets, self.key = assets, key
        self.nodes: dict[int, Node] = {}  # by transform path id
        self.by_go: dict[int, int] = {}
        objs = assets.objects[key]
        transforms = {pid: o for pid, o in objs.items() if o.type.name in ("Transform", "RectTransform")}
        for pid, o in transforms.items():
            t = assets.tt((key, pid))
            go_id = t["m_GameObject"]["m_PathID"]
            go = assets.tt((key, go_id)) if go_id in objs else {"m_Name": "", "m_IsActive": 1, "m_Layer": 0, "m_Component": []}
            comps = []
            for c in go.get("m_Component", []):
                cid = c["component"]["m_PathID"]
                if cid in objs:
                    comps.append((objs[cid].type.name, cid))
            parent = t["m_Father"]["m_PathID"] or None
            self.nodes[pid] = Node(
                key, go_id, pid, go["m_Name"], bool(go["m_IsActive"]), go["m_Layer"], parent if parent in transforms else None,
                [c["m_PathID"] for c in t["m_Children"]], comps,
            )
            self.by_go[go_id] = pid
        self._world: dict[int, np.ndarray] = {}

    def local(self, tid: int) -> np.ndarray:
        return local_matrix(self.assets.tt((self.key, tid)))

    def world(self, tid: int, root: np.ndarray | None = None) -> np.ndarray:
        """World matrix; `root` replaces the matrix above this file's roots (prefabs
        placed under a scene parent)."""
        if root is None and tid in self._world:
            return self._world[tid]
        n = self.nodes[tid]
        parent = self.world(n.parent, root) if n.parent else (root if root is not None else np.eye(4))
        m = parent @ self.local(tid)
        if root is None:
            self._world[tid] = m
        return m

    def path(self, tid: int) -> str:
        names = []
        while tid:
            n = self.nodes[tid]
            names.append(n.name)
            tid = n.parent
        return "/".join(reversed(names))

    def active_in_hierarchy(self, tid: int) -> bool:
        while tid:
            n = self.nodes[tid]
            if not n.active:
                return False
            tid = n.parent
        return True

    def descendants(self, tid: int, active_only: bool = False):
        """`tid` and everything below it; with `active_only`, inactive subtrees are skipped."""
        stack = [tid]
        while stack:
            t = stack.pop()
            if active_only and not self.nodes[t].active:
                continue
            yield t
            stack.extend(c for c in self.nodes[t].children if c in self.nodes)

    def component(self, tid: int, class_name: str) -> dict | None:
        for cname, cid in self.nodes[tid].components:
            if cname == class_name:
                return self.assets.tt((self.key, cid))
        return None

    def roots(self):
        return [t for t, n in self.nodes.items() if n.parent is None]


# --- renderers -----------------------------------------------------------------------


@dataclass
class Renderer:
    mesh: tuple[str, int]
    materials: list[tuple[str, int] | None]
    matrix: np.ndarray
    path: str = ""
    layer: int = 0


def lod_excluded_renderers(h: Hierarchy, tid: int) -> set[tuple[str, int]]:
    """Renderers in every LODGroup below `tid` that aren't in the group's LOD 0."""
    out = set()
    for t in h.descendants(tid):
        g = h.component(t, "LODGroup")
        if not g or not g.get("m_LODs"):
            continue
        for lod in g["m_LODs"][1:]:
            for r in lod["renderers"]:
                ref = h.assets.resolve(h.key, r["renderer"])
                if ref:
                    out.add(ref)
        for r in g["m_LODs"][0]["renderers"]:
            ref = h.assets.resolve(h.key, r["renderer"])
            out.discard(ref)
    return out


def collect_renderers(h: Hierarchy, tid: int, root: np.ndarray | None = None, include_inactive: bool = False) -> list[Renderer]:
    """Enabled MeshRenderers (with MeshFilters) below `tid`, LOD 0 only."""
    skip = lod_excluded_renderers(h, tid)
    out = []
    for t in h.descendants(tid, active_only=not include_inactive):
        n = h.nodes[t]
        mr_id = next((cid for cname, cid in n.components if cname == "MeshRenderer"), None)
        mf = h.component(t, "MeshFilter")
        if mr_id is None or mf is None or (h.key, mr_id) in skip:
            continue
        mr = h.assets.tt((h.key, mr_id))
        if not mr.get("m_Enabled", 1):
            continue
        mesh = h.assets.resolve(h.key, mf["m_Mesh"])
        if not mesh:
            continue
        mats = [h.assets.resolve(h.key, m) for m in mr.get("m_Materials", [])]
        out.append(Renderer(mesh, mats, h.world(t, root), h.path(t), n.layer))
    return out


# --- meshes --------------------------------------------------------------------------


@dataclass
class MeshData:
    positions: np.ndarray  # (n, 3) float32
    uv: np.ndarray | None  # (n, 2) float32
    submeshes: list[np.ndarray] = field(default_factory=list)  # (m, 3) uint32 triangles each


def read_mesh(assets: Assets, ref: tuple[str, int]) -> MeshData | None:
    """Decodes a Mesh's positions, UV0 and triangle lists with numpy."""
    m = assets.tt(ref)
    if m.get("m_MeshCompression", 0):
        return _read_mesh_unitypy(assets, ref)
    vd = m["m_VertexData"]
    count = vd["m_VertexCount"]
    if not count:
        return None
    data = vd.get("m_DataSize") or b""
    sd = m.get("m_StreamData") or {}
    if sd.get("path"):
        data = get_resource_data(sd["path"], assets.files[ref[0]], sd["offset"], sd["size"])
    data = bytes(data)
    channels = vd["m_Channels"]

    # Stream layout: channels grouped by stream, streams packed and 16-byte aligned.
    stream_count = 1 + max(c["stream"] for c in channels)
    strides, offsets, off = [], [], 0
    for s in range(stream_count):
        stride = sum((c["dimension"] & 0xF) * np.dtype(VERTEX_FORMATS[c["format"]][0]).itemsize for c in channels if c["stream"] == s and c["dimension"])
        strides.append(stride)
        offsets.append(off)
        off = (off + count * stride + 15) & ~15

    def channel(i):
        c = channels[i] if i < len(channels) else None
        if not c or not (c["dimension"] & 0xF):
            return None
        dtype, norm = VERTEX_FORMATS[c["format"]]
        dim = c["dimension"] & 0xF
        stride = strides[c["stream"]]
        buf = np.frombuffer(data, dtype=np.uint8, count=count * stride, offset=offsets[c["stream"]]).reshape(count, stride)
        raw = buf[:, c["offset"] : c["offset"] + dim * np.dtype(dtype).itemsize].copy().view(dtype).reshape(count, dim)
        out = raw.astype(np.float32)
        return out / norm if norm else out

    pos = channel(CH_POSITION)
    if pos is None:
        return None
    uv = channel(CH_UV0)
    index_dtype = np.uint16 if m.get("m_IndexFormat", 0) == 0 else np.uint32
    indices = np.frombuffer(bytes(m["m_IndexBuffer"]), dtype=index_dtype).astype(np.uint32)
    subs = []
    for sm in m["m_SubMeshes"]:
        if sm.get("topology", 0) != TOPOLOGY_TRIANGLES:
            subs.append(np.zeros((0, 3), np.uint32))
            continue
        first = sm["firstByte"] // np.dtype(index_dtype).itemsize
        tri = indices[first : first + sm["indexCount"]]
        tri = tri[: len(tri) // 3 * 3].reshape(-1, 3) + np.uint32(sm.get("baseVertex", 0))
        subs.append(tri[(tri < count).all(axis=1)])
    return MeshData(pos[:, :3], uv[:, :2] if uv is not None else None, subs)


def _read_mesh_unitypy(assets: Assets, ref) -> MeshData | None:
    from UnityPy.helpers.MeshHelper import MeshHandler

    mesh = assets.obj(ref).read()
    h = MeshHandler(mesh)
    h.process()
    if not h.m_Vertices:
        return None
    pos = np.asarray(h.m_Vertices, dtype=np.float32)[:, :3]
    uv = np.asarray(h.m_UV0, dtype=np.float32)[:, :2] if h.m_UV0 else None
    subs = [np.asarray(t, dtype=np.uint32).reshape(-1, 3) for t in h.get_triangles()]
    return MeshData(pos, uv, subs)


# --- materials -----------------------------------------------------------------------

# Texture/color property names that hold the base (albedo) color, most specific first.
BASE_TEXTURES = ("_BaseColorMap", "_BaseMap", "_MainTex", "_Albedo", "_AlbedoMap", "_Diffuse", "_DiffuseMap", "_MainTexture")
BASE_COLORS = ("_BaseColor", "_Color", "_MainColor", "_AlbedoColor", "_Tint")


@dataclass
class MaterialInfo:
    name: str
    shader: str
    color: tuple[float, float, float, float]
    texture: tuple[str, int] | None
    tiling: tuple[float, float, float, float]  # scale x, y, offset x, y
    transparent: bool
    alpha_clip: float | None


def _props(lst) -> dict:
    out = {}
    for item in lst or []:
        if isinstance(item, (list, tuple)) and len(item) == 2:
            out[item[0]] = item[1]
        elif isinstance(item, dict) and "first" in item:
            out[item["first"]] = item["second"]
    return out


def read_material(assets: Assets, ref: tuple[str, int]) -> MaterialInfo:
    m = assets.tt(ref)
    props = m.get("m_SavedProperties", {})
    texs, colors, floats = _props(props.get("m_TexEnvs")), _props(props.get("m_Colors")), _props(props.get("m_Floats"))
    shader = ""
    sh = assets.deref(ref[0], m.get("m_Shader", {}))
    if sh:
        shader = sh[1].get("m_ParsedForm", {}).get("m_Name", "") or sh[1].get("m_Name", "")
    color = (1.0, 1.0, 1.0, 1.0)
    for name in BASE_COLORS:
        if name in colors:
            c = colors[name]
            color = (c["r"], c["g"], c["b"], c["a"])
            break
    texture, tiling = None, (1.0, 1.0, 0.0, 0.0)
    for name in BASE_TEXTURES:
        t = texs.get(name)
        if t:
            r = assets.resolve(ref[0], t["m_Texture"])
            if r:
                texture = r
                tiling = (t["m_Scale"]["x"], t["m_Scale"]["y"], t["m_Offset"]["x"], t["m_Offset"]["y"])
                break
    tags = m.get("stringTagMap") or m.get("m_StringTagMap")
    tags = _props(tags) if tags else {}
    render_queue = m.get("m_CustomRenderQueue", -1)
    transparent = floats.get("_SurfaceType", 0) >= 1 or tags.get("RenderType") == "Transparent" or render_queue >= 2950
    alpha_clip = floats.get("_AlphaCutoff", floats.get("_Cutoff", 0.5)) if floats.get("_AlphaCutoffEnable", floats.get("_AlphaClip", 0)) else None
    return MaterialInfo(m.get("m_Name", ""), shader, color, texture, tiling, transparent, alpha_clip)


def read_texture_rgba(assets: Assets, ref: tuple[str, int], size: int) -> np.ndarray | None:
    """A texture decoded and resized to `size`x`size` RGBA (uint8)."""
    try:
        img = assets.obj(ref).read().image
    except Exception:
        return None
    if img is None:
        return None
    img = img.convert("RGBA").resize((size, size))
    return np.asarray(img, dtype=np.uint8)


def addressables_catalog(path: str) -> tuple[dict[str, list[str]], list[str]]:
    """Decodes an Addressables `catalog.json`: string keys (asset GUIDs, addresses)
    to internal ids (asset paths, bundle paths), plus every internal id."""
    c = json.load(open(path, encoding="utf-8"))
    keys = base64.b64decode(c["m_KeyDataString"])
    buckets = base64.b64decode(c["m_BucketDataString"])
    entries = base64.b64decode(c["m_EntryDataString"])

    def read_key(off: int):
        kind = keys[off]
        if kind in (0, 1):  # ASCII / UTF-16 string
            n = struct.unpack_from("<i", keys, off + 1)[0]
            raw = keys[off + 5 : off + 5 + n]
            return raw.decode("ascii" if kind == 0 else "utf-16-le")
        return None

    ids = c["m_InternalIds"]
    entry_count = struct.unpack_from("<i", entries, 0)[0]
    entry_ids = [struct.unpack_from("<7i", entries, 4 + i * 28)[0] for i in range(entry_count)]
    out: dict[str, list[str]] = {}
    off = 4
    for _ in range(struct.unpack_from("<i", buckets, 0)[0]):
        data_off, count = struct.unpack_from("<ii", buckets, off)
        off += 8
        members = struct.unpack_from(f"<{count}i", buckets, off)
        off += 4 * count
        key = read_key(data_off)
        if key is not None:
            out[key] = [ids[entry_ids[e]] for e in members]
    return out, ids


def bundle_files(directory: str, prefix: str) -> list[str]:
    return sorted(os.path.join(directory, f) for f in os.listdir(directory) if f.startswith(prefix) and f.endswith(".bundle"))
