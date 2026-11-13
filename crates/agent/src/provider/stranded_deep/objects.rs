//! What the things in a Stranded Deep zone are, going by their prefab names (`CONTAINER_CRATE`,
//! `SHIPWRECK_3A`, …), as the game's prefab list and save files name them. Instances add
//! "(Clone)".

/// Pin category (from `game.json`) and the kind of thing, for a prefab name.
pub fn classify(name: &str) -> Option<(&'static str, &'static str)> {
    let name = base_name(name);
    let kind = match name {
        n if n.starts_with("SHIPWRECK_") => ("wreck", "Shipwreck"),
        n if n.starts_with("PLANEWRECK_") => ("wreck", "Plane wreck"),
        n if n.starts_with("SeaFort_") => ("wreck", "Sea fort"),
        n if n.starts_with("ROWBOAT_") => ("wreck", "Rowboat"),
        n if n.starts_with("Shelter") && n[7..].chars().all(|c| c.is_ascii_digit()) && n.len() > 7 => ("shelter", "Shelter"),

        "CONTAINER_CRATE" => ("container", "Crate"),
        n if n.starts_with("CONTAINER_CRATE_ENDGAME") => ("container", "Supply crate"),
        "CONTAINER_LOCKER_LARGE" => ("container", "Large locker"),
        "CONTAINER_LOCKER_SMALL" => ("container", "Small locker"),
        "CONTAINER_CONSOLE" => ("container", "Console"),
        "SHIPPING_CONTAINER" | "SHIPPING_CONTAINER_1" | "SHIPPING_CONTAINER_2" | "SHIPPING_CONTAINER_3" => ("container", "Shipping container"),

        "TYRE_PILE" => ("salvage", "Tyres"),
        "BARREL_PILE" => ("salvage", "Barrels"),
        "BUOYBALL_PILE" => ("salvage", "Buoy balls"),
        "DRIFTWOOD_PILE" => ("salvage", "Driftwood"),
        "CLOTH" => ("salvage", "Cloth"),
        "SCRAP_PLANK" => ("salvage", "Scrap planks"),
        "SCRAP_CORRUGATED" => ("salvage", "Corrugated scrap"),

        "POTATO_PLANT" => ("food", "Potato"),
        "YUCCA" => ("food", "Yucca"),
        "KURA_TREE" => ("food", "Kura fruit"),
        "QUWAWA_TREE" => ("food", "Quwawa"),
        "PIPI_PLANT" => ("food", "Pipi"),
        "WAVULAVULA_PLANT" => ("food", "Wavulavula"),
        "BANANA_PLANT" => ("food", "Banana"),

        "ALOE_VERA_PLANT" => ("medicine", "Aloe vera"),
        "AJUGA_PLANT" => ("medicine", "Ajuga"),
        "COCA_BUSH" => ("medicine", "Coca"),

        "MINING_ROCK_CLAY" => ("mining", "Clay"),
        "MINING_ROCK" => ("mining", "Stone"),

        "GIANT_CRAB_SPAWNER" => ("den", "Giant crab"),
        "BOAR_SPAWNER" => ("den", "Boar"),
        "HOG_SPAWNER" => ("den", "Wild hog"),
        "SNAKE_SPAWNER" | "HIDESPOT_SNAKE" => ("den", "Snake"),

        "VEHICLE_LIFERAFT" => ("boat", "Life raft"),
        "STRUCTURE_RAFT" => ("boat", "Raft"),
        "GYRO_STRUCTURE" => ("boat", "Gyrocopter"),

        "STRUCTURE" | "STRUCTURE_SMALL" => ("build", "Building"),
        "BED" | "BED_SHELTER" => ("build", "Bed"),
        "WATER_STILL" => ("build", "Water still"),
        "FUEL_STILL" => ("build", "Fuel still"),
        "SMOKER" => ("build", "Smoker"),
        _ => return None,
    };
    Some(kind)
}

