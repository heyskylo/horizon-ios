//! The attribute instances a mob's own construction and load leave in its
//! `AttributeMap`, which `AttributeMap.pack` then saves (`LivingEntity.addAdditionalSaveData`).
//!
//! The expected lists are vanilla 26.3's: the saved `attributes` of
//! `/summon minecraft:<id> ~ ~ ~ {<extra>}` read back before any tick
//! (`python tools/test_command_exec_parity.py --mob-attribute-save-fixture`, Java side).
use super::*;
use crate::entity_nbt::{mob_from_nbt, mob_to_nbt};
use crate::mobs::MobType;
use horizon_world::nbt::{Tag, parse_snbt};
use std::collections::HashMap;
use std::sync::Arc;

/// A mob type of `species` whose attribute defaults are `defaults` (`createAttributes`).
fn types(id: &str, species: Species, defaults: &[(&str, f64)]) -> HashMap<String, Arc<MobType>> {
    let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
    kind.id = id.into();
    kind.species = species;
    kind.attributes = Arc::new(defaults.iter().map(|(k, v)| ((*k).into(), *v)).collect());
    HashMap::from([(id.to_owned(), Arc::new(kind))])
}

/// The mob `/summon minecraft:<id> ~ ~ ~ {<extra>}` loads, and the tag it loads from.
fn loaded(id: &str, species: Species, defaults: &[(&str, f64)], extra: &str) -> (Mob, Tag) {
    let snbt = format!(r#"{{id:"{id}",Pos:[0.5d,200d,0.5d]{extra}}}"#);
    let input = parse_snbt(&snbt).unwrap().0;
    let m = mob_from_nbt(&input, &types(id, species, defaults), 1, None).unwrap();
    (m, input)
}

/// The saved `attributes`, as (id, base) sorted by id: `AttributeMap` is a hash map, so its list has no fixed order.
/// Saved with the loaded tag and without it, which must agree.
fn saved(m: &Mob, input: &Tag) -> Vec<(String, f64)> {
    let mut out: Option<Vec<(String, f64)>> = None;
    for raw in [None, Some(input)] {
        let tag = mob_to_nbt(m, [1, 2, 3, 4], raw, None, |_| None);
        let Some(Tag::List(list)) = tag.get("attributes") else {
            panic!("no attributes list");
        };
        let mut pairs: Vec<(String, f64)> = list
            .iter()
            .map(|a| {
                let Some(Tag::String(name)) = a.get("id") else {
                    panic!("instance with no id");
                };
                let Some(Tag::Double(base)) = a.get("base") else {
                    panic!("instance with no base");
                };
                (name.to_string().unwrap(), f64::from_bits(*base))
            })
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(first) = &out {
            assert_eq!(first, &pairs, "the loaded tag changes the list");
        }
        out = Some(pairs);
    }
    out.unwrap()
}

fn expect(
    id: &str,
    species: Species,
    defaults: &[(&str, f64)],
    extra: &str,
    expected: &[(&str, f64)],
) {
    let expected: Vec<(String, f64)> = expected.iter().map(|(k, v)| ((*k).into(), *v)).collect();
    let (m, input) = loaded(id, species, defaults, extra);
    assert_eq!(saved(&m, &input), expected, "{id} {extra}");
}

const SPEED: &str = "minecraft:movement_speed";
const HEALTH: &str = "minecraft:max_health";
const DAMAGE: &str = "minecraft:attack_damage";

#[test]
fn an_enderman_load_makes_its_speed_instance() {
    // Enderman.setTarget reads movement_speed, and the load ends with it
    // (readPersistentAngerSaveData), with or without saved anger.
    let d = [(SPEED, f64::from(0.3f32))];
    for extra in ["", ",anger_end_time:5000L"] {
        expect(
            "minecraft:enderman",
            Species::Enderman,
            &d,
            extra,
            &[(SPEED, 0.300_000_011_920_928_96)],
        );
    }
}

#[test]
fn a_wolf_loaded_without_an_owner_makes_its_health_instance() {
    // TamableAnimal.readAdditionalSaveData: setTame(false, true) ->
    // Wolf.applyTamingSideEffects sets 8.
    let d = [(HEALTH, 8.0)];
    expect("minecraft:wolf", Species::Wolf, &d, "", &[(HEALTH, 8.0)]);
}

#[test]
fn a_wolf_tamed_in_game_saves_its_forty_health() {
    // Wolf.applyTamingSideEffects (setTame(true, true)) sets the base to 40.
    let (mut m, input) = loaded("minecraft:wolf", Species::Wolf, &[(HEALTH, 8.0)], "");
    crate::mobs::tame::set_tame(&mut m, true);
    assert_eq!(saved(&m, &input), [(HEALTH.to_owned(), 40.0)]);
    // And back: 8 again.
    crate::mobs::tame::set_tame(&mut m, false);
    assert_eq!(saved(&m, &input), [(HEALTH.to_owned(), 8.0)]);
}

#[test]
fn a_baby_goat_or_hoglin_makes_its_attack_instance() {
    // AgeableMob.setAge crosses the baby boundary: Goat/Hoglin.ageBoundaryReached.
    let goat = [(DAMAGE, 2.0)];
    expect("minecraft:goat", Species::Goat, &goat, "", &[]);
    expect("minecraft:goat", Species::Goat, &goat, ",Age:0", &[]);
    expect(
        "minecraft:goat",
        Species::Goat,
        &goat,
        ",Age:-24000",
        &[(DAMAGE, 1.0)],
    );
    let hoglin = [(DAMAGE, 6.0)];
    expect("minecraft:hoglin", Species::Hoglin, &hoglin, "", &[]);
    expect(
        "minecraft:hoglin",
        Species::Hoglin,
        &hoglin,
        ",Age:-24000",
        &[(DAMAGE, 0.5)],
    );
    expect(
        "minecraft:zoglin",
        Species::Zoglin,
        &[(DAMAGE, 6.0)],
        ",IsBaby:1b",
        &[(DAMAGE, 0.5)],
    );
}

#[test]
fn the_other_species_stay_as_they_were() {
    expect("minecraft:pig", Species::Pig, &[(SPEED, 0.25)], "", &[]);
    expect(
        "minecraft:cat",
        Species::Cat,
        &[],
        ",Owner:[I;1,2,3,4]",
        &[],
    );
    expect("minecraft:shulker", Species::Shulker, &[], ",Peek:30", &[]);
    expect(
        "minecraft:zombie",
        Species::Zombie,
        &[(SPEED, f64::from(0.23f32))],
        "",
        &[(SPEED, 0.230_000_004_172_325_13)],
    );
}

#[test]
fn a_wolf_with_a_health_modifier_keeps_its_taming_base() {
    // Wolf.applyTamingSideEffects owns the base (8 wild, 40 tame); a max_health
    // modifier changes the value, never the saved base, save after save.
    let d = [(HEALTH, 8.0)];
    for (extra, base) in [("", 8.0), (",Owner:[I;1,2,3,4]", 40.0)] {
        let (mut m, mut tag) = loaded("minecraft:wolf", Species::Wolf, &d, extra);
        add_modifier(&mut m, Attr::MaxHealth, "test:boost", 10.0, Op::AddValue);
        for _ in 0..2 {
            assert_eq!(saved(&m, &tag), [(HEALTH.to_owned(), base)], "{extra}");
            tag = mob_to_nbt(&m, [1, 2, 3, 4], Some(&tag), None, |_| None);
            m = mob_from_nbt(&tag, &types("minecraft:wolf", Species::Wolf, &d), 1, None).unwrap();
        }
    }
}
