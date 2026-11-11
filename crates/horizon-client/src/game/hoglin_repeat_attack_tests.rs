//! A hoglin that retaliates keeps attacking its own attacker when a
//! neighbouring hoglin is hit in the same tick: `HoglinAi.broadcastAttackTarget`
//! gives each visible adult hoglin the nearer of its current target and the
//! new one (`setAttackTargetIfCloserThanCurrent`)
//! (tools/test_hoglin_repeat_attack_parity.py measures the same on vanilla 26.3).
use super::commands::Target;
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const NEAR_HOGLIN: &str = "00006093-0000-0000-0000-000000000003";
const NEAR_PIG: &str = "00006093-0000-0000-0000-00000000000d";
const HOGLIN: &str = "00006093-0000-0000-0000-000000000001";
const PIG: &str = "00006093-0000-0000-0000-00000000000b";

fn game() -> Game<'static> {
    let root = std::path::PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
    let res: &'static _ = Box::leak(Box::new(loaded.resources));
    let mut game = Game::new(
        res,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);
    for cx in 0..2 {
        game.world
            .chunks
            .insert((cx, 0), Arc::new(Chunk::empty(cx, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(cx, 0))
            .unwrap();
    }
    for command in [
        "difficulty normal",
        "forceload add 0 0 31 15",
        "fill 0 99 0 31 99 15 warped_nylium",
        "tick freeze",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    // Two glass pens 16 blocks apart, each with a hoglin and a sturdy NoAI pig.
    for (x0, n) in [(0, 3), (16, 1)] {
        assert!(game.run_command(&format!("fill {} 100 4 {} 102 12 glass", x0 + 4, x0 + 10)));
        assert!(game.run_command(&format!("fill {} 100 5 {} 102 11 air", x0 + 5, x0 + 9)));
        assert!(game.run_command(&format!(
            "summon hoglin {}.5 100 8.5 {{PersistenceRequired:1b,IsImmuneToZombification:1b,Health:40f,UUID:[I;24723,0,0,{n}]}}",
            x0 + 6
        )));
        assert!(game.run_command(&format!(
            "summon pig {}.5 100 8.5 {{NoAI:1b,PersistenceRequired:1b,Silent:1b,attributes:[{{id:\"minecraft:max_health\",base:100d}}],Health:100f,UUID:[I;24723,0,0,{}]}}",
            x0 + 7,
            10 + n
        )));
    }
    game
}

fn health(game: &mut Game<'_>, uuid: &str) -> f32 {
    let target = game
        .target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap())
        .expect("the entity is in the level");
    match target {
        Target::Mob(id) => game.mobs.iter().find(|m| m.id == id).unwrap().health,
        other => panic!("{other:?} is not a mob"),
    }
}

fn step(game: &mut Game<'_>, ticks: u32) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_hoglin_keeps_attacking_its_attacker_beside_another_hit_hoglin() {
    let mut game = game();
    step(&mut game, 40);
    // The neighbour's hoglin first, then this one, in the same tick.
    assert!(game.run_command(&format!(
        "damage {NEAR_HOGLIN} 1 minecraft:mob_attack by {NEAR_PIG}"
    )));
    assert!(game.run_command(&format!("damage {HOGLIN} 1 minecraft:mob_attack by {PIG}")));
    step(&mut game, 5);
    let after_first = health(&mut game, PIG);
    assert!(
        after_first < 100.0,
        "the first retaliation hits ({after_first})"
    );
    let near_first = health(&mut game, NEAR_PIG);
    assert!(
        near_first < 100.0,
        "the neighbour retaliates ({near_first})"
    );
    // MeleeAttack's 40-tick cooldown: two more hits are due within 90 ticks.
    step(&mut game, 90);
    let near_later = health(&mut game, NEAR_PIG);
    assert!(
        near_later < near_first,
        "the neighbour hits again ({near_later})"
    );
    let later = health(&mut game, PIG);
    assert!(
        later < after_first,
        "this hoglin hits its attacker again ({later})"
    );
}

fn hurt_time(game: &mut Game<'_>, uuid: &str) -> i32 {
    let id = mob_id(game, uuid);
    game.mobs.iter().find(|m| m.id == id).unwrap().hurt_time
}

/// Vanilla 26.3's hit pen: the hoglin enters the fight activity at the end of
/// the tick after the hit (`HoglinAi.updateActivity` follows the brain), hits
/// in the next one, and again 41 ticks later. The pig ticks after the hoglin,
/// so it reads `HurtTime` 9 on the tick of each hit.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_hoglin_hits_from_its_second_fighting_tick_every_41_ticks() {
    let mut game = game();
    step(&mut game, 40);
    assert!(game.run_command(&format!("damage {HOGLIN} 1 minecraft:mob_attack by {PIG}")));
    let mut hits = Vec::new();
    for tick in 1..=90 {
        step(&mut game, 1);
        if hurt_time(&mut game, PIG) == 9 {
            hits.push(tick);
        }
    }
    assert_eq!(hits, [2, 43, 84]);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn deferred_stronger_hits_do_not_count_hurt_timers_down_twice() {
    use horizon_client::mobs::Damage;
    let mut game = game();
    let first = mob_id(&mut game, NEAR_HOGLIN);
    let second = mob_id(&mut game, HOGLIN);
    let target = mob_id(&mut game, PIG);
    game.creatures.ticked.extend([first, second, target]);
    game.creatures.deferred_hits = true;
    assert!(game.mob_hits_mob(target, 4.0, Damage::mob(first, [0.0; 3])));
    assert!(game.mob_hits_mob(target, 6.0, Damage::mob(second, [0.0; 3])));
    let victim = game.mobs.iter().find(|m| m.id == target).unwrap();
    assert_eq!((victim.hurt_time, victim.invulnerable), (9, 19));
    assert_eq!(victim.health, 94.0);

    // A stronger hit on the following tick also leaves the already-counted
    // timers alone rather than decrementing them again.
    let victim = game.mobs.iter_mut().find(|m| m.id == target).unwrap();
    victim.hurt_time = 8;
    victim.invulnerable = 18;
    assert!(game.mob_hits_mob(target, 8.0, Damage::mob(first, [0.0; 3])));
    let victim = game.mobs.iter().find(|m| m.id == target).unwrap();
    assert_eq!((victim.hurt_time, victim.invulnerable), (8, 18));
    assert_eq!(victim.health, 92.0);
}

/// Added controls (after the frozen baseline above).
fn mob_id(game: &mut Game<'_>, uuid: &str) -> u32 {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap()) {
        Some(Target::Mob(id)) => id,
        other => panic!("{other:?}"),
    }
}

