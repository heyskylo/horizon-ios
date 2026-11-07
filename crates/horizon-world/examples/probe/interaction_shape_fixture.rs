use crate::support::common::f64_bits;
use horizon_core::{
    geometry::{Aabb, Axis, ShapeOp, Vec3, VoxelShape},
    position::BlockPos,
};
use horizon_world::{
    interaction_shapes::InteractionShapes,
    nbt::{self, Limits, Tag},
    registry::{BlockRegistry, BlockStateId},
    world_ray::{RayBlockMode, RayWorld, clip_with_interaction},
};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::Path,
};
fn number(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}
fn vector(v: &Value) -> Vec3 {
    Vec3::new(number(&v[0]), number(&v[1]), number(&v[2]))
}
fn shape(v: &Value) -> Result<VoxelShape, Box<dyn std::error::Error>> {
    let mut result = VoxelShape::empty();
    for b in v.as_array().unwrap() {
        result = result.join(
            &VoxelShape::from_box(Aabb::new(
                Vec3::new(number(&b[0]), number(&b[1]), number(&b[2])),
                Vec3::new(number(&b[3]), number(&b[4]), number(&b[5])),
            ))?,
            ShapeOp::OR,
        )?;
    }
    Ok(result)
}
struct World<'a, 'r> {
    shapes: &'a InteractionShapes<'r>,
}
impl RayWorld for World<'_, '_> {
    type Error = io::Error;
    type Block = BlockStateId;
    type Fluid = ();
    fn block_state(&mut self, _: BlockPos) -> io::Result<BlockStateId> {
        panic!("direct interaction clip")
    }
    fn fluid_state(&mut self, _: BlockPos) -> io::Result<()> {
        panic!("direct interaction clip")
    }
    fn block_shape(
        &mut self,
        _: BlockStateId,
        _: BlockPos,
        _: RayBlockMode,
    ) -> io::Result<VoxelShape> {
        panic!("direct interaction clip")
    }
    fn interaction_shape(&mut self, s: BlockStateId, _: BlockPos) -> io::Result<VoxelShape> {
        Ok(self.shapes.shape(s)?.clone())
    }
    fn fluid_is_source(&mut self, _: ()) -> io::Result<bool> {
        panic!("direct interaction clip")
    }
    fn fluid_is_empty(&mut self, _: ()) -> io::Result<bool> {
        panic!("direct interaction clip")
    }
    fn fluid_is_water(&mut self, _: ()) -> io::Result<bool> {
        panic!("direct interaction clip")
    }
    fn fluid_shape(&mut self, _: (), _: BlockPos) -> io::Result<VoxelShape> {
        panic!("direct interaction clip")
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).expect("private fixture directory");
    let dir = Path::new(&arg);
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    let read = |n: &str| -> Result<Tag, Box<dyn std::error::Error>> {
        Ok(nbt::read_named(&fs::read(dir.join(n))?, Limits::default())?
            .0
            .tag)
    };
    let registry = BlockRegistry::from_nbt(&read("block-registry.nbt")?)?;
    let data = read("interaction-shapes.nbt")?;
    let shapes = InteractionShapes::from_nbt(&data, &registry)?;
    assert!(shapes.shape(BlockStateId(registry.len() as u32)).is_err());
    let mut bad = data.clone();
    *bad.get_mut("kind").unwrap() = Tag::String("collision".into());
    assert!(InteractionShapes::from_nbt(&bad, &registry).is_err());
    let mut bad = data.clone();
    if let Some(Tag::IntArray(ids)) = bad.get_mut("cached_shapes") {
        ids[0] = -1;
    }
    assert!(InteractionShapes::from_nbt(&bad, &registry).is_err());
    let mut bad = data.clone();
    if let Some(Tag::List(shapes)) = bad.get_mut("shapes")
        && let Tag::Compound(first) = &mut shapes[0]
    {
        first.remove(&"canonical".into());
    }
    assert!(InteractionShapes::from_nbt(&bad, &registry).is_err());
    let mut bad = data.clone();
    if let Some(Tag::List(states)) = bad.get_mut("states") {
        states.swap(0, 1);
    }
    assert!(InteractionShapes::from_nbt(&bad, &registry).is_err());
    let mut identities = Vec::<VoxelShape>::new();
    for state in 0..registry.len() {
        let s = shapes.shape(BlockStateId(state as u32))?;
        let id = identities
            .iter()
            .position(|old| old.same_identity(s))
            .unwrap_or_else(|| {
                identities.push(s.clone());
                identities.len() - 1
            });
        writeln!(
            out,
            "state {state} {id} {} {} {}",
            s.same_identity(&VoxelShape::empty()),
            s.same_identity(&VoxelShape::block()),
            s.boxes().len()
        )?;
        for b in s.boxes() {
            writeln!(
                out,
                "box {:016x} {:016x} {:016x} {:016x} {:016x} {:016x}",
                f64_bits(b.min(Axis::X)),
                f64_bits(b.min(Axis::Y)),
                f64_bits(b.min(Axis::Z)),
                f64_bits(b.max(Axis::X)),
                f64_bits(b.max(Axis::Y)),
                f64_bits(b.max(Axis::Z))
            )?;
        }
    }
    let mut w = World { shapes: &shapes };
    for line in fs::read_to_string(dir.join("inputs.txt"))?.lines() {
        let f: Value = serde_json::from_str(line)?;
        let p = &f["position"];
        let p = BlockPos::new(
            p[0].as_i64().unwrap() as i32,
            p[1].as_i64().unwrap() as i32,
            p[2].as_i64().unwrap() as i32,
        );
        match clip_with_interaction(
            &mut w,
            vector(&f["from"]),
            vector(&f["to"]),
            p,
            &shape(&f["shape"])?,
            BlockStateId(f["state"].as_u64().unwrap() as u32),
        )? {
            Some(h) => writeln!(
                out,
                "hit {:016x} {:016x} {:016x} {} {} {} {} {}",
                f64_bits(h.location.x),
                f64_bits(h.location.y),
                f64_bits(h.location.z),
                format!("{:?}", h.direction).to_lowercase(),
                h.block.x,
                h.block.y,
                h.block.z,
                h.inside
            )?,
            None => writeln!(out, "none")?,
        }
    }
    Ok(())
}
