"""Tests for the Sunkenland pin builder: python -I -m unittest discover -s tools/pins"""
import unittest

import sunkenland


def entry(kind, name, x=0.0, y=30.0, z=0.0, active=True, **props):
    return {"kind": kind, "name": name, "position": {"x": x, "y": y, "z": z}, "active": active, "props": props}


class Names(unittest.TestCase):
    def test_names(self):
        cases = {
            ("location", "Three Barrel Island."): "Three Barrel Island",
            ("trader", "GunTrader"): "Gun Trader",
            ("trader", "RandomTrader2"): "Trader",
            ("trader", "Trader New"): "Trader",
            ("npc", "TradeContractor"): "Trade Contractor",
            ("research", "SolarPanel"): "Solar Panel",
            ("research", "Aquarium bad"): "Aquarium",
            ("ore_vein", "Iron ore veins"): "Iron ore vein",
            ("mineable", "Mine Copper New"): "Copper ore",
            ("mineable", "Anatase clusters New"): "Anatase (titanium)",
            ("fish", "Marlins 1-"): "Marlins",
            ("forage", "Branches Spanwer"): "Branches",
            ("forage", "RedAlgae Spawner"): "Red Algae",
            ("supply_crate", "SupplyCrate Scrap Metal"): "Supply crate (scrap metal)",
            ("chest", "Military Container"): "Military Container",
        }
        for (kind, name), want in cases.items():
            self.assertEqual(sunkenland.pin_name(kind, name, {}), want, name)

    def test_skipped(self):
        self.assertIsNone(sunkenland.pin_name("fish", "Test clownfish black", {}))
        self.assertIsNone(sunkenland.pin_name("chest", "Chest Level", {}))

    def test_containers_split_by_value(self):
        self.assertEqual(sunkenland.category_of("chest", "Safe"), "valuable")
        self.assertEqual(sunkenland.category_of("chest", "Toolbox"), "container")


class Build(unittest.TestCase):
    def test_build(self):
        export = {
            "game": "sunkenland",
            "sea_level": 28.6,
            "entries": [
                entry("location", "The Bridge", y=40, level="3", blueprints="2", description="A shop.", icon="The Bridge"),
                entry("research", "SolarPanel", y=10, teaches="Solar panel", location="Airport"),
                entry("fish", "Sharks", active=False),
                entry("mineable", "Mine Iron New", location="Metal Mine"),
                entry("mineable", "Mine Iron New", location="Metal Mine"),  # duplicate object
            ],
        }
        pins = {p["category"]: p for p in sunkenland.build_pins(export)}
        self.assertEqual(sorted(pins), ["location", "ore", "research"])
        loc = pins["location"]
        self.assertEqual(loc["icon"], "game:The Bridge")
        self.assertEqual(loc["description"], "A shop.\nLevel 3, 2 blueprints")
        self.assertEqual(pins["research"]["description"], "Teaches: Solar panel\nAirport\nUnderwater, 19 m deep")
        self.assertEqual(len(sunkenland.build_pins(export)), 3)


if __name__ == "__main__":
    unittest.main()
