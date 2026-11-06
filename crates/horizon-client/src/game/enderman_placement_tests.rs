//! Exercise the actual registered leave-block goal and Game event handler.
use super::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    mobs::{Mob, MobEvent, goals::Cx},
};
use horizon_core::{geometry::Vec3, position::ChunkPos, random::LegacyRandom};
use std::sync::Arc;

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn leave_block_goal_uses_real_neighbor_shapes_and_keeps_air_placement() {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
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
    options.execution_role = super::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    let registry = game.pack.assets.registry;
    let air = registry.default_state("minecraft:air").unwrap();
    let stone = registry.default_state("minecraft:stone").unwrap();
    let kind = game
        .res
        .mob_types()
        .get("minecraft:enderman")
        .unwrap()
        .clone();
    for (name, expected_name) in [("poppy", "air"), ("note_block", "note_block")] {
        for x in 0..8 {
            for z in 0..8 {
                for y in 3..7 {
                    game.world
                        .set(x, y, z, if y == 3 { stone } else { air })
                        .unwrap();
                }
            }
        }
        game.mobs.clear();
        let carried = registry
            .default_state(&format!("minecraft:{name}"))
            .unwrap();
        let mut mob = Mob::new(325, kind.clone(), Vec3::new(3.5, 4.0, 3.5), 0.0);
        mob.random = LegacyRandom::new(325);
        mob.extra.carried = Some(carried);
        game.mobs.push(mob);
        for _ in 0..48_000 {
            let mut mob = game.mobs.pop().unwrap();
            let mut goals = std::mem::take(&mut mob.ai.goals);
            let snapshot = [super::creatures::seen(&mob, None)];
            let mut events = Vec::<MobEvent>::new();
            let mut lr = game.world.random.clone();
            {
                let world = game.mob_world_base(&snapshot);
                goals.tick(
                    &mut mob,
                    &mut Cx {
                        w: &world,
                        lr: &mut lr,
                        events: &mut events,
                    },
                );
            }
            game.world.random = lr;
            mob.ai.goals = goals;
            game.mobs.push(mob);
            game.handle_mob_events(events).unwrap();
            if game.mobs[0].extra.carried.is_none() {
                break;
            }
        }
        assert_eq!(
            game.mobs[0].extra.carried, None,
            "eligible {name} must empty its hand"
        );
        let mut states = Vec::new();
        for x in 0..8 {
            for z in 0..8 {
                for y in 4..7 {
                    let state = game.world.block(x, y, z);
                    if state != air {
                        states.push(registry.state(state).unwrap());
                    }
                }
            }
        }
        if expected_name == "air" {
            assert!(
                states.is_empty(),
                "poppy normalizes to AIR before placement, not a dropped unsupported flower"
            );
        } else {
            assert_eq!(states.len(), 1);
            assert_eq!(states[0].name, "minecraft:note_block");
            assert_eq!(
                states[0].properties.get("instrument").map(String::as_str),
                Some("basedrum")
            );
        }
    }
    // A waterlogged candidate schedules the real level's fluid tick even
    // when a preceding world write makes the placement ineligible.
    let fence = registry.default_state("minecraft:oak_fence").unwrap();
    let mut descriptor = registry.state(fence).unwrap().clone();
    descriptor
        .properties
        .insert("waterlogged".into(), "true".into());
    let fence = registry.resolve(&descriptor).unwrap();
    let mut mob = Mob::new(326, kind, Vec3::new(3.5, 4.0, 3.5), 0.0);
    mob.random = LegacyRandom::new(325);
    mob.extra.carried = Some(fence);
    game.mobs.clear();
    game.mobs.push(mob);
    let mut attempt = None;
    for _ in 0..48_000 {
        let mut mob = game.mobs.pop().unwrap();
        let mut goals = std::mem::take(&mut mob.ai.goals);
        let snapshot = [super::creatures::seen(&mob, None)];
        let mut events = Vec::new();
        let mut lr = game.world.random.clone();
        goals.tick(
            &mut mob,
            &mut Cx {
                w: &game.mob_world_base(&snapshot),
                lr: &mut lr,
                events: &mut events,
            },
        );
        game.world.random = lr;
        mob.ai.goals = goals;
        game.mobs.push(mob);
        if let Some(event) = events
            .into_iter()
            .find(|e| matches!(e, MobEvent::EndermanPlaceBlock { .. }))
        {
            attempt = Some(event);
            break;
        }
    }
    let attempt = attempt.expect("the registered leave goal emits an attempt");
    let MobEvent::EndermanPlaceBlock { pos, mob, .. } = &attempt else {
        unreachable!()
    };
    assert_eq!(*mob, 326);
    game.world.set(pos[0], pos[1], pos[2], stone).unwrap();
    let before = game.block_ticks.fluids.count();
    // A stale carried-state identity cannot mutate the world or clear a new hand.
    game.mobs[0].extra.carried = Some(stone);
    game.handle_mob_events(vec![attempt.clone()]).unwrap();
    assert_eq!(game.block_ticks.fluids.count(), before);
    assert_eq!(game.mobs[0].extra.carried, Some(stone));
    game.mobs[0].extra.carried = Some(fence);
    game.handle_mob_events(vec![attempt.clone()]).unwrap();
    assert!(game.block_ticks.fluids.count() > before);
    assert_eq!(game.world.block(pos[0], pos[1], pos[2]), stone);
    assert_eq!(game.mobs[0].extra.carried, Some(fence));
}
