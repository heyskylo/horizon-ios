//! The biome parameters and constants of custom uniform expressions (spec 3.4): `BIOME_<NAME>`,
//! `CAT_<NAME>` and `PPT_<NAME>`.
//!
//! Decisions (impl-plan.md, OQ-11):
//! - `biome` is not the biome's registry id (the registry lists biomes sorted by identifier):
//!   Iris 1.11.7 numbers vanilla's biomes in the order vanilla's `Biomes` class declares them
//!   ([`VANILLA_ORDER`]; forest 8, ocean 46, soul_sand_valley 60, the_end 62, read back with a
//!   probe pack). Other biomes follow, in registry order (not measured). `BIOME_<NAME>` is the
//!   number of `minecraft:<name>`. Names from before the 1.18 renames map to the biome that
//!   replaced them. A `BIOME_` name that matches no biome reads -1, so it never equals the
//!   player's biome.
//! - `biome_category`: categories left the game in 1.19. For vanilla's biomes it gives what
//!   Iris 1.11.7 read back with our probe pack (#10453), a fit to vanilla's biome tags, see
//!   [`category`]; for others, words of the biome's name.

use std::collections::HashMap;

/// `CAT_*`, in the order of their values. Iris 1.11.7 read `CAT_MOUNTAIN` 17 and
/// `CAT_UNDERGROUND` 18 in our probe's custom uniforms (#10453).
pub const CATEGORIES: [&str; 19] = [
    "NONE",
    "TAIGA",
    "EXTREME_HILLS",
    "JUNGLE",
    "MESA",
    "PLAINS",
    "SAVANNA",
    "ICY",
    "THE_END",
    "BEACH",
    "FOREST",
    "OCEAN",
    "DESERT",
    "RIVER",
    "SWAMP",
    "MUSHROOM",
    "NETHER",
    "MOUNTAIN",
    "UNDERGROUND",
];

/// Old biome names and the biomes that replaced them.
const LEGACY: &[(&str, &str)] = &[
    ("EXTREME_HILLS", "windswept_hills"),
    ("MOUNTAINS", "windswept_hills"),
    ("GRAVELLY_MOUNTAINS", "windswept_gravelly_hills"),
    ("WOODED_MOUNTAINS", "windswept_forest"),
    ("SHATTERED_SAVANNA", "windswept_savanna"),
    ("GIANT_TREE_TAIGA", "old_growth_pine_taiga"),
    ("GIANT_SPRUCE_TAIGA", "old_growth_spruce_taiga"),
    ("TALL_BIRCH_FOREST", "old_growth_birch_forest"),
    ("SNOWY_TUNDRA", "snowy_plains"),
    ("JUNGLE_EDGE", "sparse_jungle"),
    ("WOODED_BADLANDS_PLATEAU", "wooded_badlands"),
    ("STONE_SHORE", "stony_shore"),
    ("NETHER", "nether_wastes"),
];

/// Vanilla 26.3's biomes in the order `net.minecraft.world.level.biome.Biomes` declares them:
/// a pack's number for each.
pub const VANILLA_ORDER: [&str; 67] = [
    "the_void",
    "plains",
    "sunflower_plains",
    "snowy_plains",
    "ice_spikes",
    "desert",
    "swamp",
    "mangrove_swamp",
    "forest",
    "flower_forest",
    "birch_forest",
    "dappled_forest",
    "dark_forest",
    "pale_garden",
    "old_growth_birch_forest",
    "old_growth_pine_taiga",
    "old_growth_spruce_taiga",
    "taiga",
    "snowy_taiga",
    "savanna",
    "savanna_plateau",
    "windswept_hills",
    "windswept_gravelly_hills",
    "windswept_forest",
    "windswept_savanna",
    "jungle",
    "sparse_jungle",
    "bamboo_jungle",
    "badlands",
    "eroded_badlands",
    "wooded_badlands",
    "meadow",
    "cherry_grove",
    "grove",
    "snowy_slopes",
    "frozen_peaks",
    "jagged_peaks",
    "stony_peaks",
    "river",
    "frozen_river",
    "beach",
    "snowy_beach",
    "stony_shore",
    "warm_ocean",
    "lukewarm_ocean",
    "deep_lukewarm_ocean",
    "ocean",
    "deep_ocean",
    "cold_ocean",
    "deep_cold_ocean",
    "frozen_ocean",
    "deep_frozen_ocean",
    "mushroom_fields",
    "dripstone_caves",
    "lush_caves",
    "deep_dark",
    "sulfur_caves",
    "nether_wastes",
    "warped_forest",
    "crimson_forest",
    "soul_sand_valley",
    "basalt_deltas",
    "the_end",
    "end_highlands",
    "end_midlands",
    "small_end_islands",
    "end_barrens",
];

