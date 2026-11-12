use super::*;
use serde_json::json;

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

#[test]
fn custom_condition_is_evaluated_per_stack_at_branch_selection() {
    struct Env;
    impl ItemEnv for Env {
        fn item_model_property(&self, property: &str, stack: &StackView) -> bool {
            assert_eq!(property, "example:charged");
            stack.count > 1 && stack.patch.get("example:charged") == Some(&Some(Value::Bool(true)))
        }
    }
    let definition = ClientItem::from_json(
        r#"{"model":{
        "type":"minecraft:condition", "property":"example:charged",
        "on_true":{"type":"minecraft:model","model":"example:item/charged"},
        "on_false":{"type":"minecraft:model","model":"example:item/idle"}
    }}"#,
    )
    .unwrap();
    let mut models = ItemModels::default();
    models.definitions.insert("example:tool".into(), definition);
    models.prototypes.insert(
        "example:tool".into(),
        BTreeMap::from([(
            "minecraft:item_model".into(),
            Value::String("example:tool".into()),
        )]),
    );
    let context = Context {
        display: DisplayContext::Gui,
        owner: None,
        level: None,
        seed: 0,
    };
    let mut stack = StackView::new("example:tool", 1);
    let chosen = |stack: &StackView, env: &dyn ItemEnv| {
        let resolved = models.resolve(stack, &context, env);
        assert_eq!(resolved.layers.len(), 1);
        let LayerContent::Model { model } = &resolved.layers[0].content else {
            panic!("expected model leaf")
        };
        model.clone()
    };
    assert_eq!(chosen(&stack, &NoEnv), "example:item/idle");
    assert_eq!(chosen(&stack, &Env), "example:item/idle");
    stack
        .patch
        .insert("example:charged".into(), Some(Value::Bool(true)));
    assert_eq!(chosen(&stack, &Env), "example:item/idle");
    stack.count = 2;
    assert_eq!(chosen(&stack, &Env), "example:item/charged");
    assert_eq!(chosen(&stack, &NoEnv), "example:item/idle");
    stack.patch.insert("example:charged".into(), None);
    assert_eq!(chosen(&stack, &Env), "example:item/idle");
}

#[test]
fn custom_condition_validates_its_name_and_both_branches() {
    let node = |property: &str, on_true: Value| {
        ItemModelNode::decode(&serde_json::json!({
            "type": "minecraft:condition", "property": property,
            "on_true": on_true, "on_false": {"type": "minecraft:empty"}
        }))
    };
    let empty = serde_json::json!({"type": "minecraft:empty"});
    for name in [
        "minecraft:unknown",
        "unknown",
        "example:Bad",
        "bad name:charged",
    ] {
        assert!(node(name, empty.clone()).is_err(), "{name}");
    }
    assert!(node("example:charged", Value::Null).is_err());
    assert!(matches!(
        node("minecraft:damaged", empty.clone()).unwrap(),
        ItemModelNode::Condition {
            property: ConditionProperty::Damaged,
            ..
        }
    ));
    assert!(matches!(node("example:charged", empty).unwrap(),
        ItemModelNode::Condition { property: ConditionProperty::Custom(name), .. } if name == "example:charged"));
}

