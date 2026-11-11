use super::*;
/// `AgeableMob.isBaby` is `canBeABaby() && getAge() < 0`, and `Age` alone
/// says nothing: the six ageable classes that override `canBeABaby` to
/// false — `Frog`, `Parrot`, `ZombieNautilus`, `CamelHusk`, `Slime` and
/// `MagmaCube` — stay adults, and so do the `PiglinBrute` (no `isBaby` of
/// its own), the `Creeper` (outside the ageable tree) and the `Tadpole`,
/// whose own age runs up from -24000 without it ever being a baby.
///
/// Every one of them keeps its type's box, the plain 1.0 ± 0.2 voice and
/// `getAgeScale`'s 1.0, where a cow with the same age takes
/// `Cow.BABY_DIMENSIONS` (0.45 by 0.7, eyes at 0.69) and the 1.5 ± 0.2
/// that `LivingEntity.getVoicePitch` gives a baby.
#[test]
fn a_negative_age_is_a_baby_only_where_can_be_a_baby_allows_it() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("entity-types.json").exists() {
        return;
    }
    let types = super::super::MobTypes::load(&root).unwrap();
    // dims() narrows the type's box to f32 and widens it back.
    let adult = |id: &str| {
        let t = types.types.get(id).unwrap();
        (
            f64::from(t.width as f32),
            f64::from(t.height as f32),
            f64::from(t.eye_height as f32),
        )
    };
    for id in [
        "minecraft:parrot",
        "minecraft:tadpole",
        "minecraft:frog",
        "minecraft:zombie_nautilus",
        "minecraft:camel_husk",
        "minecraft:slime",
        "minecraft:magma_cube",
        "minecraft:piglin_brute",
        "minecraft:creeper",
    ] {
        let mut m = Mob::new(
            1,
            std::sync::Arc::new(types.types.get(id).unwrap().clone()),
            Vec3::ZERO,
            0.0,
        );
        m.extra.age = -24_000;
        assert!(!m.is_baby(), "{id}");
        assert_eq!(m.dims(), adult(id), "{id} keeps its type's box");
        let pitches: Vec<f32> = (0..64).map(|_| m.voice_pitch()).collect();
        assert!(
            pitches.iter().all(|p| (0.8..=1.2).contains(p)),
            "{id}: {pitches:?}"
        );
    }
    // The control: a cow at the same age is a baby in vanilla's box and
    // voice, and an adult again at age 0.
    let mut cow = Mob::new(
        1,
        std::sync::Arc::new(types.types.get("minecraft:cow").unwrap().clone()),
        Vec3::ZERO,
        0.0,
    );
    cow.extra.age = -24_000;
    assert!(cow.is_baby());
    assert_eq!(
        cow.dims(),
        (f64::from(0.45_f32), f64::from(0.7_f32), f64::from(0.69_f32))
    );
    let pitches: Vec<f32> = (0..64).map(|_| cow.voice_pitch()).collect();
    assert!(
        pitches.iter().all(|p| (1.3..=1.7).contains(p)),
        "{pitches:?}"
    );
    cow.extra.age = 0;
    assert!(!cow.is_baby());
    assert_eq!(cow.dims(), adult("minecraft:cow"));
    let pitches: Vec<f32> = (0..64).map(|_| cow.voice_pitch()).collect();
    assert!(
        pitches.iter().all(|p| (0.8..=1.2).contains(p)),
        "{pitches:?}"
    );
}

/// `Entity.getLightProbePosition(partialTick)` for a living entity is
/// `getEyePosition`: `getEyeHeight()` over `Mth.lerp(partialTick, xo, x)`.
/// Horizon probed this tick's own position, so a mob walking into the next
/// cell was drawn lit by that cell for the whole frame before it got there,
/// and a falling one for a frame after it left.
#[test]
fn a_mob_probe_follows_the_frame() {
    let eye = 1.62;
    // Walking from x = 0.9 to 1.1: the eye is in the cell at 0 until the
    // frame reaches the boundary, and in the cell at 1 from there on.
    let (previous, position) = (Vec3::new(0.9, 64.0, 0.5), Vec3::new(1.1, 64.0, 0.5));
    assert_eq!(eye_probe_cell(previous, position, eye, 0.0), [0, 65, 0]);
    assert_eq!(eye_probe_cell(previous, position, eye, 0.5), [1, 65, 0]);
    assert_eq!(eye_probe_cell(previous, position, eye, 1.0), [1, 65, 0]);
    // The old probe was the tick's own position on every frame.
    assert_eq!(position.x.floor() as i32, 1);
    // Falling a block: the eye is at 71.62 at the start of the frame and
    // 70.62 at its end.
    let (previous, position) = (Vec3::new(0.5, 70.0, 0.5), Vec3::new(0.5, 69.0, 0.5));
    assert_eq!(eye_probe_cell(previous, position, eye, 0.0), [0, 71, 0]);
    assert_eq!(eye_probe_cell(previous, position, eye, 0.5), [0, 71, 0]);
    assert_eq!(eye_probe_cell(previous, position, eye, 1.0), [0, 70, 0]);
}
/// A pig dropped hard onto a block bounces by that block's restitution
/// (`Entity.getBlockBounciness`): landing on a slime block sends it back
/// up, landing on stone stops it dead. The same 8-block fall then costs the
/// pig what the block's own `fallOn` allows (`Block.fallOn`): all 5 on
/// stone, 1 on hay or honey, 0 on slime, 1 on a bed, and a stalagmite kills.
#[test]
fn a_mob_bounces_off_a_bouncy_block_and_not_off_stone() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("entity-types.json").exists() || !root.join("movement-properties.nbt").exists() {
        return;
    }
    let assets = crate::assets::Assets::load(&root).unwrap();
    let registry = assets.registry;
    let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
    let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
    let data = super::super::MobData::load(&root, &facts);
    let types = super::super::MobTypes::load(&root).unwrap();
    let biomes = crate::biome::Biomes::load(&root).unwrap();
    let items = crate::items::Items::load(
        &root,
        registry,
        &facts,
        &horizon_world::loot::PackOverlay::NONE,
    )
    .unwrap();
    let collision = std::sync::Arc::new(crate::collisions::load(&root, registry).unwrap());
    let suffocation =
        crate::mobs::suffocation::bind(collision.clone(), &motion.causes_suffocation).unwrap();
    let lid_openers = std::collections::HashMap::new();
    for (ground, bounces, health) in [
        ("minecraft:slime_block", true, 10.0),
        ("minecraft:stone", false, 5.0),
        ("minecraft:hay_block", false, 9.0),
        ("minecraft:honey_block", false, 9.0),
        ("minecraft:red_bed", true, 9.0),
        ("minecraft:pointed_dripstone", false, 0.0),
    ] {
        let air = registry.default_state("minecraft:air").unwrap();
        let mut world = crate::world::World::new(air, 0);
        world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
        );
        let floor = registry.default_state(ground).unwrap();
        for x in 0..=1 {
            for z in 0..=1 {
                world.set(x, 0, z, floor).unwrap();
            }
        }
        let w = MobWorld {
            teams: Default::default(),
            world: &world,
            registry,
            facts: &facts,
            fluid_tables: &motion.fluids,
            movement: &motion.blocks,
            collision: &collision,
            suffocation: &suffocation,
            data: &data,
            player: None,
            controlling_player_movement: None,
            nearest_player: None,
            others: &[],
            dragon: None,
            leash_boats: &[],
            who_lookup: None,
            others_grid: None,
            others_index: None,
            others_villagers: None,
            others_solid: None,
            daylight: true,
            monsters_burn: false,
            game_time: 1000,
            day_time: 18000,
            raining: false,
            thundering: false,
            level: horizon_core::LevelKey::OVERWORLD,
            block_tags: &items.block_tags,
            landing: &motion.landing,
            biomes: &biomes,
            universal_anger: false,
            mob_griefing: true,
            mob_drops: true,
            max_entity_cramming: 24,
            difficulty: super::super::spawning::LocalDifficulty::new(2, 18000, 0, 0.0),
            dimension_type: horizon_world::dimension_type::DimensionFacts::overworld(),
            world_seed: 0,
            sea_level: 63,
            items: Some(&items),
            lid_openers: &lid_openers,
            mod_effects: None,
            border: None,
        };
        let kind = std::sync::Arc::new(types.types["minecraft:pig"].clone());
        let mut m = Mob::new(1, kind, Vec3::new(0.5, 1.0, 0.5), 0.0);
        // Falling faster than gravity, so the landing is hard enough.
        m.velocity = Vec3::new(0.0, -1.0, 0.0);
        m.fall_distance = 8.0;
        m.do_move(&w, &mut Vec::new());
        assert_eq!(m.health, health, "{ground}: health after an 8 block fall");
        assert!(
            m.vertical_collision_below,
            "{ground}: the pig lands on the block"
        );
        if bounces {
            assert!(
                m.velocity.y > 0.0,
                "{ground}: restitution sends the pig back up, got {}",
                m.velocity.y
            );
        } else {
            assert!(
                m.velocity.y <= 0.0,
                "{ground}: no bounce, got {}",
                m.velocity.y
            );
        }
    }
}

