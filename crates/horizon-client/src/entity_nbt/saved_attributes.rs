//! The saved permanent attribute modifiers a mob loads (`LivingEntity.readAdditionalSaveData`).

use super::{Mob, Tag, float, text};
use crate::mobs::finalize::Attr;

/// The save keeps the entry the mob was loaded with, so its modifier list is
/// stale where the mob holds none now (a saved `minecraft:baby` that `setBaby`
/// removed). Armor is not read by the load, so its list is kept.
pub(super) fn forget_stale_modifiers(list: &mut [Tag], attr: Attr) {
    let id = format!("minecraft:{}", attr.name());
    if attr != Attr::Armor
        && let Some(Tag::Compound(entry)) = list
            .iter_mut()
            .find(|t| text(t.get("id")).as_deref() == Some(id.as_str()))
    {
        entry.remove(&"modifiers".into());
    }
}
/// `AttributeInstance.apply` for the saved permanent modifiers of the attributes
/// the mob's store models (`LivingEntity.readAdditionalSaveData`, after the
/// bases): each lands under its id, replacing one it already holds. A malformed
/// entry is skipped; vanilla drops the whole list for one.
pub(super) fn read_attribute_modifiers(m: &mut Mob, tag: &Tag) {
    use crate::mobs::finalize::{Op, add_modifier};
    let Some(Tag::List(attrs)) = tag.get("attributes") else {
        return;
    };
    for a in attrs {
        let Some(attr) = text(a.get("id"))
            .and_then(|id| id.strip_prefix("minecraft:").map(str::to_owned))
            .and_then(|name| Attr::ALL.into_iter().find(|x| x.name() == name))
        else {
            continue;
        };
        let Some(Tag::List(modifiers)) = a.get("modifiers") else {
            continue;
        };
        // Armor lives in the sulfur cube's own map, not this store.
        if attr == Attr::Armor {
            continue;
        }
        for x in modifiers {
            let (Some(id), Some(amount)) = (text(x.get("id")), float(x.get("amount"))) else {
                continue;
            };
            let op = match text(x.get("operation")).as_deref() {
                Some("add_value") => Op::AddValue,
                Some("add_multiplied_base") => Op::AddMultipliedBase,
                Some("add_multiplied_total") => Op::AddMultipliedTotal,
                _ => continue,
            };
            // An Identifier without a namespace is `minecraft:`.
            let id = if id.contains(':') {
                id
            } else {
                format!("minecraft:{id}")
            };
            add_modifier(m, attr, id, amount, op);
        }
    }
}

