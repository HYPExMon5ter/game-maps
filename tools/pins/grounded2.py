"""Build Grounded 2 pin data.

Two sources, both from the user's own game:

- `data.json` from `tools/mapgen/grounded2-extract`: points of interest and landmark
  actors placed in the level, item names and creature names.
- A world export from the overlay app (Settings -> Advanced -> Export world data) taken in
  a new game: Grounded 2 keeps every item, science pickup, cache and creature of the whole
  world as light "LOD actors" from the start, so one export lists them all.

Usage:
    python -I tools/pins/grounded2.py --out games/grounded-2 --data DATA.json EXPORT.json

Standard library only. Re-run both steps after a game update; nothing is hand-placed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys

CM = 100.0
# Exports of the same object from different sessions differ by float noise only.
DUPLICATE_RADIUS = 0.5

POI_CATEGORIES = {
    "PointOfInterest.Wonder": "landmark",
    "PointOfInterest.FieldStation": "outpost",
    "PointOfInterest.Lab": "lab",
    "PointOfInterest.MIXR": "mixr",
}

LANDMARKS = {
    "BP_Teleporter_C": ("teleporter", "Teleporter"),
    "BP_BurgleStation_C": ("burgl", "BURG.L Station"),
    "BP_ResourceAnalyzer_Augusta_C": ("analyzer", "Resource Analyzer"),
    "BP_CommArray_simple_C": ("comm-array", "Comm Array"),
}

LOD_KINDS = {
    "ScienceCollectableLODActor": ("science", "Raw Science"),
    "ColorCollectibleLODActor": ("paint", "Paint"),
}

# Lite data (what a LOD actor stands for) worth a pin of its own.
LITE_KINDS = {
    "LD_Milk_Nugget_Personal": ("milk", "Milk Nugget"),
    "LD_World_BASE_Buried_Item": ("dig", "Buried Item"),
    "LD_BASE_OminentDeadBody": ("body", "Ominent Scientist"),
    "LD_LootBag": ("cache", "Loot Bag"),
}


def to_agent(x: float, y: float, z: float) -> dict:
    """Unreal centimeters (X forward, Y right, Z up) as the agent reports positions."""
    return {"x": round(y / CM, 2), "y": round(z / CM, 2), "z": round(x / CM, 2)}


def pin_id(category: str, name: str, pos: dict) -> str:
    key = f"{category}|{name}|{round(pos['x'])}|{round(pos['y'])}|{round(pos['z'])}"
    return f"{category}-{hashlib.sha1(key.encode()).hexdigest()[:10]}"


def item_category(row: str, item: dict | None) -> str | None:
    """The pin category for a world item, or None for everyday resources."""
    if row.startswith("TechChip_"):
        return "disk"
    if row.startswith("Recipe"):
        return "recipe"
    if row.startswith("Key") and item and item.get("type") == "EItemType::PartyKeyItem":
        return "key"
    if row.startswith("Upgrade"):
        return "upgrade"
    if row == "ORCChip":
        return "orc"
    if row.startswith("Taming_Egg"):
        return "egg"
    if "Unique" in row or "Unqiue" in row or row.startswith("Accessory"):
        return "unique"
    if item and item.get("type") == "EItemType::PartyKeyItem":
        return "note" if item.get("keyItem") in ("KeyItem.Ominent", "KeyItem.Stranger") else "key"
    return None


def egg_name(row: str) -> str:
    """`Taming_EggBlackAnt` -> `Black Ant Egg`, for eggs without a name of their own."""
    words = re.sub(r"(?<!^)(?=[A-Z])", " ", row.removeprefix("Taming_Egg")).strip()
    return f"{words} Egg" if words else "Egg"


def drop_names(rows: list[str], items: dict) -> list[str]:
    names = []
    for row in rows:
        name = (items.get(row) or {}).get("name")
        if name and name not in names:
            names.append(name)
    return names


def resource_name(rows: list[str], items: dict) -> str | None:
    """A harvest node's name: what it mainly gives (its first drop)."""
    names = drop_names(rows, items)
    return names[0] if names else None