/// `LivingEntity.travelInFluid` ends with
/// `floatInLiquidWhileRidden(FluidTags.ENTITY_FLOATABLE)`: a mount that
/// carries a passenger, is in `minecraft:can_float_while_ridden` and sits
/// deeper in the water than its jump threshold gains 0.04 a tick upwards.
/// That is what keeps a ridden horse and its rider afloat; an empty horse
/// gets nothing and goes on sinking.
#[test]
fn a_ridden_horse_floats_in_deep_water() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("entity-types.json").exists() || !root.join("movement-properties.nbt").exists() {
        return;
    }
    let assets = crate::assets::Assets::load(&root).unwrap();
    let registry = assets.registry;
    let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
    let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
    let data = super::super::MobData::load(&root, &facts);
    let types = super::super::MobTypes::load(&root).unwrap();
    let biomes = crate::biome::Biomes::load(&root).unwrap();
    let items = crate::items::Items::load(
        &root,
        registry,
        &facts,
        &horizon_world::loot::PackOverlay::NONE,
    )
    .unwrap();
    let collision = std::sync::Arc::new(crate::collisions::load(&root, registry).unwrap());
    let suffocation =
        crate::mobs::suffocation::bind(collision.clone(), &motion.causes_suffocation).unwrap();
    let lid_openers = std::collections::HashMap::new();
    let air = registry.default_state("minecraft:air").unwrap();
    let water = registry.default_state("minecraft:water").unwrap();
    let mut world = crate::world::World::new(air, 0);
    world.chunks.insert(
        (0, 0),
        std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
    );
    for x in 4..=12 {
        for z in 4..=12 {
            for y in 63..=66 {
                world.set(x, y, z, water).unwrap();
            }
        }
    }
    let w = MobWorld {
        teams: Default::default(),
        world: &world,
        registry,
        facts: &facts,
        fluid_tables: &motion.fluids,
        movement: &motion.blocks,
        collision: &collision,
        suffocation: &suffocation,
        data: &data,
        player: None,
        controlling_player_movement: None,
        nearest_player: None,
        others: &[],
        dragon: None,
        leash_boats: &[],
        who_lookup: None,
        others_grid: None,
        others_index: None,
        others_villagers: None,
        others_solid: None,
        daylight: true,
        monsters_burn: false,
        game_time: 1000,
        day_time: 18000,
        raining: false,
        thundering: false,
        level: horizon_core::LevelKey::OVERWORLD,
        block_tags: &items.block_tags,
        landing: &motion.landing,
        biomes: &biomes,
        universal_anger: false,
        mob_griefing: true,
        mob_drops: true,
        max_entity_cramming: 24,
        difficulty: super::super::spawning::LocalDifficulty::new(2, 18000, 0, 0.0),
        dimension_type: horizon_world::dimension_type::DimensionFacts::overworld(),
        world_seed: 0,
        sea_level: 63,
        items: Some(&items),
        lid_openers: &lid_openers,
        mod_effects: None,
        border: None,
    };
    let kind = std::sync::Arc::new(types.types["minecraft:horse"].clone());
    let mut sank = 0.0;
    let mut floated = 0.0;
    for ridden in [false, true] {
        let mut m = Mob::new(1, kind.clone(), Vec3::new(8.5, 64.0, 8.5), 0.0);
        if ridden {
            m.rider = Some(super::super::equine::Rider {
                strafe: 0.0,
                forward: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                jump: 0.0,
                steer: false,
                sprint: false,
                jumping: false,
                client: false,
            });
        }
        m.fluids.in_water = true;
        m.fluids.water = 1.0;
        m.travel(&w, Vec3::ZERO, &mut Vec::new());
        if ridden {
            floated = m.velocity.y;
        } else {
            sank = m.velocity.y;
        }
    }
    assert!(sank < 0.0, "an empty horse sinks, got {sank}");
    let lift = floated - sank;
    assert!(
        (lift - f64::from(0.04_f32)).abs() < 1.0e-9,
        "a ridden horse gains 0.04 a tick, got {lift}"
    );
}

