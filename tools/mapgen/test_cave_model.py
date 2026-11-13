"""Tests for the caves' 3D models (cave_model.py)."""
import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cave_model  # noqa: E402
from cave_model import NONE  # noqa: E402


def grid(heights_m, layers=1):
    """A one-layer grid (decimeters) from a 2D list of heights in meters (None: no floor)."""
    a = np.array([[NONE if v is None else round(v * 10) for v in row] for row in heights_m], np.int16)
    out = np.full((layers, *a.shape), NONE, np.int16)
    out[0] = a
    return out


def normals(p, f):
    a, b, c = p[f[:, 0]], p[f[:, 1]], p[f[:, 2]]
    n = np.cross(b - a, c - a)
    return n / np.linalg.norm(n, axis=1, keepdims=True)


def model(floors, swim=None, level=None):
    keep = floors != NONE
    swim = np.zeros(floors.shape, bool) if swim is None else swim
    level = np.full(floors.shape[1:], np.nan, np.float32) if level is None else level
    return cave_model.build(floors, keep, swim, level, 0.0, 0.0, 1.0)


def walls(p, f):
    """The vertical faces' normals and their top heights."""
    n = normals(p, f)
    vertical = np.abs(n[:, 1]) < 0.1
    return n[vertical], p[f[vertical]][:, :, 1].max(axis=1)


class Pieces(unittest.TestCase):
    def test_small_pieces_are_left_out(self):
        floors = np.full((1, 12, 12), NONE, np.int16)
        floors[0, :8, :8] = 0  # 64 cells
        floors[0, 10:, 10:] = 0  # 4 cells: a rock top
        keep = cave_model.pieces(floors, np.zeros(floors.shape, bool), [])
        self.assertTrue(keep[0, :8, :8].all())
        self.assertFalse(keep[0, 10:, 10:].any())

    def test_a_rope_joins_a_small_ledge_to_the_cave(self):
        floors = np.full((1, 12, 12), NONE, np.int16)
        floors[0, :8, :8] = 0
        floors[0, 10:, 10:] = 200  # a ledge 20 m up
        ledge, bottom = (0 * 12 + 11) * 12 + 11, 0
        keep = cave_model.pieces(floors, np.zeros(floors.shape, bool), [(bottom, ledge)])
        self.assertTrue(keep[0, 10:, 10:].all())

    def test_floors_too_far_apart_to_step_are_separate_pieces(self):
        floors = np.full((1, 4, 30), NONE, np.int16)
        floors[0, :, :20] = 0  # 80 cells
        floors[0, :, 20:25] = 30  # 20 cells, 3 m up: not a step anyone walks
        keep = cave_model.pieces(floors, np.zeros(floors.shape, bool), [])
        self.assertTrue(keep[0, :, :20].all())
        self.assertFalse(keep[0, :, 20:25].any())


class SurfaceFloors(unittest.TestCase):
    def test_rock_tops_on_a_floor_are_dropped(self):
        floors = np.full((2, 6, 6), NONE, np.int16)
        floors[0] = 0
        floors[1, 2, 2] = 15  # 1.5 m above the floor in the same cell
        f = cave_model.surface_floors(floors, floors != NONE)
        self.assertEqual(f[1, 2, 2], NONE)
        self.assertEqual(f[0, 2, 2], 0)

    def test_a_level_above_stays(self):
        floors = np.full((2, 6, 6), NONE, np.int16)
        floors[0] = 0
        floors[1, 2, 2] = 60  # another level, 6 m up
        f = cave_model.surface_floors(floors, floors != NONE)
        self.assertEqual(f[1, 2, 2], 60)

    def test_small_gaps_are_filled_at_their_surroundings_height(self):
        floors = grid([[1.0] * 7 for _ in range(7)])
        floors[0, 3, 3] = NONE
        f = cave_model.surface_floors(floors, floors != NONE)
        self.assertEqual(f[0, 3, 3], 10)

    def test_gaps_open_to_the_outside_are_not(self):
        floors = grid([[1.0] * 7 for _ in range(7)])
        floors[0, 0, 3] = NONE
        f = cave_model.surface_floors(floors, floors != NONE)
        self.assertEqual(f[0, 0, 3], NONE)


