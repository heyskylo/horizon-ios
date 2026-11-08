//! A mannequin's saved fields: `Mannequin.addAdditionalSaveData` always
//! writes `profile`, `hidden_layers`, `main_hand` and `pose`, and, as an
//! `Avatar` and not a `Mob`, none of `Mob.addAdditionalSaveData`'s fields
//! (tools/test_mannequin_save_parity.py measures the same on vanilla 26.3).
use super::*;
use horizon_world::nbt::{Tag, parse_snbt};
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
        .insert((0, 0), Arc::new(horizon_client::chunk::Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(horizon_core::position::ChunkPos::new(0, 0))
        .unwrap();
    game
}

const MOB_ONLY: [&str; 4] = ["NoAI", "PersistenceRequired", "CanPickUpLoot", "LeftHanded"];

fn snbt(text: &str) -> Tag {
    parse_snbt(text).unwrap().0
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mannequin_saves_its_own_fields_and_no_mob_fields() {
    let mut game = game();
    assert!(game.run_command("summon mannequin 4.5 100 4.5 {UUID:[I;321,0,0,1],NoGravity:1b}"));
    assert!(game.run_command(
        "summon mannequin 8.5 100 4.5 {UUID:[I;321,0,0,6],NoGravity:1b,NoAI:1b,PersistenceRequired:1b,CanPickUpLoot:1b,LeftHanded:1b}"
    ));
    let id = |g: &Game<'_>, n: i32| {
        g.mobs
            .iter()
            .find(|m| g.persist.uuids.get(&m.id) == Some(&[321, 0, 0, n]))
            .unwrap()
            .id
    };
    let (plain, mob_input) = (id(&game, 1), id(&game, 6));
    for mob in [plain, mob_input] {
        let tag = game.entity_nbt(commands::Target::Mob(mob)).unwrap();
        assert_eq!(tag.get("profile"), Some(&snbt("{}")), "{mob}");
        assert_eq!(tag.get("hidden_layers"), Some(&Tag::List(Vec::new())));
        assert_eq!(tag.get("main_hand"), Some(&Tag::String("right".into())));
        assert_eq!(tag.get("pose"), Some(&Tag::String("standing".into())));
        for key in MOB_ONLY {
            assert!(tag.get(key).is_none(), "{key} on mannequin {mob}");
        }
    }
    // A live edit of its own fields.
    assert!(game.run_command(
        "data merge entity 00000141-0000-0000-0000-000000000001 {pose:\"swimming\",main_hand:\"left\",hidden_layers:[\"jacket\"]}"
    ));
    let tag = game.entity_nbt(commands::Target::Mob(plain)).unwrap();
    assert_eq!(tag.get("pose"), Some(&Tag::String("swimming".into())));
    assert_eq!(tag.get("main_hand"), Some(&Tag::String("left".into())));
    assert_eq!(tag.get("hidden_layers"), Some(&snbt("[\"jacket\"]")));
}

/// Added controls (after the baseline): a mannequin ignores the Mob flags it
/// is given, its `main_hand` alone picks its arm, a layer list with an entry
/// that is not a part hides nothing, and its save comes from its live state
/// with no loaded compound and over a stale one; a pig keeps its Mob flags.
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_mannequin_reads_and_saves_from_its_own_state() {
    let mut game = game();
    let summon = |g: &mut Game<'_>, kind: &str, n: i32, fields: &str| {
        assert!(g.run_command(&format!(
            "summon {kind} {}.5 100 4.5 {{UUID:[I;321,0,0,{n}],NoGravity:1b,{fields}}}",
            n * 2
        )));
        g.mobs
            .iter()
            .position(|m| g.persist.uuids.get(&m.id) == Some(&[321, 0, 0, n]))
            .unwrap()
    };
    let flags = "NoAI:1b,PersistenceRequired:1b,CanPickUpLoot:1b,LeftHanded:1b";
    let mannequin = summon(
        &mut game,
        "mannequin",
        1,
        &format!("{flags},main_hand:\"right\""),
    );
    let m = &game.mobs[mannequin];
    assert_eq!(
        (
            m.no_ai,
            m.persistent,
            m.extra.pick_up_loot,
            m.extra.left_handed
        ),
        (false, false, false, false),
        "the Mob flags are not a mannequin's"
    );
    assert!(
        !m.extra.main_hand_left && !m.main_arm_left(),
        "main_hand, not LeftHanded"
    );
    let left = summon(
        &mut game,
        "mannequin",
        2,
        "LeftHanded:0b,main_hand:\"left\"",
    );
    assert!(game.mobs[left].extra.main_hand_left);
    let bogus = summon(
        &mut game,
        "mannequin",
        3,
        "hidden_layers:[\"hat\",\"bogus\"]",
    );
    assert_eq!(
        game.mobs[bogus].extra.hidden_model_parts, 0,
        "the whole list fails"
    );
    let pig = summon(&mut game, "pig", 4, flags);
    let p = &game.mobs[pig];
    assert_eq!(
        (p.no_ai, p.persistent, p.extra.left_handed),
        (true, true, true)
    );
    // The live state is what it saves: no loaded compound, then a stale one.
    let id = game.mobs[mannequin].id;
    {
        let m = &mut game.mobs[mannequin];
        m.extra.avatar_pose = crate::pose::Pose::Crouching;
        m.extra.main_hand_left = true;
        m.extra.hidden_model_parts = 1 | 1 << 6;
    }
    let stale = snbt(
        "{pose:\"sleeping\",main_hand:\"right\",hidden_layers:[\"jacket\"],NoAI:1b,PersistenceRequired:1b,LeftHanded:1b,drop_chances:{},keep:1b}",
    );
    for raw in [None, Some(stale)] {
        match &raw {
            Some(raw) => game.persist.raw.insert(id, raw.clone()),
            None => game.persist.raw.remove(&id),
        };
        let tag = game.entity_nbt(commands::Target::Mob(id)).unwrap();
        assert_eq!(tag.get("pose"), Some(&Tag::String("crouching".into())));
        assert_eq!(tag.get("main_hand"), Some(&Tag::String("left".into())));
        assert_eq!(tag.get("hidden_layers"), Some(&snbt("[\"cape\",\"hat\"]")));
        assert_eq!(tag.get("profile"), Some(&snbt("{}")));
        for key in MOB_ONLY.iter().chain(&["drop_chances"]) {
            assert!(tag.get(key).is_none(), "{key}");
        }
        assert_eq!(tag.get("keep").is_some(), raw.is_some(), "other keys stay");
        assert!(tag.get("Health").is_some() && tag.get("UUID").is_some());
    }
    // A copy carries the state; a merge reaches it.
    let copy = game.mobs[mannequin].clone();
    assert_eq!(
        (copy.extra.main_hand_left, copy.extra.hidden_model_parts),
        (true, 1 | 1 << 6)
    );
    assert!(game.run_command(
        "data merge entity 00000141-0000-0000-0000-000000000001 {hidden_layers:[\"right_sleeve\"],main_hand:\"right\"}"
    ));
    let m = game.mobs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(
        (m.extra.hidden_model_parts, m.extra.main_hand_left),
        (1 << 3, false)
    );
    // The pig's save keeps its Mob fields.
    let pig_id = game.mobs[pig].id;
    let tag = game.entity_nbt(commands::Target::Mob(pig_id)).unwrap();
    for key in MOB_ONLY {
        assert_eq!(tag.get(key), Some(&Tag::Byte(1)), "{key}");
    }
}
