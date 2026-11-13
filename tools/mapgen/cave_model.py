"""Cave models for the 3D view, built from the routing grids (see `sotf_caves.py`).

A cave is drawn as its passages, not its rock: the floor a player can stand on as a
smooth surface, a low wall along its edges, and no roof. Seen from above that reads as
open-topped tunnels, and since the grids are what routes are found on, a route always
lies on the model. The walls face into the cave only, so the ones between the camera
and a passage aren't drawn (back faces are culled) and never hide it.

Inside a passage, a step up to a floor next to it (a ledge, a rock) is drawn as the
step's face, not a wall: walls are only where the cave ends. Where a floor runs into
water there's no wall either; the water has its own mesh.
"""
from __future__ import annotations

import numpy as np
from scipy import ndimage
from scipy.sparse import coo_matrix
from scipy.sparse.csgraph import connected_components

NONE = -32768
# Neighboring floors share their corners (one smooth surface) when they're within this
# of each other (decimeters): a walkable slope.
JOIN_DM = 12
# A floor this close above another in the same cell (decimeters) is the top of a rock or
# a prop on it: left out, so the floor reads as one.
STACK_DM = 30
# Beyond an edge, a floor within this of the edge's height (meters) makes it a step
# (drawn as the step's face); with none, the cave ends there (a wall).
STEP = 4.0
# Wall height above the floor (meters): enough to read as a passage's edge from any
# angle, low enough not to hide the floor next to it.
WALL_HEIGHT = 1.4
# Pieces of floor smaller than this (cells) that nothing walkable joins are the tops of
# rocks, crates and the like: left out.
MIN_PIECE = 30
# Gaps in a floor up to this size (cells) are filled: they'd only add walls.
MAX_HOLE = 8
# Rounding of the passages' outlines and the floors' bumps (passes of averaging with
# neighbors, and how far each pass goes).
OUTLINE_PASSES = 4
SURFACE_PASSES = 2
SMOOTHING = 0.5
# A wall isn't needed where the cell beyond the edge is water within this of the
# floor's height (meters): that's a shore.
SHORE = 1.5

_CORNERS = ((0, 0), (0, 1), (1, 1), (1, 0))  # (row, column) offsets, counterclockwise from above
# The cell beyond each side of a quad (side i runs from corner i to corner i + 1).
_ACROSS = ((-1, 0), (0, 1), (1, 0), (0, -1))


def pieces(floors: np.ndarray, water: np.ndarray, links: list[tuple[int, int]]) -> np.ndarray:
    """Which floors (layers x rows x columns, decimeters, NONE for none) are part of the
    cave: those in a piece of at least MIN_PIECE cells, joined by walking (a step up or
    down of up to JOIN_DM between neighbors, or any into water), swimming, ropes and
    ziplines (`links`, node index pairs)."""
    layers, h, w = floors.shape
    f = floors.astype(np.int32)
    index = np.arange(f.size).reshape(f.shape)
    rows, cols = [], []
    for dr, dc in ((0, 1), (1, 0), (1, 1), (1, -1)):
        a_slice = (slice(max(0, -dr), h - max(0, dr)), slice(max(0, -dc), w - max(0, dc)))
        b_slice = (slice(max(0, dr), h + min(0, dr)), slice(max(0, dc), w + min(0, dc)))
        for la in range(layers):
            for lb in range(layers):
                a, b = f[la][a_slice], f[lb][b_slice]
                wet = water[la][a_slice] | water[lb][b_slice]
                joined = (a != NONE) & (b != NONE) & ((np.abs(b - a) <= JOIN_DM) | wet)
                rows.append(index[la][a_slice][joined])
                cols.append(index[lb][b_slice][joined])
    for a, b in links:
        rows.append(np.array([a]))
        cols.append(np.array([b]))
    r, c = np.concatenate(rows), np.concatenate(cols)
    graph = coo_matrix((np.ones(len(r), np.int8), (r, c)), shape=(f.size, f.size))
    _, label = connected_components(graph, directed=False)
    present = (f != NONE).reshape(-1)
    size = np.bincount(label, weights=present.astype(np.float64))
    return (present & (size[label] >= MIN_PIECE)).reshape(f.shape)


