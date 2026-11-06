//! What each of Valheim's location prefabs is on the map. Names come from the game's
//! `StreamingAssets/SoftRef/manifest`; `docs/games/valheim.md` says how to list them.

/// Categories, matching `games/valheim/game.json`.
const BOSS: &str = "boss";
const TRADER: &str = "trader";
const DUNGEON: &str = "dungeon";
const RUNESTONE: &str = "runestone";
const CAMP: &str = "camp";
const TAR: &str = "tar";
const DRAGON_EGG: &str = "dragon-egg";
const GIANT: &str = "giant";
const EXCAVATION: &str = "excavation";
const ASHLANDS_SITE: &str = "ashlands-site";
const LANDMARK: &str = "landmark";

/// Exact names of what matters most.
const NAMED: &[(&str, &str, &str)] = &[
    ("Eikthyrnir", BOSS, "Eikthyr"),
    ("GDKing", BOSS, "The Elder"),
    ("Bonemass", BOSS, "Bonemass"),
    ("Dragonqueen", BOSS, "Moder"),
    ("GoblinKing", BOSS, "Yagluth"),
    ("Mistlands_DvergrBossEntrance1", BOSS, "The Queen"),
    ("FaderLocation", BOSS, "Fader"),
    ("DN_Bossroom", BOSS, "Deep North boss"),
    ("StartTemple", LANDMARK, "Sacrificial Stones"),
    ("Vendor_BlackForest", TRADER, "Haldor"),
    ("Hildir_camp", TRADER, "Hildir"),
    ("BogWitch_Camp", TRADER, "Bog Witch"),
    ("SunkenCrypt4", DUNGEON, "Sunken Crypt"),
    ("TrollCave02", DUNGEON, "Troll Cave"),
    ("MountainCave02", DUNGEON, "Frost Cave"),
    ("Hildir_crypt", DUNGEON, "Smouldering Tomb"),
    ("Hildir_cave", DUNGEON, "Howling Cavern"),
    ("Hildir_plainsfortress", DUNGEON, "Sealed Tower"),
    ("CharredFortress", DUNGEON, "Charred Fortress"),
    ("FortressRuins", DUNGEON, "Fortress Ruins"),
    ("DrakeLorestone", RUNESTONE, "Drake Lorestone"),
    ("Greydwarf_camp1", CAMP, "Greydwarf Camp"),
    ("InfestedTree01", CAMP, "Infested Tree"),
    ("DrakeNest01", DRAGON_EGG, "Dragon Egg"),
    ("FireHole", ASHLANDS_SITE, "Fire Hole"),
    ("SulfurArch", ASHLANDS_SITE, "Sulfur Arch"),
    ("LeviathanLava", ASHLANDS_SITE, "Lava Leviathan"),
];

/// Families of prefabs, by the start of their name.
const FAMILIES: &[(&str, &str, &str)] = &[
    ("Crypt", DUNGEON, "Burial Chambers"),
    ("Mistlands_DvergrTownEntrance", DUNGEON, "Infested Mine"),
    ("MorgenHole", DUNGEON, "Morgen Hole"),
    ("GoblinCamp", CAMP, "Fuling Village"),
    ("CharredStone_Spawner", CAMP, "Charred Spawner"),
    ("TarPit", TAR, "Tar Pit"),
    ("Mistlands_Giant", GIANT, "Giant Remains"),
    ("Mistlands_Excavation", EXCAVATION, "Dvergr Excavation"),
];

/// Decoration too small and too common to be worth a pin.
const IGNORED: &[&str] = &["Dev", "BigRockClearing", "Grave1", "Mistlands_RoadPost", "Mistlands_Statue", "Mistlands_Swords", "Mistlands_Viaduct"];

/// The category and name for a location prefab, or `None` if it shouldn't be pinned.
pub fn classify(prefab: &str) -> Option<(&'static str, String)> {
    if let Some(&(_, category, name)) = NAMED.iter().find(|(p, ..)| *p == prefab) {
        return Some((category, name.into()));
    }
    if let Some(&(_, category, name)) = FAMILIES.iter().find(|(p, ..)| prefab.starts_with(p)) {
        return Some((category, name.into()));
    }
    if IGNORED.iter().any(|p| prefab.starts_with(p)) {
        return None;
    }
    if let Some(place) = prefab.strip_prefix("Runestone_") {
        return Some((RUNESTONE, format!("Runestone ({})", spaced(place))));
    }
    Some((LANDMARK, readable(prefab)))
}

