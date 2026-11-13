"""3D geometry shared by every game's map tools: box colliders as triangles, floor
scans of collision, and a compact mesh file format.

Positions are game world units (Unity: x east, y up, z north).

Mesh files (`.bin`, little-endian):
    0   "GM3D"
    4   u32 version (2)
    8   u32 vertex count
    12  u32 triangle count
    16  f32 min x, y, z
    28  f32 max x, y, z
    40  u16 positions, 3 per vertex, quantized over min..max
    ..  padding to 4 bytes
    ..  i8 normals, 2 per vertex (octahedral encoding, game axes, smooth: averaged over
        the faces around each vertex, weighted by area)
    ..  padding to 4 bytes
    ..  triangle indices: u16 when there are at most 65536 vertices, else u32

Triangles keep the source's winding: front faces are clockwise seen from the front, as
in Unity (the cross product of their edges points to the front). The smooth normals
round off corners too; the viewer falls back to the face's own normal where the two
differ a lot, so box corners stay sharp.
"""
from __future__ import annotations

import os
import struct

import numpy as np

MAGIC = b"GM3D"
VERSION = 2
HEADER = struct.Struct("<4sII I3f3f")
QUANT = 65535

# Unit cube faces as two triangles each, wound clockwise seen from outside (Unity's
# front faces), with the outward normal axis and sign.
_CUBE = np.array(
    [(x, y, z) for x in (-0.5, 0.5) for y in (-0.5, 0.5) for z in (-0.5, 0.5)],
    dtype=np.float64,
)  # index = 4*ix + 2*iy + iz
_CUBE_FACES = [
    # (quad corners in clockwise order seen from outside, axis, sign)
    ((0, 2, 6, 4), 2, -1),  # -z
    ((1, 5, 7, 3), 2, 1),  # +z
    ((0, 1, 3, 2), 0, -1),  # -x
    ((4, 6, 7, 5), 0, 1),  # +x
    ((0, 4, 5, 1), 1, -1),  # -y
    ((2, 3, 7, 6), 1, 1),  # +y
]


def box_triangles(center, size, matrix: np.ndarray, top: bool = True) -> tuple[np.ndarray, np.ndarray]:
    """A box collider as world-space triangles: (8, 3) positions and (n, 3) faces.
    `top=False` leaves out the face whose normal points most upward in world space."""
    local = _CUBE * np.asarray(size, dtype=np.float64) + np.asarray(center, dtype=np.float64)
    world = local @ matrix[:3, :3].T + matrix[:3, 3]
    faces = []
    up_face = None
    if not top:
        ups = [(matrix[:3, axis] * sign)[1] / (np.linalg.norm(matrix[:3, axis]) or 1) for _, axis, sign in _CUBE_FACES]
        up_face = int(np.argmax(ups))
    for i, (quad, _, _) in enumerate(_CUBE_FACES):
        if i == up_face:
            continue
        a, b, c, d = quad
        faces += [(a, b, c), (a, c, d)]
    # A mirroring transform (negative scale) turns the winding inside out.
    if np.linalg.det(matrix[:3, :3]) < 0:
        faces = [(a, c, b) for a, b, c in faces]
    return world.astype(np.float32), np.array(faces, dtype=np.uint32)


