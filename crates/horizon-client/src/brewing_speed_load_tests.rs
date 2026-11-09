//! A saved `speed_multiplier` is read by `ValueInput.getFloatOr`, which takes
//! any numeric tag, not only a float.
use horizon_client_lib::{containers::BlockEntity, save::decode_block_entity};
use horizon_world::nbt::Tag;

fn brewing(speed: Tag) -> f32 {
    let tag = Tag::compound([
        ("id", Tag::String("minecraft:brewing_stand".into())),
        ("x", Tag::Int(0)),
        ("y", Tag::Int(64)),
        ("z", Tag::Int(0)),
        ("speed_multiplier", speed),
    ]);
    let (_, model) = decode_block_entity(&tag).unwrap();
    let BlockEntity::Brewing(stand) = model else {
        panic!("a brewing stand");
    };
    stand.speed
}

#[test]
fn an_integer_speed_multiplier_loads() {
    assert_eq!(brewing(Tag::Int(2)), 2.0);
    assert_eq!(brewing(Tag::Double(1.5f64.to_bits())), 1.5);
}