/// The number a pack reads for the biome `name` (`biome` and `BIOME_*`), given the registry's
/// identifiers in id order: its place in [`VANILLA_ORDER`], or for a biome vanilla doesn't
/// have, the count of vanilla's plus its place among the others in the registry.
pub fn pack_id<'a>(name: &str, biomes: impl IntoIterator<Item = &'a str>) -> i32 {
    let vanilla = |b: &str| {
        b.strip_prefix("minecraft:")
            .and_then(|n| VANILLA_ORDER.iter().position(|v| *v == n))
    };
    if let Some(i) = vanilla(name) {
        return i as i32;
    }
    biomes
        .into_iter()
        .filter(|b| vanilla(b).is_none())
        .position(|b| b == name)
        .map_or(-1, |i| (VANILLA_ORDER.len() + i) as i32)
}

/// The constants every pack sees: `CAT_*` and `PPT_*`, plus `BIOME_*` for `biomes` (the
/// registry's identifiers, in id order), numbered by [`pack_id`].
pub fn constants(biomes: &[&str]) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = CATEGORIES
        .iter()
        .enumerate()
        .map(|(i, c)| (format!("CAT_{c}"), i as f64))
        .collect();
    for (i, p) in ["NONE", "RAIN", "SNOW"].iter().enumerate() {
        out.insert(format!("PPT_{p}"), i as f64);
    }
    let index = |name: &str| {
        biomes
            .iter()
            .find(|b| b.strip_prefix("minecraft:") == Some(name))
            .map(|b| pack_id(b, biomes.iter().copied()))
    };
    for b in biomes {
        if let Some(name) = b.strip_prefix("minecraft:") {
            let id = pack_id(b, biomes.iter().copied());
            out.insert(
                format!("BIOME_{}", name.to_ascii_uppercase()),
                f64::from(id),
            );
        }
    }
    for (old, new) in LEGACY {
        if let Some(i) = index(new) {
            out.entry(format!("BIOME_{old}")).or_insert(f64::from(i));
        }
    }
    out
}

/// Vanilla 26.3's biome tags (`BiomeTags`, `data/minecraft/tags/worldgen/biome/is_*.json`) that
/// give a vanilla biome its category, with the category; no biome is in two of them.
const TAGGED: [(&str, &[&str]); 11] = [
    (
        "OCEAN",
        &[
            "deep_frozen_ocean",
            "deep_cold_ocean",
            "deep_ocean",
            "deep_lukewarm_ocean",
            "frozen_ocean",
            "ocean",
            "cold_ocean",
            "lukewarm_ocean",
            "warm_ocean",
        ],
    ),
    ("RIVER", &["river", "frozen_river"]),
    ("BEACH", &["beach", "snowy_beach"]),
    ("JUNGLE", &["bamboo_jungle", "jungle", "sparse_jungle"]),
    ("MESA", &["badlands", "eroded_badlands", "wooded_badlands"]),
    (
        "TAIGA",
        &[
            "taiga",
            "snowy_taiga",
            "old_growth_pine_taiga",
            "old_growth_spruce_taiga",
        ],
    ),
    (
        "FOREST",
        &[
            "forest",
            "flower_forest",
            "birch_forest",
            "old_growth_birch_forest",
            "dappled_forest",
            "dark_forest",
            "pale_garden",
            "grove",
        ],
    ),
    (
        "EXTREME_HILLS",
        &[
            "windswept_hills",
            "windswept_forest",
            "windswept_gravelly_hills",
        ],
    ),
    (
        "MOUNTAIN",
        &[
            "meadow",
            "frozen_peaks",
            "jagged_peaks",
            "stony_peaks",
            "snowy_slopes",
            "cherry_grove",
        ],
    ),
    (
        "NETHER",
        &[
            "nether_wastes",
            "soul_sand_valley",
            "crimson_forest",
            "warped_forest",
            "basalt_deltas",
        ],
    ),
    (
        "THE_END",
        &[
            "the_end",
            "end_highlands",
            "end_midlands",
            "small_end_islands",
            "end_barrens",
        ],
    ),
];