/// How a location is named: a trader's spot is only a possibility until the game places it,
/// when it drops all the others.
pub fn pin_name(category: &str, name: String, placed: bool) -> String {
    if category == TRADER && !placed {
        format!("{name} (possible spot)")
    } else {
        name
    }
}

/// `BlackForest` -> `Black Forest`.
fn spaced(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    let mut previous_lower = false;
    for c in name.chars() {
        if c.is_ascii_uppercase() && previous_lower {
            out.push(' ');
        }
        previous_lower = c.is_ascii_lowercase();
        out.push(c);
    }
    out
}

/// A prefab's name without its numbering and variants: `StoneTowerRuins07_sunk` ->
/// `Stone Tower Ruins`.
fn readable(prefab: &str) -> String {
    let prefab = prefab.strip_prefix("Mistlands_").unwrap_or(prefab);
    let base = prefab.split('_').next().unwrap_or(prefab);
    spaced(base.trim_end_matches(|c: char| c.is_ascii_digit()))
}

/// `text` without Unity's rich text tags: `<color=orange>Ulf</color>` -> `Ulf`.
pub fn without_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
}

/// Text of a weather name: `Twilight_Clear` -> `Twilight clear`.
pub fn weather_name(env: &str) -> String {
    let text = spaced(env).replace('_', " ");
    let mut chars = text.chars();
    let first = chars.next().map(|c| c.to_ascii_uppercase()).into_iter();
    first.chain(chars.flat_map(char::to_lowercase)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_what_players_look_for() {
        assert_eq!(classify("Eikthyrnir"), Some((BOSS, "Eikthyr".to_string())));
        assert_eq!(classify("Vendor_BlackForest"), Some((TRADER, "Haldor".to_string())));
        assert_eq!(classify("Crypt3"), Some((DUNGEON, "Burial Chambers".to_string())));
        assert_eq!(classify("GoblinCamp2_1"), Some((CAMP, "Fuling Village".to_string())));
        assert_eq!(classify("Runestone_BlackForest"), Some((RUNESTONE, "Runestone (Black Forest)".to_string())));
    }

    #[test]
    fn traders_are_only_possible_until_placed() {
        assert_eq!(pin_name(TRADER, "Haldor".into(), false), "Haldor (possible spot)");
        assert_eq!(pin_name(TRADER, "Haldor".into(), true), "Haldor");
        assert_eq!(pin_name(BOSS, "Eikthyr".into(), false), "Eikthyr");
    }

    #[test]
    fn exact_names_win_over_families() {
        assert_eq!(classify("Hildir_crypt"), Some((DUNGEON, "Smouldering Tomb".to_string())));
        assert_eq!(classify("SunkenCrypt4"), Some((DUNGEON, "Sunken Crypt".to_string())));
    }

    #[test]
    fn anything_else_is_a_landmark_with_a_readable_name() {
        assert_eq!(classify("StoneTowerRuins07_sunk"), Some((LANDMARK, "Stone Tower Ruins".to_string())));
        assert_eq!(classify("Mistlands_Harbour1"), Some((LANDMARK, "Harbour".to_string())));
        assert_eq!(classify("SomethingNew3"), Some((LANDMARK, "Something New".to_string())));
    }

    #[test]
    fn leaves_out_decoration() {
        assert_eq!(classify("DevHouse1"), None);
        assert_eq!(classify("Mistlands_Statue1"), None);
    }

    #[test]
    fn strips_rich_text_from_names() {
        assert_eq!(without_markup("<color=orange>Brenna</color>"), "Brenna");
        assert_eq!(without_markup("Greyling"), "Greyling");
    }

    #[test]
    fn makes_weather_readable() {
        assert_eq!(weather_name("Twilight_Clear"), "Twilight clear");
        assert_eq!(weather_name("ThunderStorm"), "Thunder storm");
        assert_eq!(weather_name("Misty"), "Misty");
    }
}
