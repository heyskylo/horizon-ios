use horizon_world::{nbt::Tag, snbt_text::to_snbt};

#[test]
fn supplementary_keys_keep_java_utf16_order() {
    let entries = [
        ("\u{e000}", Tag::Int(2)),
        ("\u{10000}", Tag::Int(1)),
        ("\u{d7ff}", Tag::Int(0)),
    ];
    let expected = "{\"\u{d7ff}\":0,\"\u{10000}\":1,\"\u{e000}\":2}";
    assert_eq!(to_snbt(&Tag::compound(entries.clone())), expected);
    assert_eq!(to_snbt(&Tag::compound(entries.into_iter().rev())), expected);
}

#[test]
fn nested_compounds_use_the_same_key_order() {
    let inner = Tag::compound([("\u{e000}", Tag::Int(2)), ("\u{10000}", Tag::Int(1))]);
    let outer = Tag::compound([("outer", inner), ("ascii", Tag::Int(3))]);
    assert_eq!(
        to_snbt(&outer),
        "{ascii:3,outer:{\"\u{10000}\":1,\"\u{e000}\":2}}"
    );
}
