"""Tests for the Grounded 2 pin builder: python -I -m unittest discover -s tools/pins"""
import unittest

import grounded2

ITEMS = {
    "TechChip_Ominent_Hedge": {"name": "Hedge Optical Disk", "type": "EItemType::PartyKeyItem", "keyItem": "KeyItem.Stuff"},
    "WorkAssignmentsNote": {"name": "Work Assignments", "type": "EItemType::PartyKeyItem", "keyItem": "KeyItem.Ominent"},
    "KeyIceCart": {"name": "Frostbitten Key", "type": "EItemType::PartyKeyItem", "keyItem": "KeyItem.Stuff"},
    "BurglBody": {"name": "Robot Body", "type": "EItemType::PartyKeyItem", "keyItem": "KeyItem.Stuff"},
    "RecipeStatueKoi": {"name": "Koi Figurine", "type": "EItemType::PartyKeyItem", "keyItem": "KeyItem.Stuff"},
    "UpgradeWeapon1": {"name": "Brittle Quartzite Shard", "type": "EItemType::General"},
    "ORCChip": {"name": "O.R.C. Receiver", "type": "EItemType::General"},
    "WarhammerSpicyUnique": {"name": "Boom Stick", "type": "EItemType::General"},
    "SlimeMold": {"name": "Slime Mold Stalk", "type": "EItemType::General"},
    "QuartzShard": {"name": "Quartzite", "type": "EItemType::General"},
    "Web": {"name": "Spider Web", "type": "EItemType::General"},
}


def lod(lod_class, x=0.0, y=0.0, z=0.0, **props):
    return {"kind": "lod", "name": lod_class, "position": {"x": x, "y": y, "z": z}, "active": True, "props": {"lod": lod_class, **props}}


class Coordinates(unittest.TestCase):
    def test_unreal_centimeters_become_agent_meters(self):
        # Unreal X forward, Y right, Z up -> agent x = Y, y = Z, z = X.
        self.assertEqual(grounded2.to_agent(100.0, 200.0, 300.0), {"x": 2.0, "y": 3.0, "z": 1.0})


class Categories(unittest.TestCase):
    def test_items(self):
        cases = {
            "TechChip_Ominent_Hedge": "disk",
            "WorkAssignmentsNote": "note",
            "KeyIceCart": "key",
            "BurglBody": "key",
            "RecipeStatueKoi": "recipe",
            "UpgradeWeapon1": "upgrade",
            "ORCChip": "orc",
            "WarhammerSpicyUnique": "unique",
            "Taming_EggAnt": "egg",
            "SlimeMold": None,
        }
        for row, want in cases.items():
            with self.subTest(row=row):
                self.assertEqual(grounded2.item_category(row, ITEMS.get(row)), want)

    def test_eggs_without_names_are_named_by_their_row(self):
        self.assertEqual(grounded2.egg_name("Taming_EggBlackAnt"), "Black Ant Egg")


class Export(unittest.TestCase):
    def build(self, entries, characters=None):
        pins = grounded2.Pins()
        grounded2.add_export(pins, {"items": ITEMS, "characters": characters or {}}, entries)
        return pins.pins

    def test_resource_nodes_are_named_by_what_they_drop(self):
        pins = self.build([lod("HarvestNodeLODActor", drops="QuartzShard,SlimeMold", resource_finder="1")])
        self.assertEqual([(p["category"], p["name"]) for p in pins], [("resource", "Quartzite")])
        self.assertEqual(pins[0]["description"], "Drops Quartzite, Slime Mold Stalk")

    def test_kinds_known_by_their_lite_data(self):
        pins = self.build([
            lod("HarvestNodeLODActor", lite="LD_Milk_Nugget_Personal"),
            lod("HarvestNodeLODActor", x=5, lite="LD_World_BASE_Buried_Item"),
            lod("LootBagLODActor", x=10, lite="LD_BASE_OminentDeadBody"),
            lod("ScienceCollectableLODActor", x=15),
            lod("HarvestNodeLODActor", x=20, lite="LD_Bombable_Rock"),
        ])
        self.assertEqual(sorted(p["category"] for p in pins), ["body", "dig", "milk", "science"])

    def test_only_one_of_a_kind_creatures_are_bosses(self):
        characters = {"1": "Papa Toe Biter", "2": "Aphid", "3": "O.R.C. Wasp"}
        pins = self.build([
            lod("CreatureLODActor", name_id="1"),
            lod("CreatureLODActor", x=5, name_id="2"),
            lod("CreatureLODActor", x=50, name_id="2"),
            lod("CreatureLODActor", x=90, name_id="3"),
        ], characters)
        self.assertEqual([(p["category"], p["name"]) for p in pins], [("boss", "Papa Toe Biter")])

    def test_the_same_object_from_two_exports_is_one_pin(self):
        pins = self.build([lod("ScienceCollectableLODActor", x=1.0), lod("ScienceCollectableLODActor", x=1.1)])
        self.assertEqual(len(pins), 1)


class Level(unittest.TestCase):
    def test_points_of_interest_and_landmarks(self):
        pins = grounded2.Pins()
        grounded2.add_level(pins, {
            "pois": [
                {"name": "Calculator", "category": "PointOfInterest.Wonder", "x": 0, "y": 0, "z": 0},
                {"name": "Ranger Outpost: Fire Pit", "category": "PointOfInterest.FieldStation", "x": 100, "y": 0, "z": 0},
                {"name": None, "category": "PointOfInterest.Wonder", "x": 200, "y": 0, "z": 0},
            ],
            "landmarks": [{"cls": "BP_Teleporter_C", "x": 300, "y": 0, "z": 0}, {"cls": "BP_PlayerFallBackRespawn_C", "x": 400, "y": 0, "z": 0}],
        })
        self.assertEqual(sorted((p["category"], p["name"]) for p in pins.pins), [("landmark", "Calculator"), ("outpost", "Ranger Outpost: Fire Pit"), ("teleporter", "Teleporter")])


if __name__ == "__main__":
    unittest.main()
