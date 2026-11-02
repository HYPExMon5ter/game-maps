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
            for rel in ["game.json", "pins.json", "preview.png", "tiles/3/3/3.webp"]:
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

    def test_requires_tiles(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(SystemExit):
                publish.build_pack(tmp, os.path.join(tmp, "pack.zip"))


if __name__ == "__main__":
    unittest.main()
