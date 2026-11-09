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
    let mut options = Options::parse(
        [
            "--game-dir".to_owned(),
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ]
        .into_iter(),
    )
    .unwrap()
    .unwrap();
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

fn sampled_attempt(game: &mut Game<'_>, overlap_self: bool) -> MobEvent {
    for _ in 0..240_000 {
        let mut mob = game.mobs.remove(0);
        let mut goals = std::mem::take(&mut mob.ai.goals);
        let mut snapshot = vec![super::creatures::seen(&mob, None)];
        snapshot.extend(game.mobs.iter().map(|m| super::creatures::seen(m, None)));
        let own = mob.bounds();
        let mut events = Vec::new();
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
        game.mobs.insert(0, mob);
        for event in events {
            if let MobEvent::EndermanPlaceBlock { pos, .. } = &event {
                let cube = horizon_core::geometry::Aabb::new(
                    Vec3::new(pos[0] as f64, pos[1] as f64, pos[2] as f64),
                    Vec3::new(
                        pos[0] as f64 + 1.0,
                        pos[1] as f64 + 1.0,
                        pos[2] as f64 + 1.0,
                    ),
                );
                if pos[1] == 4 && cube.intersects(own) == overlap_self {
                    return event;
                }
            }
        }
    }
    panic!("registered goal did not emit the bounded candidate");
}

#[test]
#[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
fn leave_block_raw_query_uses_non_mob_presence_and_snapshot_identity() {
    use horizon_client::{
        items::ItemStack,
        projectiles::{Projectile, ProjectileKind},
    };
    let (mut game, host) = super::lan_actions::vehicle_loot_tests::fixture();
    game.boats.clear();
    game.ui.difficulty = 2;
    let registry = game.pack.assets.registry;
    let air = registry.default_state("minecraft:air").unwrap();
    let stone = registry.default_state("minecraft:stone").unwrap();
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    let kind = game.res.mob_types()["minecraft:enderman"].clone();
    let pig_type = game.res.mob_types()["minecraft:pig"].clone();
    game.lan = Some(host);
    let mut failures = Vec::new();
    for (label, blocked, self_hit, pig_kind) in [
        ("empty", false, false, 0),
        ("self", false, true, 0),
        ("already_removed_pig", false, true, 1),
        ("dying_pig", true, true, 2),
        ("later_moved_pig", true, true, 3),
        ("later_removed_pig", true, true, 3),
        ("item", true, false, 0),
        ("arrow", true, false, 0),
        ("marker", true, false, 0),
        ("player", true, false, 0),
        ("waiting_player", true, false, 0),
        ("dead_player", true, false, 0),
        ("expired_player", false, false, 0),
        ("spectator", false, false, 0),
        ("cactus_bad_support", true, false, 0),
    ] {
        for x in 0..8 {
            for z in 0..8 {
                for y in 3..8 {
                    game.world
                        .set(x, y, z, if y == 3 { stone } else { air })
                        .unwrap();
                }
            }
        }
        game.mobs.clear();
        game.dropped.clear();
        game.creatures.projectiles.clear();
        game.others.clear();
        for g in &mut game.lan.as_mut().unwrap().hub.guests {
            g.position = [20.5, 65.0, 20.5];
            g.motion = None;
            g.dead = false;
            g.death_time = 0;
            g.health = 20.0;
            g.placed = true;
            g.loaded = true;
            g.load_timeout = 0;
            g.game_mode = 0;
        }
        let carried = if label == "cactus_bad_support" {
            registry.default_state("minecraft:cactus").unwrap()
        } else {
            stone
        };
        let mut actor = Mob::new(325, kind.clone(), Vec3::new(3.5, 4.0, 3.5), 0.0);
        actor.random = LegacyRandom::new(325);
        actor.extra.carried = Some(carried);
        game.mobs.push(actor);
        if pig_kind > 0 {
            let mut pig = Mob::new(326, pig_type.clone(), Vec3::new(3.5, 4.0, 3.5), 0.0);
            pig.removed = pig_kind == 1;
            if pig_kind == 2 {
                pig.health = 0.0;
            }
            game.mobs.push(pig);
        }
        let event = sampled_attempt(&mut game, self_hit);
        let MobEvent::EndermanPlaceBlock { pos, .. } = &event else {
            unreachable!()
        };
        let pos = *pos;
        let at = Vec3::new(
            pos[0] as f64 + 0.5,
            pos[1] as f64 + 0.2,
            pos[2] as f64 + 0.5,
        );
        match label {
            "item" => game.spawn_item(
                ItemStack::new("minecraft:stone", 1),
                [at.x, at.y, at.z],
                [0.0; 3],
                0,
            ),
            "arrow" => {
                let mut arrow = Projectile::new(
                    ProjectileKind::Arrow {
                        base_damage: 2.0,
                        critical: false,
                        effect: None,
                    },
                    at,
                    None,
                );
                arrow.id = 900;
                game.creatures.projectiles.push(arrow);
            }
            "marker" => game.others.push(super::commands::OtherEntity {
                id: 900,
                kind: "minecraft:marker".into(),
                position: at,
                yaw: 0.0,
                pitch: 0.0,
                item_transformation: None,
                interaction: None,
                knot_check: None,
                eye: None,
            }),
            "player" | "waiting_player" | "dead_player" | "expired_player" | "spectator" => {
                let g = &mut game.lan.as_mut().unwrap().hub.guests[0];
                g.position = [at.x, pos[1] as f64, at.z];
                g.loaded = label != "waiting_player";
                g.load_timeout = if g.loaded { 0 } else { 60 };
                g.dead = label == "dead_player" || label == "expired_player";
                g.health = if g.dead { 0.0 } else { 20.0 };
                g.death_time = if label == "expired_player" { 20 } else { 0 };
                if label == "spectator" {
                    g.game_mode = 3;
                }
            }
            "later_moved_pig" => game.mobs[1].position = Vec3::new(50.0, 4.0, 50.0),
            "later_removed_pig" => game.mobs[1].removed = true,
            _ => {}
        }
        super::dispense_fixture::start_recording();
        game.handle_mob_events(vec![event]).unwrap();
        let trace = super::dispense_fixture::stop_recording();
        let kept = game.mobs[0].extra.carried.is_some();
        let changed = game.world.block(pos[0], pos[1], pos[2]) != air;
        if kept != blocked || changed == blocked {
            failures.push(format!(
                "{label}: kept={kept}, changed={changed}, expected_blocked={blocked}"
            ));
        }
        let emits = trace
            .iter()
            .filter(|l| l.ends_with(" minecraft:block_place"))
            .count();
        if emits != usize::from(!blocked) {
            failures.push(format!("{label}: block-place events={emits}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