/// `Entity.collide` takes `getEntityCollisions` too: a zombie dropping
/// onto a living shulker or a happy ghast held still (a player on its
/// back) lands on it; a dead shulker, a happy ghast that is not held, or
/// a ghastling lets it through (`canBeCollidedWith`).
#[test]
fn a_falling_mob_lands_on_a_shulker_or_a_held_happy_ghast() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("entity-types.json").exists() || !root.join("movement-properties.nbt").exists() {
        return;
    }
    let assets = crate::assets::Assets::load(&root).unwrap();
    let registry = assets.registry;
    let facts = crate::blocks::BlockFacts::load(&root, registry).unwrap();
    let motion = crate::motion::MotionResources::load(&root, &assets).unwrap();
    let data = super::super::MobData::load(&root, &facts);
    let types = super::super::MobTypes::load(&root).unwrap();
    let biomes = crate::biome::Biomes::load(&root).unwrap();
    let items = crate::items::Items::load(
        &root,
        registry,
        &facts,
        &horizon_world::loot::PackOverlay::NONE,
    )
    .unwrap();
    let collision = std::sync::Arc::new(crate::collisions::load(&root, registry).unwrap());
    let suffocation =
        crate::mobs::suffocation::bind(collision.clone(), &motion.causes_suffocation).unwrap();
    let lid_openers = std::collections::HashMap::new();
    let air = registry.default_state("minecraft:air").unwrap();
    let mut world = crate::world::World::new(air, 0);
    world.chunks.insert(
        (0, 0),
        std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
    );
    let mob = |id: u32, kind: &str, at: Vec3| {
        Mob::new(id, std::sync::Arc::new(types.types[kind].clone()), at, 0.0)
    };
    // Where a zombie starting 3 blocks above `below`'s top ends after 20
    // falling moves, with `below` in the mobs it sees.
    let landing = |below: &Mob| -> f64 {
        let others = [below.seen(None)];
        let w = MobWorld {
            teams: Default::default(),
            world: &world,
            registry,
            facts: &facts,
            fluid_tables: &motion.fluids,
            movement: &motion.blocks,
            collision: &collision,
            suffocation: &suffocation,
            data: &data,
            player: None,
            controlling_player_movement: None,
            nearest_player: None,
            others: &others,
            who_lookup: None,
            dragon: None,
            leash_boats: &[],
            others_grid: None,
            others_index: None,
            others_villagers: None,
            others_solid: None,
            daylight: true,
            monsters_burn: false,
            game_time: 1000,
            day_time: 6000,
            raining: false,
            thundering: false,
            level: horizon_core::LevelKey::OVERWORLD,
            block_tags: &items.block_tags,
            landing: &motion.landing,
            biomes: &biomes,
            universal_anger: false,
            mob_griefing: true,
            mob_drops: true,
            max_entity_cramming: 24,
            difficulty: super::super::spawning::LocalDifficulty::new(2, 6000, 0, 0.0),
            dimension_type: horizon_world::dimension_type::DimensionFacts::overworld(),
            world_seed: 0,
            sea_level: 63,
            items: Some(&items),
            lid_openers: &lid_openers,
            mod_effects: None,
            border: None,
        };
        let top = below.bounds().max_position().y;
        let mut zombie = mob(1, "minecraft:zombie", Vec3::new(8.5, top + 3.0, 8.5));
        for _ in 0..20 {
            zombie.velocity = Vec3::new(0.0, -0.5, 0.0);
            zombie.do_move(&w, &mut Vec::new());
        }
        zombie.position.y - top
    };
    let mut shulker = mob(2, "minecraft:shulker", Vec3::new(8.5, 64.0, 8.5));
    assert_eq!(landing(&shulker), 0.0, "a living shulker holds it up");
    shulker.health = 0.0;
    assert!(landing(&shulker) < -5.0, "a dead shulker does not");
    let mut ghast = mob(3, "minecraft:happy_ghast", Vec3::new(8.5, 64.0, 8.5));
    assert!(
        landing(&ghast) < -5.0,
        "a happy ghast left alone is not solid"
    );
    ghast.extra.counter = 10;
    assert_eq!(
        landing(&ghast),
        0.0,
        "held still, its back holds the zombie"
    );
    let mut ghastling = mob(4, "minecraft:happy_ghast", Vec3::new(8.5, 64.0, 8.5));
    super::super::finalize::set_baby(&mut ghastling, true);
    ghastling.extra.counter = 10;
    assert!(landing(&ghastling) < -5.0, "a ghastling never is");
}

/// `LivingEntity.aiStep` calls `travel` and only then
/// `applyEffectsFromBlocks`, so the lift of a bubble column is added after
/// the water's drag and is not scaled by it. A pig 1.0 high in a column of
/// two blocks of water takes the plain water velocity from `travel`, and
/// the lift (0.06 for each column block its box touches) on top of it.
#[test]
fn a_bubble_column_lifts_a_mob_after_travel_not_inside_it() {
    let run = |column: bool| {
        let mut out = None;
        let build = |world: &mut crate::world::World,
                     registry: &horizon_world::registry::BlockRegistry| {
            world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
            );
            let stone = registry.default_state("minecraft:stone").unwrap();
            let water = registry.default_state("minecraft:water").unwrap();
            let mut lift = registry
                .state(registry.default_state("minecraft:bubble_column").unwrap())
                .unwrap()
                .clone();
            lift.properties.insert("drag".into(), "false".into());
            let lift = registry.resolve(&lift).unwrap();
            for x in 0..=2 {
                for z in 0..=2 {
                    world.set(x, 0, z, stone).unwrap();
                    for y in 1..=6 {
                        world.set(x, y, z, water).unwrap();
                    }
                }
            }
            if column {
                for y in 1..=6 {
                    world.set(1, y, 1, lift).unwrap();
                }
            }
        };
        crate::mobs::world::with_built_test_world(&[], build, |w, types| {
            let kind = std::sync::Arc::new(types.types["minecraft:iron_golem"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(1.5, 2.0, 1.5), 0.0);
            m.fluids.in_water = true;
            m.fluids.water = 1.0;
            m.travel(w, Vec3::ZERO, &mut Vec::new());
            let after_travel = m.velocity.y;
            m.apply_effects_from_blocks(w, &mut Vec::new());
            out = Some((after_travel, m.velocity.y));
        });
        out
    };
    let (Some((plain, _)), Some((travelled, lifted))) = (run(false), run(true)) else {
        return;
    };
    assert_eq!(
        travelled, plain,
        "travel alone does not take the column's lift"
    );
    // The golem's 2.7 high box touches three column blocks (y 2, 3, 4).
    assert!(
        (lifted - plain - 3.0 * 0.06).abs() < 1.0e-9,
        "three column blocks lift it by 0.18 after the drag, got {}",
        lifted - plain
    );
}

