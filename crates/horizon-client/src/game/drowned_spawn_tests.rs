//! A summoned drowned (`summon minecraft:drowned x y z`, no NBT, so
//! `Drowned.finalizeSpawn` runs) holds a nautilus shell in its offhand with a
//! 0.03 chance, as a guaranteed drop (`setGuaranteedDrop(OFFHAND)`). Over 400
//! drowned, the live offhand, the saved offhand and the saved chance agree,
//! and the count is in the original probe's wide range (1..30), not an exact
//! random result.
use super::super::*;
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
    for x in -1..1 {
        for z in -1..1 {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
            game.block_ticks
                .add_empty_chunk(ChunkPos::new(x, z))
                .unwrap();
        }
    }
    for command in [
        "difficulty easy",
        "gamerule spawn_mobs false",
        "forceload add -16 -16 15 15",
        "fill -16 99 -16 15 99 15 stone",
        "tick freeze",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    game
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn summoned_drowned_hold_shells_as_guaranteed_drops() {
    let mut game = game();
    for index in 0..400 {
        let (x, z) = (f64::from(index % 20) - 9.5, f64::from(index / 20) - 9.5);
        assert!(game.run_command(&format!("summon drowned {x} 100 {z}")));
    }
    let drowned: Vec<u32> = game
        .mobs
        .iter()
        .filter(|m| m.species() == horizon_client::mobs::Species::Drowned)
        .map(|m| m.id)
        .collect();
    assert_eq!(drowned.len(), 400);
    let (mut live, mut saved, mut guaranteed, mut offhands) = (0, 0, 0, Vec::new());
    for id in drowned {
        let m = game.mobs.iter().find(|m| m.id == id).unwrap();
        offhands.push(m.extra.off_hand.clone());
        if m.extra.off_hand.as_deref() == Some("minecraft:nautilus_shell") {
            live += 1;
            if horizon_client::mobs::drops::drop_chance(m, "offhand") == 2.0 {
                guaranteed += 1;
            }
        }
        let tag = game
            .entity_nbt(super::super::commands::Target::Mob(id))
            .unwrap();
        let body = horizon_client::snbt::pretty(&tag).plain();
        if body.contains("offhand: {count: 1, id: \"minecraft:nautilus_shell\"}") {
            saved += 1;
        }
    }
    let other: Vec<_> = offhands.iter().flatten().collect();
    eprintln!("live {live} saved {saved} guaranteed {guaranteed}; other offhands {other:?}");
    assert!((1..=30).contains(&live), "live shells {live}");
    assert_eq!(saved, live, "the saved offhand");
    assert_eq!(guaranteed, live, "every shell is a guaranteed drop");
}

/// The other side of the shell opportunity, through the same
/// `spawning::finalize` route on mobs that `summon` with NBT left
/// unfinalized, from fixed seeds. The expected values were read from this
/// route before the shell branch existed (`.local/drowned/pristine-oracle.log`).
#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn only_an_empty_offhand_drowned_draws_for_the_shell() {
    use horizon_client::mobs::drops::{DEFAULT_DROP_CHANCE, SLOTS, drop_chance};
    use horizon_core::random::RandomSource;
    let mut game = game();
    for command in [
        "summon drowned 0.5 100 0.5 {NoAI:1b}",
        "summon zombie 2.5 100 0.5 {NoAI:1b}",
        "summon husk 4.5 100 0.5 {NoAI:1b}",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    let facts = game.biome_facts(0, 100, 0);
    let difficulty = game.local_difficulty([0.5, 100.0, 0.5]);
    let base = |kind: &str| {
        let m = game.mobs.iter().find(|m| &*m.kind.id == kind).unwrap();
        assert_eq!(
            m.extra.off_hand, None,
            "{kind}: not finalized by summon with NBT"
        );
        m.clone()
    };
    let finalize = |mut m: horizon_client::mobs::Mob, seed: i64| {
        m.random.set_seed(seed);
        horizon_client::mobs::spawning::finalize(&mut m, &facts, &difficulty);
        m
    };
    // Other species draw nothing more: the end state of their random is the
    // one the route had before.
    let before = [
        (1, 266611666813271u64),
        (2, 209180304053342),
        (3, 228324091639985),
    ];
    for kind in ["minecraft:zombie", "minecraft:husk"] {
        for (seed, state) in before {
            let m = finalize(base(kind), seed);
            assert_eq!(m.random.state(), state, "{kind} seed {seed}");
            assert_eq!(m.extra.off_hand, None);
        }
    }
    // An occupied offhand: no shell draw, the item and the chances stay.
    let occupied = [
        (1, 266611666813271u64),
        (2, 236527342890431),
        (3, 115845733591934),
    ];
    for (seed, state) in occupied {
        let mut m = base("minecraft:drowned");
        m.extra.off_hand = Some("minecraft:dandelion".into());
        let m = finalize(m, seed);
        assert_eq!(m.random.state(), state, "occupied seed {seed}");
        assert_eq!(m.extra.off_hand.as_deref(), Some("minecraft:dandelion"));
        assert_eq!(m.extra.drop_chances, None);
    }
    // An empty offhand: exactly one more draw, after every other one, and
    // the shell when it is under 0.03.
    let mut shells = 0;
    for seed in 0..400 {
        let mut occupied = base("minecraft:drowned");
        occupied.extra.off_hand = Some("minecraft:dandelion".into());
        let occupied = finalize(occupied, seed);
        let empty = finalize(base("minecraft:drowned"), seed);
        let mut after = occupied.random.clone();
        let draw = after.next_float();
        assert_eq!(empty.random.state(), after.state(), "seed {seed}: one draw");
        let shell = empty.extra.off_hand.as_deref() == Some("minecraft:nautilus_shell");
        assert_eq!(shell, draw < 0.03, "seed {seed}: draw {draw}");
        assert_eq!(
            empty.extra.main_hand, occupied.extra.main_hand,
            "seed {seed}"
        );
        if shell {
            shells += 1;
            // The setter makes the default array (it was absent): the
            // offhand is 2.0. Every other slot reads what it read before, but
            // the saddle: without an array this route reads 2.0 (saddles
            // that `Equippable` puts on), with one 0.085, which is vanilla's
            // `DropChances.byEquipment` for a slot with no entry. The drowned
            // wears no saddle.
            let chances = empty.extra.drop_chances.as_deref().copied();
            let mut expected = [DEFAULT_DROP_CHANCE; 8];
            expected[1] = 2.0;
            assert_eq!(chances, Some(expected), "seed {seed}");
            for slot in SLOTS
                .iter()
                .filter(|s| !matches!(**s, "offhand" | "saddle"))
            {
                assert_eq!(
                    drop_chance(&empty, slot),
                    drop_chance(&occupied, slot),
                    "{slot}"
                );
            }
            assert_eq!(drop_chance(&occupied, "saddle"), 2.0);
            assert_eq!(drop_chance(&empty, "saddle"), DEFAULT_DROP_CHANCE);
            assert!(empty.saddle_item().is_none());
        } else {
            assert_eq!(empty.extra.off_hand, None);
            assert_eq!(empty.extra.drop_chances, None);
        }
    }
    eprintln!("shells from seeds 0..400: {shells}");
    assert!((1..=30).contains(&shells), "{shells}");
}
