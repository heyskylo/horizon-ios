//! Rust side of tools/test_nether_portal_parity.py: replays
//! `VanillaPortalOracle` cases (an entity in a nether portal, the destination
//! level's terrain and portals) through `horizon_world::level::portal_travel`
//! and prints the same result text.
use horizon_core::{
    geometry::{Axis, Direction, Vec3},
    position::BlockPos,
    random::LegacyRandom,
};
use horizon_world::{
    level::{
        Behaviours, Dimension, LevelAccess, Ticks,
        data::load_registry,
        portal::PortalShape,
        portal_travel::{
            BorderBounds, closest_portal, collision_free_position, exit_position,
            largest_rectangle_around, relative_position,
        },
    },
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
    nether: bool,
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
        if self.nether { 0 } else { -64 }
    }
    fn max_y(&self) -> i32 {
        if self.nether { 256 } else { 320 }
    }
    fn random(&mut self) -> &mut LegacyRandom {
        &mut self.random
    }
    fn game_time(&self) -> i64 {
        0
    }
    fn dimension(&self) -> Dimension {
        if self.nether {
            Dimension::Nether
        } else {
            Dimension::Overworld
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
fn level(case: &Value, key: &str, air: BlockStateId, nether: bool) -> Cells {
    let mut cells = HashMap::new();
    for c in case[key].as_array().unwrap() {
        let state = BlockStateId(c[3].as_i64().unwrap() as u32);
        if state != air {
            cells.insert(pos(c), state);
        }
    }
    Cells {
        cells,
        air,
        random: LegacyRandom::new(case["seed"].as_i64().unwrap()),
        nether,
    }
}

fn run(case: &Value, b: &Behaviours, air: BlockStateId, out: &mut Vec<String>) {
    let from_nether = case["from"] == "nether";
    let source = level(case, "source", air, from_nether);
    let mut target = level(case, "target", air, !from_nether);
    let dims = &case["entity"]["dims"];
    let (width, height) = (
        dims[0].as_f64().unwrap() as f32,
        dims[1].as_f64().unwrap() as f32,
    );
    out.push(format!(
        "dims {:08x} {:08x}",
        width.to_bits(),
        height.to_bits()
    ));
    // Exact doubles, as bits.
    let e = &case["entity"]["bits"];
    let bit = |i: usize| f64::from_bits(u64::from_str_radix(e[i].as_str().unwrap(), 16).unwrap());
    let at = Vec3::new(bit(0), bit(1), bit(2));
    // WorldBorder: centre and size, the extent clamped to +-29999984.
    let border = &case["border"];
    let (cx, cz, size) = (
        border["x"].as_f64().unwrap(),
        border["z"].as_f64().unwrap(),
        border["size"].as_f64().unwrap(),
    );
    let limit = 29_999_984.0;
    let clamp = |v: f64| v.clamp(-limit, limit);
    let bounds = BorderBounds {
        min_x: clamp(cx - size / 2.0),
        min_z: clamp(cz - size / 2.0),
        max_x: clamp(cx + size / 2.0),
        max_z: clamp(cz + size / 2.0),
    };
    // NetherPortalBlock.getDimensionTransitionFromExit: where in its portal
    // the entity stands.
    let entry = pos(&case["entry"]);
    let entry_state = source.block_state(entry);
    let (entry_x_axis, offset) = match b.portal_axis(entry_state) {
        Some(x_axis) => {
            let rectangle = largest_rectangle_around(
                entry,
                if x_axis { Axis::X } else { Axis::Z },
                21,
                Axis::Y,
                21,
                |p| source.block_state(p) == entry_state,
            );
            let m = rectangle.min_corner;
            out.push(format!(
                "entry {},{},{} {}x{}",
                m.x, m.y, m.z, rectangle.axis1_size, rectangle.axis2_size
            ));
            let mut offset = relative_position(rectangle, x_axis, at, width, height);
            // LivingEntity, AbstractMinecart and AbstractBoat:
            // resetForwardDirectionOfRelativePortalPosition.
            if case["entity"]["reset_forward"].as_bool().unwrap_or(false) {
                offset.z = 0.0;
            }
            out.push(format!(
                "offset {:016x} {:016x} {:016x}",
                offset.x.to_bits(),
                offset.y.to_bits(),
                offset.z.to_bits()
            ));
            (x_axis, offset)
        }
        None => (true, Vec3::new(0.5, 0.0, 0.0)),
    };
    // NetherPortalBlock.getPortalDestination.
    let scale = if from_nether { 8.0 } else { 1.0 / 8.0 };
    let approximate = bounds.clamp(at.x * scale, at.y, at.z * scale);
    let portals: Vec<BlockPos> = case["pois"].as_array().unwrap().iter().map(pos).collect();
    let (min_section, max_section) = if from_nether { (-4, 19) } else { (0, 15) };
    let mut ticks = Ticks::new(0);
    out.push(format!(
        "approximate {},{},{}",
        approximate.x, approximate.y, approximate.z
    ));
    let found = closest_portal(
        &portals,
        approximate,
        !from_nether,
        &bounds,
        min_section,
        max_section,
    );
    out.push(match found {
        Some(p) => format!("found {},{},{}", p.x, p.y, p.z),
        None => "found none".into(),
    });
    let exit = match found {
        Some(found) => {
            let state = target.block_state(found);
            let x_axis = b.portal_axis(state).unwrap_or(true);
            Some(largest_rectangle_around(
                found,
                if x_axis { Axis::X } else { Axis::Z },
                21,
                Axis::Y,
                21,
                |p| target.block_state(p) == state,
            ))
        }
        None => {
            let logical = if from_nether { 384 } else { 128 };
            b.create_portal(
                &mut target,
                &mut ticks,
                approximate,
                entry_x_axis,
                &bounds,
                logical,
            )
        }
    };
    let mut changed: Vec<BlockPos> = target
        .cells
        .keys()
        .copied()
        .chain(
            level(case, "target", air, !from_nether)
                .cells
                .keys()
                .copied(),
        )
        .collect();
    changed.sort_by_key(|p| (p.x, p.y, p.z));
    changed.dedup();
    let before = level(case, "target", air, !from_nether);
    for p in changed {
        let now = target.block_state(p);
        if before.block_state(p) != now {
            out.push(format!("set {},{},{} {}", p.x, p.y, p.z, now.0));
        }
    }
    let Some(exit) = exit else {
        out.push("dest none".into());
        return;
    };
    let exit_x_axis = b
        .portal_axis(target.block_state(exit.min_corner))
        .unwrap_or(true);
    let (bottom, rotation) = exit_position(exit, exit_x_axis, entry_x_axis, offset, width, height);
    let free = collision_free_position(bottom, width, height, &bounds, |p| {
        b.collision_shape(target.block_state(p))
            .map(|s| s.moved(Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z))))
    });
    out.push(format!(
        "dest {:016x} {:016x} {:016x} {rotation:?}",
        free.x.to_bits(),
        free.y.to_bits(),
        free.z.to_bits()
    ));
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}

