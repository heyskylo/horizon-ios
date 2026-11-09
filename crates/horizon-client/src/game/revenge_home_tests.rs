//! A zombie hurt by a cow takes revenge (`HurtByTargetGoal`) only when the
//! cow stands inside the zombie's home: `TargetGoal.canAttack` ends with
//! `Mob.isWithinHome(target.blockPosition())`. The zombies run their real
//! goals on the game's ticks; tools/ and vanilla 26.3 measured the same
//! scene (no home and radius 12 retaliate, radius 3 does not).
use super::super::*;
use horizon_client::{chunk::Chunk, mobs::Who};
use horizon_core::position::ChunkPos;
use std::sync::Arc;

/// (pen, home NBT): pens 60 blocks apart, beyond a zombie's alert range.
const CASES: [(&str, &str); 3] = [
    ("no home", ""),
    ("radius 3", ",home_pos:[I;{x},200,4],home_radius:3"),
    ("radius 12", ",home_pos:[I;{x},200,4],home_radius:12"),
];

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
    for x in 0..9 {
        game.world
            .chunks
            .insert((x, 0), Arc::new(Chunk::empty(x, 0)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(x, 0))
            .unwrap();
    }
    for command in [
        "forceload add 0 0 143 15",
        "tick freeze",
        "difficulty easy",
        "time set midnight",
        "gamerule advance_time false",
        "gamerule spawn_mobs false",
    ] {
        assert!(game.run_command(command), "{command}");
    }
    for (i, (_, home)) in CASES.iter().enumerate() {
        let x = 4 + i * 60;
        let home = home.replace("{x}", &x.to_string());
        for command in [
            format!("fill {} 199 0 {} 204 8 glass strict", x - 3, x + 12),
            format!("fill {} 200 1 {} 204 7 air strict", x - 2, x + 11),
            format!(
                "summon zombie {x}.5 200 4.5 {{PersistenceRequired:1b,UUID:[I;24732,0,0,{}]{home}}}",
                2 * i + 1
            ),
            format!(
                "summon cow {}.5 200 4.5 {{NoAI:1b,PersistenceRequired:1b,UUID:[I;24732,0,0,{}]}}",
                x + 8,
                2 * i + 2
            ),
        ] {
            assert!(game.run_command(&command), "{command}");
        }
    }
    game
}

fn id(game: &Game<'_>, n: usize) -> u32 {
    match game.target_with_uuid([24732, 0, 0, n as i32]) {
        Some(super::commands::Target::Mob(id)) => id,
        other => panic!("{other:?}"),
    }
}

fn step(game: &mut Game<'_>, ticks: u32) {
    assert!(game.run_command(&format!("tick step {ticks}")));
    for _ in 0..ticks {
        game.tick_server(false).unwrap();
    }
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn a_zombie_takes_revenge_only_on_a_cow_inside_its_home() {
    let mut game = game();
    step(&mut game, 2);
    for (i, (name, _)) in CASES.iter().enumerate() {
        let (zombie, cow) = (id(&game, 2 * i + 1), id(&game, 2 * i + 2));
        let z = game.mobs.iter().find(|m| m.id == zombie).unwrap();
        assert_eq!(z.ai.target, None, "{name}: no target before the hit");
        assert!(!z.no_ai, "{name}: the zombie's AI runs");
        // The loaded home, as the zombie keeps it.
        let x = 4 + 60 * i as i32;
        let home = match i {
            0 => None,
            1 => Some(([x, 200, 4], 3)),
            _ => Some(([x, 200, 4], 12)),
        };
        assert_eq!(z.ai.home, home, "{name}: its home");
        assert!(game.run_command(&format!(
            "damage @e[nbt={{UUID:[I;24732,0,0,{}]}},limit=1] 1 generic by @e[nbt={{UUID:[I;24732,0,0,{}]}},limit=1]",
            2 * i + 1,
            2 * i + 2
        )));
        let z = game.mobs.iter().find(|m| m.id == zombie).unwrap();
        assert_eq!(
            z.ai.last_hurt_by,
            Some(Who::Mob(cow)),
            "{name}: the cow hurt it"
        );
        assert_eq!(z.health, 19.0, "{name}: the hit landed");
    }
    step(&mut game, 20);
    let mut outcome = Vec::new();
    for (i, (name, _)) in CASES.iter().enumerate() {
        let (zombie, cow) = (id(&game, 2 * i + 1), id(&game, 2 * i + 2));
        let z = game.mobs.iter().find(|m| m.id == zombie).unwrap();
        outcome.push((*name, z.ai.target == Some(Who::Mob(cow))));
    }
    // The cow's block is eight blocks from the home's centre: outside radius
    // 3, inside radius 12.
    assert_eq!(
        outcome,
        [("no home", true), ("radius 3", false), ("radius 12", true)]
    );
}
