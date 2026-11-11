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
            .join(format!("h248-{:?}", std::thread::current().id())),
    );
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
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
    game.creative = true;
    test(&mut game);
}
/// The pig with `[I;0,248,0,n]`: its health and its hurt cooldown state
/// (`invulnerableTime`, `lastHurt`).
fn pig(game: &Game<'_>, n: i32) -> (f32, i32, f32) {
    let m = game
        .mobs
        .iter()
        .find(|m| game.persist.uuid_of(m.id) == Some([0, 248, 0, n]))
        .unwrap();
    (m.health, m.invulnerable, m.last_hurt)
}
fn summon(game: &mut Game<'_>, n: i32, x: i32) -> String {
    assert!(game.run_command(&format!(
            "summon minecraft:pig {x}.5 200 4.5 {{UUID:[I;0,248,0,{n}],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,attributes:[{{id:\"minecraft:max_health\",base:40.0d}}],Health:40f}}"
        )));
    format!("@e[type=minecraft:pig,nbt={{UUID:[I;0,248,0,{n}]}},limit=1]")
}

/// Expected values from the official 26.3 server (LivingEntity.hurtServer):
/// inside the cooldown a hit is compared, and lastHurt kept, by its amount
/// before armor and Resistance; actuallyHurt reduces only the difference.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
fn the_hurt_cooldown_compares_the_incoming_damage_before_its_reductions() {
    with_game(|game| {
        let full = summon(game, 1, 2);
        assert!(game.run_command(&format!(
            "effect give {full} minecraft:resistance 30 4 true"
        )));
        let armored = summon(game, 2, 8);
        assert!(game.run_command(&format!("attribute {armored} minecraft:armor base set 10")));
        let mut failures = Vec::new();
        let check = |failures: &mut Vec<String>,
                     label: &str,
                     got: (f32, i32, f32),
                     want: (f32, i32, f32)| {
            if (got.0 - want.0).abs() >= 1e-4 || got.1 != want.1 || got.2 != want.2 {
                failures.push(format!(
                    "{label}: (health, invulnerable, last_hurt) {got:?}, expected {want:?}"
                ));
            }
        };
        // Full Resistance: every hit loses no health.
        let accepted = game.run_command(&format!("damage {full} 1 minecraft:generic"));
        check(
            &mut failures,
            &format!("full 1 accepted {accepted}"),
            pig(game, 1),
            (40.0, 20, 1.0),
        );
        let accepted = game.run_command(&format!("damage {full} 2 minecraft:generic"));
        if !accepted {
            failures.push("full: the stronger hit 2 was refused".into());
        }
        check(&mut failures, "full 2", pig(game, 1), (40.0, 20, 2.0));
        let accepted = game.run_command(&format!("damage {full} 2 minecraft:generic"));
        if accepted {
            failures.push("full: the equal hit 2 was taken".into());
        }
        check(&mut failures, "full equal 2", pig(game, 1), (40.0, 20, 2.0));
        // Armor 10 (cactus is not #bypasses_armor): 4 then 8 deal 2.72 each.
        assert!(game.run_command(&format!("damage {armored} 4 minecraft:cactus")));
        check(&mut failures, "armor 4", pig(game, 2), (37.28, 20, 4.0));
        assert!(game.run_command(&format!("damage {armored} 8 minecraft:cactus")));
        check(&mut failures, "armor 8", pig(game, 2), (34.56, 20, 8.0));
        assert!(failures.is_empty(), "hurt cooldown: {failures:#?}");
    });
}
