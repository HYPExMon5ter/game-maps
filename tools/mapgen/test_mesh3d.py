"""Tests for the 3D map data: python -I -m unittest discover -s tools/mapgen"""
import json
import os
import sys
import tempfile
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mesh3d  # noqa: E402


def face_normals(p: np.ndarray, f: np.ndarray) -> np.ndarray:
    a, b, c = p[f[:, 0]], p[f[:, 1]], p[f[:, 2]]
    return np.cross(b - a, c - a)


class Boxes(unittest.TestCase):
    def test_faces_point_out_of_the_box(self):
        m = np.eye(4)
        m[:3, 3] = (10, 5, -3)
        p, f = mesh3d.box_triangles((0, 1, 0), (2, 4, 6), m)
        center = np.array((10, 6, -3))
        mids = p[f].mean(axis=1)
        self.assertEqual(len(f), 12)
        self.assertTrue((np.einsum("ij,ij->i", face_normals(p, f), mids - center) > 0).all())

    def test_mirrored_boxes_still_face_out(self):
        m = np.diag((-1.0, 1.0, 1.0, 1.0))
        p, f = mesh3d.box_triangles((0, 0, 0), (1, 1, 1), m)
        mids = p[f].mean(axis=1)
        self.assertTrue((np.einsum("ij,ij->i", face_normals(p, f), mids) > 0).all())

    def test_a_negative_size_is_the_same_box(self):
        # A stair ramp in Sons of the Forest's bunkers: 7.5 x -0.2 x 2.6.
        p, f = mesh3d.box_triangles((0, 0, 0), (7.5, -0.2, 2.6), np.eye(4))
        mids = p[f].mean(axis=1)
        self.assertTrue((np.einsum("ij,ij->i", face_normals(p, f), mids) > 0).all())
        self.assertTrue(np.allclose(p.max(axis=0), (3.75, 0.1, 1.3)))

    def test_leaving_out_the_top_drops_the_upward_face(self):
        p, f = mesh3d.box_triangles((0, 0, 0), (1, 1, 1), np.eye(4), top=False)
        self.assertEqual(len(f), 10)
        self.assertTrue((face_normals(p, f)[:, 1] <= 0).all())


class MeshFiles(unittest.TestCase):
    def test_round_trip_within_quantization(self):
        rng = np.random.default_rng(1)
        p = rng.uniform(-500, 500, (300, 3)).astype(np.float32)
        f = rng.integers(0, 300, (400, 3)).astype(np.uint32)
        q, _, g = mesh3d.decode_mesh(mesh3d.encode_mesh(p, f))
        self.assertTrue(np.allclose(q[g], p[f], atol=1000 / 65535))

    def test_unused_vertices_are_dropped(self):
        p = np.zeros((70000, 3), np.float32)
        p[:, 0] = np.arange(70000)
        f = np.array([[0, 69999, 35000]], np.uint32)
        data = mesh3d.encode_mesh(p, f)
        q, _, g = mesh3d.decode_mesh(data)
        self.assertEqual(len(q), 3)
        # Few enough vertices left for 16-bit indices (positions and normals padded to 4 bytes).
        self.assertEqual(len(data), mesh3d.HEADER.size + 20 + 8 + 3 * 2)

    def test_index_entry_has_bounds_and_count(self):
        p = np.array([[0, 0, 0], [1, 2, 3], [4, 5, 6]], np.float32)
        f = np.array([[0, 1, 2]], np.uint32)
        with tempfile.TemporaryDirectory() as d:
            entry = mesh3d.write_mesh(os.path.join(d, "m.bin"), p, f)
        self.assertEqual(entry, {"min": [0.0, 0.0, 0.0], "max": [4.0, 5.0, 6.0], "triangles": 1})

    def test_index_updates_keep_other_fields(self):
        with tempfile.TemporaryDirectory() as d:
            mesh3d.update_index(d, "surface", terrain={"url": "t"})
            mesh3d.update_index(d, "surface", meshes=[{"id": "a"}])
            mesh3d.update_index(d, "surface", terrain=None)
            with open(os.path.join(d, "3d", "index.json")) as f:
                index = json.load(f)
        self.assertEqual(index, {"version": 1, "maps": {"surface": {"meshes": [{"id": "a"}]}}})


