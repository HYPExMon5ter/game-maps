"""Build Sunkenland pin data from the world export.

Every point of interest in Sunkenland is placed in the world scene, so the export comes
straight from the game files (`tools/mapgen/sunkenland.py --export-world`), no in-game
step needed. This script turns it into `games/sunkenland/pins.json`. Standard library only.

Usage:
    python -I tools/pins/sunkenland.py --out games/sunkenland world.json

Icons are referenced as `game:<SpriteName>`; `tools/mapgen/sunkenland.py --icons-only`
extracts them. Re-run both after game updates; nothing here is hand-placed.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys

# Pins this far below the sea surface say how deep they are.
UNDERWATER_DEPTH = 1.5

# Kinds kept even when inactive in the scene: the game enables them later (companions
# waiting to be rescued appear once their location is reached).
ALWAYS_SHOWN = {"companion"}
# Containers worth a visible pin; the rest are in the hidden "containers" category.
VALUABLE_CONTAINERS = {"Safe", "Military Container", "Large Container", "Stash", "Air Drop Supplies", "Loot Drop"}
# Mineable nodes: export name (without "Mine " / " New") -> display name.
ORE_NAMES = {"Iron": "Iron ore", "Copper": "Copper ore", "Sulfur": "Sulfur", "Anatase clusters": "Anatase (titanium)"}


def words(name: str) -> str:
    """`GunTrader2` -> `Gun Trader`, `SolarPanel` -> `Solar Panel`, `RedAlgae` -> `Red Algae`."""
    name = re.sub(r"(?<=[a-z])(?=[A-Z])", " ", name)
    name = re.sub(r"[\s_-]*\d+[\s-]*$", "", name).replace("_", " ")
    return re.sub(r"\s+", " ", name).strip()


def slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def pin_name(kind: str, name: str, props: dict) -> str | None:
    """Display name of an export entry, or None to leave it out."""
    if kind == "location":
        return name.rstrip(".").strip()
    if kind in ("trader", "npc"):
        base = words(name).replace(" New", "").replace("New ", "").strip()
        if base in ("Trader", "Random Trader"):
            return f"Trader ({props['location']})" if props.get("location") else "Trader"
        return base
    if kind == "companion":
        return words(name)
    if kind == "prison":
        return "Prison cage"
    if kind == "research":
        return words(re.sub(r"\s+bad$", "", name))
    if kind == "ore_vein":
        return words(name).replace(" veins", " vein")
    if kind == "mineable":
        base = re.sub(r"^Mine\s+|\s+New$", "", name)
        return ORE_NAMES.get(base, base)
    if kind == "fish":
        base = words(name)
        return None if base.lower().startswith("test") else base
    if kind == "forage":
        return words(re.sub(r"\s*Spa(?:wn|nw)er$", "", name))
    if kind == "airdrop":
        return "Airdrop platform"
    if kind == "map_fragment":
        return "Map fragment"
    if kind == "supply_crate":
        rest = name.replace("SupplyCrate", "").strip()
        return f"Supply crate ({rest.lower()})" if rest else "Supply crate"
    if kind == "chest":
        base = words(name)
        return base if base and base not in ("A", "Chest Level") else None
    if kind == "radiation":
        return "Radiation zone"
    return None


def category_of(kind: str, name: str) -> str:
    if kind in ("trader", "npc"):
        return "trader"
    if kind in ("ore_vein", "mineable"):
        return "ore"
    if kind == "chest":
        return "valuable" if name in VALUABLE_CONTAINERS else "container"
    return {
        "location": "location",
        "companion": "companion",
        "prison": "captive",
        "research": "research",
        "fish": "fish",
        "forage": "forage",
        "airdrop": "airdrop",
        "map_fragment": "fragment",
        "supply_crate": "supply",
        "radiation": "hazard",
    }[kind]


def describe(kind: str, props: dict, depth: float) -> str | None:
    parts = []
    if kind == "location":
        if props.get("description"):
            parts.append(props["description"])
        facts = [f"Level {props['level']}"] if props.get("level", "0") != "0" else []
        if props.get("blueprints", "0") != "0":
            facts.append(f"{props['blueprints']} blueprints")
        if facts:
            parts.append(", ".join(facts))
    else:
        if props.get("teaches"):
            parts.append(f"Teaches: {props['teaches']}")
        if kind == "ore_vein":
            parts.append("Place an auto-miner here")
        if props.get("captives", "0") != "0":
            parts.append(f"{props['captives']} captives")
        if props.get("radius"):
            parts.append(f"{props['radius']} m radius")
        if props.get("location"):
            parts.append(props["location"])
    if depth >= UNDERWATER_DEPTH:
        parts.append(f"Underwater, {depth:.0f} m deep")
    return "\n".join(parts) if parts else None


def build_pins(export: dict) -> list[dict]:
    sea = float(export.get("sea_level", 0.0))
    pins: dict[str, dict] = {}
    for e in export["entries"]:
        if not e.get("active", True) and e["kind"] not in ALWAYS_SHOWN:
            continue
        kind, props = e["kind"], e.get("props", {})
        name = pin_name(kind, e["name"], props)
        if not name:
            continue
        pos = e["position"]
        pin = {
            "id": f"{kind}:{slug(name)}@{round(pos['x'])},{round(pos['y'])},{round(pos['z'])}",
            "category": category_of(kind, name),
            "name": name,
            "position": {k: round(float(pos[k]), 2) for k in ("x", "y", "z")},
            "layer": "surface",
        }
        description = describe(kind, props, sea - float(pos["y"]))
        if description:
            pin["description"] = description
        if kind == "location" and props.get("icon"):
            pin["icon"] = f"game:{props['icon']}"
        pins[pin["id"]] = pin
    return sorted(pins.values(), key=lambda p: (p["category"], p["name"], p["id"]))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, help="games/sunkenland directory")
    ap.add_argument("export", help="world export from tools/mapgen/sunkenland.py --export-world")
    args = ap.parse_args()

    with open(args.export, encoding="utf-8") as f:
        export = json.load(f)
    if export.get("game") != "sunkenland":
        print(f"{args.export}: not a Sunkenland export", file=sys.stderr)
        return 1
    result = build_pins(export)

    icons_index = os.path.join(args.out, "icons", "game", "index.json")
    if os.path.exists(icons_index):
        with open(icons_index, encoding="utf-8") as f:
            known = set(json.load(f))
        missing = sorted({p["icon"][5:] for p in result if p.get("icon", "").startswith("game:")} - known)
        if missing:
            print(f"warning: icons not extracted (run tools/mapgen/sunkenland.py --icons-only): {', '.join(missing)}", file=sys.stderr)

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
