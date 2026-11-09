//! Item stacks in NBT (`ItemStack.CODEC`): id, count and components, alone or in
//! slot lists.
use horizon_client_core::nbt_value::{number, string, text};
use horizon_world::nbt::Tag;
use std::sync::Arc;

/// The two measured saved forms of `BlockPredicate.CODEC`. Operate only on
/// the fresh ItemStack save, never its model or the shared live component view.
fn canonical_saved_adventure_predicates(components: &mut Tag) {
    fn predicate(value: &mut Tag) {
        let named = match value.get("blocks") {
            Some(Tag::List(names)) => match names.as_slice() {
                [Tag::String(name)]
                    if horizon_world::identifier::Identifier::parse(&name.to_string_lossy())
                        .is_ok_and(|id| !id.path().is_empty()) =>
                {
                    Some(name.clone())
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(name) = named {
            *value.get_mut("blocks").unwrap() = Tag::String(name);
        }
        // CustomDataPredicate -> NbtPredicate -> TagParser.LENIENT_CODEC.
        // Exact `components` and the item's own custom_data are distinct.
        if let Some(custom) = value
            .get_mut("predicates")
            .and_then(|p| p.get_mut("minecraft:custom_data"))
            && matches!(custom, Tag::Compound(_))
        {
            *custom = Tag::String(horizon_world::snbt_text::to_snbt(custom).as_str().into());
        }
    }
    for key in ["minecraft:can_break", "minecraft:can_place_on"] {
        match components.get_mut(key) {
            Some(Tag::List(predicates)) => predicates.iter_mut().for_each(predicate),
            Some(value) => predicate(value),
            None => {}
        }
    }
}

pub fn encode_item(stack: &crate::items::ItemStack) -> Vec<(&'static str, Tag)> {
    let mut entries = vec![
        ("id", string(&stack.item)),
        ("count", Tag::Int(stack.count as i32)),
    ];
    if let Some(mut components) = crate::item_data::encode_components(stack) {
        canonical_saved_adventure_predicates(&mut components);
        entries.push(("components", components));
    }
    entries
}
/// ItemStack.CODEC (id, count, components).
pub fn item_to_nbt(stack: &crate::items::ItemStack) -> Tag {
    Tag::compound(encode_item(stack))
}
/// ItemStack.CODEC saved against the actual loaded item prototype.
pub fn item_to_nbt_with_items(stack: &crate::items::ItemStack, items: &crate::items::Items) -> Tag {
    let mut entries = vec![
        ("id", string(&stack.item)),
        ("count", Tag::Int(stack.count as i32)),
    ];
    if let Some(mut components) = crate::item_data::encode_components_with_items(stack, items) {
        canonical_saved_adventure_predicates(&mut components);
        entries.push(("components", components));
    }
    Tag::compound(entries)
}
/// ItemStack.CODEC's count range is independent of the item's stack limit;
/// an out-of-range count uses the field's default of one. The id is read as an
/// `Identifier`: `stone` is `minecraft:stone` (a summoned display's or a
/// command's NBT often leaves the namespace out), and an id that is no valid
/// identifier makes no stack.
pub fn item_from_nbt(tag: &Tag) -> Option<crate::items::ItemStack> {
    let (namespace, path) = horizon_world::datafix::identifier(&text(tag.get("id"))?)?;
    let id = format!("{namespace}:{path}");
    if id == "minecraft:air" {
        return None;
    }
    let count = number(tag.get("count"))
        .or_else(|| number(tag.get("Count")))
        .filter(|count| (1..=99).contains(count))
        .unwrap_or(1) as u32;
    let (damage, extra) = crate::item_data::decode_components(tag.get("components"));
    Some(crate::items::ItemStack {
        item: Arc::from(id.as_str()),
        count,
        damage,
        extra,
    })
}
pub fn encode_items(slots: &[Option<crate::items::ItemStack>]) -> Tag {
    Tag::List(
        slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let mut entries = vec![("Slot", Tag::Byte(i as i8))];
                entries.extend(encode_item(s.as_ref()?));
                Some(Tag::compound(entries))
            })
            .collect(),
    )
}
pub fn decode_items(tag: Option<&Tag>, size: usize) -> Vec<Option<crate::items::ItemStack>> {
    let mut slots = vec![None; size];
    if let Some(Tag::List(items)) = tag {
        for it in items {
            let slot = number(it.get("Slot")).unwrap_or(0) as u8 as usize;
            if let Some(cell) = slots.get_mut(slot) {
                *cell = item_from_nbt(it);
            }
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nbt_count_defaults_to_one_outside_codec_range() {
        for item in ["minecraft:stone", "minecraft:diamond_sword"] {
            for (raw, expected) in [
                (None, 1),
                (Some(Tag::Int(-1)), 1),
                (Some(Tag::Int(0)), 1),
                (Some(Tag::Int(1)), 1),
                (Some(Tag::Int(2)), 2),
                (Some(Tag::Int(64)), 64),
                (Some(Tag::Int(99)), 99),
                (Some(Tag::Int(100)), 1),
                (Some(Tag::Int(255)), 1),
                (Some(Tag::Int(256)), 1),
                (Some(Tag::Int(i32::MAX)), 1),
                (Some(Tag::Byte(1)), 1),
                (Some(Tag::Float(1.5_f32.to_bits())), 1),
                (Some(string("1")), 1),
            ] {
                let mut tag = Tag::compound([("id", string(item))]);
                if let (Tag::Compound(map), Some(raw)) = (&mut tag, &raw) {
                    map.insert("count".into(), raw.clone());
                }
                let stack = item_from_nbt(&tag).unwrap();
                assert_eq!(stack.count, expected, "{item}: {raw:?}");
                assert_eq!(
                    number(item_to_nbt(&stack).get("count")),
                    Some(i64::from(expected))
                );
            }
        }
    }

    /// `/summon item_display ~ ~ ~ {item:{id:"stone"}}` shows stone in vanilla:
    /// the id reads as an Identifier, with the default namespace (#8570).
    #[test]
    fn an_id_without_a_namespace_is_a_minecraft_item() {
        let read = |id: &str| item_from_nbt(&Tag::compound([("id", string(id))]));
        assert_eq!(
            read("stone").map(|s| s.item),
            Some("minecraft:stone".into())
        );
        assert_eq!(
            read(":diamond").map(|s| s.item),
            Some("minecraft:diamond".into())
        );
        assert_eq!(read("nexo:ruby").map(|s| s.item), Some("nexo:ruby".into()));
        assert!(read("air").is_none());
        assert!(read("Stone").is_none(), "not a valid identifier");
    }

    #[test]
    fn runtime_counts_remain_independent_of_the_nbt_decode_range() {
        let stack = crate::items::ItemStack::new("minecraft:diamond_sword", 1000);
        assert_eq!(stack.count, 1000);
        assert_eq!(number(item_to_nbt(&stack).get("count")), Some(1000));
    }
}

#[cfg(test)]
mod h285_saved_adventure_tests {
    use super::*;
    use crate::{item_data::components_tag, items::ItemStack};
    fn parse(s: &str) -> Tag {
        horizon_world::nbt::parse_snbt(s).unwrap().0
    }
    fn item(key: &str, value: Tag) -> Tag {
        Tag::compound([
            ("id", string("minecraft:stone")),
            ("count", Tag::Int(3)),
            (
                "components",
                Tag::compound([
                    (key, value),
                    ("minecraft:custom_data", parse("{keep:17}")),
                    ("minecraft:repair_cost", Tag::Int(2)),
                ]),
            ),
        ])
    }
    #[derive(Clone, Copy)]
    enum Mode {
        Decoded,
        NoSource,
        Mutated,
    }
    fn check(mode: Mode, custom: bool) {
        for key in ["minecraft:can_break", "minecraft:can_place_on"] {
            let wanted = if custom {
                parse(r#"{predicates:{"minecraft:custom_data":{mark:7}}}"#)
            } else {
                parse(r#"{blocks:["minecraft:stone"]}"#)
            };
            let stale = if custom {
                parse(r#"{predicates:{"minecraft:custom_data":{mark:1}}}"#)
            } else {
                parse(r#"{blocks:["minecraft:dirt"]}"#)
            };
            let source = item(
                key,
                if matches!(mode, Mode::Mutated) {
                    stale.clone()
                } else {
                    wanted.clone()
                },
            );
            let mut stack = if matches!(mode, Mode::NoSource) {
                let mut s = ItemStack::new("minecraft:stone", 3);
                s.data_mut().repair_cost = 2;
                s.data_mut()
                    .set_other("minecraft:custom_data", Some(parse("{keep:17}")));
                s.data_mut().set_other(key, Some(wanted.clone()));
                s
            } else {
                item_from_nbt(&source).unwrap()
            };
            if matches!(mode, Mode::Mutated) {
                assert_eq!(stack.data().unwrap().other(key), Some(&stale));
                stack.data_mut().set_other(key, Some(wanted.clone()));
                assert_eq!(source.get("components").unwrap().get(key), Some(&stale));
            }
            assert_eq!((&*stack.item, stack.count), ("minecraft:stone", 3));
            assert_eq!(stack.data().unwrap().other(key), Some(&wanted));
            assert!(stack.data().unwrap().wire.patch().is_none());
            let live = components_tag(stack.damage, stack.data()).unwrap();
            let saved = item_to_nbt(&stack);
            let components = saved.get("components").unwrap();
            assert_eq!(saved.get("id"), Some(&string("minecraft:stone")));
            assert_eq!(saved.get("count"), Some(&Tag::Int(3)));
            assert_eq!(
                components.get("minecraft:custom_data"),
                Some(&parse("{keep:17}"))
            );
            assert_eq!(components.get("minecraft:repair_cost"), Some(&Tag::Int(2)));
            assert_eq!(components_tag(stack.damage, stack.data()), Some(live));
            assert_eq!(stack.data().unwrap().other(key), Some(&wanted));
            let canonical = if custom {
                parse(r#"{predicates:{"minecraft:custom_data":"{mark:7}"}}"#)
            } else {
                parse(r#"{blocks:"minecraft:stone"}"#)
            };
            assert_eq!(
                components.get(key),
                Some(&canonical),
                "actual saved codec must canonicalize the two admitted forms"
            );
        }
    }
    #[test]
    fn decoded_singleton() {
        check(Mode::Decoded, false)
    }
    #[test]
    fn no_source_singleton() {
        check(Mode::NoSource, false)
    }
    #[test]
    fn mutated_singleton_ignores_stale_source() {
        check(Mode::Mutated, false)
    }
    #[test]
    fn decoded_custom_data() {
        check(Mode::Decoded, true)
    }
    #[test]
    fn no_source_custom_data() {
        check(Mode::NoSource, true)
    }
    #[test]
    fn mutated_custom_data_ignores_stale_source() {
        check(Mode::Mutated, true)
    }
    #[test]
    fn canonical_and_unrelated_forms_keep_their_types_and_order() {
        for key in ["minecraft:can_break", "minecraft:can_place_on"] {
            for value in [
                r#"{blocks:"minecraft:stone"}"#,
                r#"{blocks:["minecraft:stone","minecraft:dirt"]}"#,
                r#"[{blocks:"minecraft:stone"},{blocks:"minecraft:dirt"}]"#,
                r#"{predicates:{"minecraft:custom_data":"{mark:7}"}}"#,
                r#"{components:{"minecraft:custom_data":{mark:7}}}"#,
                r#"{state:{axis:"y"}}"#,
                "{}",
            ] {
                let wanted = parse(value);
                let stack = item_from_nbt(&item(key, wanted.clone())).unwrap();
                assert_eq!(
                    item_to_nbt(&stack).get("components").unwrap().get(key),
                    Some(&wanted)
                );
            }
        }
        assert!(
            item_to_nbt(&ItemStack::new("minecraft:stone", 3))
                .get("components")
                .is_none()
        );
    }
    // Added after the frozen old proof: both saved wrappers and repeat safety.
    #[test]
    fn both_saved_wrappers_are_idempotent_without_mutating_the_model() {
        let items = crate::items::Items::for_tests(Default::default());
        for key in ["minecraft:can_break", "minecraft:can_place_on"] {
            for (wanted, stale, expected) in [
                (
                    r#"{blocks:["minecraft:stone"]}"#,
                    r#"{blocks:["minecraft:dirt"]}"#,
                    r#"{blocks:"minecraft:stone"}"#,
                ),
                (
                    r#"{predicates:{"minecraft:custom_data":{mark:7}}}"#,
                    r#"{predicates:{"minecraft:custom_data":{mark:1}}}"#,
                    r#"{predicates:{"minecraft:custom_data":"{mark:7}"}}"#,
                ),
            ] {
                for mode in [Mode::Decoded, Mode::NoSource, Mode::Mutated] {
                    let source = item(
                        key,
                        parse(if matches!(mode, Mode::Mutated) {
                            stale
                        } else {
                            wanted
                        }),
                    );
                    let mut stack = if matches!(mode, Mode::NoSource) {
                        let mut s = ItemStack::new("minecraft:stone", 3);
                        s.data_mut().repair_cost = 2;
                        s.data_mut()
                            .set_other("minecraft:custom_data", Some(parse("{keep:17}")));
                        s.data_mut().set_other(key, Some(parse(wanted)));
                        s
                    } else {
                        item_from_nbt(&source).unwrap()
                    };
                    if matches!(mode, Mode::Mutated) {
                        stack.data_mut().set_other(key, Some(parse(wanted)));
                        assert_eq!(
                            source.get("components").unwrap().get(key),
                            Some(&parse(stale))
                        );
                    }
                    let before = stack.clone();
                    let live = components_tag(stack.damage, stack.data());
                    let a = item_to_nbt(&stack);
                    let b = item_to_nbt_with_items(&stack, &items);
                    assert_eq!(a, b);
                    assert_eq!(
                        a.get("components").unwrap().get(key),
                        Some(&parse(expected))
                    );
                    assert_eq!(item_to_nbt(&stack), a);
                    assert_eq!(item_to_nbt_with_items(&stack, &items), b);
                    let back = item_from_nbt(&a).unwrap();
                    assert_eq!(item_to_nbt(&back), a);
                    assert_eq!(item_to_nbt_with_items(&back, &items), a);
                    assert_eq!(stack, before);
                    assert_eq!(components_tag(stack.damage, stack.data()), live);
                    assert_eq!(stack.data().unwrap().other(key), Some(&parse(wanted)));
                    assert_eq!(a.get("id"), Some(&string("minecraft:stone")));
                    assert_eq!(a.get("count"), Some(&Tag::Int(3)));
                    assert_eq!(
                        a.get("components").unwrap().get("minecraft:custom_data"),
                        Some(&parse("{keep:17}"))
                    );
                }
            }
        }
    }
    #[test]
    fn tag_reference_unmeasured_and_exact_forms_keep_the_prior_path() {
        for key in ["minecraft:can_break", "minecraft:can_place_on"] {
            for value in [
                r##"{blocks:"#minecraft:logs"}"##,
                r##"{blocks:["#minecraft:logs"]}"##,
                r#"{blocks:[]}"#,
                r#"{blocks:[""]}"#,
                r#"{blocks:["minecraft:"]}"#,
                r#"{blocks:7}"#,
                r#"{blocks:[7]}"#,
                r#"{nbt:"{mark:7}"}"#,
                r#"{components:{"minecraft:custom_data":{mark:7}}}"#,
            ] {
                let value = parse(value);
                let stack = item_from_nbt(&item(key, value.clone())).unwrap();
                assert_eq!(
                    item_to_nbt(&stack).get("components").unwrap().get(key),
                    Some(&value)
                );
            }
        }
        let value =
            parse(r#"{blocks:["minecraft:stone"],predicates:{"minecraft:custom_data":{mark:7}}}"#);
        let mut stack = ItemStack::new("minecraft:stone", 3);
        stack
            .data_mut()
            .set_other("minecraft:custom_data", Some(value.clone()));
        stack
            .data_mut()
            .set_other("!minecraft:can_break", Some(parse("{}")));
        let saved = item_to_nbt(&stack);
        assert_eq!(
            saved
                .get("components")
                .unwrap()
                .get("minecraft:custom_data"),
            Some(&value)
        );
        assert_eq!(
            saved.get("components").unwrap().get("!minecraft:can_break"),
            Some(&parse("{}"))
        );
    }
    #[test]
    fn direct_encoder_and_nested_templates_are_outside_the_projection() {
        let raw =
            parse(r#"{blocks:["minecraft:stone"],predicates:{"minecraft:custom_data":{mark:7}}}"#);
        let mut inner = ItemStack::new("minecraft:stone", 3);
        inner
            .data_mut()
            .set_other("minecraft:can_break", Some(raw.clone()));
        assert_eq!(
            crate::item_data::encode_components(&inner)
                .unwrap()
                .get("minecraft:can_break"),
            Some(&raw)
        );
        assert_eq!(
            components_tag(inner.damage, inner.data())
                .unwrap()
                .get("minecraft:can_break"),
            Some(&raw)
        );
        let mut outer = ItemStack::new("minecraft:shulker_box", 1);
        outer.data_mut().container.push((0, inner));
        let saved = item_to_nbt(&outer);
        let Some(Tag::List(slots)) = saved.get("components").unwrap().get("minecraft:container")
        else {
            panic!("container")
        };
        assert_eq!(
            slots[0]
                .get("item")
                .unwrap()
                .get("components")
                .unwrap()
                .get("minecraft:can_break"),
            Some(&raw),
            "retained source gap, not a Java parity assertion"
        );
    }
}