class Pins:
    def __init__(self) -> None:
        self.pins: list[dict] = []

    def add(self, category: str, name: str, pos: dict, **extra) -> None:
        for p in self.pins:
            q = p["position"]
            if p["category"] == category and p["name"] == name and abs(q["x"] - pos["x"]) + abs(q["y"] - pos["y"]) + abs(q["z"] - pos["z"]) < DUPLICATE_RADIUS:
                return
        self.pins.append({"id": pin_id(category, name, pos), "category": category, "name": name, "position": pos, **extra})


def add_level(pins: Pins, data: dict) -> None:
    for poi in data["pois"]:
        category = POI_CATEGORIES.get(poi.get("category"))
        if category and poi.get("name"):
            pins.add(category, poi["name"], to_agent(poi["x"], poi["y"], poi["z"]))
    for actor in data["landmarks"]:
        if actor["cls"] in LANDMARKS:
            category, name = LANDMARKS[actor["cls"]]
            pins.add(category, name, to_agent(actor["x"], actor["y"], actor["z"]))


def add_export(pins: Pins, data: dict, entries: list[dict]) -> None:
    items = data["items"]
    characters = data["characters"]
    creatures: dict[str, list[dict]] = {}
    for e in entries:
        props = e.get("props", {})
        lod = props.get("lod")
        pos = e["position"]
        if lod in LOD_KINDS:
            category, name = LOD_KINDS[lod]
            pins.add(category, name, pos)
        elif props.get("lite") in LITE_KINDS:
            category, name = LITE_KINDS[props["lite"]]
            pins.add(category, name, pos)
        elif lod == "HarvestNodeLODActor" and props.get("drops"):
            name = resource_name(props["drops"].split(","), items)
            if name:
                pins.add("resource", name, pos, description="Drops " + ", ".join(drop_names(props["drops"].split(","), items)))
        elif "item" in props:
            row = props["item"]
            category = item_category(row, items.get(row))
            if category:
                name = (items.get(row) or {}).get("name") or (egg_name(row) if category == "egg" else row)
                pins.add(category, name, pos)
        elif lod == "CreatureLODActor" and props.get("name_id"):
            name = characters.get(props["name_id"])
            if name:
                creatures.setdefault(name.strip(), []).append(pos)
    # Bosses and named creatures: the ones there's only one of, other than the elite
    # (O.R.C.) versions of common bugs.
    for name, places in creatures.items():
        if len(places) == 1 and not name.startswith("O.R.C."):
            pins.add("boss", name, places[0])


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, help="games/grounded-2 directory")
    ap.add_argument("--data", required=True, help="data.json from tools/mapgen/grounded2-extract")
    ap.add_argument("exports", nargs="+", help="world export JSON files from the overlay app")
    args = ap.parse_args()

    with open(args.data, encoding="utf-8-sig") as f:
        data = json.load(f)
    entries = []
    for path in args.exports:
        with open(path, encoding="utf-8") as f:
            export = json.load(f)
        if export.get("game") != "grounded-2":
            print(f"{path}: not a Grounded 2 export", file=sys.stderr)
            return 1
        entries.extend(export["entries"])

    pins = Pins()
    add_level(pins, data)
    add_export(pins, data, entries)
    result = sorted(pins.pins, key=lambda p: (p["category"], p["name"], p["id"]))
    with open(os.path.join(args.out, "pins.json"), "w", encoding="utf-8") as f:
        json.dump({"version": 1, "pins": result}, f, indent=1, ensure_ascii=False)
        f.write("\n")
    counts: dict[str, int] = {}
    for p in result:
        counts[p["category"]] = counts.get(p["category"], 0) + 1
    print(f"wrote {len(result)} pins: " + ", ".join(f"{k} {v}" for k, v in sorted(counts.items())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