/// `Entity.move`'s last step, `getBlockSpeedFactor`: a mob sliding on soul
/// sand (0.4) or honey (0.4) keeps 40% of its horizontal speed each move,
/// on stone it keeps all of it, and its vertical speed is untouched.
#[test]
fn a_mob_on_soul_sand_or_honey_is_slowed_by_the_move() {
    for (block, factor) in [
        ("minecraft:stone", 1.0),
        ("minecraft:soul_sand", 0.4),
        ("minecraft:honey_block", 0.4),
    ] {
        let build = |world: &mut crate::world::World,
                     registry: &horizon_world::registry::BlockRegistry| {
            world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
            );
            let id = registry.default_state(block).unwrap();
            for x in 0..=4 {
                for z in 0..=2 {
                    world.set(x, 0, z, id).unwrap();
                }
            }
        };
        crate::mobs::world::with_built_test_world(&[], build, |w, types| {
            let kind = std::sync::Arc::new(types.types["minecraft:zombie"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(0.5, 1.0, 1.5), 0.0);
            m.on_ground = true;
            m.velocity = Vec3::new(0.2, 0.0, 0.0);
            m.do_move(w, &mut Vec::new());
            let expected = f64::from(factor as f32) * 0.2;
            assert!(
                (m.velocity.x - expected).abs() < 1.0e-9,
                "{block}: velocity.x {} expected {expected}",
                m.velocity.x
            );
        });
    }
}

/// `Entity.collectColliders`: the world border's wall stops a mob's move
/// as it stops the player's (#7080). A border of size 10 around (8, 8)
/// has its east wall at x = 13.
#[test]
fn the_world_border_stops_a_mob() {
    let border =
        horizon_core::geometry::border::BorderGeometry::new([8.0, 8.0], 10.0, 29_999_984).unwrap();
    let build = |world: &mut crate::world::World, _: &horizon_world::registry::BlockRegistry| {
        world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
        );
    };
    crate::mobs::world::with_built_test_world(&[], build, |w, types| {
        let kind = std::sync::Arc::new(types.types["minecraft:pig"].clone());
        let mut east_edge = [0.0; 2];
        for (i, bordered) in [false, true].into_iter().enumerate() {
            let w = MobWorld {
                border: bordered.then_some(&border),
                ..w.clone()
            };
            // 1.5 blocks east from x = 11.5: past the wall at 13 if nothing stops it.
            let mut m = Mob::new(1, kind.clone(), Vec3::new(11.5, 64.0, 8.5), 0.0);
            m.velocity = Vec3::new(1.5, 0.0, 0.0);
            m.do_move(&w, &mut Vec::new());
            east_edge[i] = m.bounds().max_position().x;
            assert_eq!(m.horizontal_collision, bordered);
        }
        assert!(
            east_edge[0] > 13.0,
            "no border: the pig walks on, {east_edge:?}"
        );
        assert!(
            (east_edge[1] - 13.0).abs() < 1.0e-7,
            "the pig's east side stops on the wall at x = 13, {east_edge:?}"
        );
    });
}

