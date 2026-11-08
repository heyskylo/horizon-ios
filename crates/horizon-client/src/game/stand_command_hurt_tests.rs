//! `/damage` on an armor stand: `ArmorStand.hurtServer` returns false after
//! an explosion has broken the stand (its equipment pops, then `kill`), after
//! `in_fire` has set it on fire and after `on_fire` has burnt 4 health off a
//! burning one, so the command fails while the stand changes
//! (tools/test_stand_command_hurt_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

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
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    game
}

/// What a command says, and whether it succeeded.
fn say(game: &mut Game<'_>, command: &str) -> (bool, String) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (
        ok,
        said.iter().map(horizon_client::text::Text::plain).collect(),
    )
}

const STAND: &str = "00000ad5-0000-0000-0000-000000000001";

fn summon(game: &mut Game<'_>, extra: &str) {
    assert!(game.run_command(&format!(
        "summon armor_stand 2.5 100 8.5 {{NoGravity:1b,UUID:[I;2773,0,0,1]{extra}}}"
    )));
    assert!(game.run_command(&format!("execute if entity {STAND}")));
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_blast_from_damage_breaks_the_stand_though_the_command_fails() {
    let mut game = game();
    summon(
        &mut game,
        r#",equipment:{head:{id:"minecraft:diamond_helmet",count:1}}"#,
    );
    let (ok, said) = say(&mut game, &format!("damage {STAND} 1 minecraft:explosion"));
    assert!(!ok, "hurtServer returns false: {said}");
    assert!(said.contains("invulnerable"), "{said}");
    assert!(
        !game.run_command(&format!("execute if entity {STAND}")),
        "the blast kills the stand"
    );
    assert!(
        game.run_command(
            "execute if entity @e[type=item,nbt={Item:{id:\"minecraft:diamond_helmet\"}}]"
        ),
        "its helmet pops above its block"
    );
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn fire_from_damage_lights_or_burns_the_stand_though_the_command_fails() {
    let mut game = game();
    summon(&mut game, "");
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:in_fire")));
    let (_, fire) = say(&mut game, &format!("data get entity {STAND} Fire"));
    assert!(
        fire.ends_with(": 100s"),
        "in_fire lights it for 5 s: {fire}"
    );
    // Burning, on_fire takes 4 health.
    assert!(!game.run_command(&format!("damage {STAND} 1 minecraft:on_fire")));
    let (_, health) = say(&mut game, &format!("data get entity {STAND} Health"));
    assert!(health.ends_with(": 16.0f"), "{health}");
}