class Normals(unittest.TestCase):
    def test_smooth_normals_follow_the_front_faces(self):
        # Two triangles folded along x, both facing up-ish.
        p = np.array([[0, 0, 0], [1, 0, 0], [0, 0, 1], [1, 0.5, 1]], np.float32)
        f = np.array([[0, 2, 1], [1, 2, 3]], np.uint32)
        n = mesh3d.vertex_normals(p, f)
        self.assertTrue((n[:, 1] > 0.8).all())
        self.assertTrue(np.allclose(np.linalg.norm(n, axis=1), 1, atol=1e-5))

    def test_octahedral_round_trip(self):
        rng = np.random.default_rng(2)
        n = rng.normal(size=(500, 3))
        n /= np.linalg.norm(n, axis=1, keepdims=True)
        back = mesh3d.oct_decode(mesh3d.oct_encode(n))
        self.assertGreater(np.einsum("ij,ij->i", n, back).min(), 0.999)

    def test_decoded_mesh_carries_normals(self):
        p = np.array([[0, 0, 0], [0, 0, 1], [1, 0, 0]], np.float32)
        f = np.array([[0, 1, 2]], np.uint32)
        _, n, _ = mesh3d.decode_mesh(mesh3d.encode_mesh(p, f))
        self.assertTrue(np.allclose(n, [[0, 1, 0]] * 3, atol=0.02))


class Weld(unittest.TestCase):
    def test_merges_close_vertices_and_drops_collapsed_triangles(self):
        p = np.array([[0, 0, 0], [0.001, 0, 0], [1, 0, 0], [0, 1, 0]], np.float32)
        f = np.array([[0, 2, 3], [0, 1, 2]], np.uint32)
        q, g = mesh3d.weld(p, f)
        self.assertEqual(len(q), 3)
        self.assertEqual(len(g), 1)


class Elevation(unittest.TestCase):
    def test_terrain_rgb_encoding(self):
        rgb = mesh3d.encode_dem(np.array([[0.0, 123.4, -19.0]]))
        decoded = -10000 + (rgb[..., 0].astype(int) * 65536 + rgb[..., 1].astype(int) * 256 + rgb[..., 2]) / 10
        self.assertTrue(np.allclose(decoded, [[0.0, 123.4, -19.0]], atol=0.05))

    def test_tiles_cover_the_map_tile_at_each_zoom(self):
        h = np.linspace(0, 100, 64 * 64, dtype=np.float32).reshape(64, 64)
        with tempfile.TemporaryDirectory() as d:
            lo, hi = mesh3d.write_dem_tiles(h, d, 3, 3, 3, 5)
            names = sorted(os.path.relpath(os.path.join(r, n), d).replace(os.sep, "/") for r, _, ns in os.walk(d) for n in ns)
        self.assertEqual((lo, hi), (3, 5))
        self.assertIn("3/3/3.webp", names)
        self.assertIn("4/6/7.webp", names)
        self.assertIn("5/15/15.webp", names)
        self.assertEqual(len(names), 1 + 4 + 16)


def quad(x0, z0, x1, z1, y, up=True):
    """A flat square as two triangles (n, 3, xyz), facing up or down."""
    a, b, c, d = (x0, y, z0), (x1, y, z0), (x1, y, z1), (x0, y, z1)
    tris = [[a, c, b], [a, d, c]] if up else [[a, b, c], [a, c, d]]
    return np.array(tris, np.float32)