/// `LivingEntity.checkFallDamage`'s dust: nothing within the safe distance,
/// `150 * min(0.2 + power / 15, 2.5)` block particles beyond it.
#[test]
fn a_landing_throws_up_dust_by_the_power_of_the_fall() {
    // Within the safe distance of 3, or by a hair over it: power 0.
    assert_eq!(landing_dust_count(2.5, 3.0), None);
    assert_eq!(landing_dust_count(3.0, 3.0), None);
    // A fall of 7: power 4, 150 * (0.2 + 4 / 15) = 70.
    assert_eq!(landing_dust_count(7.0, 3.0), Some(70));
    // A long fall is capped at 150 * 2.5.
    assert_eq!(landing_dust_count(60.0, 3.0), Some(375));
    // A mob that takes no fall damage still has a safe distance of its own.
    assert_eq!(landing_dust_count(7.0, 10.0), None);
}
/// Over the block it lands on the dust starts at the mob; straddling
/// into another column it is pulled to that block's near edge.
#[test]
fn landing_dust_is_pulled_to_the_block_it_landed_on() {
    let on = Vec3::new(4.3, 65.0, 7.6);
    assert_eq!(landing_dust_at(on, [4, 64, 7]), on);
    // The mob stands over block (4, 7) but its feet block is (5, 7).
    let straddling = landing_dust_at(Vec3::new(5.1, 65.0, 7.5), [4, 64, 7]);
    assert_eq!(straddling, Vec3::new(5.0, 65.0, 7.5));
}
/// `LivingEntity.getDimensions` is `getDefaultDimensions(pose).scale(
/// getScale())` on every living entity, not only on a player or mannequin
/// whose pose sets the box (`Avatar.POSES`): a cow's `EntityType.COW` box
/// (0.9 x 1.4, its eyes at 1.3) at `minecraft:scale` 2 is 1.8 x 2.8 with
/// the eyes at 2.6, and the box `Mob.bounds` hands the hitbox raycast, the
/// attack range and the leash pins grows with it.
#[test]
fn a_scale_attribute_grows_every_living_entitys_box() {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let mut cow = Mob::new(1, crate::mobs::sensor_scan_tests::kind(), Vec3::ZERO, 0.0);
    let (w, h, e) = cow.dims();
    assert!(
        close(w, 0.9) && close(h, 1.4) && close(e, 1.3),
        "no scale: {w}x{h}x{e}"
    );
    cow.extra.model_scale = 2.0;
    let (w, h, e) = cow.dims();
    assert!(
        close(w, 1.8) && close(h, 2.8) && close(e, 2.6),
        "scale 2: {w}x{h}x{e}"
    );
    let b = cow.bounds();
    assert!(
        close(b.min_position().x, -0.9)
            && close(b.max_position().x, 0.9)
            && close(b.max_position().y, 2.8),
        "scale 2 box: {b:?}"
    );
    // `RangedAttribute("attribute.name.scale", 1.0, 0.0625, 16.0)`: its
    // floor shrinks the cow to 0.05625 x 0.0875, eyes at 0.08125.
    cow.extra.model_scale = 0.0625;
    let (w, h, e) = cow.dims();
    assert!(
        close(w, 0.05625) && close(h, 0.0875) && close(e, 0.08125),
        "scale 0.0625: {w}x{h}x{e}"
    );
}
/// `LivingEntity.getDimensions` answers a sleeping entity with
/// `SLEEPING_DIMENSIONS` before it ever reaches `getScale`: a sleeper's
/// 0.2 box and eye height stay 0.2 whatever its `minecraft:scale`.
#[test]
fn a_sleeping_mob_keeps_its_bed_box_unscaled() {
    use crate::mobs::Species;
    let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
    kind.id = "minecraft:villager".into();
    kind.species = Species::Villager;
    let mut villager = Mob::new(1, std::sync::Arc::new(kind), Vec3::ZERO, 0.0);
    // Villager.sleeping: the bed's facing code, not 0.
    villager.extra.ints[1] = 1;
    villager.extra.model_scale = 4.0;
    let (w, h, e) = villager.dims();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(w, 0.2) && close(h, 0.2) && close(e, 0.2),
        "asleep: {w}x{h}x{e}"
    );
}
/// `LivingEntity.sanitizeScale`: a shulker takes at most 3 and a happy
/// ghast at most 1, and the box takes the capped value (`EntityType.SHULKER`
/// is 1 x 1 with its eyes at 0.5).
#[test]
fn a_species_caps_the_scale_it_takes() {
    use crate::mobs::Species;
    let mob = |id: &str, species: Species| {
        let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        kind.id = id.into();
        kind.species = species;
        (kind.width, kind.height, kind.eye_height) = (1.0, 1.0, 0.5);
        Mob::new(1, std::sync::Arc::new(kind), Vec3::ZERO, 0.0)
    };
    let mut shulker = mob("minecraft:shulker", Species::Shulker);
    shulker.extra.model_scale = 4.0;
    assert_eq!(shulker.model_scale(), 3.0);
    let (w, h, e) = shulker.dims();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(w, 3.0) && close(h, 3.0) && close(e, 1.5),
        "shulker: {w}x{h}x{e}"
    );
    let mut ghast = mob("minecraft:happy_ghast", Species::HappyGhast);
    ghast.extra.model_scale = 2.0;
    assert_eq!(ghast.model_scale(), 1.0);
    // Every other species takes the attribute as it came.
    let mut cow = mob("minecraft:cow", Species::Cow);
    cow.extra.model_scale = 4.0;
    assert_eq!(cow.model_scale(), 4.0);
}
/// `Sniffer.jumpFromGround`: a hop the move control asked for, taken from
/// nearly standing still, carries the sniffer a tenth of a block along its
/// yaw (`moveRelative(0.1F, (0, 0, 1))`). A sniffer that already moves, one
/// the move control did not ask to move, and every other species keep the
/// hop they had.
#[test]
fn a_sniffers_hop_from_a_standstill_carries_it_forward() {
    use crate::mobs::Species;
    let sniffer = |speed: f64, sideways: f64| {
        let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
        kind.id = "minecraft:sniffer".into();
        kind.species = Species::Sniffer;
        kind.jump_strength = 0.42;
        let mut m = Mob::new(1, std::sync::Arc::new(kind), Vec3::new(0.5, 64.0, 0.5), 0.0);
        // `MoveControl.getSpeedModifier` and the speed it moves at.
        m.ai.mov.speed_modifier = speed;
        m.velocity = Vec3::new(0.0, 0.0, sideways);
        m
    };
    // Yaw 0 faces south (+z), so the nudge lands on z.
    let hop = |m: &mut Mob| {
        let mut jumped = false;
        if crate::mobs::world::with_test_world(&[], |w, _| m.jump_from_ground(w)) {
            jumped = true;
        }
        jumped
    };
    let mut m = sniffer(1.0, 0.0);
    if !hop(&mut m) {
        return;
    }
    assert!(
        (m.velocity.z - 0.1).abs() < 1.0e-6 && m.velocity.x.abs() < 1.0e-6,
        "the hop goes forward: {:?}",
        m.velocity
    );
    assert!(m.velocity.y > 0.0, "and it is still a hop");
    // Already moving sideways: `horizontalDistanceSqr` is over the 0.01.
    let mut moving = sniffer(1.0, 0.2);
    assert!(hop(&mut moving));
    assert!(
        (moving.velocity.z - 0.2).abs() < 1.0e-6,
        "{:?}",
        moving.velocity
    );
    // The move control wanted nothing this tick.
    let mut still = sniffer(0.0, 0.0);
    assert!(hop(&mut still));
    assert_eq!(still.velocity.z, 0.0);
    // No other species gets the nudge.
    let mut kind = (*crate::mobs::sensor_scan_tests::kind()).clone();
    kind.jump_strength = 0.42;
    let mut cow = Mob::new(1, std::sync::Arc::new(kind), Vec3::new(0.5, 64.0, 0.5), 0.0);
    cow.ai.mov.speed_modifier = 1.0;
    assert!(hop(&mut cow));
    assert_eq!(cow.velocity.z, 0.0);
}

