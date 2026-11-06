"""Says which games have updated since their map was published.

Compares each game's installed Steam build with the one its published map pack was made
from. Run it now and then, or on a schedule: the list is what needs regenerating.

    python -I tools/mappack/outdated.py
"""
from __future__ import annotations

import json
import os
import sys
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from publish import GAMES_DIR, game_build  # noqa: E402

MANIFEST = "https://github.com/HYPExMon5ter/game-maps/releases/download/maps/maps.json"


def status(installed: str | None, published: str | None) -> str:
    if installed is None:
        return "not installed here"
    if published is None:
        return f"build {installed}, but the map doesn't say what it was made from"
    return "up to date" if installed == published else f"OUT OF DATE: game {installed}, map {published}"


def main() -> None:
    with urllib.request.urlopen(MANIFEST, timeout=30) as response:
        packs = json.load(response).get("packs", {})
    for game in sorted(os.listdir(GAMES_DIR)):
        folder = os.path.join(GAMES_DIR, game)
        if not os.path.exists(os.path.join(folder, "game.json")):
            continue
        with open(os.path.join(folder, "game.json"), encoding="utf-8") as f:
            definition = json.load(f)
        if definition.get("worldMaps"):
            print(f"{game}: made by the running game, nothing to update")
            continue
        print(f"{game}: {status(game_build(folder), packs.get(game, {}).get('gameBuild'))}")


if __name__ == "__main__":
    main()
