//! `Wolf.mobInteract` for the owner of a tame wolf (#12939): an item of
//! `#wolf_collar_dyes` only ever dyes the collar, so one of the colour the collar
//! already has does nothing, while another item still makes it sit or stand.
use super::*;
use horizon_client::chunk::Chunk;
use horizon_client::mobs::Mob;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const WOLF: u32 = 50;

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
    assert!(game.run_command("fill 0 63 0 15 63 15 stone"));
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
    game.screen = Screen::Playing;
    game.selected = 0;
    game.set_mode_id(0);
    let kind = game.res.mob_types().get("minecraft:wolf").unwrap().clone();
    let mut wolf = Mob::new(WOLF, kind, Vec3::new(0.5, 64.0, 2.0), 0.0);
    horizon_client::mobs::tame::set_tame(&mut wolf, true);
    wolf.extra.owner = Some(game.acting_player_uuid());
    wolf.on_ground = true;
    game.mobs.push(wolf);
    game
}

fn use_on_wolf(game: &mut Game<'_>, item: &str) -> (bool, u8, u32) {
    game.inventory[0] = Some(ItemStack::new(item, 1));
    let target = Some(InteractTarget {
        id: WOLF as i32,
        location: [0.0, 0.5, 0.0],
    });
    game.interact_mob(target).unwrap();
    let wolf = game.mobs.iter().find(|m| m.id == WOLF).unwrap();
    let left = game.inventory[0].as_ref().map_or(0, |s| s.count);
    (wolf.extra.sitting, wolf.extra.color, left)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_collars_own_dye_leaves_a_wolf_as_it_was() {
    let mut game = game();
    let red = game.mobs[0].extra.color;
    assert_eq!(
        horizon_client::mobs::render::DYE_NAMES[usize::from(red)],
        "red",
        "a tamed wolf's collar starts red"
    );
    // The colour it already has: nothing happens, and the dye stays.
    assert_eq!(use_on_wolf(&mut game, "minecraft:red_dye"), (false, red, 1));
    // Another colour dyes the collar and is used up; the wolf keeps standing.
    let (sitting, color, left) = use_on_wolf(&mut game, "minecraft:blue_dye");
    assert!(!sitting);
    assert_eq!(
        horizon_client::mobs::render::DYE_NAMES[usize::from(color)],
        "blue"
    );
    assert_eq!(left, 0);
    // Any other item still makes the owner's wolf sit.
    assert!(use_on_wolf(&mut game, "minecraft:stick").0);
    // And the collar's own dye leaves a sitting wolf sitting.
    assert!(use_on_wolf(&mut game, "minecraft:blue_dye").0);
}
