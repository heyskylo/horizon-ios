//! A mob hurt by another mob saves who hurt it: `last_hurt_by_mob` (the
//! attacker's UUID) and `ticks_since_last_hurt_by_mob`, while the record is
//! younger than 100 ticks (`LivingEntity.baseTick`); a value read from saved
//! data ages the same way (tools/test_last_hurt_by_mob_parity.py measures the
//! same on vanilla 26.3).
use super::*;
use horizon_client::chunk::Chunk;
use horizon_core::position::ChunkPos;
use std::sync::Arc;

const VICTIM: &str = "00006094-0000-0000-0000-000000000001";
const A: &str = "00006094-0000-0000-0000-000000000002";
const B: &str = "00006094-0000-0000-0000-000000000003";
const ECHO: &str = "00006094-0000-0000-0000-000000000004";

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
    for command in [
        "forceload add 0 0",
        "fill 0 99 0 15 99 15 stone",
        "tick freeze",
        "summon pig 4.5 100 4.5 {NoAI:1b,PersistenceRequired:1b,Health:10f,UUID:[I;24724,0,0,1]}",
        "summon cow 8.5 100 4.5 {NoAI:1b,PersistenceRequired:1b,UUID:[I;24724,0,0,2]}",
        "summon cow 12.5 100 4.5 {NoAI:1b,PersistenceRequired:1b,UUID:[I;24724,0,0,3]}",
        "summon pig 4.5 100 12.5 {NoAI:1b,PersistenceRequired:1b,UUID:[I;24724,0,0,4],last_hurt_by_mob:[I;24724,0,0,2]}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

fn step(game: &mut Game<'_>, ticks: u32) {
    for _ in 0..ticks {
        assert!(game.run_command("tick step 1"));
        game.tick_server(false).unwrap();
    }
}

/// The saved `last_hurt_by_mob` and `ticks_since_last_hurt_by_mob` of `who`.
fn saved(game: &mut Game<'_>, who: &str) -> (bool, bool, bool) {
    let has = |game: &mut Game<'_>, path: &str| {
        game.run_command(&format!("execute if data entity {who} {path}"))
    };
    let a = has(game, "{last_hurt_by_mob:[I;24724,0,0,2]}");
    let b = has(game, "{last_hurt_by_mob:[I;24724,0,0,3]}");
    let ticks = has(game, "ticks_since_last_hurt_by_mob");
    (a, b, ticks)
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mob_saves_the_mob_that_hurt_it_for_100_ticks() {
    let mut game = game();
    assert_eq!(
        saved(&mut game, VICTIM),
        (false, false, false),
        "no hit yet"
    );
    assert!(game.run_command(&format!("damage {VICTIM} 1 minecraft:generic")));
    assert_eq!(saved(&mut game, VICTIM), (false, false, false), "no entity");
    step(&mut game, 20);
    assert!(game.run_command(&format!("damage {VICTIM} 1 minecraft:mob_attack by {A}")));
    assert_eq!(saved(&mut game, VICTIM), (true, false, true), "hurt by A");
    assert!(game.run_command(&format!(
        "execute if data entity {VICTIM} {{ticks_since_last_hurt_by_mob:0}}"
    )));
    step(&mut game, 10);
    assert!(game.run_command(&format!(
        "execute if data entity {VICTIM} {{ticks_since_last_hurt_by_mob:10}}"
    )));
    step(&mut game, 10);
    assert!(game.run_command(&format!("damage {VICTIM} 1 minecraft:mob_attack by {B}")));
    assert_eq!(saved(&mut game, VICTIM), (false, true, true), "hurt by B");
    step(&mut game, 110);
    assert_eq!(
        saved(&mut game, VICTIM),
        (false, false, false),
        "past 100 ticks"
    );
    // A value read from the summon's data ages and ends too.
    assert_eq!(
        saved(&mut game, ECHO),
        (false, false, false),
        "the read value ended"
    );
}

// Added controls (after the frozen baseline above): the save-time projection.
use horizon_world::nbt::Tag;

fn index(game: &mut Game<'_>, uuid: &str) -> usize {
    match game.target_with_uuid(horizon_client::save::uuid_from_text(uuid).unwrap()) {
        Some(super::commands::Target::Mob(id)) => {
            game.mobs.iter().position(|m| m.id == id).unwrap()
        }
        other => panic!("{other:?}"),
    }
}

fn fields(game: &mut Game<'_>, i: usize) -> (Option<Tag>, Option<Tag>) {
    let tag = game.mob_tree_nbt(i);
    (
        tag.get("last_hurt_by_mob").cloned(),
        tag.get("ticks_since_last_hurt_by_mob").cloned(),
    )
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_saved_hurt_by_mob_follows_the_live_record_only() {
    use horizon_client::mobs::Who;
    let mut game = game();
    let victim = index(&mut game, VICTIM);
    let a_index = index(&mut game, A);
    let a = game.mobs[a_index].id;
    // No record, no raw copy: nothing.
    assert_eq!(fields(&mut game, victim), (None, None), "no record");
    // A raw copy from the summon, no live record: both keys go.
    let echo = index(&mut game, ECHO);
    assert_eq!(fields(&mut game, echo), (None, None), "stale raw copy");
    // A live record of a resident mob: both keys, at 100 ticks still.
    let age = game.mobs[victim].age as i32;
    game.mobs[victim].ai.last_hurt_by = Some(Who::Mob(a));
    game.mobs[victim].ai.last_hurt_by_time = age - 100;
    assert_eq!(
        fields(&mut game, victim),
        (
            Some(Tag::IntArray(vec![24724, 0, 0, 2])),
            Some(Tag::Int(100))
        ),
        "100 ticks"
    );
    // 101 ticks: gone.
    game.mobs[victim].ai.last_hurt_by_time = age - 101;
    assert_eq!(fields(&mut game, victim), (None, None), "101 ticks");
    // An attacker that is not in the level: nothing.
    game.mobs[victim].ai.last_hurt_by_time = age;
    game.mobs[victim].ai.last_hurt_by = Some(Who::Mob(u32::MAX));
    assert_eq!(
        fields(&mut game, victim),
        (None, None),
        "unresolved attacker"
    );
    // A player: not written (unsupported here).
    game.mobs[victim].ai.last_hurt_by = Some(Who::Player);
    assert_eq!(fields(&mut game, victim), (None, None), "player attacker");
    // A dead attacker: nothing.
    game.mobs[victim].ai.last_hurt_by = Some(Who::Mob(a));
    let ai = index(&mut game, A);
    game.mobs[ai].health = 0.0;
    assert!(!game.mobs[ai].alive());
    assert_eq!(fields(&mut game, victim), (None, None), "dead attacker");
}
