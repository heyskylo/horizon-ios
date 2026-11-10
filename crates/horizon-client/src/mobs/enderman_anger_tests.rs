//! `NeutralMob` anger on endermen (`Enderman.aiStep` → `updatePersistentAnger(level, true)`, and
//! `EndermanLookForPlayerGoal.isAngerInducing` = staring at it, or a player it is angry at).
use super::*;
use crate::mobs::world::{test_player, with_test_world};
use std::sync::Arc;

fn enderman() -> Mob {
    let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
    kind.species = Species::Enderman;
    let mut m = Mob::new(1, Arc::new(kind), Vec3::new(0.5, 64.0, 0.5), 0.0);
    m.attrs.follow_range = 64.0;
    m
}

fn look_goal() -> LookForPlayer {
    LookForPlayer {
        pending: false,
        aggro: 0,
        teleport: 0,
    }
}

/// A survival player at `at` looking straight away from the enderman (no stare).
fn world_with_player<'a>(w: &MobWorld<'a>, m: &Mob, at: Vec3) -> MobWorld<'a> {
    let mut world = w.clone();
    let mut p = test_player(at);
    let (dx, dz) = (p.position.x - m.position.x, p.position.z - m.position.z);
    let len = (dx * dx + dz * dz).sqrt();
    p.look = [dx / len, 0.0, dz / len];
    world.player = Some(p);
    world
}

/// Whether the look goal starts this tick.
fn look_goal_starts(m: &mut Mob, world: &MobWorld) -> bool {
    m.ai.goals.add(1, look_goal());
    let mut lr = horizon_core::random::LegacyRandom::new(0);
    let mut events = Vec::new();
    let mut cx = Cx {
        w: world,
        lr: &mut lr,
        events: &mut events,
    };
    let mut goals = std::mem::take(&mut m.ai.goals);
    goals.tick(m, &mut cx);
    let running = goals.is_running("LookForPlayer");
    m.ai.goals = goals;
    running
}

/// Holding the player as its target makes the enderman angry at that player for 20 to 39 s
/// (`startPersistentAngerTimer` every tick it holds it). Losing the target (the daylight teleport's
/// `setTarget(null)`) keeps the anger, which runs down tick by tick, and the look goal takes the
/// player back on sight with no stare. A calm enderman ignores the same player.
#[test]
fn an_angry_enderman_takes_its_player_back_on_sight_without_a_stare() {
    let mut out = None;
    if !with_test_world(&[], |w, _| {
        let mut m = enderman();
        let world = world_with_player(w, &m, Vec3::new(6.5, 64.0, 0.5));
        let mut events = Vec::new();
        m.ai.target = Some(Who::Player);
        ai_step(&mut m, &world, &mut events);
        let held = (m.extra.angry_at, m.extra.anger);
        m.ai.target = None;
        for _ in 0..10 {
            ai_step(&mut m, &world, &mut events);
        }
        let after_loss = (m.extra.angry_at, m.extra.anger);
        let angry_retargets = look_goal_starts(&mut m, &world);
        let mut calm = enderman();
        let calm_retargets = look_goal_starts(&mut calm, &world);
        out = Some((held, after_loss, angry_retargets, calm_retargets));
    }) {
        return;
    }
    let Some(((held_at, held), (lost_at, lost), angry, calm)) = out else {
        return;
    };
    assert_eq!(held_at, Some(Who::Player), "holding the player names it");
    assert!(
        (400..=780).contains(&held),
        "20 to 39 s of anger, got {held}"
    );
    assert_eq!(lost_at, Some(Who::Player), "a lost target keeps the anger");
    assert_eq!(lost, held - 10, "the anger runs down one tick at a time");
    assert!(angry, "an angry enderman takes its player back on sight");
    assert!(!calm, "a calm one needs a stare");
}

/// The anger ends when its time runs out with no target (`stopBeingAngry`): the enderman is calm.
#[test]
fn an_enderman_calms_down_when_its_anger_runs_out() {
    let mut out = None;
    if !with_test_world(&[], |w, _| {
        let mut m = enderman();
        let world = world_with_player(w, &m, Vec3::new(6.5, 64.0, 0.5));
        m.extra.angry_at = Some(Who::Player);
        m.extra.anger = 1;
        ai_step(&mut m, &world, &mut Vec::new());
        out = Some((m.extra.angry_at, look_goal_starts(&mut m, &world)));
    }) {
        return;
    }
    assert_eq!(
        out,
        Some((None, false)),
        "anger over: no target, no retargeting without a stare"
    );
}

/// `continueAggroTargetConditions` is `forCombat().ignoreLineOfSight()`, which has no range: an
/// enderman keeps a player target that went beyond its 64-block follow range.
#[test]
fn an_enderman_keeps_a_target_beyond_its_follow_range() {
    let mut out = None;
    if !with_test_world(&[], |w, _| {
        let mut m = enderman();
        let world = world_with_player(w, &m, Vec3::new(80.5, 64.0, 0.5));
        m.ai.target = Some(Who::Player);
        let mut lr = horizon_core::random::LegacyRandom::new(0);
        let mut events = Vec::new();
        let mut cx = Cx {
            w: &world,
            lr: &mut lr,
            events: &mut events,
        };
        out = Some(look_goal().can_continue(&mut m, &mut cx));
    }) {
        return;
    }
    assert_eq!(out, Some(true), "the target 80 blocks away is kept");
}
