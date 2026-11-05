use super::*;
use horizon_core::geometry::{Axis, VoxelShape};
use horizon_world::{
    block_collision::BlockCollisionRegistry,
    collision::CachedCollisionRegistry,
    nbt::Tag,
    registry::{BlockRegistry, BlockStateId},
};
use std::sync::Arc;

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

fn fixture() -> (BlockRegistry, crate::blocks::BlockFacts, Tag) {
    let names = ["air", "soul_sand", "stone", "scaffolding", "oak_slab"];
    let states: Vec<_> = names
        .iter()
        .map(|name| {
            let mut fields = vec![("id", text(&format!("minecraft:{name}")))];
            if *name == "scaffolding" {
                fields.push((
                    "properties",
                    Tag::compound([
                        ("distance", text("7")),
                        ("bottom", text("false")),
                        ("waterlogged", text("false")),
                    ]),
                ));
            }
            Tag::compound(fields)
        })
        .collect();
    let registry_tag = Tag::compound([
        ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ("states", Tag::List(states.clone())),
        (
            "defaults",
            Tag::Compound(
                names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        (
                            format!("minecraft:{name}").as_str().into(),
                            Tag::Int(i as i32),
                        )
                    })
                    .collect(),
            ),
        ),
    ]);
    let registry = BlockRegistry::from_nbt(&registry_tag).unwrap();
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "horizon-shulker-space-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    // Original minimal facts: air identity and generic sturdy flags, deliberately
    // distinct from the soul sand's lower collision shape.
    let facts = serde_json::json!({"states":(0..names.len()).map(|id|serde_json::json!({"id":id})).collect::<Vec<_>>()});
    std::fs::write(root.join("gameplay-facts.json"), facts.to_string()).unwrap();
    let behaviours = serde_json::json!({"states":[[0,0],[63,0],[63,0],[0,0],[1,0]],"blocks":names.iter().map(|name|serde_json::json!({"name":format!("minecraft:{name}"),"class":["Block"]})).collect::<Vec<_>>()});
    let facts =
        crate::blocks::BlockFacts::load_with_behaviour_json(&root, &registry, &behaviours).unwrap();
    std::fs::remove_dir_all(root).unwrap();
    let box_shape =
        |height| VoxelShape::from_box(Aabb::new(Vec3::ZERO, Vec3::new(1.0, height, 1.0))).unwrap();
    let shapes = [
        VoxelShape::empty(),
        box_shape(0.875),
        VoxelShape::block(),
        VoxelShape::block(),
        box_shape(0.5),
    ];
    let table = Tag::compound([
        ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
        ("states", Tag::List(states)),
        ("shapes", Tag::List(shapes.iter().map(shape_tag).collect())),
        ("cached_shapes", Tag::IntArray(vec![0, 1, 2, 3, 4])),
    ]);
    (registry, facts, table)
}

fn check(current: Vec3, cells: &[([i32; 3], u32)], target: [i32; 3], face: u8) -> bool {
    let (registry, facts, table) = fixture();
    let cached = CachedCollisionRegistry::from_nbt(&table, &registry).unwrap();
    let binding = BlockCollisionRegistry::new(Arc::new(cached)).unwrap();
    let mut world = crate::world::World::new(BlockStateId(0), 0);
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
    let mut mob = super::tests::shulker();
    mob.position = current;
    BlockSpace {
        world: &world,
        registry: &registry,
        facts: &facts,
        binding: &binding,
        powder_walkable: false,
    }
    .can_stay_at(&mob, target, face)
}

#[test]
fn soul_sand_only_accepts_attachment_to_its_bottom() {
    for (face, step) in STEPS.iter().enumerate() {
        let support = [step[0], 64 + step[1], step[2]];
        assert_eq!(
            check(
                Vec3::new(8.5, 64.0, 8.5),
                &[(support, 1)],
                [0, 64, 0],
                face as u8
            ),
            face == 1,
            "face {face}"
        );
    }
}

#[test]
fn scaffolding_uses_the_shulkers_current_height_before_and_after_teleport() {
    let cells = [([0, 63, 0], 3), ([0, 65, 0], 4)];
    assert!(check(Vec3::new(0.5, 62.0, 0.5), &cells, [0, 64, 0], 1));
    assert!(!check(Vec3::new(0.5, 64.0, 0.5), &cells, [0, 64, 0], 1));
}

#[test]
#[ignore = "run by tools/test_shulker_space_parity.py"]
fn shulker_space_parity_fixture() {
    use std::io::Write;
    let input =
        std::fs::read_to_string(std::env::var("HORIZON_SHULKER_SPACE_INPUT").unwrap()).unwrap();
    let mut output =
        std::fs::File::create(std::env::var("HORIZON_SHULKER_SPACE_OUTPUT").unwrap()).unwrap();
    for line in input.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let current = &row["current"];
        let current = Vec3::new(
            current[0].as_f64().unwrap(),
            current[1].as_f64().unwrap(),
            current[2].as_f64().unwrap(),
        );
        let target = std::array::from_fn(|i| row["target"][i].as_i64().unwrap() as i32);
        let cells: Vec<_> = row["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                let pos = std::array::from_fn(|i| cell[i].as_i64().unwrap() as i32);
                let state = match cell[3].as_str().unwrap() {
                    "minecraft:air" => 0,
                    "minecraft:soul_sand" => 1,
                    "minecraft:stone" => 2,
                    "minecraft:scaffolding" => 3,
                    "minecraft:oak_slab" => 4,
                    _ => panic!("unknown fixture block"),
                };
                (pos, state)
            })
            .collect();
        writeln!(
            output,
            "{}",
            check(current, &cells, target, row["face"].as_u64().unwrap() as u8)
        )
        .unwrap();
    }
}
