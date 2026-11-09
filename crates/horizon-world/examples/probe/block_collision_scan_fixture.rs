use crate::support::shapes::emit;
use horizon_core::{
    geometry::{Aabb, ShapeOp, Vec3, VoxelShape},
    position::{BlockPos, ChunkPos},
};
use horizon_world::{
    block_collision_scan::{BlockCollisionScan, BlockCollisionWorld, CollisionBlock},
    collision_context::{CollisionContext, CollisionEntity, EntityContextKind},
    entity_obstruction::EntityIdentity,
    ground_support::find_supporting_block,
    registry::BlockStateId,
};
use serde_json::Value;
use std::{
    cell::RefCell,
    fs,
    io::{self, Write},
    path::Path,
    rc::Rc,
};
fn number(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}

fn shape(index: u64) -> VoxelShape {
    let cuboid = |min, max| VoxelShape::from_box(Aabb::new(min, max)).unwrap();
    match index {
        0 => VoxelShape::empty(),
        1 => VoxelShape::block(),
        2 => VoxelShape::block().moved(Vec3::ZERO),
        3 => cuboid(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0)),
        4 => cuboid(Vec3::new(-0.25, -0.5, -0.25), Vec3::new(1.25, 1.5, 1.25)),
        5 => cuboid(Vec3::ZERO, Vec3::new(1e-8, 1.0, 1.0)),
        6 => cuboid(Vec3::ZERO, Vec3::new(0.25, 1.0, 0.25))
            .join(
                &cuboid(Vec3::new(0.75, 0.0, 0.75), Vec3::new(1.0, 1.0, 1.0)),
                ShapeOp::OR,
            )
            .unwrap(),
        _ => unreachable!(),
    }
}
struct Entry {
    block: CollisionBlock,
    shape: VoxelShape,
    suffocating: bool,
}
struct World<W> {
    out: Rc<RefCell<W>>,
    calls: i32,
    fail: i32,
    missing: i32,
    seed: i32,
    palette: Vec<Entry>,
}
impl<W: Write> World<W> {
    fn query(&mut self, value: &str) -> io::Result<()> {
        writeln!(self.out.borrow_mut(), "{value}")?;
        let index = self.calls;
        self.calls += 1;
        if index == self.fail {
            return Err(io::Error::other("injected collision scan failure"));
        }
        Ok(())
    }
}
impl<W: Write> BlockCollisionWorld for World<W> {
    type Error = io::Error;
    type Chunk = ChunkPos;
    fn collision_chunk(&mut self, p: ChunkPos) -> io::Result<Option<ChunkPos>> {
        self.query(&format!("chunk {} {}", p.x, p.z))?;
        Ok(
            if self.missing == 1 || self.missing == 2 && (p.x ^ p.z) & 1 != 0 {
                None
            } else {
                Some(p)
            },
        )
    }
    fn collision_block(&mut self, chunk: ChunkPos, p: BlockPos) -> io::Result<CollisionBlock> {
        assert_eq!(chunk, p.chunk());
        self.query(&format!("block {} {} {}", p.x, p.y, p.z))?;
        let hash =
            p.x.wrapping_mul(3)
                .wrapping_add(p.y.wrapping_mul(5))
                .wrapping_add(p.z.wrapping_mul(7))
                .wrapping_add(self.seed)
                & i32::MAX;
        Ok(self.palette[hash as usize % self.palette.len()].block)
    }
    fn is_suffocating(
        &mut self,
        chunk: ChunkPos,
        block: CollisionBlock,
        p: BlockPos,
    ) -> io::Result<bool> {
        assert_eq!(chunk, p.chunk());
        self.query(&format!(
            "suffocating {} {} {} {}",
            p.x, p.y, p.z, block.state.0
        ))?;
        Ok(self.palette[block.state.0 as usize].suffocating)
    }
    fn context_shape(
        &mut self,
        block: CollisionBlock,
        p: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        self.query(&format!("shape {} {} {} {}", p.x, p.y, p.z, block.state.0))?;
        Ok(self.palette[block.state.0 as usize].shape.clone())
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).expect("private fixture directory");
    let dir = Path::new(&arg);
    let out = Rc::new(RefCell::new(io::BufWriter::new(fs::File::create(
        dir.join("rust.txt"),
    )?)));
    for line in fs::read_to_string(dir.join("inputs.txt"))?.lines() {
        let f: Value = serde_json::from_str(line)?;
        let palette = f["palette"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, v)| Entry {
                block: CollisionBlock {
                    state: BlockStateId(i as u32),
                    large_collision_shape: v[1].as_bool().unwrap(),
                    moving_piston: v[2].as_bool().unwrap(),
                    empty_shape: false,
                },
                shape: shape(v[0].as_u64().unwrap()),
                suffocating: v[3].as_bool().unwrap(),
            })
            .collect();
        let mut world = World {
            out: out.clone(),
            calls: 0,
            fail: f["fail"].as_i64().unwrap() as i32,
            missing: f["missing"].as_i64().unwrap() as i32,
            seed: f["seed"].as_i64().unwrap() as i32,
            palette,
        };
        let v = &f["box"];
        let bounds = Aabb::new(
            Vec3::new(number(&v[0]), number(&v[1]), number(&v[2])),
            Vec3::new(number(&v[3]), number(&v[4]), number(&v[5])),
        );
        if let Some(p) = f.get("support_position") {
            let position = Vec3::new(number(&p[0]), number(&p[1]), number(&p[2]));
            let context = CollisionContext::for_entity(
                CollisionEntity {
                    identity: EntityIdentity(1),
                    kind: EntityContextKind::Other,
                    y: position.y,
                    descending: false,
                    main_hand: None,
                },
                false,
            );
            match find_supporting_block(&mut world, context, position, bounds, 1_000_000) {
                Ok(Some(p)) => writeln!(out.borrow_mut(), "support {} {} {}", p.x, p.y, p.z)?,
                Ok(None) => writeln!(out.borrow_mut(), "support none")?,
                Err(_) => writeln!(out.borrow_mut(), "support error")?,
            }
            writeln!(out.borrow_mut(), "done support {}", world.calls)?;
            continue;
        }
        let mut hits = 0;
        {
            let mut scan = BlockCollisionScan::new(
                &mut world,
                CollisionContext::empty(false),
                bounds,
                f["suffocating"].as_bool().unwrap(),
                1_000_000,
            )?;
            for _ in 0..f["limit"].as_u64().unwrap() {
                match scan.next() {
                    None => break,
                    Some(Ok(hit)) => {
                        let p = hit.position;
                        writeln!(out.borrow_mut(), "hit {} {} {}", p.x, p.y, p.z)?;
                        emit(&mut *out.borrow_mut(), &hit.shape)?;
                        hits += 1;
                    }
                    Some(Err(_)) => {
                        writeln!(out.borrow_mut(), "error")?;
                        break;
                    }
                }
            }
        }
        writeln!(out.borrow_mut(), "done {hits} {}", world.calls)?;
    }
    Ok(())
}