/// #8140: `LivingEntity.handleOnClimbable` slows any mob in vines, not
/// only a spider on a wall. A cow let go at rest moves before gravity
/// pulls it: 0, then 0.0784, then 0.155232 in open air ((v - 0.08) *
/// 0.98); in vines each move down from the third on is held to 0.15F,
/// and the fall distance, reset before each move, is that one move.
#[test]
fn a_mob_in_vines_slides_down_slowly() {
    let run = |vines: bool| {
        let mut out = None;
        let build = |world: &mut crate::world::World,
                     registry: &horizon_world::registry::BlockRegistry| {
            let stone = registry.default_state("minecraft:stone").unwrap();
            let mut vine = registry
                .state(registry.default_state("minecraft:vine").unwrap())
                .unwrap()
                .clone();
            vine.properties.insert("south".into(), "true".into());
            let vine = registry.resolve(&vine).unwrap();
            world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
            );
            world.set(1, 0, 1, stone).unwrap();
            for y in 1..=12 {
                world.set(1, y, 2, stone).unwrap();
                if vines {
                    world.set(1, y, 1, vine).unwrap();
                }
            }
        };
        crate::mobs::world::with_built_test_world(&[], build, |w, types| {
            let kind = std::sync::Arc::new(types.types["minecraft:cow"].clone());
            let mut m = Mob::new(1, kind, Vec3::new(1.5, 10.0, 1.5), 0.0);
            m.on_ground = false;
            let mut drops = Vec::new();
            for _ in 0..12 {
                let before = m.position.y;
                m.travel(w, Vec3::ZERO, &mut Vec::new());
                drops.push(before - m.position.y);
            }
            out = Some((drops, m.fall_distance));
        });
        out
    };
    let (Some((open, open_fall)), Some((vines, vine_fall))) = (run(false), run(true)) else {
        return;
    };
    assert_eq!(open[0], 0.0, "open air, tick 1: {open:?}");
    assert!(
        (open[1] - 0.0784).abs() < 1.0e-6,
        "open air, tick 2: {open:?}"
    );
    assert!(
        (open[2] - 0.155232).abs() < 1.0e-6,
        "open air, tick 3: {open:?}"
    );
    assert!(
        open[11] > 0.5 && open_fall > 3.0,
        "open air keeps speeding up: {open:?}"
    );
    let limit = f64::from(0.15f32);
    assert_eq!(vines[0], 0.0, "vines, tick 1: {vines:?}");
    assert!(
        (vines[1] - 0.0784).abs() < 1.0e-6,
        "vines, tick 2: {vines:?}"
    );
    for (tick, drop) in vines.iter().enumerate().skip(2) {
        assert!(
            (drop - limit).abs() < 1.0e-9,
            "vines, tick {}: {vines:?}",
            tick + 1
        );
    }
    // Reset before the move, then the move's own drop: never more.
    assert!(
        (vine_fall - limit).abs() < 1.0e-9,
        "the vines reset the fall each tick, got {vine_fall}"
    );
}

/// `LivingEntity.onClimbable`: `isFallFlying` in a block of
/// `#minecraft:can_glide_through` (vines) answers false before the
/// climbable test; a ladder is not in that tag, so a glider still holds
/// on to it.
#[test]
fn a_gliding_mob_passes_through_vines_but_not_a_ladder() {
    let build = |world: &mut crate::world::World,
                 registry: &horizon_world::registry::BlockRegistry| {
        world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
        );
        let vine = registry.default_state("minecraft:vine").unwrap();
        let ladder = registry.default_state("minecraft:ladder").unwrap();
        world.set(1, 5, 1, vine).unwrap();
        world.set(3, 5, 1, ladder).unwrap();
    };
    crate::mobs::world::with_built_test_world(&[], build, |w, types| {
        let kind = std::sync::Arc::new(types.types["minecraft:zombie"].clone());
        for (x, block, gliding_holds) in [(1.5, "vine", false), (3.5, "ladder", true)] {
            let mut m = Mob::new(1, kind.clone(), Vec3::new(x, 5.0, 1.5), 0.0);
            assert!(m.on_climbable(w), "{block}: climbable when not gliding");
            m.extra.fall_flying = true;
            assert_eq!(m.on_climbable(w), gliding_holds, "{block}: while gliding");
        }
    });
}
#[test]
fn damaging_mob_fall_plays_block_fall_sound() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |world, registry| {
            world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
            );
            for (x, block) in [(0, "minecraft:stone"), (1, "minecraft:oak_planks")] {
                world
                    .set(x, 0, 0, registry.default_state(block).unwrap())
                    .unwrap();
            }
        },
        |w, types| {
            for (x, block_sound) in [
                (0.5, "minecraft:block.stone.fall"),
                (1.5, "minecraft:block.wood.fall"),
            ] {
                let kind = std::sync::Arc::new(types.types["minecraft:zombie"].clone());
                let mut mob = Mob::new(1, kind, Vec3::new(x, 1.0, 0.5), 0.0);
                mob.fall_distance = 5.0;
                let mut events = Vec::new();
                mob.fall_on_block(w, &mut events);
                assert!(
                    matches!(events.as_slice(), [MobEvent::Sound("entity.hostile.small_fall", _, 1.0, 1.0), MobEvent::SoundId(id, _, 0.5, 0.75), ..] if id == block_sound),
                    "{events:?}"
                );
            }
        },
    );
    // Without the vanilla data under .local/horizon-client the world
    // cannot be built: the test has nothing to check.
    let _ = ran;
}

/// `FarmlandBlock.turnToBaseBlock`: a mob whose landing tramples farmland
/// posts `block_change` at the block, with the mob as its source, so a sculk
/// sensor, a calibrated sensor and a warden hear it - right after the write.
#[test]
fn a_mob_trampling_farmland_posts_block_change_from_it() {
    let ran = crate::mobs::world::with_built_test_world(
        &[],
        |world, registry| {
            world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
            );
            let state = registry.default_state("minecraft:farmland").unwrap();
            world.set(0, 0, 0, state).unwrap();
        },
        |w, types| {
            let dirt = w.registry.default_state("minecraft:dirt").unwrap();
            let kind = std::sync::Arc::new(types.types["minecraft:zombie"].clone());
            let mut mob = Mob::new(7, kind, Vec3::new(0.5, 1.0, 0.5), 0.0);
            mob.fall_distance = 5.0;
            let mut events = Vec::new();
            mob.fall_on_block(w, &mut events);
            // The write, then the event the rule posts for it.
            assert!(
                matches!(
                    events.as_slice(),
                    [
                        MobEvent::SetBlock {
                            pos: [0, 0, 0],
                            state,
                        },
                        MobEvent::LandingGameEvent {
                            event: "minecraft:block_change",
                            at: [0, 0, 0],
                            source: Some(7),
                            state: affected,
                        },
                        ..
                    ] if *state == dirt && *affected == dirt
                ),
                "{events:?}"
            );
        },
    );
    // Without the vanilla data under .local/horizon-client the world
    // cannot be built: the test has nothing to check.
    let _ = ran;
}

