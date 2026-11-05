//! Client avatar state has different tick timing for players and mannequins.
use horizon_client::mobs::{Mob, Species};
use horizon_protocol::entity_data::Value;

/// The caller has performed the entity's interpolation/base animation work.
/// Players keep their existing previous-position tick and player-only bob;
/// ClientMannequin inherits the living pose tick and updates cloak state at
/// its current position after the base tick, without the player's bob. Both
/// parts still tick while dead, but not while the remote level is frozen.
pub(crate) fn tick(mob: &mut Mob, player: bool, running: bool) {
    if player {
        mob.tick_avatar_pose(mob.fluids.in_water);
    } else if running && mob.species() == Species::Mannequin {
        mob.tick_living_avatar_pose(mob.fluids.in_water);
        mob.extra
            .avatar
            .tick([mob.position.x, mob.position.y, mob.position.z]);
    }
}

/// Entity shared flags and pose feed every client avatar, not only players.
/// Keep player-only metadata indices (shoulder parrots, etc.) out of this path.
pub(crate) fn apply(mob: &mut Mob, values: &[(u8, i32, Value)]) {
    for (index, _, value) in values {
        match (*index, value) {
            (0, Value::Byte(flags)) => {
                mob.extra.fall_flying = flags & -128 != 0;
                mob.extra.crouching = flags & 2 != 0;
            }
            (6, Value::Id(pose)) => {
                mob.extra.synced_pose = (*pose).clamp(0, 17) as u8;
                mob.extra.avatar_pose = horizon_client::pose::Pose::from_id(*pose);
            }
            _ => {}
        }
    }
}
