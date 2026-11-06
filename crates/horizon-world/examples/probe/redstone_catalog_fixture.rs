//! Rust side of `tools/test_redstone_catalog_parity.py`: builds each Logical
//! Redstone Archive contraption in a map-backed level, drives its levers and
//! buttons like a player's click and runs whole level ticks (scheduled ticks,
//! block events, moving pistons), printing what `VanillaRedstoneCatalogOracle`
//! prints. Comparator levels of containers and lecterns come from the oracle
//! (`analog`): nothing here moves items.
use horizon_core::{
    geometry::{Direction, Vec3},
    position::{BlockPos, ChunkPos},
    random::LegacyRandom,
};
use horizon_world::{
    level::{Behaviours, LevelAccess, PlayerContext, Ticks, data::load_registry},
    registry::BlockStateId,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{BufRead, Write},
    path::Path,
};

struct Cells {
    cells: HashMap<BlockPos, BlockStateId>,
    air: BlockStateId,
    random: LegacyRandom,
    time: i64,
    analog: HashMap<BlockPos, i32>,
}
impl LevelAccess for Cells {
    fn block_state(&self, pos: BlockPos) -> BlockStateId {
        self.cells.get(&pos).copied().unwrap_or(self.air)
    }
    fn set_raw(&mut self, pos: BlockPos, state: BlockStateId) -> Option<BlockStateId> {
        let old = if state == self.air {
            self.cells.remove(&pos)
        } else {
            self.cells.insert(pos, state)
        };
        Some(old.unwrap_or(self.air))
    }
    fn is_loaded(&self, _pos: BlockPos) -> bool {
        true
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn game_time(&self) -> i64 {
        self.time
    }
    fn block_entity_analog_signal(
        &self,
        pos: BlockPos,
        _state: BlockStateId,
        _direction: Direction,
    ) -> i32 {
        self.analog.get(&pos).copied().unwrap_or(0)
    }
}

/// The oracle's `mix`: one block's share of the box hash.
fn mix(x: i32, y: i32, z: i32, value: u64) -> u64 {
    let mut k = (x as i64 as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add((y as i64 as u64).wrapping_mul(0xC2B2AE3D27D4EB4F))
        .wrapping_add((z as i64 as u64).wrapping_mul(0x165667B19E3779F9))
        .wrapping_add(value.wrapping_mul(0xD6E8FEB86659FD93));
    k = (k ^ (k >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    k = (k ^ (k >> 27)).wrapping_mul(0x94D049BB133111EB);
    k ^ (k >> 31)
}

fn ints(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect()
}

fn pos_of(c: &[i64]) -> BlockPos {
    BlockPos::new(c[0] as i32, c[1] as i32, c[2] as i32)
}

/// What the registry says about a state: lever (and whether it is on), button, comparator.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Lever(bool),
    Button,
    Comparator,
    Other,
}

struct Run<'a> {
    behaviours: &'a Behaviours,
    kinds: &'a [Kind],
    level: Cells,
    ticks: Ticks,
    outputs: Vec<BlockPos>,
    /// The hashed box: the schematic and a margin of 2.
    min: BlockPos,
    max: BlockPos,
    /// Ticks the contraption may still run (the oracle counts the same).
    left: i64,
    /// `HORIZON_CATALOG_TRACE`: every block at a step's end, as `# cell` lines.
    trace: bool,
    out: String,
}
impl Run<'_> {
    fn kind(&self, state: BlockStateId) -> Kind {
        self.kinds
            .get(state.0 as usize)
            .copied()
            .unwrap_or(Kind::Other)
    }
    fn comparator(&self, pos: BlockPos) -> Option<i32> {
        (self.kind(self.level.block_state(pos)) == Kind::Comparator).then(|| {
            self.ticks
                .comparator_outputs
                .get(&pos)
                .copied()
                .unwrap_or(0)
        })
    }
    fn outputs(&self) -> String {
        let mut s = String::new();
        for (i, &pos) in self.outputs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let _ = write!(s, "{}", self.level.block_state(pos).0);
            if let Some(out) = self.comparator(pos) {
                let _ = write!(s, "/{out}");
            }
        }
        s
    }
    fn quiet(&self) -> bool {
        self.ticks.blocks.count() == 0
            && self.ticks.fluids.count() == 0
            && self.ticks.block_events.is_empty()
            && self.ticks.moving_pistons.is_empty()
    }
    /// `ServerLevel.tick`'s block work until the level is quiet or `limit` ticks.
    fn ticks(&mut self, limit: i64) {
        let mut last = self.outputs();
        let _ = writeln!(self.out, "t 0 {last}");
        let mut t = 0;
        while t < limit && self.left > 0 && !self.quiet() {
            t += 1;
            self.left -= 1;
            self.level.time += 1;
            let b = self.behaviours;
            b.tick_scheduled(&mut self.level, &mut self.ticks);
            b.run_block_events(&mut self.level, &mut self.ticks);
            b.tick_block_entities(&mut self.level, &mut self.ticks);
            let now = self.outputs();
            if now != last {
                let _ = writeln!(self.out, "t {t} {now}");
            }
            last = now;
        }
        let mut hash = 0u64;
        for (&pos, &state) in &self.level.cells {
            let inside = (self.min.x..=self.max.x).contains(&pos.x)
                && (self.min.y..=self.max.y).contains(&pos.y)
                && (self.min.z..=self.max.z).contains(&pos.z);
            if inside {
                let extra = self.comparator(pos).map_or(0, |o| ((o + 1) as u64) << 32);
                let value = state.0 as u64 + extra;
                hash = hash.wrapping_add(mix(pos.x, pos.y, pos.z, value));
                if self.trace {
                    let _ = writeln!(self.out, "# cell {},{},{} {value}", pos.x, pos.y, pos.z);
                }
            }
        }
        let quiet = u8::from(self.quiet());
        let _ = writeln!(self.out, "end {t} {quiet} {hash:x}");
    }
    fn act(&mut self, pos: BlockPos, on: bool) {
        let use_it = match self.kind(self.level.block_state(pos)) {
            Kind::Lever(powered) => powered != on,
            Kind::Button => on,
            _ => false,
        };
        if use_it {
            let hit = Vec3::new(pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5);
            let player = PlayerContext {
                yaw: 0.0,
                pitch: 0.0,
                may_build: true,
                can_eat: false,
                game_master: false,
            };
            self.behaviours.use_without_item(
                &mut self.level,
                &mut self.ticks,
                pos,
                Direction::Up,
                hit,
                player,
            );
        }
    }
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
    let kinds: Vec<Kind> = (0..registry.len() as u32)
        .map(|id| match registry.state(BlockStateId(id)) {
            Some(s) if s.name == "minecraft:lever" => {
                Kind::Lever(s.properties.get("powered").is_some_and(|v| v == "true"))
            }
            Some(s) if s.name.ends_with("_button") => Kind::Button,
            Some(s) if s.name == "minecraft:comparator" => Kind::Comparator,
            _ => Kind::Other,
        })
        .collect();
    let mut out = std::io::BufWriter::new(std::fs::File::create(output)?);
    for line in std::io::BufReader::new(std::fs::File::open(cases)?).lines() {
        let case: Value = serde_json::from_str(&line?)?;
        let size = ints(&case["size"]);
        let mut ticks = Ticks::new(0);
        for x in -1..=(size[0] as i32 >> 4) + 1 {
            for z in -1..=(size[2] as i32 >> 4) + 1 {
                ticks.add_empty_chunk(ChunkPos::new(x, z))?;
            }
        }
        let cells: Vec<Vec<i64>> = case["cells"].as_array().unwrap().iter().map(ints).collect();
        let mut level = Cells {
            cells: HashMap::new(),
            air,
            random: LegacyRandom::new(0),
            time: case["time"].as_i64().unwrap(),
            analog: HashMap::new(),
        };
        for c in &cells {
            level.set_raw(pos_of(c), BlockStateId(c[3] as u32));
        }
        for c in case["comparators"].as_array().unwrap().iter().map(ints) {
            ticks.comparator_outputs.insert(pos_of(&c), c[3] as i32);
        }
        for c in case["analog"].as_array().unwrap().iter().map(ints) {
            level.analog.insert(pos_of(&c), c[3] as i32);
        }
        let outputs = case["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| pos_of(&ints(v)))
            .collect();
        let mut run = Run {
            behaviours: &behaviours,
            kinds: &kinds,
            level,
            ticks,
            outputs,
            min: BlockPos::new(-2, -2, -2),
            max: BlockPos::new(size[0] as i32 + 1, size[1] as i32 + 1, size[2] as i32 + 1),
            left: case["budget"].as_i64().unwrap(),
            trace: std::env::var_os("HORIZON_CATALOG_TRACE").is_some(),
            out: String::new(),
        };
        let _ = writeln!(run.out, "case {}", case["code"].as_str().unwrap());
        for c in &cells {
            let pos = pos_of(c);
            let block = behaviours.block(run.level.block_state(pos));
            behaviours.handle_neighbor_changed(&mut run.level, &mut run.ticks, pos, block);
        }
        run.out.push_str("step init\n");
        run.ticks(case["settle"].as_i64().unwrap());
        let limit = case["max"].as_i64().unwrap();
        for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            if run.left <= 0 {
                run.out.push_str("budget spent\n");
                break;
            }
            let _ = writeln!(run.out, "step {index}");
            for act in step.as_array().unwrap().iter().map(ints) {
                run.act(pos_of(&act), act[3] != 0);
            }
            run.ticks(limit);
        }
        out.write_all(run.out.as_bytes())?;
        out.flush()?;
    }
    Ok(())
}