class Build(unittest.TestCase):
    def test_floors_face_up_and_walls_face_in(self):
        p, f = model(grid([[0.0] * 5 for _ in range(5)]))
        n = normals(p, f)
        flat = n[:, 1] > 0.9
        self.assertGreater(flat.sum(), 0)
        self.assertTrue((n[:, 1] > -0.1).all())  # nothing faces down
        wall_n, top = walls(p, f)
        self.assertGreater(len(wall_n), 0)
        np.testing.assert_allclose(top, cave_model.WALL_HEIGHT, atol=0.01)
        # Into the cave: toward its middle.
        a = p[f[np.abs(n[:, 1]) < 0.1]].mean(axis=1)
        inward = np.column_stack([2.5 - a[:, 0], 2.5 - a[:, 2]])
        self.assertTrue((np.einsum("ij,ij->i", wall_n[:, [0, 2]], inward) > 0).all())

    def test_a_step_inside_a_cave_has_a_face_down_not_a_wall(self):
        floors = grid([[0.0] * 6 + [2.0] * 6 for _ in range(6)])
        p, f = model(floors)
        n = normals(p, f)
        x = p[f][:, :, 0].mean(axis=1)
        y = p[f][:, :, 1]
        step = (np.abs(n[:, 1]) < 0.1) & (np.abs(x - 6) < 0.6)
        self.assertGreater(step.sum(), 0)
        # From the upper floor down to the lower one, facing the lower side (-x).
        self.assertTrue((n[step, 0] < 0).all())
        self.assertAlmostEqual(float(y[step].min()), 0.0, delta=0.3)
        self.assertLess(float(y[step].max()), 2.0 + 0.3)

    def test_no_wall_at_a_shore(self):
        floors = grid([[0.0] * 6 for _ in range(6)])
        floors[0, :, 3:] = NONE  # swum cells aren't floors here: open water beyond x = 3
        level = np.full((6, 6), np.nan, np.float32)
        level[:, 3:] = 0.2
        p, f = model(floors, level=level)
        wall_n, _ = walls(p, f)
        a = p[f[np.abs(normals(p, f)[:, 1]) < 0.1]].mean(axis=1)
        self.assertFalse((np.abs(a[:, 0] - 3) < 0.6).any(), "a wall along the shore")
        self.assertGreater(len(wall_n), 0)  # the other sides still have theirs

    def test_swum_floors_are_left_to_the_water(self):
        floors = grid([[0.0] * 6 for _ in range(6)])
        swim = np.zeros(floors.shape, bool)
        swim[0, :, 3:] = True
        with_swim = model(floors, swim=swim)[1]
        without = model(floors)[1]
        self.assertLess(len(with_swim), len(without))

    def test_empty_grid(self):
        p, f = model(np.full((1, 3, 3), NONE, np.int16))
        self.assertEqual(len(f), 0)


class Water(unittest.TestCase):
    def test_wet_cells_at_the_water_level(self):
        level = np.full((4, 4), np.nan, np.float32)
        level[1:3, 1:3] = 5.0
        wet = np.zeros((4, 4), bool)
        wet[1:3, 1:3] = True
        wet[0, 0] = True  # no level known: left out
        p, f = cave_model.water(level, wet, 10.0, 20.0, 1.0)
        self.assertEqual(len(f), 8)
        np.testing.assert_allclose(p[:, 1], 5.0, atol=0.01)
        self.assertTrue((normals(p, f)[:, 1] > 0.9).all())
        self.assertGreaterEqual(p[:, 0].min(), 11.0 - 1e-3)
        self.assertGreaterEqual(p[:, 2].min(), 21.0 - 1e-3)


if __name__ == "__main__":
    unittest.main()
