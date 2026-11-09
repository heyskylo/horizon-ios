//! `AvoidEntityGoal`/`TemptGoal` targeting predicates (test module of `goals.rs`).
use super::*;

use super::*;
use crate::mobs::world::test_player;
use crate::mobs::world::{with_built_test_world, with_test_world};
use std::sync::Arc;

/// `(the player is inside the plain distance range, the goal picks them)`.
/// The wall stands at z 5..6, between the rabbit at z 0.5 and the player at
/// z 10.5, so sight is the only thing that changes between the two.
fn target(difficulty: u8, wall: bool) -> Option<(bool, bool)> {
    let build = move |world: &mut crate::world::World,
                      registry: &horizon_world::registry::BlockRegistry| {
        for cx in -1..1 {
            for cz in -1..1 {
                world.chunks.insert(
                    (cx, cz),
                    std::sync::Arc::new(crate::chunk::Chunk::empty(cx, cz)),
                );
            }
        }
        let stone = registry.default_state("minecraft:stone").unwrap();
        for x in -4..5 {
            for z in -4..15 {
                world.set(x, 64, z, stone).unwrap();
            }
        }
        if wall {
            for x in -4..5 {
                for y in 65..68 {
                    for z in 5..7 {
                        world.set(x, y, z, stone).unwrap();
                    }
                }
            }
        }
    };
    let mut out = None;
    let ran = with_built_test_world(&[], build, |w, types| {
        let mut world = w.clone();
        world.difficulty = super::super::spawning::LocalDifficulty::new(difficulty, 0, 0, 0.0);
        let player = test_player(Vec3::new(0.5, 65.0, 10.5));
        world.player = Some(player.clone());
        let mut m = Mob::new(
            1,
            Arc::new(types.types["minecraft:rabbit"].clone()),
            Vec3::new(0.5, 65.0, 0.5),
            0.0,
        );
        let mut goal = AvoidEntity::new(Avoid::Player, 20.0, 1.0, 1.0);
        let (mut lr, mut events) = (horizon_core::random::LegacyRandom::new(0), Vec::new());
        let mut cx = Cx {
            w: &world,
            lr: &mut lr,
            events: &mut events,
        };
        let in_range = m.distance_sqr(player.position) <= 20.0 * 20.0;
        out = Some((in_range, goal.avoid_target(&mut m, &mut cx).is_some()));
    });
    ran.then_some(out).flatten()
}

/// `Sensing.hasLineOfSight`: the player is ten blocks away, well inside the
/// 20 block range, but the wall is between them, so the goal must not pick
/// them (it did before: only the distance was checked).
#[test]
fn a_rabbit_does_not_flee_a_player_behind_a_wall() {
    if let Some((in_range, picked)) = target(2, true) {
        assert!(in_range, "the player is inside the plain distance range");
        assert!(!picked, "the rabbit picked a player it cannot see");
    }
}

/// `LivingEntity.canAttack`: in Peaceful no player is a target at all, so
/// the goal does not pick them even in plain sight.
#[test]
fn a_rabbit_does_not_flee_in_peaceful() {
    if let Some((in_range, picked)) = target(0, false) {
        assert!(in_range, "the player is inside the plain distance range");
        assert!(!picked, "the rabbit picked a player in Peaceful");
    }
}

/// The control: the same world on normal difficulty with nothing in the way
/// still picks the player.
#[test]
fn a_rabbit_flees_a_player_it_can_see() {
    if let Some((in_range, picked)) = target(2, false) {
        assert!(in_range, "the player is inside the plain distance range");
        assert!(picked, "the rabbit ignored a player in the open");
    }
}

/// `TemptGoal` scales its range by the player's visibility, with a floor
/// of two blocks: an invisible player five blocks away does not tempt.
#[test]
fn an_invisible_player_tempts_only_within_two_blocks() {
    let mut outcomes = Vec::new();
    if !with_test_world(&[], |w, types| {
        let kind = Arc::new(types.types["minecraft:cow"].clone());
        for (invisible, x) in [(false, 5.5), (true, 5.5), (true, 1.5)] {
            let mut world = w.clone();
            let mut p = test_player(Vec3::new(x, 64.0, 0.5));
            p.main_hand = Some("minecraft:wheat".into());
            p.invisible = invisible;
            world.player = Some(p);
            let mut m = Mob::new(1, kind.clone(), Vec3::new(0.5, 64.0, 0.5), 0.0);
            m.ai.goals.add(
                3,
                Tempt::new(1.25, Arc::from(vec!["minecraft:wheat".to_string()]), false),
            );
            let mut lr = LegacyRandom::new(0);
            let mut events = Vec::new();
            let mut cx = Cx {
                w: &world,
                lr: &mut lr,
                events: &mut events,
            };
            let mut goals = std::mem::take(&mut m.ai.goals);
            goals.tick(&mut m, &mut cx);
            outcomes.push(goals.is_running("Tempt"));
            m.ai.goals = goals;
        }
    }) {
        return;
    }
    assert_eq!(
        outcomes,
        vec![true, false, true],
        "visible at five, invisible at five, invisible at one and a half"
    );
}

/// `TemptGoal` finds its player with `TargetingConditions.forNonCombat()`,
/// which never picks a spectator: a spectator holding wheat does not tempt
/// an animal.
#[test]
fn a_spectator_does_not_tempt_an_animal() {
    let mut outcomes = Vec::new();
    if !with_test_world(&[], |w, types| {
        let kind = Arc::new(types.types["minecraft:cow"].clone());
        for spectator in [false, true] {
            let mut world = w.clone();
            let mut p = test_player(Vec3::new(2.5, 64.0, 0.5));
            p.main_hand = Some("minecraft:wheat".into());
            p.spectator = spectator;
            world.player = Some(p);
            let mut m = Mob::new(1, kind.clone(), Vec3::new(0.5, 64.0, 0.5), 0.0);
            m.ai.goals.add(
                3,
                Tempt::new(1.25, Arc::from(vec!["minecraft:wheat".to_string()]), false),
            );
            let mut lr = LegacyRandom::new(0);
            let mut events = Vec::new();
            let mut cx = Cx {
                w: &world,
                lr: &mut lr,
                events: &mut events,
            };
            let mut goals = std::mem::take(&mut m.ai.goals);
            goals.tick(&mut m, &mut cx);
            outcomes.push(goals.is_running("Tempt"));
            m.ai.goals = goals;
        }
    }) {
        return;
    }
    assert_eq!(
        outcomes,
        vec![true, false],
        "a survival player tempts; a spectator does not"
    );
}
