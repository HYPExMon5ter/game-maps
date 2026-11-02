"""Tests for the pin builder: python -I -m unittest discover -s tools/pins"""
import unittest

import sotf


def entry(kind, name, x=0.0, y=10.0, z=0.0, **props):
    return {"kind": kind, "name": name, "position": {"x": x, "y": y, "z": z}, "active": False, "props": props}


class Classification(unittest.TestCase):
    def test_categories(self):
        cases = {
            ("BlueprintSpinTrap", "Equippable, UniqueItem"): "blueprint",
            ("KeyCardVIP", "Equippable, UniqueItem"): "key",
            ("Shovel", "Equippable, Droppable, UniqueItem, Weapon"): "tool",
            ("PistolSuppressor", "CraftingMaterial, Extension"): "upgrade",
            ("9mmAmmo", "Droppable, Ammo, Craftable"): "ammo",
            ("GoldenArmour", "Clothing, Droppable, UniqueItem"): "outfit",
            ("Katana", "Equippable, Droppable, UniqueItem, Weapon"): "weapon",
            ("EnergyBar", "Edible, Droppable"): "food",
            ("OldNoteA", "Equippable, UniqueItem"): "collectible",
            ("Rope", "Equippable, CraftingMaterial, Droppable"): "material",
        }
        for (name, types), want in cases.items():
            got = sotf.item_category(name, {t.strip() for t in types.split(",")})
            self.assertEqual(got, want, name)

    def test_names(self):
        self.assertEqual(sotf.item_name("EmailPrintoutGolfA", "Note"), "Email Printout Golf A")
        self.assertEqual(sotf.item_name("BlueprintGoreChair", "Blueprint"), "Blueprint: Gore Chair")
        self.assertEqual(sotf.item_name("FlightAttendantUniform", "Stewardess Dress​"), "Stewardess Dress")

    def test_layers(self):
        self.assertEqual(sotf.layer_of(entry("pickup", "a", terrain_y="11")), "surface")
        self.assertEqual(sotf.layer_of(entry("pickup", "a", terrain_y="30")), "caves")
        self.assertEqual(sotf.layer_of(entry("pickup", "a", terrain_y="9", cave_area="2")), "caves")

    def test_common_label(self):
        self.assertEqual(sotf.common_label({"Cave DA", "Cave DB"}), "Cave D")
        self.assertEqual(sotf.common_label({"Cave A"}), "Cave A")


class Building(unittest.TestCase):
    def test_plants_clones_and_duplicates_are_skipped(self):
        entries = [
            entry("pickup", "RopePickup", item="Rope", item_type="CraftingMaterial", terrain_y="10"),
            entry("pickup", "RopePickup", x=0.1, item="Rope", item_type="CraftingMaterial", terrain_y="10"),
            entry("pickup", "TwinberriesPickup", item="Twinberries", item_type="Edible"),
            entry("pickup", "RockPickup(Clone)", item="Stone", item_type="CraftingMaterial"),
        ]
        pins = sotf.Pins()
        sotf.add_items(pins, entries, {})
        self.assertEqual([p["name"] for p in pins.pins], ["Rope"])

    def test_cave_entrances_list_their_loot(self):
        entries = [
            entry("gps_locator", "CaveAEntranceGPS", x=0, icon="Cave"),
            entry("cave_entrance", "CaveACaveEntranceA", x=5, area="2"),
            entry("cave_entrance", "CaveACaveExit", x=500, area="2"),
            entry("pickup", "RebreatherPickup", x=100, y=-20, item="Rebreather", title="Rebreather",
                  item_type="Equippable, Clothing, UniqueItem", terrain_y="10", cave_area="2"),
        ]
        pins = sotf.Pins()
        areas = sotf.add_points(pins, entries)
        notable = sotf.add_items(pins, entries, areas)
        sotf.describe_areas(pins, areas, notable)
        by_name = {p["name"]: p for p in pins.pins}
        self.assertEqual(by_name["Cave A"]["description"], "Contains: Rebreather")
        self.assertIn("Cave A exit", by_name)
        self.assertEqual(by_name["Rebreather"]["description"], "In Cave A")
        self.assertEqual(by_name["Rebreather"]["layer"], "caves")


if __name__ == "__main__":
    unittest.main()