def surface_floors(floors: np.ndarray, keep: np.ndarray) -> np.ndarray:
    """The floors to draw (decimeters, NONE elsewhere): the kept ones without what's
    stacked just above another (STACK_DM), with small gaps filled at their
    surroundings' height."""
    f = np.where(keep, floors.astype(np.int32), NONE)
    has = f != NONE
    stacked = np.zeros(f.shape, bool)
    for a in range(f.shape[0]):
        for b in range(f.shape[0]):
            if a != b:
                stacked[a] |= has[a] & has[b] & (f[b] < f[a]) & (f[a] - f[b] < STACK_DM)
    f[stacked] = NONE
    occupied = (f != NONE).any(axis=0)
    gaps, count = ndimage.label(~occupied)
    if count:
        size = ndimage.sum(np.ones_like(occupied), gaps, np.arange(1, count + 1))
        # A gap touching the grid's edge isn't enclosed.
        edge = np.unique(np.concatenate([gaps[0], gaps[-1], gaps[:, 0], gaps[:, -1]]))
        small = np.zeros(count + 1, bool)
        small[1:] = size <= MAX_HOLE
        small[edge] = False
        fill = small[gaps]
        if fill.any():
            lowest = np.where(f != NONE, f, np.iinfo(np.int32).max).min(axis=0).astype(np.float64)
            known = occupied.astype(np.float64)
            heights = np.where(occupied, lowest, 0.0)
            kernel = np.ones((3, 3))
            total = ndimage.convolve(heights, kernel, mode="constant")
            n = ndimage.convolve(known, kernel, mode="constant")
            # Gaps more than a cell across fill from the outside in.
            for _ in range(MAX_HOLE):
                todo = fill & ~occupied & (n > 0)
                if not todo.any():
                    break
                heights = np.where(todo, total / np.maximum(n, 1), heights)
                occupied = occupied | todo
                known = occupied.astype(np.float64)
                total = ndimage.convolve(heights, kernel, mode="constant")
                n = ndimage.convolve(known, kernel, mode="constant")
            filled = fill & occupied
            f[0] = np.where(filled, np.round(heights).astype(np.int32), f[0])
    return f


def _smooth(values: np.ndarray, edges: np.ndarray, moving: np.ndarray, passes: int) -> np.ndarray:
    """Moves each `moving` vertex's values toward the mean of its neighbors' (along
    `edges`, vertex index pairs), `passes` times."""
    n = len(values)
    if not len(edges) or not moving.any():
        return values
    both = np.concatenate([edges, edges[:, ::-1]])
    adjacency = coo_matrix((np.ones(len(both)), (both[:, 0], both[:, 1])), shape=(n, n)).tocsr()
    adjacency.data[:] = 1.0
    degree = np.asarray(adjacency.sum(axis=1)).reshape(-1)
    out = values.astype(np.float64)
    m = moving & (degree > 0)
    for _ in range(passes):
        mean = adjacency @ out / np.maximum(degree, 1)[:, None]
        out[m] += SMOOTHING * (mean[m] - out[m])
    return out.astype(np.float32)


