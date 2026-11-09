use super::*;

/// `ItemStackTemplate.CODEC` reads a stack's fields or a bare item id.
#[test]
fn a_result_is_an_object_with_an_id_or_a_bare_item_id() {
    let id = |json: &str| {
        let v: Value = serde_json::from_str(json).unwrap();
        recipe_result_id(&v).map(str::to_owned)
    };
    assert_eq!(
        id(r#"{"id": "ruby:ruby", "count": 3}"#).as_deref(),
        Some("ruby:ruby")
    );
    assert_eq!(id(r#""ruby:ruby""#).as_deref(), Some("ruby:ruby"));
    // No `id` (a transmuting recipe keeps its input's item), the `item` key of older
    // versions and anything that is neither an object nor a string name no item.
    assert_eq!(id(r#"{"count": 2}"#), None);
    assert_eq!(id(r#"{"item": "minecraft:stick"}"#), None);
    assert_eq!(id(r#"{"id": 7}"#), None);
    assert_eq!(id("null"), None);
    assert_eq!(id("[]"), None);
}