/// `biome_category` of the biome `minecraft:<name>`.
///
/// Vanilla's biomes read what Iris 1.11.7 gave each of them with our probe pack (#10453): the
/// category of its tag in [`TAGGED`] (`is_savanna` gives none: savannas read `PLAINS`); else
/// `DESERT`, `SWAMP`, `MUSHROOM` and `ICY` for desert, swamp and mangrove swamp, mushroom
/// fields and snowy plains, `NONE` for the void, and `PLAINS` for every other one (the cave
/// biomes, ice spikes, stony shore among them). That rule is a fit of the 67 values, not a
/// rule Iris states. Other biomes go by words of their name (Horizon's choice; not measured).
pub fn category(name: &str) -> i32 {
    let index = |cat: &str| CATEGORIES.iter().position(|c| *c == cat).unwrap_or(0) as i32;
    let Some(vanilla) = name
        .strip_prefix("minecraft:")
        .filter(|n| VANILLA_ORDER.contains(n))
    else {
        return index(category_by_words(name));
    };
    if let Some((cat, _)) = TAGGED.iter().find(|(_, biomes)| biomes.contains(&vanilla)) {
        return index(cat);
    }
    index(match vanilla {
        "the_void" => "NONE",
        "desert" => "DESERT",
        "swamp" | "mangrove_swamp" => "SWAMP",
        "mushroom_fields" => "MUSHROOM",
        "snowy_plains" => "ICY",
        _ => "PLAINS",
    })
}

/// The category of a biome vanilla doesn't have, from words of its name.
fn category_by_words(name: &str) -> &'static str {
    let name = name.split_once(':').map_or(name, |(_, n)| n);
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if name == "the_void" {
        "NONE"
    } else if name == "the_end" || name.starts_with("end_") || name == "small_end_islands" {
        "THE_END"
    } else if has(&["nether", "crimson", "warped", "basalt", "soul_sand"]) {
        "NETHER"
    } else if has(&["ocean"]) {
        "OCEAN"
    } else if has(&["river"]) {
        "RIVER"
    } else if has(&["beach", "shore"]) {
        "BEACH"
    } else if has(&["swamp"]) {
        "SWAMP"
    } else if has(&["mushroom"]) {
        "MUSHROOM"
    } else if has(&["desert"]) {
        "DESERT"
    } else if has(&["badlands"]) {
        "MESA"
    } else if has(&["savanna"]) {
        "SAVANNA"
    } else if has(&["jungle"]) {
        "JUNGLE"
    } else if has(&["taiga"]) {
        "TAIGA"
    } else if has(&["windswept", "peaks", "slopes", "meadow", "grove"]) && name != "cherry_grove" {
        "EXTREME_HILLS"
    } else if has(&["frozen", "snowy", "ice"]) {
        "ICY"
    } else if has(&["forest", "garden", "cherry"]) {
        "FOREST"
    } else if has(&["plains"]) {
        "PLAINS"
    } else {
        "NONE"
    }
}

