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

// Added controls (after the frozen baseline above): the two fields.
use horizon_world::nbt::Tag;

fn mob(game: &mut Game<'_>) -> usize {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(EDIT).unwrap()) {
        Some(super::commands::Target::Mob(id)) => {
            game.mobs.iter().position(|m| m.id == id).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

fn step(game: &mut Game<'_>, ticks: u32) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

/// The hurt cooldown and `invulnerable_time` are independent: a hit sets only
/// the cooldown; a loaded `invulnerable_time` sets only its own field, makes
/// the (NoAI) mob refuse a hit while above 0, counts down each stepped tick,
/// is saved only while above 0, and is not passed by a stronger hit, but is
/// by `/kill` (`#bypasses_invulnerability`).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_hurt_cooldown_and_invulnerable_time_are_separate_fields() {
    let mut game = game();
    let i = mob(&mut game);
    // A hit: the cooldown only.
    assert!(game.run_command(&format!("damage {EDIT} 2 minecraft:generic")));
    assert_eq!(
        (game.mobs[i].invulnerable, game.mobs[i].invulnerable_time),
        (20, 0),
        "a hit"
    );
    assert_eq!(
        game.mob_tree_nbt(i).get("invulnerable_time"),
        None,
        "the cooldown is not saved"
    );
    // A loaded value: its own field only; the cooldown stays as it was.
    assert!(game.run_command(&format!("data merge entity {EDIT} {{invulnerable_time:3}}")));
    let i = mob(&mut game);
    assert_eq!(
        (game.mobs[i].invulnerable, game.mobs[i].invulnerable_time),
        (20, 3),
        "both positive"
    );
    assert_eq!(
        game.mob_tree_nbt(i).get("invulnerable_time"),
        Some(&Tag::Int(3)),
        "saved while above 0"
    );
    // Both positive: even a stronger hit is refused (the cooldown alone would let 2 through).
    assert!(!game.run_command(&format!("damage {EDIT} 4 minecraft:generic")));
    assert_eq!(
        game.mobs[i].health, 18.0,
        "refused while invulnerable_time > 0"
    );
    // The NoAI mob's invulnerable_time counts down with the ticks, then ends.
    step(&mut game, 1);
    assert_eq!(
        (game.mobs[i].invulnerable, game.mobs[i].invulnerable_time),
        (19, 2),
        "one tick"
    );
    step(&mut game, 2);
    assert_eq!(game.mobs[i].invulnerable_time, 0, "expired");
    assert_eq!(
        game.mob_tree_nbt(i).get("invulnerable_time"),
        None,
        "not saved at 0"
    );
    // At 0 the cooldown rules alone: a stronger hit deals the difference.
    assert!(game.run_command(&format!("damage {EDIT} 4 minecraft:generic")));
    assert_eq!(game.mobs[i].health, 16.0, "the cooldown alone");
    // A bypassing source passes invulnerable_time.
    assert!(game.run_command(&format!(
        "data merge entity {EDIT} {{invulnerable_time:100}}"
    )));
    assert!(
        game.run_command(&format!("kill {EDIT}")),
        "#bypasses_invulnerability"
    );
}
