//! `/data merge entity <orb> {Age:42s}`: `EntityDataAccessor.setData` keeps
//! the UUID around `Entity.load`, and `ExperienceOrb.readAdditionalSaveData`
//! reads the new `Age` into the same live orb
//! (tools/test_orb_data_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_world::nbt::{Tag, parse_snbt};
use std::sync::Arc;

const ORB: &str = "00000009-0000-0000-0000-000000000001";

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
        .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
    game
}

/// Runs `command`: whether it succeeded and what it said first.
fn run(game: &mut Game<'_>, command: &str) -> (bool, Option<String>) {
    game.chat_capture = Some(Vec::new());
    let ok = game.run_command(command);
    let said = game.chat_capture.take().unwrap_or_default();
    (ok, said.first().map(horizon_client::text::Text::plain))
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_merged_age_reaches_the_same_live_orb() {
    let mut game = game();
    let saved = parse_snbt(
        "{Pos:[4.5d,100d,4.5d],Motion:[0d,0d,0d],NoGravity:1b,Value:3s,Age:0s,Health:5s,\
         Count:1,UUID:[I;9,0,0,1],Tags:[\"h009\"]}",
    )
    .unwrap()
    .0;
    game.load_orb(saved, Some([9, 0, 0, 1]));
    assert_eq!(game.orbs.len(), 1);
    // The live state a reload would lose: its id, join and tick count.
    game.orbs[0].ticks = 17;
    let (id, join) = (game.orbs[0].id, game.orbs[0].join);
    let modified = Some("Modified entity data of Experience Orb".to_owned());
    assert_eq!(
        run(&mut game, &format!("data merge entity {ORB} {{Age:42s}}")),
        (true, modified.clone())
    );
    assert_eq!(game.orbs.len(), 1, "the same orb, no second one");
    let o = &game.orbs[0];
    assert_eq!((o.id, o.join, o.ticks), (id, join, 17), "the same live orb");
    assert_eq!((o.age, o.value, o.count, o.health), (42, 3, 1, 5));
    assert_eq!(game.persist.uuids.get(&id), Some(&[9, 0, 0, 1]));
    assert!(
        run(
            &mut game,
            &format!("execute if data entity {ORB} {{Age:42s,Tags:[\"h009\"]}}")
        )
        .0
    );
    // Nothing to change: the same failure as before.
    assert_eq!(
        run(&mut game, &format!("data merge entity {ORB} {{Age:42s}}")),
        (
            false,
            Some("Nothing changed. The specified properties already have these values".to_owned())
        )
    );
    // The UUID stays the orb's own.
    assert_eq!(
        run(
            &mut game,
            &format!("data merge entity {ORB} {{UUID:[I;1,2,3,4]}}")
        ),
        (true, modified)
    );
    assert_eq!(game.persist.uuids.get(&id), Some(&[9, 0, 0, 1]));
    let tag = game.entity_nbt(commands::Target::Orb(id)).unwrap();
    assert_eq!(tag.get("UUID"), Some(&Tag::IntArray(vec![9, 0, 0, 1])));
    assert_eq!(game.orbs[0].id, id);
}

/// Loads an orb from `snbt` with the UUID `[9, 0, 0, n]`; returns its id.
fn load(game: &mut Game<'_>, snbt: &str, n: i32) -> u32 {
    game.load_orb(parse_snbt(snbt).unwrap().0, Some([9, 0, 0, n]));
    game.orbs.last().unwrap().id
}
fn orb<'a>(game: &'a Game<'_>, id: u32) -> &'a horizon_client::entities::ExperienceOrb {
    game.orbs.iter().find(|o| o.id == id).unwrap()
}

/// Added controls (after the baseline): the merge changes only what the
/// saved compound says, on the same orb; the edited compound replaces a
/// stale one; and the portal cooldown is read while a live portal process
/// stays.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn the_merge_keeps_the_live_orb_and_its_portal_process() {
    use horizon_client::nether_portal::{PortalState, Process};
    let mut game = game();
    // No Tags, so no raw compound; a moving orb with a rotation.
    let id = load(
        &mut game,
        "{Pos:[4.5d,100d,4.5d],Motion:[0.1d,0d,0d],Rotation:[30f,10f],Value:3s,Age:0s,\
         Health:5s,Count:2}",
        1,
    );
    assert!(!game.persist.raw.contains_key(&id), "no raw compound yet");
    game.orbs.iter_mut().find(|o| o.id == id).unwrap().ticks = 17;
    let join = orb(&game, id).join;
    assert!(run(&mut game, &format!("data merge entity {ORB} {{Age:42s}}")).0);
    let o = orb(&game, id);
    assert_eq!(
        (o.id, o.join, o.ticks, o.count, o.age),
        (id, join, 17, 2, 42)
    );
    assert_eq!(
        (o.velocity.x, o.rotation, o.no_gravity),
        (0.1, [30.0, 10.0], false)
    );
    assert_eq!(game.orbs.len(), 1);
    // A merge that moves it changes the same orb.
    assert!(
        run(
            &mut game,
            &format!("data merge entity {ORB} {{Motion:[0d,0.2d,0d],NoGravity:1b}}")
        )
        .0
    );
    let o = orb(&game, id);
    assert_eq!(
        (o.id, o.velocity.y, o.velocity.x, o.no_gravity, o.age),
        (id, 0.2, 0.0, true, 42)
    );
    // A stale raw compound gives way to the edited one.
    game.persist
        .raw
        .insert(id, parse_snbt("{Age:7s,Tags:[\"old\"]}").unwrap().0);
    assert!(
        run(
            &mut game,
            &format!("data merge entity {ORB} {{Age:44s,Tags:[\"new\"]}}")
        )
        .0
    );
    assert_eq!(orb(&game, id).age, 44);
    assert!(
        run(
            &mut game,
            &format!("execute if data entity {ORB} {{Age:44s,Tags:[\"new\"]}}")
        )
        .0
    );
    assert!(
        !run(
            &mut game,
            &format!("execute if data entity {ORB} {{Tags:[\"old\"]}}")
        )
        .0
    );
    // A live portal process stays; the cooldown is read, 0 when it is 0.
    let process = Process {
        entry: horizon_core::position::BlockPos::new(1, 2, 3),
        time: 5,
        inside: true,
    };
    game.portals.entities.insert(
        id,
        PortalState {
            cooldown: 300,
            process: Some(process),
        },
    );
    assert!(
        run(
            &mut game,
            &format!("data merge entity {ORB} {{Age:45s,PortalCooldown:0}}")
        )
        .0
    );
    let state = game.portals.entities.get(&id).unwrap();
    let kept = state.process.map(|p| (p.entry, p.time, p.inside));
    assert_eq!(
        (state.cooldown, kept),
        (0, Some((process.entry, process.time, process.inside)))
    );
    // An orb with no portal state gets the saved positive cooldown.
    let other = load(
        &mut game,
        "{Pos:[8.5d,100d,8.5d],Value:1s,Age:0s,Health:5s,Count:1}",
        2,
    );
    assert!(!game.portals.entities.contains_key(&other));
    assert!(
        run(
            &mut game,
            "data merge entity 00000009-0000-0000-0000-000000000002 {PortalCooldown:120}"
        )
        .0
    );
    let state = game.portals.entities.get(&other).unwrap();
    assert_eq!((state.cooldown, state.process.is_none()), (120, true));
    assert_eq!(game.orbs.len(), 2);
}
