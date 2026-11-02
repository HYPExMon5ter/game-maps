"""Build Sons of the Forest pin data from in-game world exports.

The game spawns pickups from runtime data, so positions come from the agent's world
export (overlay app: Settings -> Diagnostics -> Export world data, best done right
after starting a new game, before anything is picked up). This script turns one or
more exports into `games/sons-of-the-forest/pins.json`. Standard library only.

Usage:
    python -I tools/pins/sotf.py --out games/sons-of-the-forest EXPORT.json [EXPORT.json ...]

Icons are referenced as `game:<TextureName>`; `tools/mapgen/sotf.py --icons-only`
extracts them. Re-run both after game updates; nothing here is hand-placed.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys

# Pins deeper than this below the terrain surface are on the underground layer.
UNDERGROUND_DEPTH = 4.0
# Exports of the same object from different sessions differ by float noise only.
DUPLICATE_RADIUS = 0.5
# A cave trigger this close to a GPS entrance marker belongs to that entrance.
ENTRANCE_MATCH_RADIUS = 25.0

# Items not worth a pin: everywhere, respawning, or body parts.
SKIP_ITEMS = {
    "Rock", "Stone", "SmallRock", "Stick", "Bone", "Skull", "Meat", "Fish", "GolfBall",
    "SeveredArm", "SeveredLeg", "SeveredHead", "SeagullHead",
    # Covered by the GPS markers placed on the same objects.
    "HangGlider", "GPSLocator",
    # Animal drops.
    "DeerHide", "RabbitHide",
}
# Wild plants are spawned procedurally around the player, so an export only ever has
# the ones near where the player has been; leave them out rather than show a partial set.
PLANT = re.compile(r"(berries|Mushroom)$|^(Fireweed|DevilsClub|AloeVera|Yarrow|Horsetail|Chicory|ArrowleafBalsamroot|Guarana)$")
TOOLS = {
    "Shovel", "Pickaxe", "RopeGun", "Rebreather", "NightVisionGoggles", "Binoculars", "Flashlight",
    "KnightV", "ActionCamera", "Radio", "CookingPot", "CanOpener", "Guitar", "AirCanister",
}
KEY_ITEMS = {"Cross", "GoldMask"}
EXPLOSIVES = {"Flare", "Grenade", "TimeBomb", "C4Brick", "MolotovAmmo", "ZiplineRope"}
# Display titles that don't identify the item; the internal name is used instead.
GENERIC_TITLES = {"Note", "Blueprint", "???", "Cult Sign"}

GPS_SIGNALS = {
    "WrenchIcon": "Worker GPS signal",
    "MysteryIcon": "Mystery GPS signal",
    "DeadTactiLogo": "Mercenary GPS signal",
}


def humanize(name: str) -> str:
    """`EmailPrintoutGolfA` -> `Email Printout Golf A`."""
    words = re.findall(r"[A-Z]?[a-z]+|[A-Z]+(?![a-z])|\d+[a-z]*", name)
    return " ".join(words) or name


def item_category(name: str, types: set[str]) -> str:
    if name.startswith("Blueprint"):
        return "blueprint"
    if name.startswith(("KeyCard", "ArtifactPiece")) or name in KEY_ITEMS:
        return "key"
    if name in TOOLS:
        return "tool"
    if "Extension" in types:
        return "upgrade"
    if name in EXPLOSIVES or name.endswith("Ammo") or ("Ammo" in types and "Weapon" not in types):
        return "ammo"
    if types & {"Clothing", "Armor"}:
        return "outfit"
    if types & {"Weapon", "RangedWeapon"}:
        return "weapon"
    if "Edible" in types:
        return "food"
    if "UniqueItem" in types:
        return "collectible"
    return "material"


def item_name(name: str, title: str | None) -> str:
    title = re.sub(r"[​-‍﻿]", "", title or "").strip()
    if name.startswith("Blueprint"):
        return f"Blueprint: {humanize(name[len('Blueprint'):])}"
    if not title or title in GENERIC_TITLES:
        return humanize(name)
    return title


def depth(entry: dict) -> float:
    ground = entry["props"].get("terrain_y")
    return float(ground) - entry["position"]["y"] if ground is not None else 0.0


def layer_of(entry: dict) -> str:
    """Inside a cave system (bunkers included), or well below the terrain surface."""
    underground = "cave_area" in entry["props"] or depth(entry) > UNDERGROUND_DEPTH
    return "caves" if underground else "surface"


def common_label(labels: set[str]) -> str:
    """`{"Cave DA", "Cave DB"}` -> `"Cave D"`: one cave with several entrance markers."""
    prefix = os.path.commonprefix(sorted(labels)).strip()
    return prefix if len(labels) > 1 and len(prefix) > len("Cave ") else sorted(labels)[0]


def distance(a: dict, b: dict) -> float:
    return math.dist((a["x"], a["y"], a["z"]), (b["x"], b["y"], b["z"]))


def rounded(p: dict) -> dict:
    return {k: round(p[k], 2) for k in ("x", "y", "z")}


def pin_id(prefix: str, key: str, p: dict) -> str:
    return f"{prefix}:{key}@{p['x']:.0f},{p['y']:.0f},{p['z']:.0f}"


class Pins:
    def __init__(self):
        self.pins: list[dict] = []
        self.ids: set[str] = set()
        self.by_name: dict[tuple[str, str], list[dict]] = {}

    def add(self, pin: dict) -> None:
        same = self.by_name.setdefault((pin["category"], pin["name"]), [])
        if any(distance(q["position"], pin["position"]) < DUPLICATE_RADIUS for q in same):
            return
        same.append(pin)
        base, n = pin["id"], 2
        while pin["id"] in self.ids:
            pin["id"] = f"{base}#{n}"
            n += 1
        self.ids.add(pin["id"])
        self.pins.append(pin)


def add_items(pins: Pins, entries: list[dict], area_names: dict[str, str]) -> dict[str, set[str]]:
    """Adds item pins. Returns the notable (unique, non-note) items found in each area."""
    notable: dict[str, set[str]] = {}
    for e in entries:
        if e["kind"] not in ("pickup", "virtual_pickup", "pickup_switcher"):
            continue
        props = e["props"]
        name = props.get("item")
        # Runtime-spawned clones (e.g. rocks around the crash site) aren't world content.
        if not name or name in SKIP_ITEMS or PLANT.search(name) or "(Clone)" in e["name"]:
            continue
        types = {t.strip() for t in props.get("item_type", "").split(",") if t.strip()}
        pos = rounded(e["position"])
        pin = {
            "id": pin_id("item", name, pos),
            "category": item_category(name, types),
            "name": item_name(name, props.get("title")),
            "position": pos,
            "layer": layer_of(e),
        }
        if props.get("icon"):
            pin["icon"] = f"game:{props['icon']}"
        area = props.get("cave_area")
        if area in area_names:
            pin["description"] = f"In {area_names[area]}"
            if "UniqueItem" in types and pin["category"] not in ("collectible", "food", "material"):
                notable.setdefault(area, set()).add(pin["name"])
        pins.add(pin)
    return notable


def add_points(pins: Pins, entries: list[dict]) -> dict[str, str]:
    """Adds cave, bunker, vehicle and GPS pins. Returns display names by cave area id."""
    gps = [e for e in entries if e["kind"] == "gps_locator"]
    triggers = [e for e in entries if e["kind"] == "cave_entrance"]

    # Cave and bunker entrances: the GPS markers name them; each appears several
    # times (one locator per icon state), keep one per name.
    entrances: dict[str, dict] = {}
    for e in gps:
        m = re.fullmatch(r"(Snow)?Cave(\w+?)EntranceGPS", e["name"])
        if m:
            label = f"{'Snow ' if m.group(1) else ''}Cave {m.group(2)}"
            entrances.setdefault(label, {"entry": e, "category": "cave", "icon": "game:Cave"})
        elif e["name"].startswith("Bunker") and e["props"].get("poi"):
            icon = e["props"].get("icon")
            entrances[e["props"]["poi"]] = {"entry": e, "category": "bunker", "icon": f"game:{icon}" if icon else None}

    # Cave triggers share an area id per cave/bunker; name extra openings after the
    # entrance marker they share an area with.
    area_labels: dict[str, set[str]] = {}
    for t in triggers:
        area = t["props"].get("area", "")
        for label, ent in entrances.items():
            if distance(t["position"], ent["entry"]["position"]) < ENTRANCE_MATCH_RADIUS:
                area_labels.setdefault(area, set()).add(label)
    area_names = {area: common_label(labels) for area, labels in area_labels.items()}
    # Caves without a GPS marker: the trigger object names carry the cave letter.
    for t in triggers:
        m = re.match(r"(Cave|Bunker)([A-Z])(?=[A-Z])", t["name"])
        area = t["props"].get("area", "")
        if m and area not in area_names:
            area_names[area] = f"{m.group(1)} {m.group(2)}"

    for label, ent in sorted(entrances.items()):
        e = ent["entry"]
        pos = rounded(e["position"])
        pin = {"id": pin_id(ent["category"], label, pos), "category": ent["category"], "name": label, "position": pos, "layer": "surface"}
        if ent["icon"]:
            pin["icon"] = ent["icon"]
        pins.add(pin)

    for t in triggers:
        if any(distance(t["position"], ent["entry"]["position"]) < ENTRANCE_MATCH_RADIUS for ent in entrances.values()):
            continue
        owner = area_names.get(t["props"].get("area", ""))
        is_exit = "exit" in t["name"].lower()
        name = f"{owner} {'exit' if is_exit else 'entrance'}" if owner else ("Cave exit" if is_exit else "Cave entrance")
        category = "bunker" if owner and owner.startswith("Bunker") else "cave"
        pos = rounded(t["position"])
        pins.add({
            "id": pin_id(category, name, pos),
            "category": category,
            "name": name,
            "position": pos,
            "layer": "surface",
            "icon": "game:CaveOutline" if category == "cave" else "game:BunkerIconOutline",
        })

    for e in gps:
        icon = e["props"].get("icon")
        pos = rounded(e["position"])
        if e["name"].startswith("GolfCartGPSLocator"):
            pin = {"category": "vehicle", "name": "Golf Cart", "icon": "game:GolfCartIcon"}
        elif e["name"].startswith("WorldHangGlider"):
            pin = {"category": "vehicle", "name": "Hang Glider", "icon": "game:HangGliderIcon"}
        elif icon in GPS_SIGNALS:
            pin = {"category": "gps", "name": GPS_SIGNALS[icon], "icon": f"game:{icon}"}
        else:
            continue
        pin.update({"id": pin_id(pin["category"], pin["name"], pos), "position": pos, "layer": layer_of(e)})
        pins.add(pin)
    return area_names


def describe_areas(pins: Pins, area_names: dict[str, str], notable: dict[str, set[str]]) -> None:
    """Lists each cave's or bunker's notable loot on its entrance pins."""
    for area, items in notable.items():
        name = area_names[area]
        for pin in pins.pins:
            if pin["category"] in ("cave", "bunker") and pin["name"].startswith(name):
                pin["description"] = "Contains: " + ", ".join(sorted(items))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, help="games/sons-of-the-forest directory")
    ap.add_argument("exports", nargs="+", help="world export JSON files from the overlay app")
    args = ap.parse_args()

    entries = []
    for path in args.exports:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        if data.get("game") != "sons-of-the-forest":
            print(f"{path}: not a Sons of the Forest export", file=sys.stderr)
            return 1
        entries.extend(data["entries"])

    pins = Pins()
    area_names = add_points(pins, entries)
    notable = add_items(pins, entries, area_names)
    describe_areas(pins, area_names, notable)
    result = sorted(pins.pins, key=lambda p: (p["category"], p["name"], p["id"]))

    icons_index = os.path.join(args.out, "icons", "game", "index.json")
    if os.path.exists(icons_index):
        with open(icons_index, encoding="utf-8") as f:
            known = set(json.load(f))
        missing = sorted({p["icon"][5:] for p in result if p.get("icon", "").startswith("game:")} - known)
        if missing:
            print(f"warning: icons not extracted (run tools/mapgen --icons-only): {', '.join(missing)}", file=sys.stderr)

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
