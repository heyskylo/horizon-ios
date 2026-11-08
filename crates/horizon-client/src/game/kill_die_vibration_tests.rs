//! `/kill` on a mob: `LivingEntity.kill` hurts it with `genericKill`, so
//! `LivingEntity.die` runs before the command returns and posts
//! `gameEvent(ENTITY_DIE)` at the mob (tools/test_kill_die_vibration_parity.py
//! measures the same on vanilla 26.3: the sensor's listener holds the event
//! before any world tick).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::Who;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

pub(super) const PIG: &str = "00000251-0000-0000-0000-000000000001";

pub(super) fn game() -> Game<'static> {
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
    // The sensor joins the listeners through the level's own index.
    game.sculk_prepare();
    assert!(game.run_command("setblock 8 100 8 sculk_sensor"));
    game.sculk_sync_index();
    assert!(game.run_command(
        "summon pig 5.5 100 8.5 {NoAI:1b,NoGravity:1b,PersistenceRequired:1b,UUID:[I;593,0,0,1]}"
    ));
    game
}

/// Runs `command`: whether it succeeded and what it said.
pub(super) fn run(game: &mut Game<'_>, command: &str) -> (bool, Vec<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (
        ok,
        said.iter().map(horizon_client::text::Text::plain).collect(),
    )
}

/// The `entity_die` events posted while `f` runs.
pub(super) fn deaths(
    game: &mut Game<'_>,
    f: impl FnOnce(&mut Game<'_>),
) -> Vec<([f64; 3], Option<Who>)> {
    super::sculk::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    f(game);
    super::sculk::TEST_POSTED_EVENTS.with(|events| {
        events
            .borrow_mut()
            .take()
            .unwrap()
            .into_iter()
            .filter(|e| e.0 == "minecraft:entity_die")
            .map(|e| (e.1, e.2))
            .collect()
    })
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_killed_mob_posts_its_death_before_the_command_returns() {
    let mut game = game();
    let pig = game
        .mobs
        .iter()
        .find(|m| m.species() == horizon_client::mobs::Species::Pig)
        .unwrap()
        .id;
    let time = game.world.game_time;
    let (_, said) = run(&mut game, "data get block 8 100 8 listener");
    assert!(
        said.concat().contains("{tick: -1L}"),
        "nothing selected yet: {said:?}"
    );
    // A kill that finds no one posts nothing.
    let none = deaths(&mut game, |g| {
        assert!(!run(g, "kill @e[type=pig,tag=h251_none]").0);
    });
    assert_eq!(none, []);
    let posted = deaths(&mut game, |g| {
        assert_eq!(
            run(g, &format!("kill {PIG}")),
            (true, vec!["Killed Pig".to_owned()])
        );
    });
    assert_eq!(game.world.game_time, time, "no world tick ran");
    assert_eq!(
        posted,
        [([5.5, 100.0, 8.5], Some(Who::Mob(pig)))],
        "one death, by the pig"
    );
    assert_eq!(game.mobs.iter().find(|m| m.id == pig).unwrap().health, 0.0);
    // The sensor's listener holds it at the pig, before any tick.
    let (_, said) = run(&mut game, "data get block 8 100 8 listener.selector.event");
    let said = said.concat();
    assert!(said.contains("minecraft:entity_die"), "{said}");
    assert!(said.contains("[5.5d, 100.0d, 8.5d]"), "{said}");
    // Killed again: no second hit, no second event.
    let again = deaths(&mut game, |g| assert!(run(g, &format!("kill {PIG}")).0));
    assert_eq!(again, []);
    // The next tick's death phase posts no second event; the body stays.
    let later = deaths(&mut game, |g| g.tick_creature_entities().unwrap());
    assert_eq!(later, [], "no next-tick duplicate");
    assert!(
        game.mobs.iter().any(|m| m.id == pig && !m.removed),
        "the body stays"
    );
    // The rest of the death still runs once: the pig's loot is on the ground.
    let porkchops = |g: &Game<'_>| {
        g.dropped
            .iter()
            .filter(|i| &*i.stack.item == "minecraft:porkchop")
            .count()
    };
    assert_eq!(porkchops(&game), 1, "its loot dropped");
    let _ = deaths(&mut game, |g| g.tick_creature_entities().unwrap());
    assert_eq!(porkchops(&game), 1, "and only once");
}
