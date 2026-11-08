//! Shulker bullets for tools/test_shulker_bullet_parity.py: where a shulker's
//! bullet starts (`shulker::bullet_origin` through `Projectile::shulker_bullet`)
//! and the block test that turns it in flight
//! (`collisions::projectile_can_stand_on`), one line per fixture.
#![forbid(unsafe_code)]
use horizon_client::{
    Result, collisions,
    mobs::{Mob, MobTypes, Who, shulker},
    projectiles::{Projectile, ProjectileWorld},
    world::World,
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
};
use horizon_world::{chunk::BlockState, registry::BlockStateId};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    sync::Arc,
};

/// The bullet's first move only asks which neighbours are empty: all are.
struct Open(Vec<(Who, Aabb)>);
impl ProjectileWorld for Open {
    fn clip(&self, _: Vec3, _: Vec3) -> Option<(Vec3, [i32; 3], [i32; 3])> {
        None
    }
    fn in_water(&self, _: Aabb) -> bool {
        false
    }
    fn targets(&self) -> &[(Who, Aabb)] {
        &self.0
    }
}

fn vector(v: &Value) -> Vec3 {
    let n = |i: usize| v[i].as_f64().unwrap();
    Vec3::new(n(0), n(1), n(2))
}

/// `"minecraft:oak_slab[type=top]"` as a registry state.
fn parse_state(text: &str) -> BlockState {
    let (name, properties) = text.split_once('[').unwrap_or((text, ""));
    BlockState {
        name: name.into(),
        properties: properties
            .trim_end_matches(']')
            .split(',')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (key, value) = p.split_once('=').unwrap();
                (key.to_string(), value.to_string())
            })
            .collect(),
    }
}

pub fn main() -> Result<()> {
    let args: Vec<_> = crate::args().collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    let registry = horizon_world::level::data::load_registry(&root)?;
    let binding = collisions::load(&root, &registry)?;
    let types = MobTypes::load(&root)?;
    let kind = Arc::new(
        types
            .types
            .get("minecraft:shulker")
            .ok_or("no shulker")?
            .clone(),
    );
    let mut out = BufWriter::new(File::create(&args[2])?);
    for line in BufReader::new(File::open(&args[1])?).lines() {
        let f: Value = serde_json::from_str(&line?)?;
        if f["mode"] == "origin" {
            let mut m = Mob::new(1, kind.clone(), vector(&f["position"]), 0.0);
            m.extra.variant3 = f["face"].as_u64().unwrap() as u8;
            m.anim.v[shulker::PEEK] = f32::from_bits(f["peek"].as_u64().unwrap() as u32);
            m.extra.model_scale = f["scale"].as_f64().unwrap() as f32;
            let target = Aabb::new(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5))
                .move_by(vector(&f["target"]));
            let axis = match m.extra.variant3 {
                0 | 1 => 1,
                2 | 3 => 2,
                _ => 0,
            };
            let world = Open(vec![(Who::Mob(2), target)]);
            let at = shulker::bullet_origin(&m);
            let p = Projectile::shulker_bullet(&world, at, Who::Mob(1), Who::Mob(2), axis, 1);
            let b = p.position;
            writeln!(
                out,
                "{:016x} {:016x} {:016x}",
                b.x.to_bits(),
                b.y.to_bits(),
                b.z.to_bits()
            )?;
        } else {
            let pos = &f["pos"];
            let pos = BlockPos::new(
                pos[0].as_i64().unwrap() as i32,
                pos[1].as_i64().unwrap() as i32,
                pos[2].as_i64().unwrap() as i32,
            );
            let mut world = World::new(BlockStateId(0), 0);
            let chunk = (pos.x >> 4, pos.z >> 4);
            world.chunks.insert(
                chunk,
                Arc::new(horizon_client::chunk::Chunk::empty(chunk.0, chunk.1)),
            );
            let state = registry.resolve(&parse_state(f["block"].as_str().unwrap()))?;
            world.set(pos.x, pos.y, pos.z, state)?;
            let y = f["y"].as_f64().unwrap();
            writeln!(
                out,
                "{}",
                collisions::projectile_can_stand_on(&world, &binding, pos, y)
            )?;
        }
    }
    Ok(())
}
