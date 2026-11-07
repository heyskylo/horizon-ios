//! Rust side of tools/test_falling_block_parity.py: replays the landing branch
//! of `FallingBlockEntity.tick` over the case's block map through
//! `horizon_world::level::Behaviours::falling_block_landed` and prints the same
//! decision, changed blocks and numeric level events as the Java oracle.
use horizon_core::{position::BlockPos, random::LegacyRandom};
use horizon_world::{
    fluid::FluidKind,
    level::{Behaviours, Dimension, Landing, LevelAccess, LevelEvent, Ticks, data::load_registry},
    registry::BlockStateId,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::Path,
};

struct Cells {
    cells: HashMap<BlockPos, BlockStateId>,
    air: BlockStateId,
    random: LegacyRandom,
    events: Vec<String>,
}
impl LevelAccess for Cells {
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.cells.get(&pos).copied().unwrap_or(self.air)
    }
    fn set_raw(&mut self, pos: BlockPos, state: BlockStateId) -> Option<BlockStateId> {
        let old = self.block_state(pos);
        if state == self.air {
            self.cells.remove(&pos);
        } else {
            self.cells.insert(pos, state);
        }
        Some(old)
    }
    fn is_loaded(&self, _pos: BlockPos) -> bool {
        true
    }
    fn min_y(&self) -> i32 {
        -64
    }
    fn max_y(&self) -> i32 {
        320
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn game_time(&self) -> i64 {
        0
    }
    fn dimension(&self) -> Dimension {
        Dimension::Overworld
    }
    fn emit(&mut self, event: LevelEvent) {
        if let LevelEvent::LevelEvent { id, pos, data } = event
            && id == 1031
        {
            self.events
                .push(format!("L {} {},{},{} {}", id, pos.x, pos.y, pos.z, data));
        }
    }
}

fn pos(v: &Value) -> BlockPos {
    BlockPos::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}

fn main_landing(case: &Value, b: &Behaviours, air: BlockStateId, out: &mut Vec<String>) {
    let mut level = Cells {
        cells: HashMap::new(),
        air,
        random: LegacyRandom::new(case["seed"].as_i64().unwrap()),
        events: Vec::new(),
    };
    for c in case["cells"].as_array().unwrap() {
        let state = BlockStateId(c[3].as_i64().unwrap() as u32);
        if state != air {
            level.cells.insert(pos(c), state);
        }
    }
    let at = pos(&case["pos"]);
    let before = level.cells.clone();
    let original = BlockStateId(case["state"].as_i64().unwrap() as u32);
    // `FallingBlockEntity.fall` drops the WATERLOGGED property before the entity
    // carries the state and puts the source's legacy fluid block at the cell;
    // `falling_block_fall` does the same for the caller. The case's destination
    // state is then restored for the landing probe.
    let state = b
        .table
        .with_bool(original, b.props.waterlogged, false)
        .unwrap_or(original);
    let destination = level.cells.get(&at).copied();
    let legacy = b.legacy_block(b.fluid(original));
    if legacy == air {
        level.cells.remove(&at);
    } else {
        level.cells.insert(at, legacy);
    }
    if let Some(destination) = destination {
        level.cells.insert(at, destination);
    }
    let stuck = matches!(
        b.fluid(level.block_state(at)).kind(),
        FluidKind::Water | FluidKind::FlowingWater
    );
    let mut ticks = Ticks::new(0);
    let landing = b.falling_block_landed(&mut level, &mut ticks, at, state, stuck);
    match landing {
        Landing::Placed(placed) => {
            out.push(format!("decision placed {}", placed.0));
        }
        Landing::Broken => out.push("decision broken".into()),
        Landing::Continue => out.push("decision continue".into()),
    }
    let mut changed: Vec<BlockPos> = before
        .keys()
        .copied()
        .chain(level.cells.keys().copied())
        .collect();
    changed.sort_by_key(|p| (p.x, p.y, p.z));
    changed.dedup();
    for p in changed {
        let now = level.block_state(p);
        if before.get(&p).copied().unwrap_or(air) != now {
            out.push(format!("set {},{},{} {}", p.x, p.y, p.z, now.0));
        }
    }
    out.extend(level.events);
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = crate::args().skip(1);
    let resources = args.next().expect("resource directory");
    let cases = args.next().expect("case file");
    let output = args.next().expect("output file");
    let resources = Path::new(&resources);
    let registry = load_registry(resources)?;
    let behaviours = Behaviours::load(&registry, resources)?;
    let air = registry.default_state("minecraft:air").unwrap();
    let mut out = std::io::BufWriter::new(std::fs::File::create(output)?);
    for (index, line) in std::io::BufReader::new(std::fs::File::open(cases)?)
        .lines()
        .enumerate()
    {
        let case: Value = serde_json::from_str(&line?)?;
        let mut lines = Vec::new();
        main_landing(&case, &behaviours, air, &mut lines);
        writeln!(out, "== {index}")?;
        for line in lines {
            writeln!(out, "{line}")?;
        }
    }
    Ok(())
}