def _floor_surface(f: np.ndarray, origin_x: float, origin_z: float, cell: float):
    """The floors as quads sharing corners with their neighbors within JOIN_DM: (vertex
    positions, quads as 4 vertex indices counterclockwise from above, each quad's node)."""
    layers, h, w = f.shape
    node = np.flatnonzero((f != NONE).reshape(-1))
    rest = node % (h * w)
    row, col = np.divmod(rest, w)
    height = f.reshape(-1)[node].astype(np.int64)
    # Every quad corner: the grid corner it's at, and the floor height there.
    corner_row = row[:, None] + np.array([dr for dr, _ in _CORNERS])[None]
    corner_col = col[:, None] + np.array([dc for _, dc in _CORNERS])[None]
    corner = (corner_row * (w + 1) + corner_col).reshape(-1)
    corner_h = np.repeat(height, 4)
    # At each grid corner, heights within JOIN_DM of the one below chain into one vertex.
    order = np.lexsort((corner_h, corner))
    sorted_corner, sorted_h = corner[order], corner_h[order]
    new = np.ones(len(order), bool)
    new[1:] = (sorted_corner[1:] != sorted_corner[:-1]) | (sorted_h[1:] - sorted_h[:-1] > JOIN_DM)
    vertex_of_sorted = np.cumsum(new) - 1
    vertex = np.empty(len(order), np.int64)
    vertex[order] = vertex_of_sorted
    y = np.bincount(vertex_of_sorted, weights=sorted_h) / np.bincount(vertex_of_sorted) / 10.0
    grid_corner = sorted_corner[new]
    x = origin_x + (grid_corner % (w + 1)) * cell
    z = origin_z + (grid_corner // (w + 1)) * cell
    return np.column_stack([x, y, z]).astype(np.float32), vertex.reshape(-1, 4), node


def _outline(quads: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Each quad's four sides as directed vertex pairs (counterclockwise from above), and
    whether each is on the outline (no other quad has it)."""
    sides = np.stack([quads, np.roll(quads, -1, axis=1)], axis=2).reshape(-1, 2)
    key = np.sort(sides, axis=1)
    _, inverse, counts = np.unique(key, axis=0, return_inverse=True, return_counts=True)
    return sides, counts[inverse.reshape(-1)] == 1


def _beyond(f: np.ndarray, node: np.ndarray, edge_height: np.ndarray) -> np.ndarray:
    """For each outline side (4 per node, in order), the height (meters) of the floor
    across it nearest the side's height within STEP, or NaN if there's none."""
    layers, h, w = f.shape
    rest = np.repeat(node % (h * w), 4)
    row, col = np.divmod(rest, w)
    which = np.tile(np.arange(4), len(node))
    dr = np.array([d for d, _ in _ACROSS])[which]
    dc = np.array([d for _, d in _ACROSS])[which]
    r, c = row + dr, col + dc
    inside = (r >= 0) & (r < h) & (c >= 0) & (c < w)
    best = np.full(len(rest), np.nan)
    gap = np.full(len(rest), np.inf)
    for layer in range(layers):
        other = np.full(len(rest), NONE, np.int64)
        other[inside] = f[layer, r[inside], c[inside]]
        y = other / 10.0
        d = np.abs(y - edge_height)
        closer = (other != NONE) & (d <= STEP) & (d < gap)
        best[closer] = y[closer]
        gap[closer] = d[closer]
    return best


def _at_shore(positions, sides, water_level, origin_x, origin_z, cell) -> np.ndarray:
    """Which outline sides have water just beyond them, at about the floor's height."""
    h, w = water_level.shape
    a, b = positions[sides[:, 0]], positions[sides[:, 1]]
    mid = (a + b) / 2
    along = b - a
    # Outward: right of the side, as sides run counterclockwise seen from above.
    outward = np.column_stack([along[:, 2], -along[:, 0]])
    outward /= np.maximum(np.linalg.norm(outward, axis=1, keepdims=True), 1e-6)
    c = np.floor((mid[:, 0] + outward[:, 0] * cell * 0.75 - origin_x) / cell).astype(np.int64)
    r = np.floor((mid[:, 2] + outward[:, 1] * cell * 0.75 - origin_z) / cell).astype(np.int64)
    inside = (r >= 0) & (r < h) & (c >= 0) & (c < w)
    level = np.full(len(sides), np.nan, np.float32)
    level[inside] = water_level[r[inside], c[inside]]
    return ~np.isnan(level) & (np.abs(level - mid[:, 1]) <= SHORE)


def _quads_up(ends: np.ndarray, positions: np.ndarray, rise: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Vertical faces on sides (directed vertex pairs) from the floor up by `rise` per
    side (negative: down). One rising faces left of its side (into the cave, for sides
    running counterclockwise); one falling faces right (toward the floor below)."""
    p, q = positions[ends[:, 0]], positions[ends[:, 1]]
    up = np.zeros((len(ends), 3), np.float32)
    up[:, 1] = rise
    n = len(ends)
    verts = np.concatenate([p, q, q + up, p + up])
    i = np.arange(n)
    a, b, c, d = i, i + n, i + 2 * n, i + 3 * n
    return verts, np.concatenate([np.column_stack([a, b, c]), np.column_stack([a, c, d])])


def water(level: np.ndarray, wet: np.ndarray, origin_x: float, origin_z: float, cell: float) -> tuple[np.ndarray, np.ndarray]:
    """The water a cave's routes swim or dive through, as surfaces: (positions,
    triangles facing up). `level`: the water's height per cell (meters, NaN where
    unknown); `wet`: the cells with water nodes."""
    cells = wet & ~np.isnan(level)
    f = np.where(cells, np.round(np.nan_to_num(level) * 10), NONE).astype(np.int32)[None]
    if not cells.any():
        return np.zeros((0, 3), np.float32), np.zeros((0, 3), np.uint32)
    positions, quads, _ = _floor_surface(f, origin_x, origin_z, cell)
    sides, outline = _outline(quads)
    on_outline = np.zeros(len(positions), bool)
    on_outline[sides[outline].reshape(-1)] = True
    xz = _smooth(positions[:, [0, 2]], sides[outline], on_outline, OUTLINE_PASSES)
    positions[:, 0], positions[:, 2] = xz[:, 0], xz[:, 1]
    return positions, np.concatenate([quads[:, [0, 3, 2]], quads[:, [0, 2, 1]]]).astype(np.uint32)


def build(floors: np.ndarray, keep: np.ndarray, swim: np.ndarray, water_level: np.ndarray, origin_x: float, origin_z: float, cell: float) -> tuple[np.ndarray, np.ndarray]:
    """A cave's model: (positions, triangles) in game axes, front faces wound as in
    Unity (the cross product of their edges points out of the front).

    `floors`: the routing grid (layers x rows x columns, decimeters; row 0 south).
    `keep`: which of its floors make up the cave (see `pieces`).
    `swim`: which are swum at the water's surface (drawn as water, not floor).
    `water_level`: water surface height per cell (meters, NaN where none), for shores."""
    f = surface_floors(floors, keep)
    if not (f != NONE).any():
        return np.zeros((0, 3), np.float32), np.zeros((0, 3), np.uint32)
    positions, quads, node = _floor_surface(f, origin_x, origin_z, cell)
    sides, outline = _outline(quads)

    # Round off the outline's steps, then the floor's bumps (heights only).
    on_outline = np.zeros(len(positions), bool)
    on_outline[sides[outline].reshape(-1)] = True
    xz = _smooth(positions[:, [0, 2]], sides[outline], on_outline, OUTLINE_PASSES)
    positions[:, 0], positions[:, 2] = xz[:, 0], xz[:, 1]
    positions[:, 1] = _smooth(positions[:, 1:2], sides[~outline], ~on_outline, SURFACE_PASSES)[:, 0]

    # Outline sides: a step's face down to the floor beyond, a wall where there's none
    # (the cave's edge), nothing at a shore or at the foot of a step.
    edge_height = positions[sides, 1].mean(axis=1)
    beyond = _beyond(f, node, edge_height)
    wall = outline & np.isnan(beyond)
    wall[wall] = ~_at_shore(positions, sides[wall], water_level, origin_x, origin_z, cell)
    step = outline & ~np.isnan(beyond) & (beyond < edge_height - 0.05)

    swum = swim.reshape(-1)[node] & (floors.reshape(-1)[node] == f.reshape(-1)[node])
    q = quads[~swum]
    parts = [(positions, np.concatenate([q[:, [0, 3, 2]], q[:, [0, 2, 1]]]))]
    if wall.any():
        parts.append(_quads_up(sides[wall], positions, np.full(int(wall.sum()), WALL_HEIGHT, np.float32)))
    if step.any():
        parts.append(_quads_up(sides[step], positions, (beyond[step] - edge_height[step]).astype(np.float32)))
    offset = 0
    out_p, out_f = [], []
    for p, t in parts:
        out_p.append(p)
        out_f.append(t + offset)
        offset += len(p)
    return np.concatenate(out_p).astype(np.float32), np.concatenate(out_f).astype(np.uint32)
