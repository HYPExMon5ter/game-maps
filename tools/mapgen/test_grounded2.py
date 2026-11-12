"""Tests for the Grounded 2 map: python -I -m unittest discover -s tools/mapgen"""
import os
import sys
import unittest

from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import grounded2  # noqa: E402

PARK = {"minX": -254749.0, "minY": -135000.0, "maxX": 76250.0, "maxY": 195999.0}
ABYSS = {"minX": 159500.0, "minY": 137500.0, "maxX": 284500.0, "maxY": 262500.0}


class Projection(unittest.TestCase):
    def test_union_is_the_square_holding_both_maps(self):
        square = grounded2.union([PARK, ABYSS])
        self.assertEqual((square["minX"], square["minY"]), (PARK["minX"], PARK["minY"]))
        self.assertEqual(square["maxX"] - square["minX"], square["maxY"] - square["minY"])
        self.assertEqual(square["maxX"], ABYSS["maxX"])

    def test_east_is_unreal_y_and_north_unreal_x(self):
        p = grounded2.projection({"minX": -100.0, "minY": -300.0, "maxX": 900.0, "maxY": 700.0})
        self.assertEqual((p["east"], p["north"], p["up"]), ("x", "z", "y"))
        self.assertEqual((p["minEast"], p["maxEast"], p["minNorth"], p["maxNorth"]), (-3.0, 7.0, -1.0, 9.0))

    def test_places_each_map_at_its_bounds(self):
        square = grounded2.union([PARK, ABYSS])
        left, top, width, height = grounded2.placement(square, PARK, 1000)
        self.assertEqual((left, top), (0, 0))
        self.assertEqual(width, height)
        left, top, _, _ = grounded2.placement(square, ABYSS, 1000)
        self.assertGreater(left, 700)
        self.assertGreater(top, 450)


class Compose(unittest.TestCase):
    def test_north_up_puts_unreal_x_at_the_top(self):
        # A painting with a mark at its +X edge (right) ends up with the mark at the top.
        painting = Image.new("RGBA", (10, 10), (0, 0, 0, 0))
        painting.putpixel((9, 5), (255, 0, 0, 255))
        image = grounded2.compose([(painting, {"minX": 0.0, "minY": 0.0, "maxX": 10.0, "maxY": 10.0})], 10)
        self.assertEqual(image.getpixel((5, 0))[:3], (255, 0, 0))


if __name__ == "__main__":
    unittest.main()
