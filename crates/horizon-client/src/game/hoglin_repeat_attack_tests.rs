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
