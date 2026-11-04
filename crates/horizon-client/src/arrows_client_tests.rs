use super::*;
use horizon_core::geometry::{Axis, VoxelShape};
use horizon_world::{collision::CachedCollisionRegistry, nbt::Tag, registry::BlockRegistry};
use std::{collections::BTreeSet, sync::Arc};
fn text(value: &str) -> Tag {
    Tag::String(value.into())
}
fn shape_tag(shape: &VoxelShape) -> Tag {
    let sizes = shape.sizes();
    let mut bits = Vec::new();
    for z in 0..sizes[2] {
        for y in 0..sizes[1] {
            for x in 0..sizes[0] {
                bits.push(u8::from(shape.is_full(x as i32, y as i32, z as i32)));
            }
        }
    }
    let (min, max) = shape.discrete_bounds();
    let mut fields = vec![
        ("occupied", Tag::ByteArray(bits)),
        ("cube", Tag::Byte(i8::from(shape.is_cube()))),
        (
            "cube_axes",
            Tag::Byte(
                shape
                    .cube_coordinate_axes()
                    .iter()
                    .enumerate()
                    .fold(0, |mask, (i, on)| mask | (i8::from(*on) << i)),
            ),
        ),
        ("discrete_min", Tag::IntArray(min.to_vec())),
        ("discrete_max", Tag::IntArray(max.to_vec())),
    ];
    for (axis, name) in [(Axis::X, "x"), (Axis::Y, "y"), (Axis::Z, "z")] {
        fields.push((
            name,
            Tag::LongArray(
                shape
                    .coordinates(axis)
                    .iter()
                    .map(|v| v.to_bits() as i64)
                    .collect(),
            ),
        ));
    }
    Tag::compound(fields)
}

fn run(cells: &[([i32; 3], u32)], ticks: usize, shared_fire: bool) -> Vec<Projectile> {
    run_with(cells, ticks, shared_fire, |_| {})
}
fn run_with(
    cells: &[([i32; 3], u32)],
    ticks: usize,
    shared_fire: bool,
    setup: impl FnOnce(&mut Projectile),
) -> Vec<Projectile> {
    let mut definitions = vec![("air", None), ("stone", None)];
    for level in 0..16 {
        definitions.push(("water", Some(("level", level.to_string()))));
    }
    for drag in [false, true] {
        definitions.push(("bubble_column", Some(("drag", drag.to_string()))));
    }
    let states: Vec<_> = definitions
        .iter()
        .map(|(name, property)| {
            let mut fields = vec![("id", text(&format!("minecraft:{name}")))];
            if let Some((key, value)) = property {
                fields.push(("properties", Tag::compound([(*key, text(value))])));
            }
            Tag::compound(fields)
        })
        .collect();
    let registry = BlockRegistry::from_nbt(&Tag::compound([
        ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ("states", Tag::List(states.clone())),
        (
            "defaults",
            Tag::compound([
                ("minecraft:air", Tag::Int(0)),
                ("minecraft:stone", Tag::Int(1)),
                ("minecraft:water", Tag::Int(2)),
                ("minecraft:bubble_column", Tag::Int(18)),
            ]),
        ),
    ]))
    .unwrap();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "horizon-client-arrow-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("gameplay-facts.json"),
        serde_json::json!({"states":(0..states.len()).map(|id|serde_json::json!({"id":id})).collect::<Vec<_>>()} ).to_string(),
    )
    .unwrap();
    let behaviours = serde_json::json!({"states":(0..states.len()).map(|id|if id==1{[63,0]}else{[0,0]}).collect::<Vec<_>>(),"blocks":(["air","stone","water","bubble_column"].iter().map(|name|serde_json::json!({"name":format!("minecraft:{name}"),"class":["Block"]})).collect::<Vec<_>>())});
    let facts = BlockFacts::load_with_behaviour_json(&root, &registry, &behaviours).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    let table = Tag::compound([
        ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ("states", Tag::List(states)),
        (
            "shapes",
            Tag::List(vec![
                shape_tag(&VoxelShape::empty()),
                shape_tag(&VoxelShape::block()),
            ]),
        ),
        (
            "cached_shapes",
            Tag::IntArray((0..20).map(|id| i32::from(id == 1)).collect()),
        ),
    ]);
    let collision = BlockCollisionRegistry::new(Arc::new(
        CachedCollisionRegistry::from_nbt(&table, &registry).unwrap(),
    ))
    .unwrap();
    let fluids = FluidResources::from_registry(&registry, &facts, &BTreeSet::new()).unwrap();
    let mut world = World::new(BlockStateId(0), 0);
    for x in -1..=1 {
        for z in -1..=1 {
            world
                .chunks
                .insert((x, z), Arc::new(crate::chunk::Chunk::empty(x, z)));
        }
    }
    for &(p, state) in cells {
        world.set(p[0], p[1], p[2], BlockStateId(state)).unwrap();
    }
    let scene = ArrowScene {
        world: &world,
        registry: &registry,
        facts: &facts,
        collision: &collision,
        fluids: &fluids,
        raining: &|_| false,
        mob_griefing: true,
        tnt_explodes: true,
    };
    let mut p = Projectile::new(
        ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        },
        Vec3::new(0.5, 18.5, 0.5),
        None,
    );
    p.velocity = Vec3::new(1.0, 0.2, 0.0);
    p.arrow.as_mut().unwrap().synced_fire = shared_fire;
    p.arrow.as_mut().unwrap().fire_ticks = 80;
    setup(&mut p);
    let mut result = Vec::new();
    for _ in 0..ticks {
        p.arrow.as_mut().unwrap().events.clear();
        tick_client(&mut p, &scene);
        result.push(p.clone());
    }
    result
}
#[test]
fn first_water_tick_moves_before_water_inertia_and_keeps_synced_flames() {
    let mut cells = Vec::new();
    for x in -2..6 {
        for y in 16..21 {
            for z in -2..3 {
                cells.push(([x, y, z], 2));
            }
        }
    }
    for shared_fire in [false, true] {
        let steps = run(&cells, 5, shared_fire);
        let p = &steps[0];
        assert_eq!(p.position, Vec3::new(1.5, 18.7, 0.5));
        assert_eq!(p.velocity.x.to_bits(), 0x3fef_ae14_8000_0000);
        assert_eq!(p.velocity.y.to_bits(), 0x3fc2_f1aa_0000_0000);
        for p in steps {
            let st = p.arrow.as_ref().unwrap();
            assert!(st.was_touching_water);
            assert_eq!(st.fire_ticks, 0);
            assert_eq!(st.on_fire(), shared_fire);
        }
    }
}

