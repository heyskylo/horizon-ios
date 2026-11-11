//! `TamableAnimal.isOrderedToSit` against `isInSittingPose`: the order is the
//! saved flag an interaction flips, the pose is bit 1 of `DATA_FLAGS_ID`,
//! which only `SitWhenOrderedToGoal` (once it can start: not in water, on the
//! ground) and a cat's chest/bed goals write, and which the models and the
//! metadata read.
use super::*;
use crate::mobs::sensor_scan_tests::kind;
use horizon_core::geometry::Vec3;
use std::sync::Arc;

fn pet(id: &str, species: Species, at: Vec3) -> Mob {
    let mut kind = kind();
    let k = Arc::make_mut(&mut kind);
    k.id = id.into();
    k.species = species;
    let mut m = Mob::new(1, kind, at, 0.0);
    m.extra.tame = true;
    m
}

/// `SitWhenOrderedToGoal.canUse`: ordered to sit is not enough while the pet
/// is in water or in the air, so a pet fed a bone mid-swim keeps swimming;
/// `start` and `stop` are the only writers of the pose.
#[test]
fn ordering_a_pet_to_sit_in_water_leaves_the_pose_alone() {
    let ran = crate::mobs::world::with_test_world(&[], |w, _| {
        let mut lr = horizon_core::random::LegacyRandom::new(0);
        let mut events = Vec::new();
        let mut cx = Cx {
            w,
            lr: &mut lr,
            events: &mut events,
        };
        for (id, species) in [
            ("minecraft:wolf", Species::Wolf),
            ("minecraft:cat", Species::Cat),
            ("minecraft:parrot", Species::Parrot),
        ] {
            let mut m = pet(id, species, Vec3::new(0.5, 64.0, 0.5));
            // The interaction's flip: the order, no pose.
            m.extra.sitting = true;
            m.fluids.in_water = true;
            let mut goal = SitWhenOrdered;
            assert!(!goal.can_use(&mut m, &mut cx), "{id}: sits while swimming");
            assert!(!m.extra.sitting_pose, "{id}: still standing in the water");
            // In the air (falling, jumping) it does not sit either.
            m.fluids.in_water = false;
            m.on_ground = false;
            assert!(!goal.can_use(&mut m, &mut cx), "{id}: sits in the air");
            assert!(!m.extra.sitting_pose, "{id}: still standing in the air");
            // On dry ground it sits, and standing up ends the pose but not
            // the order.
            m.on_ground = true;
            assert!(goal.can_use(&mut m, &mut cx), "{id}: does not sit on land");
            goal.start(&mut m, &mut cx);
            assert!(m.extra.sitting_pose, "{id}: not sitting on land");
            assert!(m.extra.sitting, "{id}: the order is the goal's to keep");
            goal.stop(&mut m, &mut cx);
            assert!(!m.extra.sitting_pose, "{id}: stood up but still posed");
            assert!(m.extra.sitting, "{id}: standing up dropped the order");
        }
    });
    if !ran {
        eprintln!("skipped: no extracted vanilla data");
    }
}

/// `CatSitOnBlockGoal`: the cat on the chest it claimed is in the sitting
/// pose the renderer, the metadata and `ChestBlock` read, and the order alone
/// (a cat told to sit in water) is not that pose.
#[test]
fn a_cat_on_a_chest_is_in_the_sitting_pose() {
    use crate::mobs::world::with_built_test_world;
    let mut seen = None;
    if !with_built_test_world(
        &[],
        |world, registry| {
            for (cx, cz) in [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 0), (0, 1)] {
                world
                    .chunks
                    .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
            }
            let stone = registry.default_state("minecraft:stone").unwrap();
            let chest = registry.default_state("minecraft:chest").unwrap();
            for x in -4..=4 {
                for z in -4..=4 {
                    world.set(x, 63, z, stone).unwrap();
                }
            }
            world.set(0, 64, 0, chest).unwrap();
        },
        |w, _| {
            let mut lr = horizon_core::random::LegacyRandom::new(0);
            let mut events = Vec::new();
            let mut cx = Cx {
                w,
                lr: &mut lr,
                events: &mut events,
            };
            let mut m = pet("minecraft:cat", Species::Cat, Vec3::new(0.5, 65.0, 0.5));
            m.on_ground = true;
            let mut goal = CatSitOnBlock(super::super::goals::MoveToBlock::new(0.8, 8, 1));
            let found = goal.can_use(&mut m, &mut cx);
            goal.start(&mut m, &mut cx);
            let at_start = cat_sitting(&m);
            // Standing on the chest: the goal has reached its target.
            goal.tick(&mut m, &mut cx);
            let reached = cat_sitting(&m);
            goal.stop(&mut m, &mut cx);
            let after = cat_sitting(&m);
            seen = Some((found, at_start, reached, after));
        },
    ) {
        return;
    }
    assert_eq!(
        seen,
        Some((true, false, true, false)),
        "a cat sits on the chest it reached and stands when its goal stops"
    );
}

/// `Cat.registerGoals` strolls out of water one tick in 100000 (the ocelot's
/// `1.0000001E-5F`), not the default 1 in 1000.
#[test]
fn a_cats_stroll_is_one_tick_in_a_hundred_thousand() {
    assert_eq!(cat_stroll().water_avoiding, Some(1.000_000_1e-5));
}
