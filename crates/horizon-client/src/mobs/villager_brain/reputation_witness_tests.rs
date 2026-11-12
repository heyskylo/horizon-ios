use super::*;
use std::sync::Arc;

fn villager(id: u32, x: f64, z: f64, invisible: bool) -> Mob {
    let mut kind = super::super::sensor_scan_tests::kind();
    let k = Arc::make_mut(&mut kind);
    k.id = "minecraft:villager".into();
    k.species = Species::Villager;
    let mut m = Mob::new(id, kind, Vec3::new(x, 1.0, z), 0.0);
    m.invisible = invisible;
    m
}
#[test]
#[ignore = "requires explicit prepared resources and vanilla oracle output"]
fn witnesses_use_sensor_candidates_radius_invisibility_and_cached_results() {
    let root = std::env::var_os("HORIZON_TEST_RESOURCES").expect("explicit fixture resources");
    let oracle = std::fs::read_to_string(
        std::env::var_os("HORIZON_REPUTATION_ORACLE").expect("oracle output"),
    )
    .unwrap();
    let expected: Vec<Vec<u32>> = oracle
        .lines()
        .filter(|line| {
            line.starts_with("witnesses ")
                || line.starts_with("cached ")
                || line.starts_with("fresh ")
        })
        .map(|line| {
            line.split_once(' ')
                .unwrap()
                .1
                .trim_matches(['[', ']'])
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().unwrap())
                .collect()
        })
        .collect();
    let mut candidates = [
        villager(2, 10.0, 0.0, false),
        villager(3, 10.0, 0.0, true),
        villager(4, 15.0, 15.0, false),
        villager(5, 1.0, 0.0, false), // arrived after the sensor scan
        villager(6, 1.5, 0.0, true),
        villager(7, 20.0, 0.0, false),
    ];
    let list = || {
        vec![
            Who::Mob(2),
            Who::Mob(3),
            Who::Mob(4),
            Who::Mob(6),
            Who::Mob(7),
        ]
    };
    let mut victim = villager(1, 0.0, 0.0, false);
    victim.attrs.follow_range = 16.0;
    let seen: Vec<_> = candidates.iter().map(|m| m.seen(None)).collect();
    assert!(super::super::world::with_resource_test_world(
        std::path::Path::new(&root),
        &seen,
        true,
        true,
        |_, _| {},
        |w, _| {
            assert!(murder_witnesses(&mut victim, w).is_empty());
            mem(&mut victim).visible_mobs.set(Visible {
                list: list(),
                cache: Vec::new(),
            });
            assert_eq!(murder_witnesses(&mut victim, w), expected[0]);
        },
    ));
    candidates[0].invisible = true;
    candidates[1].invisible = false;
    let seen: Vec<_> = candidates.iter().map(|m| m.seen(None)).collect();
    assert!(super::super::world::with_resource_test_world(
        std::path::Path::new(&root),
        &seen,
        true,
        true,
        |_, _| {},
        |w, _| {
            assert_eq!(murder_witnesses(&mut victim, w), expected[1]);
            mem(&mut victim).visible_mobs.set(Visible {
                list: list(),
                cache: Vec::new(),
            });
            assert_eq!(murder_witnesses(&mut victim, w), expected[2]);
            // New candidates remain excluded until the next scan, even in range.
            assert!(!murder_witnesses(&mut victim, w).contains(&5));
        },
    ));
}
