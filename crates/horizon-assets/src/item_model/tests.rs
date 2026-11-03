use super::*;

#[test]
fn component_condition_decodes_predicate_type_and_value() {
    let json = serde_json::json!({
        "property": "minecraft:component", "predicate": "custom_data",
        "value": "{td_attune_mode:1}"
    });
    assert_eq!(
        ConditionProperty::decode(json.as_object().unwrap()).unwrap(),
        ConditionProperty::Component(serde_json::json!({
            "minecraft:custom_data": "{td_attune_mode:1}"
        }))
    );
    let missing_value = serde_json::json!({"property":"component", "predicate":"custom_data"});
    assert!(ConditionProperty::decode(missing_value.as_object().unwrap()).is_err());
}

#[test]
fn identifier_adds_default_namespace() {
    assert_eq!(identifier("stone").unwrap(), "minecraft:stone");
    assert_eq!(identifier("a:b").unwrap(), "a:b");
}

#[test]
fn positive_modulo_wraps_negatives() {
    assert!((positive_modulo(-0.25, 1.0) - 0.75).abs() < 1e-6);
    assert!((positive_modulo(1.5, 1.0) - 0.5).abs() < 1e-6);
}

#[test]
fn display_context_names_round_trip() {
    for name in ["gui", "ground", "fixed", "head"] {
        assert_eq!(DisplayContext::by_name(name).unwrap().name(), name);
    }
}

#[test]
fn a_mods_tint_source_is_untinted_and_an_unknown_vanilla_one_fails() {
    let tint = |json: &str| TintSource::decode(&serde_json::from_str(json).unwrap());
    assert_eq!(
        tint(r#"{"type":"ae2:storage_cell_state"}"#).unwrap(),
        TintSource::Constant(-1)
    );
    assert!(tint(r#"{"type":"minecraft:nonsense"}"#).is_err());
}