/// `setAttackTargetIfCloserThanCurrent` on the receiver: pacified ignores
/// the broadcast; a nearer current target stays; a nearer (or equally near)
/// incoming one replaces a farther one; none current takes the new one.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_broadcast_target_replaces_only_a_farther_one() {
    use horizon_client::mobs::{Who, piglin::receive_attack_target};
    use horizon_core::geometry::Vec3;
    let mut game = game();
    let receiver = mob_id(&mut game, HOGLIN);
    let (near, far) = (mob_id(&mut game, PIG), mob_id(&mut game, NEAR_PIG));
    let place = |game: &Game<'_>, id: u32| game.mobs.iter().find(|m| m.id == id).unwrap().position;
    let (near_at, far_at) = (place(&game, near), place(&game, far));
    let position = |who: Who| match who {
        Who::Mob(id) if id == near => Some(near_at),
        Who::Mob(id) if id == far => Some(far_at),
        _ => None::<Vec3>,
    };
    let m = game.mobs.iter_mut().find(|m| m.id == receiver).unwrap();
    // No current target: the broadcast's is taken.
    m.ai.target = None;
    receive_attack_target(m, Who::Mob(far), position);
    assert_eq!(m.ai.target, Some(Who::Mob(far)), "no current target");
    // Pacified: nothing changes.
    m.ai.target = None;
    m.extra.ints[0] = 200;
    receive_attack_target(m, Who::Mob(near), position);
    assert_eq!(m.ai.target, None, "pacified");
    m.extra.ints[0] = 0;
    // A nearer current target stays.
    m.ai.target = Some(Who::Mob(near));
    receive_attack_target(m, Who::Mob(far), position);
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "nearer current");
    // A nearer incoming target replaces a farther current one.
    m.ai.target = Some(Who::Mob(far));
    receive_attack_target(m, Who::Mob(near), position);
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "nearer incoming");
    // A tie takes the incoming one (`getTargetNearestMe`: current only if strictly nearer).
    m.ai.target = Some(Who::Mob(far));
    receive_attack_target(m, Who::Mob(near), |_| Some(near_at));
    assert_eq!(m.ai.target, Some(Who::Mob(near)), "tie");
}

/// The broadcast's receivers are the sensed visible adult hoglins
/// (`NEAREST_VISIBLE_ADULT_HOGLINS`): not a baby, not one behind stone.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_sensed_receivers_are_visible_adult_hoglins() {
    let mut game = game();
    // Inside this hoglin's pen: an adult and a baby. Beyond a stone wall: an adult.
    assert!(game.run_command(
        "summon hoglin 24.5 100 10.5 {PersistenceRequired:1b,IsImmuneToZombification:1b,NoAI:1b,UUID:[I;24723,0,0,6]}"
    ));
    assert!(game.run_command(
        "summon hoglin 24.5 100 6.5 {PersistenceRequired:1b,IsImmuneToZombification:1b,Age:-24000,UUID:[I;24723,0,0,4]}"
    ));
    assert!(game.run_command("fill 27 100 0 27 104 15 stone"));
    assert!(game.run_command(
        "summon hoglin 29.5 100 8.5 {PersistenceRequired:1b,IsImmuneToZombification:1b,NoAI:1b,UUID:[I;24723,0,0,5]}"
    ));
    step(&mut game, 40);
    let receiver = mob_id(&mut game, HOGLIN);
    let near = mob_id(&mut game, "00006093-0000-0000-0000-000000000006");
    let baby = mob_id(&mut game, "00006093-0000-0000-0000-000000000004");
    let walled = mob_id(&mut game, "00006093-0000-0000-0000-000000000005");
    let adults = &game
        .mobs
        .iter()
        .find(|m| m.id == receiver)
        .unwrap()
        .extra
        .piglin
        .adults;
    assert!(
        adults.contains(&near),
        "the visible adult neighbour: {adults:?}"
    );
    assert!(!adults.contains(&baby), "not a baby: {adults:?}");
    assert!(
        !adults.contains(&walled),
        "not one behind stone: {adults:?}"
    );
}
