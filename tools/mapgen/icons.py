"""Game icon export helpers shared by the map generators."""
from __future__ import annotations

import json
import os

from PIL import Image


def trim_icon(img: Image.Image, margin: float = 0.06) -> Image.Image:
    """Crops an icon to its visible glyph and centers it on a square canvas, so every
    icon fills pin badges the same way (inventory icons carry wide, uneven padding)."""
    img = img.convert("RGBA")
    bbox = img.getchannel("A").point(lambda a: 255 if a > 8 else 0).getbbox()
    if not bbox:
        return img
    glyph = img.crop(bbox)
    side = round(max(glyph.size) * (1 + 2 * margin))
    out = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    out.paste(glyph, ((side - glyph.width) // 2, (side - glyph.height) // 2))
    return out


def write_icon_index(icons_dir: str) -> None:
    """Lists every exported icon (`icons/game/index.json`), for the custom pin icon picker."""
    names = sorted(n[:-4] for n in os.listdir(icons_dir) if n.endswith(".png"))
    with open(os.path.join(icons_dir, "index.json"), "w") as f:
        json.dump(names, f)