#[test]
fn a_patched_stack_reads_its_patch_over_its_items_defaults() {
    let mut models = ItemModels::default();
    models.prototypes.insert(
        "minecraft:diamond_sword".into(),
        Components::from_iter([
            (
                "minecraft:item_model".into(),
                json!("minecraft:diamond_sword"),
            ),
            ("minecraft:max_damage".into(), json!(1561)),
            ("minecraft:rarity".into(), json!("common")),
        ]),
    );
    let mut stack = StackView::new("minecraft:diamond_sword", 1);
    stack.patch.insert(
        "minecraft:enchantments".into(),
        Some(json!({"minecraft:sharpness": 5})),
    );
    stack
        .patch
        .insert("minecraft:rarity".into(), Some(json!("rare")));
    stack.patch.insert("minecraft:max_damage".into(), None);
    let view = models.components_view(&stack);
    assert_eq!(view.get("minecraft:rarity"), Some(&json!("rare")));
    assert_eq!(view.get("minecraft:max_damage"), None, "a removed default");
    assert_eq!(
        view.get("minecraft:item_model"),
        Some(&json!("minecraft:diamond_sword"))
    );
    assert!(view.contains_key("minecraft:enchantments"));
    assert!(!view.contains_key("minecraft:custom_name"));
    // Every key reads as the merged map has it.
    let merged = models.components(&stack);
    for key in [
        "minecraft:rarity",
        "minecraft:max_damage",
        "minecraft:item_model",
        "minecraft:enchantments",
        "minecraft:custom_name",
    ] {
        assert_eq!(view.get(key), merged.get(key), "{key}");
    }
    assert_eq!(view.to_map(), *merged);
}

#[test]
fn last_index_less_or_equal_linear_and_binary() {
    use super::last_index_less_or_equal;
    let make = |thresholds: &[f32]| -> Vec<(f32, ItemModelNode)> {
        thresholds
            .iter()
            .map(|&t| (t, ItemModelNode::Empty))
            .collect()
    };

    // Empty
    assert_eq!(last_index_less_or_equal(&[], 0.0), None);

    // Short (< 16): linear search
    let short = make(&[-1.0, 0.0, 1.0, 2.5, 5.0]);
    assert_eq!(last_index_less_or_equal(&short, -2.0), None);
    assert_eq!(last_index_less_or_equal(&short, -1.0), Some(0));
    assert_eq!(last_index_less_or_equal(&short, -0.5), Some(0));
    assert_eq!(last_index_less_or_equal(&short, 0.0), Some(1));
    assert_eq!(last_index_less_or_equal(&short, 2.5), Some(3));
    assert_eq!(last_index_less_or_equal(&short, 3.0), Some(3));
    assert_eq!(last_index_less_or_equal(&short, 5.0), Some(4));
    assert_eq!(last_index_less_or_equal(&short, 10.0), Some(4));

    // Long (>= 16): binary search
    let thresholds: Vec<f32> = (0..32).map(|i| i as f32 * 10.0).collect();
    let long = make(&thresholds);
    assert_eq!(last_index_less_or_equal(&long, -5.0), None);
    assert_eq!(last_index_less_or_equal(&long, 0.0), Some(0));
    assert_eq!(last_index_less_or_equal(&long, 5.0), Some(0));
    assert_eq!(last_index_less_or_equal(&long, 10.0), Some(1));
    assert_eq!(last_index_less_or_equal(&long, 150.0), Some(15));
    assert_eq!(last_index_less_or_equal(&long, 155.0), Some(15));
    assert_eq!(last_index_less_or_equal(&long, 310.0), Some(31));
    assert_eq!(last_index_less_or_equal(&long, 1000.0), Some(31));

    // Short lists use upper-bound tie behavior, including signed zero.
    let duplicates = make(&[1.0, 1.0, 2.0]);
    assert_eq!(last_index_less_or_equal(&duplicates, 1.0), Some(1));

    let zeroes = make(&[-0.0, 0.0]);
    assert_eq!(last_index_less_or_equal(&zeroes, -0.0), Some(1));
    assert_eq!(last_index_less_or_equal(&zeroes, 0.0), Some(1));

    // At 16 entries, vanilla returns Arrays.binarySearch's hit, not the last tie.
    // Expected indices were checked on the JVM.
    let duplicates = make(&[
        0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 7.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0,
    ]);
    assert_eq!(last_index_less_or_equal(&duplicates, 7.0), Some(7));

    let zeroes = make(&[
        -8.0, -7.0, -6.0, -5.0, -4.0, -3.0, -2.0, -0.0, -0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0,
    ]);
    assert_eq!(last_index_less_or_equal(&zeroes, -0.0), Some(7));
    assert_eq!(last_index_less_or_equal(&zeroes, 0.0), Some(9));
}
