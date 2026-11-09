use super::super::{LootData, PackOverlay};
use super::LootOdds;

/// Loot data holding these tables (`minecraft:<name>`).
fn data(name: &str, tables: &[(&str, &str)]) -> LootData {
    let root = std::env::temp_dir().join(format!("loot-odds-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/minecraft/loot_table")).unwrap();
    for (id, json) in tables {
        let path = root.join(format!("data/minecraft/loot_table/{id}.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json).unwrap();
    }
    std::fs::write(root.join("items.json"), "{}").unwrap();
    let data = LootData::load(&root, &PackOverlay::NONE).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    data
}

fn find<'a>(odds: &'a [LootOdds], item: &str) -> &'a LootOdds {
    odds.iter()
        .find(|o| o.item == item)
        .unwrap_or_else(|| panic!("{item} not in {odds:?}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Braid's Shallows (void fishing): one roll, weights summing to 100, so
/// each item's chance is its weight in percent.
const SHALLOWS: &str = r#"{"type": "minecraft:fishing", "pools": [{"rolls": 1, "entries": [{"type": "minecraft:item", "name": "void_fishing:pebble", "weight": 20, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 2, "max": 4}}}, {"type": "minecraft:item", "name": "void_fishing:dirt_clump", "weight": 13, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 1, "max": 3}}}, {"type": "minecraft:item", "name": "minecraft:oak_log", "weight": 6, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 1, "max": 2}}}, {"type": "minecraft:item", "name": "minecraft:spruce_log", "weight": 3}, {"type": "minecraft:item", "name": "minecraft:moss_block", "weight": 5}, {"type": "minecraft:item", "name": "void_fishing:sealed_jar", "weight": 7}, {"type": "minecraft:item", "name": "void_fishing:hot_jar", "weight": 2}, {"type": "minecraft:item", "name": "minecraft:bone", "weight": 8, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 1, "max": 3}}}, {"type": "minecraft:item", "name": "minecraft:wheat_seeds", "weight": 4, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 1, "max": 2}}}, {"type": "minecraft:item", "name": "minecraft:oak_sapling", "weight": 3}, {"type": "minecraft:item", "name": "minecraft:spruce_sapling", "weight": 2}, {"type": "minecraft:item", "name": "minecraft:birch_sapling", "weight": 2}, {"type": "minecraft:item", "name": "void_fishing:rusty_scrap", "weight": 9, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 1, "max": 2}}}, {"type": "minecraft:item", "name": "minecraft:copper_nugget", "weight": 10, "modifier": {"type": "minecraft:set_count", "count": {"type": "minecraft:uniform", "min": 2, "max": 4}}}, {"type": "minecraft:item", "name": "minecraft:string", "weight": 3}, {"type": "minecraft:item", "name": "minecraft:cod", "weight": 3}]}]}"#;

#[test]
fn shallows_chances_are_the_weights_and_counts_their_set_count() {
    let odds = data("shallows", &[("shallows", SHALLOWS)])
        .odds("minecraft:shallows", None)
        .unwrap();
    let expected: [(&str, f64, u32, u32); 16] = [
        ("void_fishing:pebble", 0.20, 2, 4),
        ("void_fishing:dirt_clump", 0.13, 1, 3),
        ("minecraft:oak_log", 0.06, 1, 2),
        ("minecraft:spruce_log", 0.03, 1, 1),
        ("minecraft:moss_block", 0.05, 1, 1),
        ("void_fishing:sealed_jar", 0.07, 1, 1),
        ("void_fishing:hot_jar", 0.02, 1, 1),
        ("minecraft:bone", 0.08, 1, 3),
        ("minecraft:wheat_seeds", 0.04, 1, 2),
        ("minecraft:oak_sapling", 0.03, 1, 1),
        ("minecraft:spruce_sapling", 0.02, 1, 1),
        ("minecraft:birch_sapling", 0.02, 1, 1),
        ("void_fishing:rusty_scrap", 0.09, 1, 2),
        ("minecraft:copper_nugget", 0.10, 2, 4),
        ("minecraft:string", 0.03, 1, 1),
        ("minecraft:cod", 0.03, 1, 1),
    ];
    assert_eq!(odds.len(), expected.len());
    for (item, chance, min, max) in expected {
        let o = find(&odds, item);
        assert!(close(o.chance, chance), "{item}: {} != {chance}", o.chance);
        assert_eq!((o.min, o.max), (min, max), "{item}");
        assert!(o.conditions.is_empty(), "{item}: {:?}", o.conditions);
    }
}

/// Braid's gravel: the vanilla pool when not brushed, and one pool per
/// sifted item, each a brush and a random chance.
const GRAVEL: &str = r#"{"type": "minecraft:block", "pools": [{"entries": [{"type": "minecraft:alternatives", "children": [{"type": "minecraft:item", "condition": "minecraft:tool/can_silk_touch", "name": "minecraft:gravel"}, {"type": "minecraft:alternatives", "children": [{"type": "minecraft:item", "condition": {"type": "minecraft:table_bonus", "chances": [0.1, 0.14285715, 0.25, 1.0], "enchantment": "minecraft:fortune"}, "name": "minecraft:flint"}, {"type": "minecraft:item", "name": "minecraft:gravel"}], "condition": {"type": "minecraft:survives_explosion"}}]}], "rolls": 1, "condition": {"type": "minecraft:inverted", "term": {"type": "minecraft:match_tool", "predicate": {"items": "minecraft:brush"}}}}, {"condition": {"type": "minecraft:all_of", "terms": [{"type": "minecraft:match_tool", "predicate": {"items": "minecraft:brush"}}, {"type": "minecraft:random_chance", "chance": 0.25}]}, "entries": [{"type": "minecraft:item", "name": "minecraft:flint"}], "rolls": 1}, {"condition": {"type": "minecraft:all_of", "terms": [{"type": "minecraft:match_tool", "predicate": {"items": "minecraft:brush"}}, {"type": "minecraft:random_chance", "chance": 0.2}]}, "entries": [{"type": "minecraft:item", "name": "minecraft:iron_nugget"}], "rolls": 1}, {"condition": {"type": "minecraft:all_of", "terms": [{"type": "minecraft:match_tool", "predicate": {"items": "minecraft:brush"}}, {"type": "minecraft:random_chance", "chance": 0.2}]}, "entries": [{"type": "minecraft:item", "name": "minecraft:copper_nugget"}], "rolls": 1}, {"condition": {"type": "minecraft:all_of", "terms": [{"type": "minecraft:match_tool", "predicate": {"items": "minecraft:brush"}}, {"type": "minecraft:random_chance", "chance": 0.04}]}, "entries": [{"type": "minecraft:item", "name": "minecraft:pointed_dripstone"}], "rolls": 1}], "random_sequence": "minecraft:blocks/gravel"}"#;

#[test]
fn brushed_gravel_gives_only_the_sifted_items_at_their_chances() {
    let odds = data("gravel", &[("blocks/gravel", GRAVEL)])
        .odds("minecraft:blocks/gravel", Some("minecraft:brush"))
        .unwrap();
    let expected = [
        ("minecraft:flint", 0.25),
        ("minecraft:iron_nugget", 0.20),
        ("minecraft:copper_nugget", 0.20),
        ("minecraft:pointed_dripstone", 0.04),
    ];
    assert_eq!(odds.len(), expected.len(), "{odds:?}");
    for (item, chance) in expected {
        let o = find(&odds, item);
        assert!(close(o.chance, chance), "{item}: {}", o.chance);
        assert_eq!((o.min, o.max), (1, 1));
        assert!(o.conditions.is_empty());
    }
}

#[test]
fn gravel_with_no_tool_named_keeps_both_ways_and_says_which_needs_the_brush() {
    let odds = data("gravel_any", &[("blocks/gravel", GRAVEL)])
        .odds("minecraft:blocks/gravel", None)
        .unwrap();
    // Unbrushed: silk touch's predicate isn't in this data (it fails), so
    // flint 10% (table_bonus with no Fortune), else gravel.
    let gravel = find(&odds, "minecraft:gravel");
    assert!(close(gravel.chance, 0.9), "{}", gravel.chance);
    assert_eq!(gravel.conditions, ["without a brush"]);
    // Flint either way: 1 - 0.9 * 0.75.
    let flint = find(&odds, "minecraft:flint");
    assert!(close(flint.chance, 0.325), "{}", flint.chance);
    assert_eq!(flint.conditions, ["without a brush", "needs a brush"]);
    let iron = find(&odds, "minecraft:iron_nugget");
    assert!(close(iron.chance, 0.2));
    assert_eq!(iron.conditions, ["needs a brush"]);
}

#[test]
fn a_tool_that_does_not_match_leaves_out_what_needs_it() {
    let odds = data("gravel_shovel", &[("blocks/gravel", GRAVEL)])
        .odds("minecraft:blocks/gravel", Some("minecraft:iron_shovel"))
        .unwrap();
    assert!(
        odds.iter()
            .all(|o| o.item == "minecraft:gravel" || o.item == "minecraft:flint")
    );
    assert!(close(find(&odds, "minecraft:flint").chance, 0.1));
}

#[test]
fn an_entry_left_to_chance_shares_the_roll_with_the_sure_one() {
    // A and B weigh the same; B is valid half the time. A: 1/2 + 1/2 * 1/2.
    let odds = data(
        "share",
        &[(
            "t",
            r#"{"pools":[{"rolls":1,"entries":[
                {"type":"minecraft:item","name":"minecraft:apple"},
                {"type":"minecraft:item","name":"minecraft:stick",
                 "condition":{"type":"minecraft:random_chance","chance":0.5}}]}]}"#,
        )],
    )
    .odds("minecraft:t", None)
    .unwrap();
    assert!(close(find(&odds, "minecraft:apple").chance, 0.75));
    assert!(close(find(&odds, "minecraft:stick").chance, 0.25));
}

