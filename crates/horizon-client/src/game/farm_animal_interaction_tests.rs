//! Milking a farm animal and shearing a sheep or mooshroom by hand: the milk
//! pitch is fixed at 1.0 (`AbstractCow.mobInteract`, `Goat.mobInteract`,
//! `MushroomCow.mobInteract`) and shearing posts `GameEvent.SHEAR` with the
//! player (`Sheep.mobInteract`, `MushroomCow.mobInteract`).
use super::super::*;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::{Mob, Who};
use horizon_client::motion::PlayerMotion;
use horizon_core::{geometry::Vec3, position::ChunkPos};
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
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    game.screen = Screen::Playing;
    game.selected = 0;
    game.set_mode_id(0);
    game
}

fn spawn(game: &mut Game<'_>, kind: &str, id: u32, at: [f64; 3]) {
    game.mobs.clear();
    let kind = game.res.mob_types().get(kind).unwrap().clone();
    game.mobs
        .push(Mob::new(id, kind, Vec3::new(at[0], at[1], at[2]), 0.0));
}

fn target(id: u32) -> Option<InteractTarget> {
    Some(InteractTarget {
        id: id as i32,
        location: [0.0, 0.0, 0.0],
    })
}

fn take_shear_events() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
    super::TEST_POSTED_EVENTS.with(|events| {
        std::mem::take(events.borrow_mut().as_mut().unwrap())
            .into_iter()
            .filter(|event| event.0 == "minecraft:shear")
            .collect()
    })
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn milk_sounds_carry_pitch_one_and_draw_no_level_random() {
    let mut game = game();
    for (kind, item, sound) in [
        (
            "minecraft:cow",
            "minecraft:bucket",
            "minecraft:entity.cow.milk",
        ),
        (
            "minecraft:goat",
            "minecraft:bucket",
            "minecraft:entity.goat.milk",
        ),
        (
            "minecraft:mooshroom",
            "minecraft:bowl",
            "minecraft:entity.mooshroom.milk",
        ),
    ] {
        spawn(&mut game, kind, 7, [2.5, 64.0, 2.5]);
        game.inventory[0] = Some(ItemStack::new(item, 1));
        game.audio.capture = Some(Vec::new());
        let before = game.world.random.state();
        assert!(game.interact_mob(target(7)).unwrap(), "{kind}");
        let sounds = game.audio.capture.take().unwrap();
        let heard: Vec<_> = sounds.iter().filter(|s| s.event == sound).collect();
        assert_eq!(heard.len(), 1, "{kind}: {sounds:?}");
        assert_eq!(heard[0].pitch, 1.0, "{kind} milk pitch");
        assert_eq!(
            game.world.random.state(),
            before,
            "{kind} milk drew the level random"
        );
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn shearing_by_hand_posts_the_shear_game_event() {
    let mut game = game();
    game.inventory[0] = Some(ItemStack::new("minecraft:shears", 1));
    super::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
    for (kind, id, at) in [
        ("minecraft:sheep", 11, [1.5, 64.0, 2.5]),
        ("minecraft:mooshroom", 12, [2.5, 64.0, 2.5]),
        ("minecraft:snow_golem", 13, [3.5, 64.0, 2.5]),
    ] {
        spawn(&mut game, kind, id, at);
        assert!(game.interact_mob(target(id)).unwrap(), "{kind}");
        assert_eq!(
            take_shear_events(),
            vec![("minecraft:shear", at, Some(Who::Player))],
            "{kind}"
        );
    }
    super::TEST_POSTED_EVENTS.with(|events| *events.borrow_mut() = None);
}