#[test]
#[ignore = "run by client-arrow differential harness"]
fn client_arrow_parity_fixture() {
    use std::io::Write;
    let input =
        std::fs::read_to_string(std::env::var("HORIZON_CLIENT_ARROW_INPUT").unwrap()).unwrap();
    let mut output =
        std::fs::File::create(std::env::var("HORIZON_CLIENT_ARROW_OUTPUT").unwrap()).unwrap();
    let bits = |v: Vec3| {
        format!(
            "{:016x},{:016x},{:016x}",
            v.x.to_bits(),
            v.y.to_bits(),
            v.z.to_bits()
        )
    };
    for line in input.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let cells: Vec<_> = row["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                let at = std::array::from_fn(|i| cell[i].as_i64().unwrap() as i32);
                let id = match cell[3].as_str().unwrap() {
                    "minecraft:air" => 0,
                    "minecraft:stone" => 1,
                    "minecraft:water" => {
                        2 + cell.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as u32
                    }
                    "minecraft:bubble_column" => {
                        18 + u32::from(cell.get(4).and_then(|v| v.as_bool()).unwrap_or(false))
                    }
                    other => panic!("unsupported fixture block {other}"),
                };
                (at, id)
            })
            .collect();
        assert_eq!(row["position"], serde_json::json!([0.5, 18.5, 0.5]));
        assert_eq!(row["velocity"], serde_json::json!([1, 0.2, 0]));
        let steps = run_with(
            &cells,
            row["ticks"].as_u64().unwrap() as usize,
            row["sharedFire"].as_bool().unwrap_or(false),
            |p| {
                p.no_gravity = row["noGravity"].as_bool().unwrap_or(false);
                let st = p.arrow.as_mut().unwrap();
                st.no_physics = row["noPhysics"].as_bool().unwrap_or(false);
                if let Some(in_ground) = row["inGround"].as_bool() {
                    st.in_ground = in_ground;
                    p.stuck = in_ground;
                    st.life = 1199;
                }
            },
        );
        let records: Vec<_> = steps
            .iter()
            .map(|p| {
                let st = p.arrow.as_ref().unwrap();
                let mut record = format!(
                    "{}/{}/{}",
                    bits(p.position),
                    bits(p.velocity),
                    st.was_touching_water
                );
                if row.get("sharedFire").is_some() {
                    record.push_str(&format!("/{}/{}", st.fire_ticks, st.on_fire()));
                }
                if row.get("inGround").is_some() {
                    record.push_str(&format!("/{}", st.removed));
                }
                record
            })
            .collect();
        writeln!(output, "{}", records.join(" ")).unwrap();
    }
}

