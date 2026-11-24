import hashlib
import os
import tempfile
import unittest
import zipfile

import publish


class BuildPackTest(unittest.TestCase):
    def test_packs_generated_data_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            game = os.path.join(tmp, "game")
            os.makedirs(os.path.join(game, "tiles", "3", "3"))
            os.makedirs(os.path.join(game, "build"))
            for rel in ["game.json", "pins.json", "preview.png", "tiles/3/3/3.webp", "build/traversal.json"]:
                with open(os.path.join(game, rel), "wb") as f:
                    f.write(b"x")
            out = os.path.join(tmp, "pack.zip")

            info = publish.build_pack(game, out)

            with zipfile.ZipFile(out) as zf:
                self.assertEqual(sorted(zf.namelist()), ["pins.json", "tiles/3/3/3.webp"])
                self.assertEqual(zf.getinfo("tiles/3/3/3.webp").compress_type, zipfile.ZIP_STORED)
            with open(out, "rb") as f:
                self.assertEqual(info["sha256"], hashlib.sha256(f.read()).hexdigest())
            self.assertEqual(info["size"], os.path.getsize(out))

    def test_same_files_give_same_pack(self):
        with tempfile.TemporaryDirectory() as tmp:
            game = os.path.join(tmp, "game")
            os.makedirs(os.path.join(game, "tiles"))
            tile = os.path.join(game, "tiles", "a.webp")
            with open(tile, "wb") as f:
                f.write(b"x")
            first = publish.build_pack(game, os.path.join(tmp, "1.zip"))
            os.utime(tile, (0, 1_000_000_000))  # a newer timestamp alone isn't a change
            second = publish.build_pack(game, os.path.join(tmp, "2.zip"))
            self.assertEqual(first["sha256"], second["sha256"])

            with open(tile, "wb") as f:
                f.write(b"y")
            third = publish.build_pack(game, os.path.join(tmp, "3.zip"))
            self.assertNotEqual(first["sha256"], third["sha256"])

    def test_optional_parts_get_packs_of_their_own(self):
        with tempfile.TemporaryDirectory() as tmp:
            game = os.path.join(tmp, "game")
            for rel in ["pins.json", "tiles/a.webp", "3d/index.json", "3d/cave-a.bin", "nav/caves.json"]:
                os.makedirs(os.path.dirname(os.path.join(game, rel)), exist_ok=True)
                with open(os.path.join(game, rel), "wb") as f:
                    f.write(b"x")
            map_zip, extra_zip = os.path.join(tmp, "map.zip"), os.path.join(tmp, "3d.zip")
            publish.build_pack(game, map_zip)
            publish.build_pack(game, extra_zip, "3d")
            with zipfile.ZipFile(map_zip) as zf:
                self.assertEqual(sorted(zf.namelist()), ["nav/caves.json", "pins.json", "tiles/a.webp"])
            with zipfile.ZipFile(extra_zip) as zf:
                self.assertEqual(sorted(zf.namelist()), ["3d/cave-a.bin", "3d/index.json"])
            self.assertEqual(publish.pack_files(os.path.join(tmp, "nothing"), "3d"), [])

    def test_requires_tiles(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(SystemExit):
                publish.build_pack(tmp, os.path.join(tmp, "pack.zip"))


if __name__ == "__main__":
    unittest.main()


class PinGateTest(unittest.TestCase):
    def test_small_changes_pass(self):
        self.assertEqual(publish.lost_pins({"a": 100}, {"a": 95}), [])

    def test_a_lost_category_stops_the_publish(self):
        self.assertEqual(publish.lost_pins({"a": 100, "b": 10}, {"a": 100}), ["b: 10 -> 0"])

    def test_nothing_to_compare_with_passes(self):
        self.assertEqual(publish.lost_pins({}, {"a": 1}), [])


class Completed:
    def __init__(self, returncode=0, stdout="", stderr=""):
        self.returncode, self.stdout, self.stderr = returncode, stdout, stderr


class ManifestTest(unittest.TestCase):
    """The manifest lists every game: one that can't be read must stop the run, not start over."""

    def fetch(self, answers):
        calls = []

        def gh(*args, check=True):
            calls.append(args[:2])
            return answers[args[:2]]

        original, publish.gh = publish.gh, gh
        try:
            return publish.fetch_manifest("owner/repo"), calls
        finally:
            publish.gh = original

    def test_reads_the_published_manifest(self):
        manifest, _ = self.fetch({
            ("release", "view"): Completed(stdout="maps.json\nsotf-1.zip\n"),
            ("release", "download"): Completed(stdout='{"packs": {"sotf": {"version": "1"}}}'),
        })
        self.assertEqual(manifest, {"packs": {"sotf": {"version": "1"}}})

    def test_starts_empty_only_when_there_is_none(self):
        manifest, calls = self.fetch({("release", "view"): Completed(stdout="sotf-1.zip\n")})
        self.assertEqual(manifest, {"packs": {}})
        self.assertEqual(calls, [("release", "view")])

        manifest, calls = self.fetch({
            ("release", "view"): Completed(returncode=1, stderr="release not found"),
            ("release", "create"): Completed(),
        })
        self.assertEqual(manifest, {"packs": {}})
        self.assertIn(("release", "create"), calls)

    def test_a_failed_lookup_or_download_stops_the_publish(self):
        with self.assertRaises(SystemExit):
            self.fetch({("release", "view"): Completed(returncode=1, stderr="HTTP 502")})
        with self.assertRaises(SystemExit):
            self.fetch({
                ("release", "view"): Completed(stdout="maps.json\n"),
                ("release", "download"): Completed(returncode=1, stderr="connection reset"),
            })

    def test_a_manifest_that_is_not_one_stops_the_publish(self):
        for text in ["", "{broken", "[]", "{}", '{"packs": []}']:
            with self.assertRaises(SystemExit, msg=text):
                publish.parse_manifest(text)
