//! Client-side `Allay.mobInteract` state, before the authoritative server reply.
use super::super::{Mob, Species};
use crate::items::ItemStack;

#[derive(Debug)]
pub enum Interaction {
    Duplicate,
    Give,
    Take(ItemStack),
}

impl Interaction {
    pub fn sound(&self) -> &'static str {
        match self {
            Self::Duplicate => "minecraft:block.amethyst_block.chime",
            Self::Give => "minecraft:entity.allay.item_given",
            Self::Take(_) => "minecraft:entity.allay.item_taken",
        }
    }
}

/// `Allay.mobInteract`: clients update the hand and duplication flag too, so a
/// second click must use that predicted state. ClientLevel does not insert the
/// duplicate into its entity list; the server pairs the real child later.
pub fn predict(
    mob: &mut Mob,
    offered: Option<&ItemStack>,
    main_hand: bool,
    duplicates: bool,
    actor: [i32; 4],
) -> Option<Interaction> {
    if mob.species() != Species::Allay || !mob.alive() {
        return None;
    }
    let offered = offered.filter(|s| s.count > 0 && &*s.item != "minecraft:air");
    if mob.extra.flag && duplicates && offered.is_some() && mob.extra.egg_time == 0 {
        mob.extra.egg_time = 6000;
        return Some(Interaction::Duplicate);
    }
    let held = mob.extra.main_hand_stack().cloned();
    if held.is_none()
        && let Some(mut given) = offered.cloned()
    {
        given.count = 1;
        mob.extra.set_main_hand_stack(Some(given));
        super::like_player(mob, actor);
        return Some(Interaction::Give);
    }
    if main_hand
        && offered.is_none()
        && let Some(held) = held
    {
        mob.extra.set_main_hand(None);
        super::forget_liked_player(mob);
        mob.swing();
        return Some(Interaction::Take(held));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    fn allay() -> Mob {
        let mut kind = crate::mobs::sensor_scan_tests::kind();
        let kind_mut = Arc::make_mut(&mut kind);
        kind_mut.species = Species::Allay;
        kind_mut.id = "minecraft:allay".into();
        Mob::new(37, kind, Vec3::ZERO, 0.0)
    }

    #[test]
    fn giving_and_taking_update_the_state_used_by_the_next_click() {
        let mut mob = allay();
        let offered = ItemStack::new("minecraft:stone", 3);
        assert!(matches!(
            predict(&mut mob, Some(&offered), false, false, [1; 4]),
            Some(Interaction::Give)
        ));
        assert_eq!(mob.extra.main_hand_stack().unwrap().count, 1);
        assert_eq!(mob.extra.owner, Some([1; 4]));
        assert!(predict(&mut mob, Some(&offered), true, false, [1; 4]).is_none());
        assert!(predict(&mut mob, None, false, false, [1; 4]).is_none());
        let Some(Interaction::Take(returned)) = predict(&mut mob, None, true, false, [1; 4]) else {
            panic!("empty main hand takes the item")
        };
        assert_eq!(&*returned.item, "minecraft:stone");
        assert_eq!(returned.count, 1);
        assert!(mob.extra.main_hand_stack().is_none());
        assert!(!mob.extra.tame);
        assert_eq!(mob.extra.owner, None);
        assert!(predict(&mut mob, None, true, false, [1; 4]).is_none());
    }

    #[test]
    fn duplication_requires_the_tag_and_ready_flag_and_is_not_repeated() {
        let mut mob = allay();
        mob.extra.flag = true;
        mob.extra
            .set_main_hand_stack(Some(ItemStack::new("minecraft:stone", 1)));
        let offered = ItemStack::new("fixture:tagged_item", 2);
        assert!(predict(&mut mob, Some(&offered), false, false, [0; 4]).is_none());
        assert!(matches!(
            predict(&mut mob, Some(&offered), false, true, [0; 4]),
            Some(Interaction::Duplicate)
        ));
        assert_eq!(mob.extra.egg_time, 6000);
        assert!(predict(&mut mob, Some(&offered), false, true, [0; 4]).is_none());
        assert_eq!(
            &*mob.extra.main_hand_stack().unwrap().item,
            "minecraft:stone"
        );
    }
}
