//! Commands see the leash knot a restored fence leash ties
//! (`LeashFenceKnotEntity.getOrCreateKnot`), as they see an explicitly
//! summoned one (tools/test_knot_command_view_parity.py measures the same on
//! vanilla 26.3). No server tracks the level here: the knot must exist for
//! commands whether or not anyone observes it.
use super::*;
use horizon_client::{
    chunk::Chunk,
    mobs::leash::{Holder, Pending, knot_state},
};
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const PIG: &str = "0000609a-0000-0000-0000-000000000001";
const EXPLICIT: &str = "0000609a-0000-0000-0000-000000000002";
const NATURAL: &str = "@e[type=minecraft:leash_knot,x=8.5,y=201.375,z=0.5,distance=..0.001]";

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
    for x in 0..3 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(x, 0))
            .unwrap();
    }
    for command in [
        "forceload add 0 0 47 15",
        "fill 0 200 0 47 200 15 stone",
        "tick freeze",
        "setblock 8 201 0 oak_fence",
        "setblock 40 201 0 oak_fence",
        "summon leash_knot 40.5 201.375 0.5 {UUID:[I;24730,0,0,2]}",
        "summon pig 16.5 201 0.5 {NoAI:1b,PersistenceRequired:1b,Health:10f,Rotation:[0f,0f],Motion:[0d,0d,0d],leash:[I;8,201,0],UUID:[I;24730,0,0,1]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn pig(game: &mut Game<'_>) -> usize {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(PIG).unwrap()) {
        Some(super::commands::Target::Mob(id)) => {
            game.mobs.iter().position(|m| m.id == id).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn commands_see_the_knot_a_restored_fence_leash_ties() {
    let mut game = game();
    assert!(game.lan.is_none(), "no server tracks the level");
    // Before restoration: only the pending tag, no knot at the fence; the
    // explicit knot is found.
    let i = pig(&mut game);
    assert_eq!(
        game.mobs[i].extra.leash_pending,
        Some(Pending::Knot([8, 201, 0]))
    );
    assert_eq!(game.mobs[i].extra.leash, None);
    assert!(
        !game.run_command(&format!("execute if entity {NATURAL}")),
        "no knot yet"
    );
    assert!(
        game.run_command(&format!("execute if entity {EXPLICIT}")),
        "the explicit knot"
    );
    // One tick: the real restoration ties the fence's knot.
    assert!(game.run_command("tick step 1"));
    game.tick_server(false).unwrap();
    let i = pig(&mut game);
    let (leash, pending) = (game.mobs[i].extra.leash, game.mobs[i].extra.leash_pending);
    let at = knot_state([8, 201, 0]).position;
    eprintln!("tick-1 binding: leash={leash:?} pending={pending:?} knot at {at:?}");
    assert_eq!(
        leash,
        Some(Holder::Knot([8, 201, 0])),
        "the live holder is the fence's knot"
    );
    assert_eq!(pending, None, "restored");
    assert_eq!([at.x, at.y, at.z], [8.5, 201.375, 0.5]);
    assert!(game.run_command("execute if block 8 201 0 oak_fence"));
    assert!(
        game.run_command(&format!("execute if entity {EXPLICIT}")),
        "the explicit knot stays"
    );
    // Commands see that knot, as vanilla's.
    assert!(
        game.run_command(&format!("execute if entity {NATURAL}")),
        "commands see the restored knot"
    );
}

/// `/kill`, `/tp`, `/rotate`, `/spreadplayers` and `/tag add|remove` leave a natural knot out
/// rather than failing for every target, and find no entity when only knots matched.
#[test]
fn moving_commands_leave_natural_knots_out() {
    use super::commands::{Target, without_knots};
    assert_eq!(
        without_knots(vec![Target::Knot(3), Target::Player]).unwrap(),
        vec![Target::Player]
    );
    assert!(without_knots(vec![Target::Knot(3)]).is_err());
    assert!(without_knots(Vec::new()).unwrap().is_empty());
}