#[test]
fn rolls_ranges_counts_with_zero_and_nested_tables() {
    let odds = data(
        "rolls",
        &[
            (
                "outer",
                r#"{"pools":[
                    {"rolls":{"type":"minecraft:uniform","min":1,"max":2},"entries":[
                        {"type":"minecraft:item","name":"minecraft:apple",
                         "condition":{"type":"minecraft:random_chance","chance":0.5}}]},
                    {"rolls":1,"entries":[
                        {"type":"minecraft:item","name":"minecraft:stick",
                         "modifier":{"type":"minecraft:set_count","count":{"type":"minecraft:uniform","min":0,"max":2}}}]},
                    {"rolls":1,"entries":[{"type":"minecraft:loot_table","value":"minecraft:inner","weight":1},
                                          {"type":"minecraft:empty","weight":3}]}]}"#,
            ),
            (
                "inner",
                r#"{"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:apple",
                    "modifier":{"type":"minecraft:set_count","count":3}}]}]}"#,
            ),
        ],
    )
    .odds("minecraft:outer", None)
    .unwrap();
    // Apple: one or two tries at 1/2 (5/8), or the nested table (1/4).
    let apple = find(&odds, "minecraft:apple");
    assert!(
        close(apple.chance, 1.0 - (1.0 - 0.625) * 0.75),
        "{}",
        apple.chance
    );
    assert_eq!((apple.min, apple.max), (1, 5));
    // Stick: a count of 0, 1 or 2, so two times in three.
    let stick = find(&odds, "minecraft:stick");
    assert!(close(stick.chance, 2.0 / 3.0), "{}", stick.chance);
    assert_eq!((stick.min, stick.max), (1, 2));
}

#[test]
fn a_missing_table_has_no_odds() {
    assert!(data("missing", &[]).odds("minecraft:nope", None).is_none());
}