#[test]
fn water_bubbles_use_damped_motion_and_crit_streaks_use_original_motion() {
    let cells: Vec<_> = (-2..6)
        .flat_map(|x| (16..21).flat_map(move |y| (-2..3).map(move |z| ([x, y, z], 2))))
        .collect();
    let steps = run_with(&cells, 2, false, |p| p.set_critical(true));
    let previous = &steps[0];
    let current = &steps[1];
    let original = previous.velocity;
    let damped = original.scale(f64::from(0.6f32));
    let events = &current.arrow.as_ref().unwrap().events;
    let bubbles: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ArrowEvent::Particle {
                kind: ArrowParticle::Bubble,
                position,
                velocity,
            } => Some((*position, *velocity)),
            _ => None,
        })
        .collect();
    assert_eq!(bubbles.len(), 4);
    for (position, velocity) in bubbles {
        assert_eq!(velocity, damped);
        assert_eq!(position, previous.position.add_vector(damped.scale(-0.25)));
    }
    let crits: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ArrowEvent::Particle {
                kind: ArrowParticle::Crit,
                position,
                velocity,
            } => Some((*position, *velocity)),
            _ => None,
        })
        .collect();
    assert_eq!(crits.len(), 4);
    for (i, (position, velocity)) in crits.into_iter().enumerate() {
        assert_eq!(
            position,
            previous.position.add_vector(original.scale(i as f64 / 4.0))
        );
        assert_eq!(
            velocity,
            Vec3::new(-original.x, -original.y + 0.2, -original.z)
        );
    }
}
#[test]
fn air_and_stuck_arrows_leave_no_water_bubbles() {
    for critical in [false, true] {
        let steps = run_with(&[], 1, false, |p| p.set_critical(critical));
        let events = &steps[0].arrow.as_ref().unwrap().events;
        assert!(!events.iter().any(|e| matches!(
            e,
            ArrowEvent::Particle {
                kind: ArrowParticle::Bubble,
                ..
            }
        )));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    ArrowEvent::Particle {
                        kind: ArrowParticle::Crit,
                        ..
                    }
                ))
                .count(),
            if critical { 4 } else { 0 }
        );
    }
    let steps = run_with(&[([0, 18, 0], 1)], 2, false, |p| p.set_critical(true));
    assert!(steps[1].stuck);
    assert!(
        !steps[1]
            .arrow
            .as_ref()
            .unwrap()
            .events
            .iter()
            .any(|e| matches!(e, ArrowEvent::Particle { .. }))
    );
}
#[test]
fn remote_arrow_preserves_previous_rotation_as_it_turns_and_falls() {
    let steps = run_with(&[], 30, false, |p| {
        p.shoot(&mut LegacyRandom::new(7), [0.0, 1.0, 0.0], 0.3, 0.0);
        assert_eq!(p.previous_pitch, p.pitch);
        assert_eq!(p.previous_yaw, p.yaw);
        assert!((p.pitch - 90.0).abs() < 1e-4);
    });
    for pair in steps.windows(2) {
        assert_eq!(pair[1].previous_pitch, pair[0].pitch);
        assert_eq!(pair[1].previous_yaw, pair[0].yaw);
    }
    assert!(steps.last().unwrap().pitch < 0.0);
}
/// `Entity.playSound` hands `Level.playSound` no player and
/// `ClientLevel.playSeededSound` plays only for its own player, so a client's
/// arrow makes no sound itself: the server's sound packets carry its hit and
/// splash.
#[test]
fn a_client_arrow_hitting_stone_or_entering_water_plays_no_sound_itself() {
    let stone = run(&[([1, 18, 0], 1)], 2, false);
    assert!(
        stone.iter().any(|p| p.stuck),
        "the arrow sticks in the stone"
    );
    let mut water = Vec::new();
    for x in 2..8 {
        for y in 16..21 {
            for z in -2..3 {
                water.push(([x, y, z], 2));
            }
        }
    }
    let wet = run(&water, 4, false);
    assert!(
        wet.last()
            .unwrap()
            .arrow
            .as_ref()
            .unwrap()
            .was_touching_water,
        "the arrow entered the water"
    );
    for p in stone.iter().chain(&wet) {
        let events = &p.arrow.as_ref().unwrap().events;
        assert!(
            !events.iter().any(|e| matches!(e, ArrowEvent::Sound { .. })),
            "{events:?}"
        );
    }
}
