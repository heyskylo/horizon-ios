//! A stack's damage capacity, as `ItemStack.getMaxDamage` and
//! `ItemStack.isDamageableItem` read it.

use crate::items::{ItemStack, Items};

/// `ItemStack.getMaxDamage` when `ItemStack.isDamageableItem`, else 0:
/// MAX_DAMAGE present, no UNBREAKABLE and DAMAGE present. A durable
/// prototype supplies the last; a normally non-durable item type carries
/// it on the stack, and `StackData::explicit_zero_damage` keeps an explicit
/// zero apart from no component at all.
pub fn stack_max_damage(items: &Items, stack: &ItemStack) -> u32 {
    if stack.count == 0 || &*stack.item == "minecraft:air" {
        return 0;
    }
    let component = |id| stack.data().and_then(|d| d.other(id));
    if component("!minecraft:max_damage").is_some()
        || component("!minecraft:damage").is_some()
        || component("minecraft:unbreakable").is_some()
    {
        return 0;
    }
    let prototype_max = items.get(&stack.item).map_or(0, |info| info.max_damage);
    let has_damage = prototype_max > 0
        || stack.damage > 0
        || stack.data().is_some_and(|d| d.explicit_zero_damage);
    if !has_damage {
        return 0;
    }
    match component("minecraft:max_damage") {
        Some(value) => horizon_world::datafix::dynamic::as_int(Some(value), 0).max(0) as u32,
        None => prototype_max,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item_data::{command_components, decode_components};
    use crate::item_nbt::{item_from_nbt, item_to_nbt};
    use horizon_world::loot::PackOverlay;
    use std::fs;

    /// A normally non-durable item type becomes damageable when the stack
    /// supplies DAMAGE as well: `ItemStack.isDamageableItem` asks for the
    /// component, not for the prototype's durability. Removing either
    /// component takes that away again, and an explicit zero DAMAGE survives
    /// saving.
    #[test]
    fn a_non_durable_stack_can_carry_its_own_durability() {
        let root = std::env::temp_dir().join(format!("horizon-durability-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data/minecraft/loot_table")).unwrap();
        fs::write(root.join("items.json"), r#"{"minecraft:stick":{}}"#).unwrap();
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let mut stick = ItemStack::new("minecraft:stick", 1);

        let durable = command_components(
            r#"[tool={rules:[{blocks:"stone",speed:7.0f,correct_for_drops:true}],damage_per_block:3},max_damage=20,damage=0]"#,
        )
        .unwrap();
        (stick.damage, stick.extra) = decode_components(Some(&durable));
        assert_eq!(stack_max_damage(&items, &stick), 20);
        assert_eq!(items.mining_damage_capacity(&stick), 20);
        let restored = item_from_nbt(&item_to_nbt(&stick)).unwrap();
        assert_eq!(
            stack_max_damage(&items, &restored),
            20,
            "the explicit zero DAMAGE survives saving"
        );
        for (patch, wanted) in [
            (",!minecraft:damage", 0),
            (",!minecraft:max_damage", 0),
            ("", 20),
        ] {
            let components = command_components(&format!(
                r#"[tool={{rules:[],damage_per_block:3}},max_damage=20,damage=0{patch}]"#
            ))
            .unwrap();
            (stick.damage, stick.extra) = decode_components(Some(&components));
            assert_eq!(stack_max_damage(&items, &stick), wanted, "{patch:?}");
        }

        // MAX_DAMAGE alone does not supply DAMAGE.
        let only = command_components("[max_damage=20]").unwrap();
        (stick.damage, stick.extra) = decode_components(Some(&only));
        assert_eq!(stack_max_damage(&items, &stick), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
