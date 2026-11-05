use horizon_core::position::BlockPos;
use horizon_world::{
    climbing::{ClimbingRegistry, ClimbingWorld, on_climbable},
    nbt::{self, Limits},
    registry::{BlockRegistry, BlockStateId},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::Path,
};
fn pos(v: &Value) -> BlockPos {
    BlockPos::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}
fn text(p: BlockPos) -> String {
    format!("{} {} {}", p.x, p.y, p.z)
}
struct World<'a> {
    op: &'a Value,
    out: &'a mut dyn Write,
    calls: i64,
    position: BlockPos,
    last: Option<BlockPos>,
}
impl World<'_> {
    fn query(&mut self, s: impl AsRef<str>) -> io::Result<()> {
        writeln!(self.out, "{}", s.as_ref())?;
        let n = self.calls;
        self.calls += 1;
        if n == self.op["fail"].as_i64().unwrap() {
            return Err(io::Error::other("injected climbing failure"));
        }
        if n == self.op["mutate_at"].as_i64().unwrap() {
            self.position = BlockPos::new(31, 32, 33);
            self.last = Some(BlockPos::new(-8, -9, -10));
        }
        Ok(())
    }
}
impl ClimbingWorld for World<'_> {
    type Error = io::Error;
    fn is_spectator(&mut self) -> io::Result<bool> {
        self.query("spectator")?;
        Ok(self.op["spectator"].as_bool().unwrap())
    }
    fn block_position(&mut self) -> io::Result<BlockPos> {
        self.query("position")?;
        Ok(self.position)
    }
    fn in_block_state(&mut self) -> io::Result<BlockStateId> {
        self.query("in-state")?;
        Ok(BlockStateId(self.op["state"].as_u64().unwrap() as u32))
    }
    fn is_fall_flying(&mut self) -> io::Result<bool> {
        self.query("gliding")?;
        Ok(self.op["gliding"].as_bool().unwrap())
    }
    fn block_state(&mut self, p: BlockPos) -> io::Result<BlockStateId> {
        self.query(format!("below {}", text(p)))?;
        Ok(BlockStateId(self.op["below"].as_u64().unwrap() as u32))
    }
    fn set_last_climbable_position(&mut self, p: BlockPos) {
        self.last = Some(p);
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).unwrap();
    let dir = Path::new(&arg);
    let registry = BlockRegistry::from_nbt(
        &nbt::read_named(
            &fs::read(dir.join("block-registry.nbt"))?,
            Limits::default(),
        )?
        .0
        .tag,
    )?;
    let info: Value = serde_json::from_str(&fs::read_to_string(dir.join("info.json"))?)?;
    let names = |key: &str| -> BTreeSet<String> {
        info[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    let all: BTreeSet<String> = (0..registry.len())
        .map(|i| registry.state(BlockStateId(i as u32)).unwrap().name.clone())
        .collect();
    let trapdoors = names("trapdoors");
    let climb = [names("climbable"), BTreeSet::new(), all.clone()];
    let glide = [names("glide_through"), BTreeSet::new(), all];
    let mut bindings = Vec::new();
    for c in &climb {
        for g in &glide {
            bindings.push(ClimbingRegistry::new(&registry, &trapdoors, c, g)?);
        }
    }
    let invalid = BTreeSet::from(["horizon:missing".to_owned()]);
    assert!(ClimbingRegistry::new(&registry, &invalid, &climb[0], &glide[0]).is_err());
    assert!(ClimbingRegistry::new(&registry, &trapdoors, &invalid, &glide[0]).is_err());
    assert!(ClimbingRegistry::new(&registry, &trapdoors, &climb[0], &invalid).is_err());
    // A tagged block that is not a trapdoor (stone has no `open`) is not an error: vanilla tests
    // `instanceof TrapDoorBlock`, so it is just not one.
    assert!(
        ClimbingRegistry::new(
            &registry,
            &BTreeSet::from(["minecraft:stone".to_owned()]),
            &climb[0],
            &glide[0]
        )
        .is_ok()
    );
    assert!(
        bindings[0]
            .is_climbable_tag(BlockStateId(u32::MAX))
            .is_err()
    );
    assert!(std::ptr::eq(bindings[0].registry(), &registry));
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    for line in fs::read_to_string(dir.join("inputs.txt"))?.lines() {
        let f: Value = serde_json::from_str(line)?;
        let mut last = Some(BlockPos::new(7, 8, 9));
        let binding = &bindings[f["climb_tags"].as_u64().unwrap() as usize * 3
            + f["glide_tags"].as_u64().unwrap() as usize];
        for op in f["ops"].as_array().unwrap() {
            let mut w = World {
                op,
                out: &mut out,
                calls: 0,
                position: pos(&op["position"]),
                last,
            };
            match on_climbable(&mut w, binding) {
                Ok(b) => writeln!(w.out, "result {b}")?,
                Err(_) => writeln!(w.out, "error")?,
            };
            writeln!(w.out, "position {}", text(w.position))?;
            if let Some(p) = w.last {
                writeln!(w.out, "last {}", text(p))?;
            } else {
                writeln!(w.out, "last none")?;
            }
            writeln!(w.out, "queries {}", w.calls)?;
            last = w.last;
        }
    }
    Ok(())
}
