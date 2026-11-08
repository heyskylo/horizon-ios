//! A mob's hurt cooldown (`LivingEntity.damageCooldownTime`) is not saved, and
//! a `/data` edit keeps it; `invulnerable_time` is `Entity.invulnerableTime`,
//! another field (tools/test_invulnerable_time_parity.py measures the same on
//! vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const EDIT: &str = "00006095-0000-0000-0000-000000000002";

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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    for command in [
        "forceload add 0 0",
        "fill 0 99 0 15 99 15 stone",
        "tick freeze",
        "summon pig 8.5 100 4.5 {NoAI:1b,PersistenceRequired:1b,Health:20f,attributes:[{id:\"minecraft:max_health\",base:20d}],UUID:[I;24725,0,0,2]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn health(game: &mut Game<'_>) -> f32 {
    let target = game
        .target_with_uuid(horizon_client::save::uuid_from_text(EDIT).unwrap())
        .unwrap();
    match target {
        super::commands::Target::Mob(id) => game.mobs.iter().find(|m| m.id == id).unwrap().health,
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_hurt_cooldown_survives_an_edit_and_is_not_saved() {
    let mut game = game();
    assert!(game.run_command(&format!("damage {EDIT} 2 minecraft:generic")));
    assert_eq!(health(&mut game), 18.0, "the hit");
    for _ in 0..4 {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
    // An unrelated edit during the cooldown keeps it: an equal hit does nothing,
    // a stronger one deals only the difference.
    assert!(game.run_command(&format!(
        "data merge entity {EDIT} {{CustomName:\"edited\"}}"
    )));
    let _ = game.run_command(&format!("damage {EDIT} 2 minecraft:generic"));
    assert_eq!(health(&mut game), 18.0, "an equal hit during the cooldown");
    assert!(game.run_command(&format!("damage {EDIT} 4 minecraft:generic")));
    assert_eq!(
        health(&mut game),
        16.0,
        "a stronger hit deals the difference"
    );
    // The cooldown is not part of the saved entity.
    assert!(
        !game.run_command(&format!("execute if data entity {EDIT} invulnerable_time")),
        "a hurt cooldown is not saved as invulnerable_time"
    );
}
