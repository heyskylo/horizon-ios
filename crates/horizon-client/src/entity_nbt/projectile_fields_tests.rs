use crate::entity_nbt::{arrow_from_nbt, arrow_to_nbt};
use horizon_world::nbt::{Tag, parse_snbt};

/// `Entity.load` reads `Air` and `saveWithoutId` writes it back.
#[test]
fn arrow_keeps_its_saved_air() {
    let parse = |text: &str| parse_snbt(text).unwrap().0;
    let raw = parse("{id:\"minecraft:arrow\",Pos:[1d,200d,1d],Air:32767s,Fire:7s}");
    let (arrow, _) = arrow_from_nbt(&raw, None, |_| None).unwrap();
    assert_eq!(arrow.air_supply, 32767);
    let saved = arrow_to_nbt(&arrow, [197, 0, 0, 1], None, None, Some(&raw)).unwrap();
    assert_eq!(saved.get("Air"), Some(&Tag::Short(32767)));
    assert_eq!(saved.get("Fire"), Some(&Tag::Short(7)));
    let absent = parse("{id:\"minecraft:arrow\",Pos:[1d,200d,1d]}");
    let (fresh, _) = arrow_from_nbt(&absent, None, |_| None).unwrap();
    assert_eq!(fresh.air_supply, 300);
}
