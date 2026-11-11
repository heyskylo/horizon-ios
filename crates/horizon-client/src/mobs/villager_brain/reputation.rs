//! Visibility gates used by villager targeting and reputation witnesses.
use super::{FOLLOW_RANGE, Mob, MobWorld, Species, Who, mem_ref, visible_test};

/// `Sensor.isEntityTargetable`: `TargetingConditions.forNonCombat()` within
/// the follow range, in sight.
pub(super) fn targetable(m: &mut Mob, w: &MobWorld, who: Who) -> bool {
    match who {
        Who::Player => m.can_see_player(w, FOLLOW_RANGE),
        Who::Mob(id) => {
            let Some(s) = w.seen(id) else { return false };
            let visibility = s.villager.as_ref().map_or(1.0, |v| v.visibility(w.items));
            let range = (m.attrs.follow_range * visibility).max(2.0);
            s.alive && m.distance_sqr(s.position) <= range * range && m.sees(w, who)
        }
    }
}

/// `Villager.tellWitnessesThatIWasMurdered`: only the candidates retained
/// by the last nearest-living scan, with the same lazy visibility cache.
pub fn murder_witnesses(m: &mut Mob, w: &MobWorld) -> Vec<u32> {
    let Some(list) = mem_ref(m)
        .and_then(|mm| mm.visible_mobs.get())
        .map(|v| v.list.clone())
    else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|who| {
            let Who::Mob(id) = who else { return None };
            (w.seen(id).is_some_and(|s| s.species == Species::Villager) && visible_test(m, w, who))
                .then_some(id)
        })
        .collect()
}
