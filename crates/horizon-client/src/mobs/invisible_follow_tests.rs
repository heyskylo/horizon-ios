//! `FollowMobGoal` skips an invisible mob: the flag or the invisibility effect.
use super::goals::{Cx, FollowMob};
use super::{Mob, Species};
use crate::mobs::world::with_test_world;
use crate::mobs::LegacyRandom;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

/// `FollowMobGoal.canUse` skips an invisible mob (`!isInvisible()`), which
/// is the flag or the invisibility effect.
#[test]
fn a_follow_mob_goal_ignores_an_invisible_mob() {
    use crate::mobs::sensor_scan_tests::kind;
    let mut outcomes = Vec::new();
    if !with_test_world(&[], |w, _| {
        for invisible in [false, true] {
            let mut follower_kind = (*kind()).clone();
            follower_kind.species = Species::Parrot;
            let mut target_kind = (*kind()).clone();
            target_kind.species = Species::Cow;
            let mut m = Mob::new(1, Arc::new(follower_kind), Vec3::new(0.5, 64.0, 0.5), 0.0);
            let mut target = Mob::new(2, Arc::new(target_kind), Vec3::new(1.5, 64.0, 0.5), 0.0);
            target.invisible = invisible;
            let seen = [target.seen(None)];
            let world = crate::mobs::MobWorld {
                others: &seen,
                ..w.clone()
            };
            m.ai.goals.add(1, FollowMob::new(1.0, 3.0, 7.0));
            let mut lr = LegacyRandom::new(0);
            let mut events = Vec::new();
            let mut cx = Cx {
                w: &world,
                lr: &mut lr,
                events: &mut events,
            };
            let mut goals = std::mem::take(&mut m.ai.goals);
            goals.tick(&mut m, &mut cx);
            outcomes.push(goals.is_running("FollowMobGoal"));
            m.ai.goals = goals;
        }
    }) {
        return;
    }
    assert_eq!(
        outcomes,
        vec![true, false],
        "a visible mob is followed; an invisible one is not"
    );
}