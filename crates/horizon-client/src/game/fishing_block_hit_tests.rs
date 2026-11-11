//! A flying bobber that strikes a block calls its `onProjectileHit`
//! (`FishingHook.onHitBlock` -> `Projectile.onHitBlock`, #12936): a target
//! block is powered and a bell rings, both credited to the rod's owner.
use super::super::*;
use super::Hook;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;

fn game() -> Game<'static> {
    let root = PathBuf::from(
        std::env::var_os("HORIZON_TEST_RESOURCES")
            .expect("set HORIZON_TEST_RESOURCES to prepared resources"),
    );
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
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
    for command in ["forceload add 0 0", "fill 0 63 0 15 63 15 stone"] {
        assert!(game.run_command(command), "{command}");
    }
    game.player = Some(PlayerMotion::new(&game.motion, Vec3::new(0.5, 64.0, 8.5)).unwrap());
    game.selected = 0;
    game.set_mode_id(0);
    game.inventory[0] = Some(ItemStack::new("minecraft:fishing_rod", 1));
    game
}

/// A bobber at `from` flying east at 0.5 a tick, ticked until the custom
/// statistic `stat` counts its first hit (or 20 ticks).
fn cast_east(game: &mut Game<'_>, from: Vec3, stat: &str) {
    let mut hook = Hook::remote(7, from);
    hook.velocity = Vec3::new(0.5, 0.0, 0.0);
    game.hook = Some(hook);
    for _ in 0..20 {
        game.tick_fishing();
        if game.stats.get("custom", stat) > 0 {
            break;
        }
    }
}

fn property(game: &Game<'_>, at: [i32; 3], key: &str) -> Option<String> {
    let state = game.world.block(at[0], at[1], at[2]);
    game.pack
        .assets
        .registry
        .state(state)
        .and_then(|s| s.properties.get(key).cloned())
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_bobber_powers_a_target_block_and_counts_the_hit() {
    let mut game = game();
    assert!(game.run_command("setblock 4 64 2 minecraft:target"));
    // Nothing under its flight: the target is the first block it meets.
    assert!(game.run_command("fill 0 63 0 3 63 4 minecraft:air"));
    assert_eq!(property(&game, [4, 64, 2], "power").as_deref(), Some("0"));
    // checkCollision's ray (the bobber's centre) comes before move(): from 2.7 its
    // third tick's ray, 3.66 to 4.08, crosses the face before the box can stop
    // it at 3.875.
    cast_east(&mut game, Vec3::new(2.7, 64.6, 2.5), "minecraft:target_hit");
    let power: u32 = property(&game, [4, 64, 2], "power")
        .and_then(|p| p.parse().ok())
        .expect("a power");
    assert!(power > 0, "the target stayed unpowered");
    assert_eq!(game.stats.get("custom", "minecraft:target_hit"), 1);
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_bobber_rings_a_floor_bell_on_its_side() {
    let mut game = game();
    assert!(game.run_command("setblock 4 64 5 minecraft:bell[attachment=floor,facing=east]"));
    // Struck on the west face (the bell's axis), low enough for isProperHit.
    cast_east(&mut game, Vec3::new(2.5, 64.6, 5.5), "minecraft:bell_ring");
    assert_eq!(game.stats.get("custom", "minecraft:bell_ring"), 1);
}