fn shape_line(shape: &PortalShape) -> String {
    format!(
        "axis={} right={} bottom={},{},{} size={}x{} blocks={} valid={} complete={}",
        if shape.x_axis { "x" } else { "z" },
        direction_name(shape.right),
        shape.bottom_left.x,
        shape.bottom_left.y,
        shape.bottom_left.z,
        shape.width,
        shape.height,
        shape.portal_blocks,
        shape.is_valid(),
        shape.is_complete()
    )
}

fn run_shape(case: &Value, b: &Behaviours, air: BlockStateId, out: &mut Vec<String>) {
    let mut source = level(case, "source", air, false);
    let mut ticks = Ticks::new(0);
    let probe = &case["probe"];
    let at = pos(&probe["pos"]);
    let x_axis = probe["axis"] == "x";
    out.push(format!(
        "shape {}",
        shape_line(&b.portal_shape(&mut source, &mut ticks, at, x_axis))
    ));
    let before = source.cells.clone();
    match b.empty_portal_shape(&mut source, &mut ticks, at, x_axis) {
        None => out.push("empty none".into()),
        Some(shape) => {
            out.push(format!("empty {}", shape_line(&shape)));
            b.create_portal_blocks(&mut source, &mut ticks, &shape);
            let mut changed: Vec<BlockPos> = before
                .keys()
                .copied()
                .chain(source.cells.keys().copied())
                .collect();
            changed.sort_by_key(|p| (p.x, p.y, p.z));
            changed.dedup();
            for p in changed {
                let now = source.block_state(p);
                if before.get(&p).copied().unwrap_or(air) != now {
                    out.push(format!("set {},{},{} {}", p.x, p.y, p.z, now.0));
                }
            }
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
    let mut out = std::io::BufWriter::new(std::fs::File::create(output)?);
    for (index, line) in std::io::BufReader::new(std::fs::File::open(cases)?)
        .lines()
        .enumerate()
    {
        let case: Value = serde_json::from_str(&line?)?;
        let mut lines = Vec::new();
        if case["kind"] == "shape" {
            run_shape(&case, &behaviours, air, &mut lines);
        } else {
            run(&case, &behaviours, air, &mut lines);
        }
        writeln!(out, "case {index}")?;
        for l in lines {
            writeln!(out, "{l}")?;
        }
    }
    Ok(())
}
