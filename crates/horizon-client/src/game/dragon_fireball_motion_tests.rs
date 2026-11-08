//! A summoned dragon fireball flies (H127): `DragonFireball` is an
//! `AbstractHurtingProjectile` that moves by its motion and accelerates by its
//! `acceleration_power` every tick, as a summoned `fireball` does
//! (tools/test_dragon_fireball_motion_parity.py measures the same on vanilla 26.3).
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

fn pos(game: &mut Game<'_>, uuid: &str) -> String {
    game.chat_capture = Some(Vec::new());
    assert!(game.run_command(&format!("data get entity {uuid} Pos")));
    let said = game.chat_capture.take().unwrap_or_default();
    said.iter().map(horizon_client::text::Text::plain).collect()
}

const DRAGON: &str = "0000d127-0000-0000-0000-000000000001";
const FIREBALL: &str = "0000d127-0000-0000-0000-000000000002";

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_summoned_dragon_fireball_moves_like_a_fireball() {
    let mut game = game();
    for command in [
        "forceload add 0 0",
        "tick freeze",
        "summon dragon_fireball 4.5 120 4.5 {Motion:[0.1d,0d,0d],acceleration_power:0.1d,UUID:[I;53543,0,0,1]}",
        "summon fireball 4.5 120 10.5 {Motion:[0.1d,0d,0d],acceleration_power:0.1d,UUID:[I;53543,0,0,2]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    let (dragon, fireball) = (pos(&mut game, DRAGON), pos(&mut game, FIREBALL));
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
    let fireball_moved = pos(&mut game, FIREBALL);
    assert_ne!(fireball_moved, fireball, "the control fireball flies");
    assert!(
        fireball_moved.contains("4.689999997615814d"),
        "{fireball_moved}"
    );
    let dragon_moved = pos(&mut game, DRAGON);
    assert_ne!(dragon_moved, dragon, "the dragon fireball flies too");
}