/// `biome_precipitation`: none where the biome has none, else rain from temperature 0.15 up,
/// snow below it (spec 3.4).
pub fn precipitation(has_precipitation: bool, temperature: f32) -> i32 {
    match (has_precipitation, temperature >= 0.15) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_categories_and_precipitation() {
        let c = constants(&[
            "minecraft:badlands",
            "minecraft:pale_garden",
            "minecraft:windswept_hills",
        ]);
        assert_eq!(c["BIOME_PALE_GARDEN"], 13.0);
        assert_eq!(c["BIOME_EXTREME_HILLS"], 21.0);
        assert_eq!(c["CAT_NETHER"], 16.0);
        assert_eq!(c["PPT_SNOW"], 2.0);
        assert!(!c.contains_key("BIOME_SULFUR_CAVES"));
        assert_eq!(category("minecraft:pale_garden"), 10);
        assert_eq!(category("minecraft:deep_frozen_ocean"), 11);
        assert_eq!(category("minecraft:snowy_taiga"), 1);
        assert_eq!(category("minecraft:crimson_forest"), 16);
        assert_eq!(category("minecraft:cherry_grove"), 17);
        assert_eq!(precipitation(true, 0.0), 2);
        assert_eq!(precipitation(true, 0.8), 1);
        assert_eq!(precipitation(false, 2.0), 0);
    }

    /// Iris 1.11.7's `biome_category` in each of vanilla's biomes, read back with a probe pack
    /// of ours after `fillbiome` around the camera (#10453), and its `CAT_MOUNTAIN` and
    /// `CAT_UNDERGROUND` (17 and 18) in a custom uniform.
    #[test]
    fn categories_follow_iris() {
        let iris: [(&str, i32); 67] = [
            ("the_void", 0),
            ("plains", 5),
            ("sunflower_plains", 5),
            ("snowy_plains", 7),
            ("ice_spikes", 5),
            ("desert", 12),
            ("swamp", 14),
            ("mangrove_swamp", 14),
            ("forest", 10),
            ("flower_forest", 10),
            ("birch_forest", 10),
            ("dappled_forest", 10),
            ("dark_forest", 10),
            ("pale_garden", 10),
            ("old_growth_birch_forest", 10),
            ("old_growth_pine_taiga", 1),
            ("old_growth_spruce_taiga", 1),
            ("taiga", 1),
            ("snowy_taiga", 1),
            ("savanna", 5),
            ("savanna_plateau", 5),
            ("windswept_hills", 2),
            ("windswept_gravelly_hills", 2),
            ("windswept_forest", 2),
            ("windswept_savanna", 5),
            ("jungle", 3),
            ("sparse_jungle", 3),
            ("bamboo_jungle", 3),
            ("badlands", 4),
            ("eroded_badlands", 4),
            ("wooded_badlands", 4),
            ("meadow", 17),
            ("cherry_grove", 17),
            ("grove", 10),
            ("snowy_slopes", 17),
            ("frozen_peaks", 17),
            ("jagged_peaks", 17),
            ("stony_peaks", 17),
            ("river", 13),
            ("frozen_river", 13),
            ("beach", 9),
            ("snowy_beach", 9),
            ("stony_shore", 5),
            ("warm_ocean", 11),
            ("lukewarm_ocean", 11),
            ("deep_lukewarm_ocean", 11),
            ("ocean", 11),
            ("deep_ocean", 11),
            ("cold_ocean", 11),
            ("deep_cold_ocean", 11),
            ("frozen_ocean", 11),
            ("deep_frozen_ocean", 11),
            ("mushroom_fields", 15),
            ("dripstone_caves", 5),
            ("lush_caves", 5),
            ("deep_dark", 5),
            ("sulfur_caves", 5),
            ("nether_wastes", 16),
            ("warped_forest", 16),
            ("crimson_forest", 16),
            ("soul_sand_valley", 16),
            ("basalt_deltas", 16),
            ("the_end", 8),
            ("end_highlands", 8),
            ("end_midlands", 8),
            ("small_end_islands", 8),
            ("end_barrens", 8),
        ];
        for (biome, cat) in iris {
            assert_eq!(category(&format!("minecraft:{biome}")), cat, "{biome}");
        }
        let c = constants(&[]);
        assert_eq!((c["CAT_MOUNTAIN"], c["CAT_UNDERGROUND"]), (17.0, 18.0));
    }

    /// Iris 1.11.7's `BIOME_*` constants, read back with a probe pack: each of the 67 is the
    /// biome's place in vanilla's `Biomes` (here with the registry in identifier order).
    #[test]
    fn biome_constants_follow_iris() {
        let c = constants(&[
            "minecraft:badlands",
            "minecraft:forest",
            "minecraft:ocean",
            "minecraft:soul_sand_valley",
            "minecraft:the_end",
            "minecraft:the_void",
        ]);
        assert_eq!(c["BIOME_FOREST"], 8.0);
        assert_eq!(c["BIOME_OCEAN"], 46.0);
        assert_eq!(c["BIOME_SOUL_SAND_VALLEY"], 60.0);
        assert_eq!(c["BIOME_THE_END"], 62.0);
        assert_eq!(c["BIOME_BADLANDS"], 28.0);
        assert_eq!(c["BIOME_THE_VOID"], 0.0);
    }

    /// Iris 1.11.7's `biome`, read back with a probe pack where the scene world is forest,
    /// ocean, soul sand valley and the End: 8, 46, 60 and 62. A biome vanilla doesn't have
    /// comes after vanilla's (Horizon's choice; not measured in Iris).
    #[test]
    fn biomes_are_numbered_as_iris_numbers_them() {
        let registry = [
            "minecraft:forest",
            "minecraft:ocean",
            "mod:glade",
            "minecraft:soul_sand_valley",
            "minecraft:the_end",
            "mod:marsh",
        ];
        let ids: Vec<i32> = registry.iter().map(|b| pack_id(b, registry)).collect();
        assert_eq!(ids, [8, 46, 67, 60, 62, 68]);
        assert_eq!(pack_id("minecraft:unknown", registry), -1);
        assert_eq!(constants(&registry)["BIOME_SOUL_SAND_VALLEY"], 60.0);
    }
}