def transform(positions: np.ndarray, faces: np.ndarray, matrix: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Mesh positions into world space; mirrored transforms keep their faces' front side."""
    world = (positions.astype(np.float64) @ matrix[:3, :3].T + matrix[:3, 3]).astype(np.float32)
    if np.linalg.det(matrix[:3, :3]) < 0:
        faces = faces[:, [0, 2, 1]]
    return world, faces


def merge(parts: list[tuple[np.ndarray, np.ndarray]]) -> tuple[np.ndarray, np.ndarray]:
    """Concatenates (positions, faces) parts into one mesh."""
    parts = [(p, f) for p, f in parts if len(p) and len(f)]
    if not parts:
        return np.zeros((0, 3), np.float32), np.zeros((0, 3), np.uint32)
    offsets = np.cumsum([0] + [len(p) for p, _ in parts[:-1]])
    positions = np.concatenate([p for p, _ in parts]).astype(np.float32)
    faces = np.concatenate([f.astype(np.uint32) + np.uint32(o) for (_, f), o in zip(parts, offsets)])
    return positions, faces


def weld(positions: np.ndarray, faces: np.ndarray, tolerance: float = 0.01) -> tuple[np.ndarray, np.ndarray]:
    """Merges vertices closer than `tolerance` and drops triangles that collapse."""
    if not len(faces):
        return positions, faces
    keys = np.round(positions / tolerance).astype(np.int64)
    _, first, inverse = np.unique(keys, axis=0, return_index=True, return_inverse=True)
    faces = inverse.reshape(-1)[faces]
    keep = (faces[:, 0] != faces[:, 1]) & (faces[:, 1] != faces[:, 2]) & (faces[:, 0] != faces[:, 2])
    return positions[first], faces[keep].astype(np.uint32)


def drop_unused(positions: np.ndarray, faces: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    used = np.unique(faces)
    remap = np.zeros(len(positions), np.uint32)
    remap[used] = np.arange(len(used), dtype=np.uint32)
    return positions[used], remap[faces]


def vertex_normals(positions: np.ndarray, faces: np.ndarray) -> np.ndarray:
    """Smooth unit normals: each vertex's faces' normals, weighted by their area."""
    p = positions.astype(np.float64)
    a, b, c = p[faces[:, 0]], p[faces[:, 1]], p[faces[:, 2]]
    face = np.cross(b - a, c - a)  # length = twice the area
    out = np.zeros_like(p)
    for i in range(3):
        np.add.at(out, faces[:, i], face)
    length = np.linalg.norm(out, axis=1, keepdims=True)
    return np.where(length > 0, out / np.where(length > 0, length, 1), (0.0, 1.0, 0.0)).astype(np.float32)


def oct_encode(n: np.ndarray) -> np.ndarray:
    """Unit vectors as 2 signed bytes each (octahedral mapping)."""
    n = n / np.abs(n).sum(axis=1, keepdims=True)
    xy = n[:, :2].copy()
    below = n[:, 2] < 0
    folded = (1 - np.abs(xy[:, ::-1])) * np.where(xy >= 0, 1.0, -1.0)
    xy[below] = folded[below]
    return np.clip(np.round(xy * 127), -127, 127).astype(np.int8)


def oct_decode(e: np.ndarray) -> np.ndarray:
    xy = e.astype(np.float32) / 127
    z = 1 - np.abs(xy).sum(axis=1)
    below = z < 0
    folded = (1 - np.abs(xy[:, ::-1])) * np.where(xy >= 0, 1.0, -1.0)
    xy[below] = folded[below]
    n = np.column_stack([xy, z])
    return n / np.linalg.norm(n, axis=1, keepdims=True)


def _pad(buf: bytearray) -> None:
    buf += b"\0" * (-len(buf) % 4)


def encode_mesh(positions: np.ndarray, faces: np.ndarray) -> bytes:
    """The `.bin` mesh format (see the module docstring)."""
    positions, faces = drop_unused(positions, faces)
    lo = positions.min(axis=0)
    hi = positions.max(axis=0)
    span = np.where(hi > lo, hi - lo, 1.0)
    q = np.round((positions - lo) / span * QUANT).astype("<u2")
    out = bytearray(HEADER.pack(MAGIC, VERSION, len(positions), len(faces), *lo.tolist(), *hi.tolist()))
    out += q.tobytes()
    _pad(out)
    out += oct_encode(vertex_normals(positions, faces)).tobytes()
    _pad(out)
    index_type = "<u2" if len(positions) <= 65536 else "<u4"
    out += faces.astype(index_type).tobytes()
    return bytes(out)


def decode_mesh(data: bytes) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Reads a `.bin` mesh back: positions (dequantized), unit normals, faces."""
    magic, version, n, t, *bounds = HEADER.unpack_from(data, 0)
    if magic != MAGIC or version != VERSION:
        raise ValueError(f"not a GM3D v{VERSION} mesh")
    lo, hi = np.array(bounds[:3], np.float32), np.array(bounds[3:], np.float32)
    off = HEADER.size
    q = np.frombuffer(data, "<u2", n * 3, off).reshape(n, 3).astype(np.float32)
    off += n * 6
    off += -off % 4
    normals = oct_decode(np.frombuffer(data, np.int8, n * 2, off).reshape(n, 2))
    off += n * 2
    off += -off % 4
    index_type = "<u2" if n <= 65536 else "<u4"
    faces = np.frombuffer(data, index_type, t * 3, off).reshape(t, 3).astype(np.uint32)
    return lo + q / QUANT * (hi - lo), normals, faces


def write_mesh(path: str, positions: np.ndarray, faces: np.ndarray) -> dict:
    """Writes a mesh file; returns its index entry fields (bounds and triangle count)."""
    data = encode_mesh(positions, faces)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)
    p, _, f_ = decode_mesh(data)
    return {"min": [round(float(v), 2) for v in p.min(0)], "max": [round(float(v), 2) for v in p.max(0)], "triangles": int(len(f_))}


# --- floor scans --------------------------------------------------------------------

NO_FLOOR = -32768
# Small triangles (most of them) are tested against this many cell centers at once.
_SPAN = 4


def column_hits(tris: np.ndarray, origin_x: float, origin_z: float, cell: float, width: int, height: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Where vertical lines through the cell centers cross the triangles (n, 3, xyz):
    (cell index (row-major, rows south first), height, the triangle's normal y)."""
    tris = np.asarray(tris, np.float64)
    a, b, c = tris[:, 0], tris[:, 1], tris[:, 2]
    n = np.cross(b - a, c - a)
    ny = n[:, 1] / np.maximum(np.linalg.norm(n, axis=1), 1e-12)
    # Cell-center coordinates: cell (r, c) is at (c, r).
    xs = (tris[..., 0] - origin_x) / cell - 0.5
    zs = (tris[..., 2] - origin_z) / cell - 0.5
    d = (zs[:, 1] - zs[:, 2]) * (xs[:, 0] - xs[:, 2]) + (xs[:, 2] - xs[:, 1]) * (zs[:, 0] - zs[:, 2])
    keep = np.abs(d) > 1e-12  # not seen edge-on from above
    c0 = np.maximum(np.ceil(xs.min(1)), 0).astype(np.int64)
    c1 = np.minimum(np.floor(xs.max(1)), width - 1).astype(np.int64)
    r0 = np.maximum(np.ceil(zs.min(1)), 0).astype(np.int64)
    r1 = np.minimum(np.floor(zs.max(1)), height - 1).astype(np.int64)
    keep &= (c0 <= c1) & (r0 <= r1)
    out_cell, out_y, out_ny = [], [], []

    def test(i, cc, rr):
        """Hits of triangles `i` at cell centers (cc, rr) (arrays of the same shape)."""
        px, pz = cc.astype(np.float64), rr.astype(np.float64)
        x, z, y = xs[i], zs[i], tris[i, :, 1]
        l1 = ((z[:, 1] - z[:, 2]) * (px - x[:, 2]) + (x[:, 2] - x[:, 1]) * (pz - z[:, 2])) / d[i]
        l2 = ((z[:, 2] - z[:, 0]) * (px - x[:, 2]) + (x[:, 0] - x[:, 2]) * (pz - z[:, 2])) / d[i]
        l3 = 1 - l1 - l2
        inside = (l1 >= -1e-9) & (l2 >= -1e-9) & (l3 >= -1e-9)
        out_cell.append((rr * width + cc)[inside])
        out_y.append((l1 * y[:, 0] + l2 * y[:, 1] + l3 * y[:, 2])[inside])
        out_ny.append(ny[i][inside])

    small = np.nonzero(keep & (c1 - c0 < _SPAN) & (r1 - r0 < _SPAN))[0]
    for dr in range(_SPAN):
        for dc in range(_SPAN):
            i = small[(c0[small] + dc <= c1[small]) & (r0[small] + dr <= r1[small])]
            if len(i):
                test(i, c0[i] + dc, r0[i] + dr)
    for i in np.nonzero(keep & ~((c1 - c0 < _SPAN) & (r1 - r0 < _SPAN)))[0]:
        cc, rr = np.meshgrid(np.arange(c0[i], c1[i] + 1), np.arange(r0[i], r1[i] + 1))
        test(np.full(cc.size, i), cc.ravel(), rr.ravel())
    if not out_cell:
        return np.zeros(0, np.int64), np.zeros(0), np.zeros(0)
    return np.concatenate(out_cell), np.concatenate(out_y), np.concatenate(out_ny)


# Surfaces looked at per cell before picking the floors kept (see `pick_floors`).
CANDIDATES = 8
# Room (decimeters) above a surface that makes it a floor someone stands on; less is the
# top of solid rock, or of one of the surfaces stacked in it.
MIN_ROOM_DM = 16
# Headroom as far as it's measured (decimeters): nothing above at all.
OPEN_DM = 120


def pick_floors(floors: np.ndarray, clearance: np.ndarray, layers: int, min_room_dm: int = MIN_ROOM_DM) -> tuple[np.ndarray, np.ndarray]:
    """Up to `layers` floors per cell from candidates (stacked on axis 0, decimeters,
    NO_FLOOR for none) and their headroom. The room over each counts up to the next
    floor above too (collision is one-sided: a floor laid over rock has the rock's top
    just under it, with nothing facing down between).

    First come floors with room to stand and a ceiling over them: inside a cave there's
    always one. Then those with nothing over them at all, which underground are mostly
    the outsides of rocks buried in the ground (there's no collision inside solid
    ground to stop a ray), and big chambers. Then the rest: tops of rock between stacked
    tunnels, which would otherwise crowd a lower tunnel out. Highest first within each.
    Returned highest first, with that room."""
    has = floors != NO_FLOOR
    f = np.where(has, floors.astype(np.int64), -(10**9))
    order = np.argsort(-f, axis=0, kind="stable")
    floors, clearance, f, has = (np.take_along_axis(a, order, 0) for a in (floors, clearance, f, has))
    gap = np.full(f.shape, np.iinfo(np.int64).max)
    gap[1:] = np.where(has[:-1] & has[1:], f[:-1] - f[1:], gap[1:])
    clearance = np.where(has, np.minimum(clearance, gap), clearance).astype(clearance.dtype)
    roomy = clearance >= min_room_dm
    rank = np.where(has, roomy * 100000 + (roomy & (clearance < OPEN_DM)) * 100000 + f, -(10**9))
    order = np.argsort(-rank, axis=0, kind="stable")[:layers]
    f, c = np.take_along_axis(floors, order, 0), np.take_along_axis(clearance, order, 0)
    order = np.argsort(-np.where(f != NO_FLOOR, f.astype(np.int64), -(10**9)), axis=0, kind="stable")
    return np.take_along_axis(f, order, 0), np.take_along_axis(c, order, 0)


# Collision is made of pieces, and where two meet there can be a slit a ray falls
# through but a player walks over. Floors at most this many cells apart across (and
# BRIDGE_STEP_DM apart in height) with nothing between them at that height are one floor.
BRIDGE_CELLS = 2
BRIDGE_STEP_DM = 5


def _bridge_seams(floors: np.ndarray, room: np.ndarray, headroom) -> int:
    """Fills slits in floors (candidates x rows x columns, decimeters, changed in place)
    between floors with room to stand on either side, where the gap still has a ceiling
    over it at about their headroom. Solid rock between two tunnels has nothing over it
    (collision faces into the tunnels): it isn't bridged. Returns how many were filled."""
    layers, h, w = floors.shape
    proposals = []
    for dr, dc in ((0, 1), (1, 0), (1, 1), (1, -1)):
        for span in range(2, BRIDGE_CELLS + 2):
            ar, br = (slice(0, h - span), slice(span, h)) if dr else (slice(0, h), slice(0, h))
            if dc > 0:
                ac, bc = slice(0, w - span), slice(span, w)
            elif dc < 0:
                ac, bc = slice(span, w), slice(0, w - span)
            else:
                ac, bc = slice(0, w), slice(0, w)
            rows, cols = np.mgrid[ar, ac]
            for la in range(layers):
                a, ra = floors[la][ar, ac], room[la][ar, ac]
                ok_a = (a != NO_FLOOR) & (ra >= MIN_ROOM_DM)
                if not ok_a.any():
                    continue
                for lb in range(layers):
                    b, rb = floors[lb][br, bc], room[lb][br, bc]
                    ok = ok_a & (b != NO_FLOOR) & (rb >= MIN_ROOM_DM) & (np.abs(a.astype(np.int32) - b) <= BRIDGE_STEP_DM)
                    if not ok.any():
                        continue
                    for j in range(1, span):
                        hh = a[ok] + (b[ok].astype(np.int32) - a[ok]) * j / span
                        proposals.append(np.column_stack([rows[ok] + j * dr, cols[ok] + j * dc, hh, np.maximum(ra[ok], rb[ok])]))
    if not proposals:
        return 0
    p = np.concatenate(proposals)
    r, c, hh, ends_room = p[:, 0].astype(np.int64), p[:, 1].astype(np.int64), p[:, 2], p[:, 3]
    # Only gaps: nothing at that height there already.
    near = ((floors[:, r, c] != NO_FLOOR) & (np.abs(floors[:, r, c] - hh[None]) <= BRIDGE_STEP_DM)).any(axis=0)
    r, c, hh, ends_room = r[~near], c[~near], hh[~near], ends_room[~near]
    room_here, roofed = headroom(r * w + c, hh / 10)
    covered = roofed & (room_here * 10 <= ends_room + 10) & (room_here * 10 >= MIN_ROOM_DM)
    added = 0
    seen = set()
    for ri, ci, hi, rm in zip(r[covered], c[covered], np.round(hh[covered]).astype(np.int64), np.round(room_here[covered] * 10).astype(np.int64)):
        k = (ri, ci, hi // BRIDGE_STEP_DM)
        if k in seen:
            continue
        seen.add(k)
        column = floors[:, ri, ci]
        if ((column != NO_FLOOR) & (np.abs(column.astype(np.int64) - hi) <= BRIDGE_STEP_DM)).any():
            continue
        free = np.flatnonzero(column == NO_FLOOR)
        if not len(free):
            continue
        floors[free[0], ri, ci] = hi
        room[free[0], ri, ci] = rm
        added += 1
    return added


def floor_scan(tris: np.ndarray, cell: float = 1.0, layers: int = 3, floor_normal_y: float = 0.35, max_clearance: float = 12.0, ground=None, below_ground: float = 0.0) -> dict:
    """What a game's floor scan finds in collision triangles, offline: on a grid of
    `cell` squares over their extent, up to `layers` surfaces facing up per cell
    (decimeters, NO_FLOOR where none; see `pick_floors`), and the headroom over each
    (decimeters, up to `max_clearance`): the distance to the nearest surface above it
    that faces down. Collision is one-sided, so that's what a ray straight down or up
    would hit.

    `ground(x, z)`, if given, is the terrain's height at cell centers (arrays): only
    floors more than `below_ground` under it count (not the rocks at a cave's entrance),
    and the terrain stops their headroom (a buried bunker's roof isn't a floor)."""
    lo, hi = tris.reshape(-1, 3).min(0), tris.reshape(-1, 3).max(0)
    ox, oz = float(np.floor(lo[0])), float(np.floor(lo[2]))
    width, height = int(np.ceil((hi[0] - ox) / cell)) + 1, int(np.ceil((hi[2] - oz) / cell)) + 1
    cells, y, ny = column_hits(tris, ox, oz, cell, width, height)
    surface = None
    if ground is not None:
        xs = ox + (np.arange(width) + 0.5) * cell
        zs = oz + (np.arange(height) + 0.5) * cell
        surface = np.asarray(ground(xs[None, :], zs[:, None]), np.float64).reshape(-1)
        under = y < surface[cells] - below_ground
        cells, y, ny = cells[under], y[under], ny[under]
    floors = np.full((CANDIDATES, height * width), NO_FLOOR, np.int16)
    room = np.full((CANDIDATES, height * width), NO_FLOOR, np.int16)
    up = ny >= floor_normal_y
    fc, fy = cells[up], y[up]
    order = np.lexsort((-fy, fc))  # by cell, highest first
    fc, fy = fc[order], fy[order]
    # Floors closer than 0.3 m are one (the edges of neighboring triangles).
    dup = np.zeros(len(fc), bool)
    dup[1:] = (fc[1:] == fc[:-1]) & (fy[:-1] - fy[1:] < 0.3)
    fc, fy = fc[~dup], fy[~dup]
    first = np.r_[0, np.nonzero(fc[1:] != fc[:-1])[0] + 1] if len(fc) else np.zeros(0, np.int64)
    rank = np.arange(len(fc)) - np.repeat(first, np.diff(np.r_[first, len(fc)]))
    keep = rank < CANDIDATES
    fc, fy, rank = fc[keep], fy[keep], rank[keep]
    # Headroom: the first downward-facing surface above each floor, in the same cell.
    down = ny < 0
    key = lambda c, h: c.astype(np.float64) * 8192 + (h + 4096)  # noqa: E731
    ck = np.sort(key(cells[down], y[down]))

    def headroom(c: np.ndarray, h: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
        """Room over points (cell, height), and whether a surface ends it."""
        i = np.searchsorted(ck, key(c, h + 0.1))
        out = np.full(len(c), max_clearance)
        hit = i < len(ck)
        same = np.zeros(len(c), bool)
        same[hit] = np.floor(ck[i[hit]] / 8192) == c[hit]
        out[same] = np.minimum(ck[i[same]] - key(c[same], h[same] + 0.1) + 0.1, max_clearance)
        if surface is not None:
            out = np.minimum(out, np.maximum(surface[c] - h, 0.0))
        return out, same & (out < max_clearance)

    above, _ = headroom(fc, fy)
    floors[rank, fc] = np.round(fy * 10).astype(np.int16)
    room[rank, fc] = np.round(above * 10).astype(np.int16)
    _bridge_seams(floors.reshape(CANDIDATES, height, width), room.reshape(CANDIDATES, height, width), headroom)
    floors, room = pick_floors(floors, room, layers)
    return {
        "origin_x": ox, "origin_z": oz, "cell": cell, "width": width, "height": height, "layers": layers,
        "floors": floors.reshape(layers, height, width), "clearance": room.reshape(layers, height, width),
    }
