"""Tests for the cave grids: python -I -m unittest discover -s tools/mapgen"""
import base64
import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sotf_caves  # noqa: E402

NONE = sotf_caves.NONE


def square(x0, z0, x1, z1, y):
    """A flat water surface as two triangles (n, 3, xyz)."""
    a, b, c, d = (x0, y, z0), (x1, y, z0), (x1, y, z1), (x0, y, z1)
    return np.array([[a, b, c], [a, c, d]], np.float32)


class Surfaces(unittest.TestCase):
    def test_cells_under_a_surface_get_its_height(self):
        s = sotf_caves.rasterize_surfaces([square(1, 1, 3, 3, 5.0)], 0, 0, 1, 5, 4)
        self.assertTrue(np.isnan(s[0, 0]))
        self.assertEqual(s[1, 1], 5.0)
        self.assertEqual(s[2, 2], 5.0)
        self.assertTrue(np.isnan(s[3, 4]))

    def test_overlapping_surfaces_keep_the_highest(self):
        s = sotf_caves.rasterize_surfaces([square(0, 0, 2, 2, 1.0), square(0, 0, 2, 2, 4.0)], 0, 0, 1, 2, 2)
        self.assertTrue((s == 4.0).all())


class Swimming(unittest.TestCase):
    def grid(self, floors, rooms):
        return np.array(floors, np.int16)[None, None], np.array(rooms, np.int16)[None, None]

    def test_floors_under_open_water_are_swum_at_the_surface(self):
        # A lake bed 3 m under a surface at 10 m, with the water itself stopping the
        # headroom measurement; a dry floor beside it.
        floors, room = self.grid([70, 100], [30, 50])
        surface = np.array([[10.0, np.nan]], np.float32)
        out, swim = sotf_caves.swim_floors(floors, room, surface)
        self.assertEqual(out.tolist(), [[[100, 100]]])
        self.assertEqual(swim.tolist(), [[[True, False]]])

    def test_rock_between_floor_and_water_is_another_level(self):
        # A floor 20 m under the surface but with 2 m of headroom: a passage below.
        floors, room = self.grid([-100], [20])
        out, _ = sotf_caves.swim_floors(floors, room, np.array([[10.0]], np.float32))
        self.assertEqual(out.tolist(), [[[-100]]])

    def test_shallow_water_is_walked(self):
        floors, room = self.grid([98], [40])
        out, swim = sotf_caves.swim_floors(floors, room, np.array([[10.0]], np.float32))
        self.assertEqual(out.tolist(), [[[98]]])
        self.assertFalse(swim.any())

    def test_low_floors_without_water_stay_unwalkable(self):
        floors, room = self.grid([50], [5])
        out, _ = sotf_caves.swim_floors(floors, room, np.full((1, 1), np.nan, np.float32))
        self.assertEqual(out.tolist(), [[[NONE]]])


def box(x0, z0, x1, z1, y0, y1):
    """A closed box of water (n, 3, xyz): its top and bottom are what count."""
    return np.concatenate([square(x0, z0, x1, z1, y1), square(x0, z0, x1, z1, y0)])


class Diving(unittest.TestCase):
    def test_floors_inside_a_volume_under_rock_are_dives(self):
        # Two cells under 5 m of water; the second is open water (swum at the top).
        floors = np.array([[[0, 0, 0]]], np.int16)
        room = np.array([[[20, 20, 20]]], np.int16)
        under = np.array([[[False, True, False]]])
        dive = sotf_caves.dive_floors(floors, room, under, [box(0, 0, 2, 1, -1.0, 5.0)], 0, 0, 1)
        self.assertEqual(dive.tolist(), [[[True, False, False]]])

    def test_shallow_water_and_floors_below_the_volume_are_not(self):
        floors = np.array([[[40, -100]]], np.int16)  # 1 m under the top; 10 m under its bottom
        room = np.array([[[20, 20]]], np.int16)
        dive = sotf_caves.dive_floors(floors, room, np.zeros((1, 1, 2), bool), [box(0, 0, 2, 1, -1.0, 5.0)], 0, 0, 1)
        self.assertFalse(dive.any())

    def test_a_surface_without_a_bottom_reaches_down_to_its_lowest_point(self):
        # The volume's only face over the cell is its top; its sides go down to -10 m.
        tris = np.concatenate([square(0, 0, 1, 1, 5.0), np.array([[[5, -10, 5], [6, -10, 5], [6, 5, 5]]], np.float32)])
        dive = sotf_caves.dive_floors(np.array([[[-50]]], np.int16), np.array([[[20]]], np.int16), np.zeros((1, 1, 1), bool), [tris], 0, 0, 1)
        self.assertTrue(dive.all())


