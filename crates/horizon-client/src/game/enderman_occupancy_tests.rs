//! `EndermanLeaveBlockGoal.canPlaceBlock`'s `level.getEntities(this.enderman,
//! cube)`: every entity but the enderman blocks the cube, not just mobs.
use super::super::{Audio, Game, Options, Progress, Settings, execution_role, load_resources};
use horizon_client::{chunk::Chunk, mobs::Mob, motion::PlayerMotion};
use horizon_core::geometry::Vec3;
use std::sync::Arc;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_carried_block_is_not_placed_into_an_entity() {
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
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
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
    assert!(game.run_command("fill 0 99 0 15 99 15 stone"));
    let registry = game.pack.assets.registry;
    let (air, stone) = (
        registry.default_state("minecraft:air").unwrap(),
        registry.default_state("minecraft:stone").unwrap(),
    );
    let kind = game
        .res
        .mob_types()
        .get("minecraft:enderman")
        .unwrap()
        .clone();
    let summons: [(&str, Option<&str>); 4] = [
        ("nothing", None),
        (
            "a dropped item",
            Some("summon minecraft:item 5.5 100 5.5 {Item:{id:\"minecraft:stick\",count:1}}"),
        ),
        (
            "an armor stand",
            Some("summon minecraft:armor_stand 5.5 100 5.5"),
        ),
        ("the player", None),
    ];
    for (what, summon) in summons {
        game.dropped.clear();
        game.armor_stands.clear();
        game.player = (what == "the player")
            .then(|| PlayerMotion::new(game.motion, Vec3::new(5.5, 100.0, 5.5)).unwrap());
        if let Some(command) = summon {
            assert!(game.run_command(command), "{what}");
        }
        game.world.set(5, 100, 5, air).unwrap();
        let mut mob = Mob::new(900, kind.clone(), Vec3::new(9.5, 100.0, 9.5), 0.0);
        mob.extra.carried = Some(stone);
        game.mobs.clear();
        game.mobs.push(mob);
        game.enderman_place_block(900, [5, 100, 5], stone, &[])
            .unwrap();
        let placed = game.world.block(5, 100, 5) == stone;
        assert_eq!(placed, what == "nothing", "{what}");
        assert_eq!(game.mobs[0].extra.carried.is_none(), placed, "{what}");
    }
}