/// `Entity.applyMovementEmissionAndPlaySound` asks `getMovementEmission`:
/// a squid, a glow squid and a bat answer `EVENTS`, so a beached squid
/// flopping across stone sends the `minecraft:step` game event and no
/// sound, where a sheep walking the same block is heard at the block's
/// step volume times 0.15. A silverfish's own `playStepSound` names
/// `entity.silverfish.step` and is never heard either, and a shulker, whose
/// emission is `NONE`, sends nothing at all.
#[test]
fn a_squid_and_a_bat_step_without_a_sound() {
    let build = |world: &mut crate::world::World,
                 registry: &horizon_world::registry::BlockRegistry| {
        world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
        );
        let id = registry.default_state("minecraft:stone").unwrap();
        for x in 0..=8 {
            for z in 0..=8 {
                world.set(x, 0, z, id).unwrap();
            }
        }
    };
    crate::mobs::world::with_built_test_world(&[], build, |w, types| {
        // One step's worth of walking: 2 blocks times the 0.6 factor, over
        // the 1.0 the first step waits for.
        let walk = |id: &str| {
            let kind = std::sync::Arc::new(types.types[id].clone());
            let mut m = Mob::new(1, kind, Vec3::new(0.5, 1.0, 0.5), 0.0);
            m.on_ground = true;
            m.supporting = Some([0, 0, 0]);
            let mut events = Vec::new();
            m.movement_emission(w, Vec3::new(2.0, 0.0, 0.0), &mut events);
            events
        };
        let heard = |events: &[MobEvent]| {
            events.iter().find_map(|e| match e {
                MobEvent::Sound(sound, _, volume, pitch) => Some((*sound, *volume, *pitch)),
                _ => None,
            })
        };
        let stepped = |events: &[MobEvent]| {
            events.iter().any(
                |e| matches!(e, MobEvent::GameEvent { event, .. } if *event == "minecraft:step"),
            )
        };
        for quiet in ["minecraft:squid", "minecraft:glow_squid", "minecraft:bat"] {
            let events = walk(quiet);
            assert_eq!(heard(&events), None, "{quiet} is silent");
            assert!(stepped(&events), "{quiet} still sends the step event");
        }
        // A silverfish asks for its own step sound and its type's emission
        // never plays it.
        let events = walk("minecraft:silverfish");
        assert_eq!(heard(&events), None);
        assert!(stepped(&events));
        // The same walk from a type whose emission is ALL is heard.
        let events = walk("minecraft:sheep");
        assert_eq!(heard(&events), Some(("entity.sheep.step", 0.15, 1.0)));
        // Shulker.getMovementEmission is NONE: not even the event.
        let events = walk("minecraft:shulker");
        assert_eq!(heard(&events), None);
        assert!(!stepped(&events), "a shulker emits nothing");
    });
}

/// #9058: `LivingEntity.getJumpPower` calculates
/// `jump_strength * multiplier * getBlockJumpFactor() + getJumpBoostPower()`.
/// On honey blocks the jump factor is 0.5, halving the jump impulse.
/// Jump Boost effects add 0.1 * (amplifier + 1.0).
#[test]
#[ignore = "requires prepared vanilla resources; run explicitly with --ignored"]
fn living_jump_power_respects_honey_block_and_jump_boost() {
    let build = |world: &mut crate::world::World,
                 registry: &horizon_world::registry::BlockRegistry| {
        let stone = registry.default_state("minecraft:stone").unwrap();
        let honey = registry.default_state("minecraft:honey_block").unwrap();
        world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(crate::chunk::Chunk::empty(0, 0)),
        );
        // Stone column at (0, 0, 0)
        world.set(0, 0, 0, stone).unwrap();
        // Honey column at (2, 0, 0)
        world.set(2, 0, 0, honey).unwrap();
    };
    let ran = crate::mobs::world::with_built_test_world(&[], build, |w, types| {
        let cow_kind = std::sync::Arc::new(types.types["minecraft:cow"].clone());
        let close = |a: f32, b: f32| (a - b).abs() < 1.0e-5;

        // 1. Baseline: On normal block (stone, factor 1.0) with no effects, jump power matches m.attrs.jump_strength.
        let mut on_stone = Mob::new(1, cow_kind.clone(), Vec3::new(0.5, 1.0, 0.5), 0.0);
        assert!((on_stone.attrs.jump_strength - 0.42).abs() < 1.0e-5);
        assert!(close(on_stone.block_jump_factor(w), 1.0));
        assert_eq!(on_stone.jump_boost_power(), 0.0);
        assert!(close(on_stone.jump_power(w), 0.42));
        on_stone.jump_from_ground(w);
        assert!(
            (on_stone.velocity.y - 0.42).abs() < 1.0e-5,
            "stone baseline velocity.y"
        );

        // 2. Blocked negative / Reduction on honey: On honey block (jump factor 0.5), jump power is halved.
        let mut on_honey = Mob::new(2, cow_kind.clone(), Vec3::new(2.5, 1.0, 0.5), 0.0);
        assert!(
            close(on_honey.block_jump_factor(w), 0.5),
            "honey jump factor must be 0.5"
        );
        assert!(
            close(on_honey.jump_power(w), 0.42 * 0.5),
            "honey halving jump power"
        );
        on_honey.jump_from_ground(w);
        assert!(
            (on_honey.velocity.y - 0.21).abs() < 1.0e-5,
            "honey reduced velocity.y"
        );

        // 3. Enabled positive / Jump boost: With minecraft:jump_boost effect (amp 0 and amp 1),
        // jump power increases by +0.1 and +0.2 respectively.
        let mut boosted_0 = Mob::new(3, cow_kind.clone(), Vec3::new(0.5, 1.0, 0.5), 0.0);
        boosted_0
            .extra
            .effects
            .push(("minecraft:jump_boost", 200, 0));
        assert!(close(boosted_0.jump_boost_power(), 0.1));
        assert!(close(boosted_0.jump_power(w), 0.42 + 0.1));
        boosted_0.jump_from_ground(w);
        assert!((boosted_0.velocity.y - 0.52).abs() < 1.0e-5);

        let mut boosted_1 = Mob::new(4, cow_kind.clone(), Vec3::new(0.5, 1.0, 0.5), 0.0);
        boosted_1
            .extra
            .effects
            .push(("minecraft:jump_boost", 200, 1));
        assert!(close(boosted_1.jump_boost_power(), 0.2));
        assert!(close(boosted_1.jump_power(w), 0.42 + 0.2));
        boosted_1.jump_from_ground(w);
        assert!((boosted_1.velocity.y - 0.62).abs() < 1.0e-5);

        // 4. Edge / Special case: Combined honey block + jump boost and custom modified attrs.jump_strength.
        let mut combined = Mob::new(5, cow_kind.clone(), Vec3::new(2.5, 1.0, 0.5), 0.0);
        combined
            .extra
            .effects
            .push(("minecraft:jump_boost", 200, 1));
        // jump_strength * 0.5 + 0.2 = 0.42 * 0.5 + 0.2 = 0.41
        assert!(close(combined.jump_power(w), 0.42 * 0.5 + 0.2));
        combined.jump_from_ground(w);
        assert!((combined.velocity.y - 0.41).abs() < 1.0e-5);

        // Custom modified attribute (e.g. 0.80) on honey with jump boost 0 (0.1)
        let mut custom_attr = Mob::new(6, cow_kind, Vec3::new(2.5, 1.0, 0.5), 0.0);
        custom_attr.attrs.jump_strength = 0.80;
        custom_attr
            .extra
            .effects
            .push(("minecraft:jump_boost", 200, 0));
        // 0.80 * 0.5 + 0.1 = 0.50
        assert!(close(custom_attr.jump_power(w), 0.50));
        custom_attr.jump_from_ground(w);
        assert!((custom_attr.velocity.y - 0.50).abs() < 1.0e-5);

        // 5. Cube jump verification (Slime & MagmaCube on honey vs stone)
        let slime_kind = std::sync::Arc::new(types.types["minecraft:slime"].clone());
        let mut slime_stone = Mob::new(7, slime_kind.clone(), Vec3::new(0.5, 1.0, 0.5), 0.0);
        slime_stone.jump_from_ground(w);
        assert!((slime_stone.velocity.y - 0.42).abs() < 1.0e-5);

        let mut slime_honey = Mob::new(8, slime_kind, Vec3::new(2.5, 1.0, 0.5), 0.0);
        slime_honey.jump_from_ground(w);
        assert!(
            (slime_honey.velocity.y - 0.21).abs() < 1.0e-5,
            "slime jump on honey"
        );

        let magma_kind = std::sync::Arc::new(types.types["minecraft:magma_cube"].clone());
        let mut magma_honey = Mob::new(9, magma_kind, Vec3::new(2.5, 1.0, 0.5), 0.0);
        magma_honey.extra.size = 2; // boost = 2 * 0.1 = 0.2
        magma_honey.jump_from_ground(w);
        // 0.42 * 0.5 + 0.2 = 0.41
        assert!(
            (magma_honey.velocity.y - 0.41).abs() < 1.0e-5,
            "magma cube jump on honey"
        );
    });
    assert!(ran, "built test world ran");
}