/// `LivingEntity.readAdditionalSaveData` applies the saved permanent attribute
/// modifiers (`AttributeInstance.apply`), then Zombie/Piglin `setBaby(IsBaby)`
/// removes a saved `minecraft:baby`. Expected values come from the vanilla
/// formula (`AttributeInstance.calculateValue`) worked by hand.
#[cfg(test)]
mod saved_attribute_modifier_tests {
    use crate::entity_nbt::*;
    use crate::mobs::{
        Species,
        finalize::{self, Attr, Op},
        sensor_scan_tests::kind,
    };
    fn types() -> HashMap<String, Arc<MobType>> {
        let mut zombie = (*kind()).clone();
        zombie.id = "minecraft:zombie".into();
        zombie.species = Species::Zombie;
        // The zombie's registered defaults.
        zombie.attributes = Arc::new(HashMap::from([
            ("minecraft:max_health".to_owned(), 20.0),
            ("minecraft:movement_speed".to_owned(), 0.23),
            ("minecraft:attack_damage".to_owned(), 3.0),
        ]));
        HashMap::from([("minecraft:zombie".to_owned(), Arc::new(zombie))])
    }
    fn load(attributes: &str, extra: &str) -> Mob {
        let snbt = format!(
            r#"{{id:"minecraft:zombie",Pos:[0.5d,64d,0.5d],{extra}attributes:{attributes}}}"#
        );
        let tag = horizon_world::nbt::parse_snbt(&snbt).unwrap().0;
        mob_from_nbt(&tag, &types(), 1, None).unwrap()
    }
    #[test]
    fn saved_modifiers_apply_by_operation_and_raise_the_loaded_health() {
        // attack_damage: 4 + 8 = 12; 12 + 12 * 0.5 = 18; 18 * (1 + 1) = 36.
        let m = load(
            r#"[{id:"minecraft:attack_damage",base:4d,modifiers:[
                {id:"t:total",amount:1d,operation:"add_multiplied_total"},
                {id:"t:base",amount:0.5d,operation:"add_multiplied_base"},
                {id:"t:flat",amount:8d,operation:"add_value"}]},
              {id:"minecraft:max_health",base:20d,modifiers:[
                {id:"minecraft:leader_zombie_bonus",amount:2d,operation:"add_value"}]}]"#,
            "",
        );
        assert_eq!(finalize::value(&m, Attr::AttackDamage), 36.0);
        assert_eq!(finalize::value(&m, Attr::MaxHealth), 22.0);
        // No saved Health: the max health, which now counts the modifier.
        assert_eq!(m.health, 22.0);
        assert!(finalize::has_modifier(
            &m,
            Attr::MaxHealth,
            "minecraft:leader_zombie_bonus"
        ));
        // And it is saved again.
        let saved = mob_to_nbt(&m, [1, 0, 0, 1], None, None, |_| None);
        let Some(Tag::List(list)) = saved.get("attributes") else {
            panic!("no attributes");
        };
        let health = list
            .iter()
            .find(|a| text(a.get("id")).as_deref() == Some("minecraft:max_health"))
            .unwrap();
        let Some(Tag::List(modifiers)) = health.get("modifiers") else {
            panic!("no modifiers");
        };
        assert_eq!(modifiers.len(), 1);
        assert_eq!(float(modifiers[0].get("amount")), Some(2.0));
    }
    #[test]
    fn a_modifier_replaces_its_id_and_an_unreadable_one_is_skipped() {
        let m = load(
            r#"[{id:"minecraft:movement_speed",base:0.23d,modifiers:[
                {id:"slow",amount:-0.2d,operation:"add_multiplied_total"},
                {id:"minecraft:slow",amount:-0.5d,operation:"add_multiplied_total"},
                {id:"t:bad_op",amount:9d,operation:"multiply"},
                {id:"t:no_amount",operation:"add_value"}]},
              {id:"minecraft:unknown_attribute",base:1d,modifiers:[
                {id:"t:x",amount:1d,operation:"add_value"}]}]"#,
            "",
        );
        // The unnamespaced id is `minecraft:slow`: the later entry wins.
        assert_eq!(
            finalize::modifier_amount(&m, Attr::MovementSpeed, "minecraft:slow"),
            Some(-0.5)
        );
        assert!(!finalize::has_modifier(&m, Attr::MovementSpeed, "t:bad_op"));
        assert!(!finalize::has_modifier(
            &m,
            Attr::MovementSpeed,
            "t:no_amount"
        ));
        // 0.23 * (1 - 0.5).
        assert_eq!(finalize::value(&m, Attr::MovementSpeed), 0.115);
        assert_eq!(m.attrs.speed, 0.115);
    }
    #[test]
    fn the_save_does_not_echo_a_modifier_the_load_removed() {
        let saved = r#"[{id:"minecraft:movement_speed",base:0.23d,modifiers:[
            {id:"minecraft:baby",amount:0.125d,operation:"add_value"}]},
          {id:"minecraft:armor",base:0d,modifiers:[
            {id:"t:armor",amount:2d,operation:"add_value"}]}]"#;
        let snbt = format!(
            r#"{{id:"minecraft:zombie",Pos:[0.5d,64d,0.5d],IsBaby:0b,attributes:{saved}}}"#
        );
        let raw = horizon_world::nbt::parse_snbt(&snbt).unwrap().0;
        let m = mob_from_nbt(&raw, &types(), 1, None).unwrap();
        let out = mob_to_nbt(&m, [1, 0, 0, 1], Some(&raw), None, |_| None);
        let Some(Tag::List(list)) = out.get("attributes") else {
            panic!("no attributes");
        };
        let entry = |id: &str| {
            list.iter()
                .find(|a| text(a.get("id")).as_deref() == Some(id))
                .unwrap()
        };
        // The speed entry stays with its base and no modifiers, as vanilla
        // packs it; the unread armor entry is kept as it was saved.
        let speed = entry("minecraft:movement_speed");
        assert_eq!(float(speed.get("base")), Some(0.23));
        assert!(speed.get("modifiers").is_none());
        assert!(entry("minecraft:armor").get("modifiers").is_some());
    }
    #[test]
    fn a_saved_baby_modifier_is_removed_by_set_baby_for_either_age() {
        let saved = r#"[{id:"minecraft:movement_speed",base:0.23d,modifiers:[
            {id:"minecraft:baby",amount:0.5d,operation:"add_multiplied_base"},
            {id:"t:keep",amount:0.1d,operation:"add_value"}]}]"#;
        for (extra, baby) in [("", false), ("IsBaby:0b,", false), ("IsBaby:1b,", true)] {
            let m = load(saved, extra);
            assert!(
                !finalize::has_modifier(&m, Attr::MovementSpeed, "minecraft:baby"),
                "{extra}"
            );
            assert!(finalize::has_modifier(&m, Attr::MovementSpeed, "t:keep"));
            assert_eq!(m.is_baby(), baby, "{extra}");
            // A baby holds the transient modifier instead, an adult none.
            assert_eq!(
                finalize::transient(&m, Attr::MovementSpeed),
                baby.then_some((0.5, Op::AddMultipliedBase)),
                "{extra}"
            );
            // The permanent modifier is not saved again either.
            let out = mob_to_nbt(&m, [1, 0, 0, 1], None, None, |_| None);
            let text = format!("{out:?}");
            assert!(!text.contains("minecraft:baby"), "{extra}: {text}");
        }
    }

    /// `readAdditionalSaveData` stores `AbsorptionAmount` through
    /// `internalSetAbsorptionAmount`, which does not clamp (only
    /// `setAbsorptionAmount` does), and `addAdditionalSaveData` writes it back
    /// as it is: a negative amount, or one above `max_absorption` (0 without
    /// the Absorption effect), survives a load and a save.
    #[test]
    fn absorption_amount_loads_and_saves_unclamped() {
        for (saved, expected) in [
            ("", 0.0_f32),
            ("AbsorptionAmount:-1.0f,", -1.0),
            ("AbsorptionAmount:0.0f,", 0.0),
            ("AbsorptionAmount:3.5f,", 3.5),
            ("AbsorptionAmount:1.0E30f,", 1.0e30),
        ] {
            let m = load("[]", saved);
            assert_eq!(m.absorption, expected, "{saved}");
            let out = mob_to_nbt(&m, [1, 0, 0, 1], None, None, |_| None);
            assert_eq!(
                out.get("AbsorptionAmount"),
                Some(&Tag::Float(expected.to_bits())),
                "{saved}"
            );
        }
    }
}