class FloorScans(unittest.TestCase):
    def test_floors_highest_first_with_their_headroom(self):
        # A 4 m square room: floor at 0, a ceiling at 3, a 1 m ledge over its east half
        # at 1.5 (whose underside faces down too).
        room = np.concatenate([quad(0, 0, 4, 4, 0.0), quad(0, 0, 4, 4, 3.0, up=False), quad(2, 0, 4, 4, 1.5), quad(2, 0, 4, 4, 1.4, up=False)])
        s = mesh3d.floor_scan(room)
        self.assertEqual((s["origin_x"], s["origin_z"], s["width"], s["height"]), (0.0, 0.0, 5, 5))
        west, east = (slice(None), 0, 0), (slice(None), 0, 3)
        self.assertEqual(s["floors"][west].tolist(), [0, mesh3d.NO_FLOOR, mesh3d.NO_FLOOR])
        self.assertEqual(s["clearance"][west].tolist()[0], 30)
        self.assertEqual(s["floors"][east].tolist(), [15, 0, mesh3d.NO_FLOOR])
        self.assertEqual(s["clearance"][east].tolist()[:2], [15, 14])

    def test_open_above_is_full_headroom(self):
        s = mesh3d.floor_scan(quad(0, 0, 2, 2, 5.0), max_clearance=12.0)
        self.assertEqual(s["clearance"][0, 0, 0], 120)

    def test_big_triangles_are_scanned_too(self):
        s = mesh3d.floor_scan(quad(0, 0, 40, 40, 2.0))
        self.assertTrue((s["floors"][0, :40, :40] == 20).all())

    def test_rock_tops_dont_crowd_out_a_tunnel_below(self):
        # Four tunnels stacked in one column, each 2.5 m tall, and a buried rock's outer
        # top (nothing above it) over them all.
        parts = [quad(0, 0, 2, 2, 30.0)]
        for y in (0.0, 5.0, 10.0, 15.0):
            parts += [quad(0, 0, 2, 2, y), quad(0, 0, 2, 2, y + 2.5, up=False)]
        s = mesh3d.floor_scan(np.concatenate(parts))
        # The three highest with a ceiling: the rock top waits for a free slot.
        self.assertEqual(s["floors"][:, 0, 0].tolist(), [150, 100, 50])

    def test_ground_stops_headroom_and_hides_what_is_above_it(self):
        parts = np.concatenate([quad(0, 0, 2, 2, 0.0), quad(0, 0, 2, 2, 9.0)])
        s = mesh3d.floor_scan(parts, ground=lambda x, z: np.full(np.broadcast(x, z).shape, 8.0), below_ground=0.3)
        self.assertEqual(s["floors"][:, 0, 0].tolist(), [0, mesh3d.NO_FLOOR, mesh3d.NO_FLOOR])
        self.assertEqual(s["clearance"][0, 0, 0], 80)

    def test_a_slit_between_collision_pieces_is_bridged(self):
        # A tunnel (floor at 0, ceiling at 3) whose floor has a 1 m slit at x 3..4.
        tunnel = np.concatenate([quad(0, 0, 3, 2, 0.0), quad(4, 0, 8, 2, 0.0), quad(0, 0, 8, 2, 3.0, up=False)])
        s = mesh3d.floor_scan(tunnel)
        self.assertEqual(s["floors"][0, 0, 3], 0)
        self.assertEqual(s["clearance"][0, 0, 3], 30)

    def test_solid_rock_between_two_tunnels_is_not(self):
        # Two tunnels side by side, a 1 m wall of rock between them (nothing over it).
        tunnels = np.concatenate([quad(0, 0, 3, 2, 0.0), quad(0, 0, 3, 2, 3.0, up=False), quad(4, 0, 8, 2, 0.0), quad(4, 0, 8, 2, 3.0, up=False)])
        s = mesh3d.floor_scan(tunnels)
        self.assertEqual(s["floors"][0, 0, 3], mesh3d.NO_FLOOR)


class PickFloors(unittest.TestCase):
    def test_a_floor_right_under_another_has_only_the_room_between(self):
        floors = np.array([[[50]], [[45]], [[0]]], np.int16)
        room = np.array([[[30]], [[120]], [[40]]], np.int16)
        f, r = mesh3d.pick_floors(floors, room, 3)
        self.assertEqual(f[:, 0, 0].tolist(), [50, 45, 0])
        self.assertEqual(r[:, 0, 0].tolist(), [30, 5, 40])

    def test_stair_flights_come_before_the_landing_slabs_over_them(self):
        # Bunker Residential's stairwell: flights with 5.5 m over them, and the slabs
        # under the next flight up with 1.6 m.
        floors = np.array([[[2076]], [[2058]], [[2002]], [[1984]], [[1928]]], np.int16)
        room = np.array([[[55]], [[16]], [[55]], [[16]], [[55]]], np.int16)
        f, _ = mesh3d.pick_floors(floors, room, 3)
        self.assertEqual(f[:, 0, 0].tolist(), [2076, 2002, 1928])


if __name__ == "__main__":
    unittest.main()