class Links(unittest.TestCase):
    scan = {"origin_x": 0.0, "origin_z": 0.0, "cell": 1.0}

    def test_ropes_join_the_floors_at_their_ends_both_ways(self):
        # A ledge at 10 m on layer 0 and the floor below it at 0 m on layer 1, at (2, 2).
        floors = np.full((2, 5, 5), NONE, np.int16)
        floors[0, 2, 2] = 100
        floors[1, 2, 2] = 0
        rope = {"top": [2.5, 11.0, 2.5], "bottom": [2.5, 1.0, 2.5]}
        out = sotf_caves.links(floors, self.scan, {"ropes": [rope], "ziplines": []})
        top, bottom = 2 * 5 + 2, 25 + 2 * 5 + 2
        self.assertEqual(out, [{"from": bottom, "to": top, "kind": "rope"}, {"from": top, "to": bottom, "kind": "rope"}])

    def test_ziplines_go_downhill_only(self):
        floors = np.full((1, 1, 10), NONE, np.int16)
        floors[0, 0, 0] = 50
        floors[0, 0, 9] = 0
        line = {"high": [0.5, 5.0, 0.5], "low": [9.5, 0.0, 0.5]}
        self.assertEqual(sotf_caves.links(floors, self.scan, {"ropes": [], "ziplines": [line]}), [{"from": 0, "to": 9, "kind": "zipline"}])

    def test_a_zipline_lands_on_the_floor_under_where_the_ride_stops(self):
        # The low anchor is level with a ledge at 5 m (column 9); the ride stops 1.5 m
        # short of it, over the floor at 1 m (column 8), and drops onto that.
        floors = np.full((1, 1, 10), NONE, np.int16)
        floors[0, 0, 0] = 120
        floors[0, 0, 8] = 10
        floors[0, 0, 9] = 50
        line = {"high": [0.5, 13.0, 0.5], "low": [9.5, 5.0, 0.5]}
        self.assertEqual(sotf_caves.links(floors, self.scan, {"ropes": [], "ziplines": [line]}), [{"from": 0, "to": 8, "kind": "zipline"}])

    def test_an_end_with_no_floor_near_it_has_no_link(self):
        floors = np.zeros((1, 3, 3), np.int16)  # all at 0 m: nothing near a rope top at 20 m
        rope = {"top": [1.5, 21.0, 1.5], "bottom": [1.5, 1.0, 1.5]}
        self.assertEqual(sotf_caves.links(floors, self.scan, {"ropes": [rope], "ziplines": []}), [])


class FillFromCollision(unittest.TestCase):
    def test_a_stairwell_keeps_all_four_flights(self):
        # The game found three floors in a 1-cell scan; the collision has a fourth below.
        def encode(a):
            return base64.b64encode(np.asarray(a, "<i2").tobytes()).decode()

        scan = {"area": 1, "origin_x": 0.0, "origin_z": 0.0, "cell": 1.0, "width": 1, "height": 1, "layers": 3,
                "floors": encode([2132, 2058, 1984]), "clearance": encode([55, 55, 55])}
        flights = np.array([2132, 2058, 1984, 1910], np.int16).reshape(4, 1, 1)
        collision = [{"origin_x": 0.0, "origin_z": 0.0, "cell": 1.0, "width": 1, "height": 1,
                      "floors": flights, "clearance": np.full((4, 1, 1), 55, np.int16)}]
        grown, added = sotf_caves.fill_from_collision(scan, collision)
        floors = np.frombuffer(base64.b64decode(grown["floors"]), "<i2")
        self.assertEqual((grown["layers"], added), (4, 1))
        self.assertEqual(floors.tolist(), [2132, 2058, 1984, 1910])


class Names(unittest.TestCase):
    def test_a_grid_takes_the_name_of_the_cave_holding_its_floors(self):
        floors = np.full((1, 10, 10), NONE, np.int16)
        floors[0] = 50
        cave = {"scan": {"origin_x": 100.0, "origin_z": 200.0, "cell": 1.0}, "floors": floors}
        groups = {
            "elsewhere": {"name": "Cave X", "min": [0, 0, 0], "max": [50, 50, 50]},
            "here": {"name": "Cave Y", "min": [99, 0, 199], "max": [111, 10, 211]},
        }
        self.assertEqual(sotf_caves.group_of(cave, groups), "here")
        self.assertIsNone(sotf_caves.group_of(cave, {"elsewhere": groups["elsewhere"]}))


if __name__ == "__main__":
    unittest.main()
