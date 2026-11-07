"""Tests for the surface map: python -I -m unittest discover -s tools/mapgen"""
import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sotf  # noqa: E402


class Ocean(unittest.TestCase):
    def test_reaches_only_what_connects_to_the_open_sea(self):
        below, above = sotf.SEA_LEVEL - 5, sotf.SEA_LEVEL + 5
        h = np.full((9, 9), below, dtype=np.float32)
        h[2:7, 2:7] = above  # an island...
        h[4, 2] = below  # ...with an inlet...
        h[4, 4] = below  # ...and a dry hollow below sea level
        ocean = sotf.ocean_mask(h)
        self.assertTrue(ocean[0, 0])
        self.assertTrue(ocean[4, 2])
        self.assertFalse(ocean[4, 4])
        self.assertFalse(ocean[3, 3])


class LayerColors(unittest.TestCase):
    def test_specific_layers_win_over_their_general_names(self):
        self.assertNotEqual(sotf.layer_color("GolfGrassA_Albedo"), sotf.layer_color("GrassAAlbedo"))
        self.assertNotEqual(sotf.layer_color("BedRockAlbedo"), sotf.layer_color("GrayRocksAAlbedo"))
        self.assertNotEqual(sotf.layer_color("SharpRocksAlbedo"), sotf.layer_color("GrayRocksAAlbedo"))

    def test_every_terrain_layer_has_a_color(self):
        names = ["SandAAlbedo", "DirtAAlbedo", "SandBAlbedo", "GrayRocksAAlbedo", "GrassAAlbedo", "MuddyPathAlbedo",
                 "RootsAlbedoDom", "RiverBedBAlbedo", "MossAAlbedo", "SharpRocksAlbedo", "GolfGrassA_Albedo", "BedRockAlbedo"]
        for name in names:
            self.assertNotEqual(sotf.layer_color(name), sotf.DEFAULT_LAYER_COLOR, name)


if __name__ == "__main__":
    unittest.main()
