use super::*;
use crate::game::{Audio, Options, Progress, Settings, load_resources};
use horizon_client::motion::PlayerMotion;

fn with_game(test: impl FnOnce(&mut Game<'_>)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    options.game_dir = Some(
        std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
            .join(format!("h182-{:?}", std::thread::current().id())),
    );
    // A fixed name: no generated horizon-player.txt in the working directory.
    options.player_name = Some("H182".into());
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
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
    assert!(game.run_command("forceload add 0 0"));
    assert!(game.run_command("difficulty normal"));
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
    game.creative = true;
    test(&mut game);
}

/// Expected values from the official 26.3 server: a delay of 5 is gone
/// after the fifth tick and a delay of 0 after the first; a tame and a
/// persistent llama stay.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
fn a_trader_llama_leaves_the_level_in_the_tick_its_delay_runs_out() {
    with_game(|game| {
        for (n, extra) in [
            (1, "DespawnDelay:5"),
            (2, "DespawnDelay:0"),
            (3, "DespawnDelay:5,Tame:1b"),
            (4, "DespawnDelay:5,PersistenceRequired:1b"),
        ] {
            assert!(game.run_command(&format!(
                    "summon minecraft:trader_llama {}.5 200 8.5 {{UUID:[I;0,182,0,{n}],NoAI:0b,Silent:1b,{extra}}}",
                    2 + 3 * n
                )));
        }
        let present = |game: &Game<'_>| -> [bool; 4] {
            [1, 2, 3, 4].map(|n| {
                game.mobs.iter().any(|m| {
                    game.persist.uuid_of(m.id) == Some([0, 182, 0, n])
                        && &*m.kind.id == "minecraft:trader_llama"
                })
            })
        };
        let mut failures = Vec::new();
        for tick in 1..=6 {
            game.tick_world(true).unwrap();
            let want = [tick < 5, false, true, true];
            let got = present(game);
            if got != want {
                failures.push(format!(
                    "after tick {tick}: in the list {got:?}, expected {want:?}"
                ));
            }
        }
        assert!(failures.is_empty(), "trader llama discard: {failures:#?}");
    });
}
