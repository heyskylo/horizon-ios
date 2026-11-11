//! `LivingEntity.updateFallFlying` for a mob: the glide's checks and its gliders' wear.
use super::*;

/// `LivingEntity.updateFallFlying` on the server, which `aiStep` calls while
/// the glide flag is set, with or without AI: a mob that cannot glide
/// (`canGlide`: on the ground, riding, levitating, or no slot holding a
/// glider whose next damage will not break it, `canGlideUsing`) stops
/// gliding; else on every 20th glide tick one of its gliders, picked with
/// `Util.getRandom` from the mob's own random, takes a point of wear.
/// It starts with `checkFallDistanceAccumulation`: a glide that is not
/// diving keeps at most 1 of its fall. The `ELYTRA_GLIDE` game event of
/// every 10th tick is not ported here.
pub(super) fn update_fall_flying(m: &mut Mob, w: &MobWorld, events: &mut Vec<MobEvent>) {
    use horizon_world::player_inventory::EquipmentSlot;
    if !m.extra.fall_flying {
        return;
    }
    crate::mobs::leash::check_fall_distance(m);
    let gliders: Vec<EquipmentSlot> = match w.items {
        Some(items) => EquipmentSlot::ALL
            .into_iter()
            .filter(|&slot| {
                crate::mobs::finalize::equipment_stack(m, slot).is_some_and(|stack| {
                    crate::item_sync::can_glide_using(&stack, glider_slot_name(slot), items)
                })
            })
            .collect(),
        None => Vec::new(),
    };
    if m.on_ground
        || m.extra.mount.is_some()
        || m.has_effect("minecraft:levitation")
        || gliders.is_empty()
    {
        m.extra.fall_flying = false;
        return;
    }
    let ticks = m.extra.fall_fly_ticks + 1;
    if ticks % 10 == 0 && (ticks / 10) % 2 == 0 {
        let pick = m.random.next_int_bound(gliders.len() as i32).unwrap_or(0) as usize;
        events.push(MobEvent::GliderWear {
            mob: m.id,
            slot: gliders[pick],
        });
    }
}
/// `EquipmentSlot`'s serialized name, which `equippable` names its slot by.
fn glider_slot_name(slot: horizon_world::player_inventory::EquipmentSlot) -> &'static str {
    use horizon_world::player_inventory::EquipmentSlot as S;
    match slot {
        S::MainHand => "mainhand",
        S::OffHand => "offhand",
        S::Feet => "feet",
        S::Legs => "legs",
        S::Chest => "chest",
        S::Head => "head",
        S::Body => "body",
        S::Saddle => "saddle",
    }
}
