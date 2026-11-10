//! A mob named in a command reply carries `Entity.getDisplayName`'s hover and
//! insertion (#14220): `HoverEvent.ShowEntity` of its type, UUID and name, and
//! its UUID string to insert.
use super::*;
use crate::game::commands::Target;
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
    for command in ["forceload add 0 0", "fill 0 63 0 15 63 15 stone"] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_summoned_mobs_name_shows_the_entity_on_hover() {
    let mut game = game();
    assert!(game.run_command("summon minecraft:pig 4.5 64 4.5 {NoAI:1b}"));
    let id = game.mobs.last().unwrap().id;
    let uuid = horizon_client::save::uuid_text(game.persist.uuid_of(id).expect("a UUID"));
    let name = game.target_name(Target::Mob(id)).component();
    assert_eq!(name["translate"], "entity.minecraft.pig");
    assert_eq!(name["insertion"], uuid.as_str());
    let hover = &name["hover_event"];
    assert_eq!(hover["action"], "show_entity");
    assert_eq!(hover["id"], "minecraft:pig");
    assert_eq!(hover["uuid"], uuid.as_str());
    assert_eq!(hover["name"]["translate"], "entity.minecraft.pig");
    // On a team the name is the team's formatting of it, the hover on the whole.
    assert!(game.run_command("team add reds"));
    assert!(game.run_command("team modify reds color red"));
    assert!(game.run_command("team join reds @e[type=minecraft:pig]"));
    let name = game.target_name(Target::Mob(id)).component();
    assert_eq!(name["color"], "red");
    assert_eq!(name["insertion"], uuid.as_str());
    assert_eq!(name["hover_event"]["uuid"], uuid.as_str());
}