/// Things of a kind this close together are one pin: a sea fort is several pieces, and a
/// field of potatoes is one place to go.
pub fn cluster_meters(category: &str) -> f32 {
    match category {
        "wreck" | "shelter" => 40.0,
        "boat" | "build" => 6.0,
        _ => 15.0,
    }
}

/// Whether a prefab spawns land animals (crabs, boars, snakes and so on).
pub fn is_spawner(name: &str) -> bool {
    let name = base_name(name);
    name.ends_with("_SPAWNER") || name.starts_with("HIDESPOT_")
}

/// A prefab's name without what Unity adds to instances.
pub fn base_name(name: &str) -> &str {
    name.trim_end_matches("(Clone)").trim()
}

/// What a creature is called, from its prefab name.
pub fn creature_name(name: &str) -> String {
    let name = base_name(name);
    let upper = name.to_ascii_uppercase();
    let known = [
        ("TIGER", "Tiger shark"),
        ("HAMMERHEAD", "Hammerhead shark"),
        ("GOBLIN", "Goblin shark"),
        ("GREAT WHITE", "Great white shark"),
        ("GREAT_WHITE", "Great white shark"),
        ("SHARK_WHALE", "Whale shark"),
        ("REEF", "Reef shark"),
        ("WHALE", "Whale"),
        ("MARLIN", "Marlin"),
        ("SHARK", "Shark"),
        ("GIANT_CRAB", "Giant crab"),
        ("CRAB", "Crab"),
        ("BOAR", "Boar"),
        ("HOG", "Wild hog"),
        ("SNAKE", "Snake"),
    ];
    if let Some((_, pretty)) = known.iter().find(|(key, _)| upper.contains(key)) {
        return (*pretty).to_owned();
    }
    let words = name.replace('_', " ").to_lowercase();
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_what_the_save_files_hold() {
        assert_eq!(classify("SHIPWRECK_3A(Clone)"), Some(("wreck", "Shipwreck")));
        assert_eq!(classify("CONTAINER_CRATE(Clone)"), Some(("container", "Crate")));
        assert_eq!(classify("CONTAINER_CRATE_ENDGAME_2"), Some(("container", "Supply crate")));
        assert_eq!(classify("MINING_ROCK_CLAY(Clone)"), Some(("mining", "Clay")));
        assert_eq!(classify("MINING_ROCK(Clone)"), Some(("mining", "Stone")));
        assert_eq!(classify("Shelter04(Clone)"), Some(("shelter", "Shelter")));
        assert_eq!(classify("HIDESPOT_SNAKE(Clone)"), Some(("den", "Snake")));
        assert_eq!(classify("STRUCTURE_SMALL(Clone)"), Some(("build", "Building")));
        assert_eq!(classify("CLOTH(Clone)"), Some(("salvage", "Cloth")));
        assert_eq!(classify("VEHICLE_LIFERAFT(Clone)"), Some(("boat", "Life raft")));
    }

    #[test]
    fn leaves_out_scenery_and_loose_items() {
        for name in ["PALM_1(Clone)", "SHORELINE_ROCK_2(Clone)", "STICK(Clone)", "ROCK(Clone)", "COCONUT_ORANGE(Clone)", "SHIPPING_CONTAINER_PANEL_STATIC(Clone)", "YUCCA_CUTTING(Clone)", "CONTAINER_SHELF(Clone)", "CRAB_SPAWNER(Clone)", "Shelter", "SHIPPING_CONTAINER_DOOR"] {
            assert_eq!(classify(name), None, "{name}");
        }
    }

    #[test]
    fn names_creatures() {
        assert_eq!(creature_name("PATROL_TIGERSHARK(Clone)"), "Tiger shark");
        assert_eq!(creature_name("SHARK_GREAT WHITE"), "Great white shark");
        assert_eq!(creature_name("SHARK_WHALE(Clone)"), "Whale shark");
        assert_eq!(creature_name("GIANT_CRAB(Clone)"), "Giant crab");
        assert_eq!(creature_name("CRAB(Clone)"), "Crab");
        assert_eq!(creature_name("SEA_TURTLE"), "Sea turtle");
    }
}