/// `LivingEntity.getFlyingSpeed`: a horse its player steers (saddled, ridden) moves
/// through the air at a tenth of its speed; riderless or unsaddled at 0.02.
#[test]
fn a_steered_mount_moves_through_the_air_at_a_tenth_of_its_speed() {
    let mut kind = crate::mobs::sensor_scan_tests::kind();
    std::sync::Arc::make_mut(&mut kind).species = super::super::Species::Horse;
    let mut m = Mob::new(1, kind, Vec3::ZERO, 0.0);
    m.ai.speed = 0.3;
    assert_eq!(m.flying_speed(), 0.02, "no rider");
    m.rider = Some(super::super::equine::Rider::default());
    assert_eq!(m.flying_speed(), 0.02, "unsaddled");
    m.extra.saddled = true;
    assert_eq!(m.flying_speed(), 0.3_f32 * 0.1);
}

/// `Strider.checkFallDamage`: in lava the fall is reset before the base runs, so
/// a strider that drops into a lava sea lands with no damage and no dust — and
/// walks out owing nothing — while a pig in the same lava takes the fall, and a
/// strider landing on stone takes it too.
#[test]
fn a_strider_landing_in_lava_takes_no_fall_damage() {
    use std::sync::Arc;
    // A stone floor with a lava pool on it: the mob stands at y = 1.
    let landed = |id: &str, lava: bool| -> Option<(f32, f64, bool)> {
        let mut out = None;
        let ran = crate::mobs::world::with_built_test_world(
            &[],
            move |world, registry| {
                let stone = registry.default_state("minecraft:stone").unwrap();
                let lava_state = registry.default_state("minecraft:lava").unwrap();
                for cx in -1..1 {
                    for cz in -1..1 {
                        world
                            .chunks
                            .insert((cx, cz), Arc::new(crate::chunk::Chunk::empty(cx, cz)));
                    }
                }
                for x in -4..4 {
                    for z in -4..4 {
                        world.set(x, 1, z, stone).unwrap();
                    }
                }
                if lava {
                    for x in -4..4 {
                        for z in -4..4 {
                            for y in 2..=4 {
                                world.set(x, y, z, lava_state).unwrap();
                            }
                        }
                    }
                }
            },
            |w, types| {
                let kind = Arc::new(types.types[id].clone());
                let mut m = Mob::new(1, kind, Vec3::new(0.5, 2.0, 0.5), 0.0);
                m.on_ground = true;
                // Six blocks of fall: three of damage on a safe distance of 3.
                m.fall_distance = 6.0;
                let start = m.health;
                let mut events = Vec::new();
                m.check_fall_damage(w, Vec3::ZERO, &mut events);
                let dust = events
                    .iter()
                    .any(|e| matches!(e, MobEvent::LandingDust { .. }));
                out = Some((start - m.health, m.fall_distance, dust));
            },
        );
        if !ran {
            return None;
        }
        out
    };
    let Some((strider_lava, rest, dust)) = landed("minecraft:strider", true) else {
        return;
    };
    assert_eq!(strider_lava, 0.0, "a strider in lava takes no fall damage");
    assert_eq!(rest, 0.0, "and its fall is reset");
    assert!(!dust, "and no dust is sent up");
    let (pig_lava, _, _) = landed("minecraft:pig", true).unwrap();
    assert_eq!(pig_lava, 3.0, "a pig in the same lava takes the fall");
    let (strider_stone, _, _) = landed("minecraft:strider", false).unwrap();
    assert_eq!(
        strider_stone, 3.0,
        "a strider landing on stone takes it too"
    );
}
