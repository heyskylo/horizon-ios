use crate::support::common::{f64_bits, position};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
};
use horizon_world::{
    block_support::BlockSupportRegistry,
    dynamic_support::DynamicSupportWorld,
    entity_fluid::*,
    entity_fluid_tick::{self, EntityFluidTickWorld},
    fluid::{FluidKind, FluidState},
    nbt::{self, Limits},
    registry::{BlockRegistry, BlockStateId},
    registry_fluid::{FluidBlockWorld, FluidEntityWorld, FluidRegistry, RegistryFluidWorld},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::Path,
};
fn number(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}
fn single(v: &Value) -> f32 {
    f32::from_bits(u32::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}
fn vector(v: &Value) -> Vec3 {
    Vec3::new(number(&v[0]), number(&v[1]), number(&v[2]))
}
fn show(v: Vec3) -> String {
    format!(
        "{:016x} {:016x} {:016x}",
        f64_bits(v.x),
        f64_bits(v.y),
        f64_bits(v.z)
    )
}
fn pos(p: BlockPos) -> String {
    format!("{} {} {}", p.x, p.y, p.z)
}
#[derive(Clone, Copy, Default)]
struct Tick {
    touch: bool,
    first: bool,
    fall: f64,
}
struct World<'a> {
    op: &'a Value,
    out: &'a mut dyn Write,
    calls: i64,
    velocity: Vec3,
    block: BlockPos,
    wet: Vec<bool>,
    states: [FluidState; 5],
    player: bool,
    tick: Tick,
    pushed: usize,
}
impl World<'_> {
    fn query(&mut self, s: String) -> io::Result<()> {
        writeln!(self.out, "{s}")?;
        let n = self.calls;
        self.calls += 1;
        if n == self.op["fail"].as_i64().unwrap() {
            return Err(io::Error::other("injected query failure"));
        }
        if n == self.op["mutate_at"].as_i64().unwrap() {
            self.velocity = vector(&self.op["after_velocity"]);
            self.block = position(&self.op["after_block"]);
            if self.op.get("after_touch").is_some() {
                self.tick.touch = self.op["after_touch"].as_bool().unwrap();
                self.tick.first = self.op["after_first"].as_bool().unwrap();
                self.tick.fall = number(&self.op["after_fall"]);
            }
        }
        Ok(())
    }
    fn cell(&self, p: BlockPos) -> &Value {
        self.op["cells"].get(pos(p)).unwrap_or(&self.op["default"])
    }
}
impl FluidMembership for World<'_> {
    fn fluid_in_tag(&self, fluid: FluidKind, tag: FluidTagId) -> bool {
        self.op["tags"][tag.0 as usize].as_u64().unwrap() & (1 << fluid.index()) != 0
    }
}
impl FluidInteractionWorld for World<'_> {
    type Error = io::Error;
    type Chunk = ();
    fn interaction_box(&mut self) -> io::Result<Option<Aabb>> {
        self.query("box".into())?;
        let b = &self.op["bounds"];
        Ok(if b.is_null() {
            None
        } else {
            Some(Aabb::new(
                Vec3::new(number(&b[0]), number(&b[1]), number(&b[2])),
                Vec3::new(number(&b[3]), number(&b[4]), number(&b[5])),
            ))
        })
    }
    fn loaded_chunk(&mut self, x: i32, z: i32) -> io::Result<Option<()>> {
        self.query(format!("chunk {x} {z}"))?;
        Ok((!self.op["missing"]
            .as_array()
            .unwrap()
            .contains(&Value::String(format!("{x} {z}"))))
        .then_some(()))
    }
    fn chunk_sections(&mut self, _: ()) -> io::Result<FluidSections<'_>> {
        self.query("sections".into())?;
        Ok(FluidSections {
            minimum_section_y: self.op["min_section"].as_i64().unwrap() as i32,
            has_fluid: &self.wet,
        })
    }
    fn bounding_box_min_y(&self) -> f64 {
        number(&self.op["base_y"])
    }
    fn block_x(&self) -> i32 {
        self.block.x
    }
    fn block_z(&self) -> i32 {
        self.block.z
    }
    fn eye_y(&mut self) -> io::Result<f64> {
        self.query("eye".into())?;
        Ok(number(&self.op["eye"]))
    }
    fn fluid_state(&mut self, p: BlockPos) -> io::Result<FluidState> {
        self.query(format!("fluid {}", pos(p)))?;
        Ok(self.states[self.cell(p)["kind"].as_u64().unwrap() as usize])
    }
    fn fluid_height(&mut self, _: FluidState, p: BlockPos) -> io::Result<f32> {
        self.query(format!("height {}", pos(p)))?;
        Ok(single(&self.cell(p)["height"]))
    }
    fn camera_height(&mut self, s: FluidState, p: BlockPos) -> io::Result<f32> {
        if matches!(s.kind(), FluidKind::Water | FluidKind::Lava) {
            self.query(format!(
                "block {}",
                pos(BlockPos::new(p.x, p.y.wrapping_add(1), p.z))
            ))?;
            if self.op["sturdy"].as_bool().unwrap() {
                return Ok(1.0);
            }
        }
        self.fluid_height(s, p)
    }
    fn flow(&mut self, _: FluidState, p: BlockPos) -> io::Result<Vec3> {
        self.query(format!("flow {}", pos(p)))?;
        Ok(vector(&self.cell(p)["flow"]))
    }
}
impl DynamicSupportWorld for World<'_> {
    type Error = io::Error;
    fn shulker_is_closed(&mut self, p: [i32; 3]) -> io::Result<Option<bool>> {
        self.query(format!("entity {} {} {}", p[0], p[1], p[2]))?;
        Ok(None)
    }
    fn moving_piston_shape(
        &mut self,
        p: [i32; 3],
    ) -> io::Result<Option<horizon_core::geometry::VoxelShape>> {
        self.query(format!("entity {} {} {}", p[0], p[1], p[2]))?;
        Ok(None)
    }
}
impl FluidBlockWorld for World<'_> {
    fn block_state(&mut self, p: BlockPos) -> io::Result<BlockStateId> {
        self.query(format!("block {}", pos(p)))?;
        Ok(BlockStateId(self.cell(p)["state"].as_u64().unwrap() as u32))
    }
    fn fluid_block_state(&mut self, p: BlockPos) -> io::Result<BlockStateId> {
        self.query(format!("fluid {}", pos(p)))?;
        Ok(BlockStateId(self.cell(p)["state"].as_u64().unwrap() as u32))
    }
}
impl FluidEntityWorld for World<'_> {
    type Chunk = ();
    fn interaction_box(&mut self) -> io::Result<Option<Aabb>> {
        FluidInteractionWorld::interaction_box(self)
    }
    fn loaded_chunk(&mut self, x: i32, z: i32) -> io::Result<Option<()>> {
        FluidInteractionWorld::loaded_chunk(self, x, z)
    }
    fn chunk_sections(&mut self, c: ()) -> io::Result<FluidSections<'_>> {
        FluidInteractionWorld::chunk_sections(self, c)
    }
    fn bounding_box_min_y(&self) -> f64 {
        FluidInteractionWorld::bounding_box_min_y(self)
    }
    fn block_x(&self) -> i32 {
        FluidInteractionWorld::block_x(self)
    }
    fn eye_y(&mut self) -> io::Result<f64> {
        FluidInteractionWorld::eye_y(self)
    }
    fn block_z(&self) -> i32 {
        FluidInteractionWorld::block_z(self)
    }
}
impl CurrentTarget for World<'_> {
    type Error = io::Error;
    fn is_player_class(&self) -> bool {
        self.player
    }
    fn velocity(&mut self) -> io::Result<Vec3> {
        self.query("velocity".into())?;
        Ok(self.velocity)
    }
    fn add_delta_movement(&mut self, v: Vec3) -> io::Result<()> {
        if [v.x, v.y, v.z].iter().all(|v| v.is_finite()) {
            let old = CurrentTarget::velocity(self)?;
            let next = Vec3::new(old.x + v.x, old.y + v.y, old.z + v.z);
            self.query(format!("set {}", show(next)))?;
            self.velocity = next;
        }
        Ok(())
    }
}
impl EntityFluidTickWorld for World<'_> {
    fn is_pushed_by_fluid(&mut self) -> io::Result<bool> {
        self.query("pushed".into())?;
        let values = self.op["pushed"].as_array().unwrap();
        let result = values[self.pushed.min(values.len() - 1)].as_bool().unwrap();
        self.pushed += 1;
        Ok(result)
    }
    fn reset_fall_distance(&mut self) -> io::Result<()> {
        self.query("reset-fall".into())?;
        self.tick.fall = 0.0;
        Ok(())
    }
    fn was_touching_water(&self) -> bool {
        self.tick.touch
    }
    fn first_tick(&self) -> bool {
        self.tick.first
    }
    fn water_splash(&mut self) -> io::Result<()> {
        self.query("splash".into())
    }
    fn set_was_touching_water(&mut self, value: bool) {
        self.tick.touch = value;
    }
    fn fast_lava(&mut self) -> io::Result<bool> {
        self.query("fast-lava".into())?;
        Ok(self.op["fast_lava"].as_bool().unwrap())
    }
}
fn limit_checks(template: &Value, states: [FluidState; 5]) {
    let mut op = template.clone();
    op["cells"] = serde_json::json!({});
    op["default"]["kind"] = 2.into();
    op["default"]["height"] = format!("{:08x}", 0.8f32.to_bits()).into();
    op["eye"] = format!("{:016x}", 0.5f64.to_bits()).into();
    let mut sink = io::sink();
    let mut world = World {
        op: &op,
        out: &mut sink,
        calls: 0,
        velocity: Vec3::ZERO,
        block: BlockPos::new(0, 0, 0),
        wet: vec![false, true, false],
        states,
        player: false,
        tick: Tick::default(),
        pushed: 0,
    };
    let mut interaction = EntityFluidInteraction::new([FluidTagId(0)]);
    assert!(matches!(
        interaction.update(&mut world, false, 0),
        Err(FluidInteractionError::BudgetExceeded)
    ));
    assert_eq!(world.calls, 1);
    world.calls = 0;
    assert!(matches!(
        interaction.update(&mut world, false, 1),
        Err(FluidInteractionError::BudgetExceeded)
    ));
    assert_eq!(world.calls, 3);
    world.calls = 0;
    assert!(matches!(
        interaction.update(&mut world, false, 8),
        Err(FluidInteractionError::BudgetExceeded)
    ));
    assert_eq!(world.calls, 10);
    assert!(interaction.trackers().is_empty());
    assert!(interaction.update(&mut world, false, 9).unwrap());
    assert_eq!(interaction.trackers().len(), 1);
    assert_eq!(interaction.currents()[0].1.count, 1);
    let mut invalid = op.clone();
    invalid["bounds"][0] = format!("{:016x}", f64::NAN.to_bits()).into();
    world.op = &invalid;
    world.calls = 0;
    assert!(matches!(
        interaction.update(&mut world, false, 9),
        Err(FluidInteractionError::NonFiniteBounds)
    ));
    assert_eq!(world.calls, 1);
    assert_eq!(interaction.trackers().len(), 1);
    assert_eq!(interaction.trackers()[0].height, 0.0);
    assert!(!interaction.trackers()[0].eyes_inside);
    assert_eq!(interaction.currents()[0].1.count, 0);
    let mut absent = op.clone();
    absent["bounds"] = Value::Null;
    world.op = &absent;
    assert!(!interaction.update(&mut world, false, 0).unwrap());
    assert!(interaction.trackers().is_empty());
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = crate::args().nth(1).expect("private fixture directory");
    let dir = Path::new(&arg);
    let mut out = io::BufWriter::new(fs::File::create(dir.join("rust.txt"))?);
    let load = |n: &str| -> Result<_, Box<dyn std::error::Error>> {
        Ok(nbt::read_named(&fs::read(dir.join(n))?, Limits::default())?
            .0
            .tag)
    };
    let registry = if dir.join("block-registry.nbt").exists() {
        Some(BlockRegistry::from_nbt(&load("block-registry.nbt")?)?)
    } else {
        None
    };
    let support = registry
        .as_ref()
        .map(|r| {
            Ok::<_, Box<dyn std::error::Error>>(BlockSupportRegistry::from_nbt(
                &load("cached-support.nbt")?,
                r,
            )?)
        })
        .transpose()?;
    let bindings = registry
        .as_ref()
        .map(|r| -> Result<_, Box<dyn std::error::Error>> {
            let info: Value = serde_json::from_slice(&fs::read(dir.join("fluid-info.json"))?)?;
            let names = |v: &Value| -> BTreeSet<String> {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
                    .collect()
            };
            let ice = names(&info["ice"]);
            let all = (0..r.len())
                .map(|i| r.state(BlockStateId(i as u32)).unwrap().name.clone())
                .collect();
            Ok([
                FluidRegistry::new(r, &names(&info["blocks_flow"]), &ice)?,
                FluidRegistry::new(r, &BTreeSet::new(), &ice)?,
                FluidRegistry::new(r, &all, &ice)?,
            ])
        })
        .transpose()?;
    let states = FluidKind::ALL
        .map(|k| FluidState::from_json(&format!("{{\"id\":\"{}\"}}", k.name())).unwrap());
    let input = fs::read_to_string(dir.join("inputs.txt"))?;
    let first: Value = serde_json::from_str(input.lines().next().unwrap())?;
    limit_checks(&first["ops"][0], states);
    for line in input.lines() {
        let f: Value = serde_json::from_str(line)?;
        let mut interaction = EntityFluidInteraction::new(
            f["order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| FluidTagId(v.as_u64().unwrap() as u32)),
        );
        let initial = f.get("tick_initial");
        let mut tick = initial.map_or(Tick::default(), |v| Tick {
            touch: v["touch"].as_bool().unwrap(),
            first: v["first"].as_bool().unwrap(),
            fall: number(&v["fall"]),
        });
        for op in f["ops"].as_array().unwrap() {
            let mut w = World {
                op,
                out: &mut out,
                calls: 0,
                velocity: vector(&op["velocity"]),
                block: position(&op["block"]),
                wet: op["wet"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_bool().unwrap())
                    .collect(),
                states,
                player: f["player"].as_bool().unwrap(),
                tick,
                pushed: 0,
            };
            if op["mode"].as_str().unwrap() == "tick" {
                match entity_fluid_tick::update(
                    &mut interaction,
                    &mut w,
                    FluidTagId(0),
                    FluidTagId(1),
                    1_000_000,
                ) {
                    Ok(v) => writeln!(w.out, "result {v}")?,
                    Err(_) => writeln!(w.out, "error")?,
                }
            } else if op["mode"].as_str().unwrap() == "registered" {
                let binding =
                    &bindings.as_ref().unwrap()[op["block_tag"].as_u64().unwrap() as usize];
                let result = interaction.update(
                    &mut RegistryFluidWorld::new(binding, support.as_ref().unwrap(), &mut w)?,
                    op["ignore"].as_bool().unwrap(),
                    1_000_000,
                );
                match result {
                    Ok(v) => writeln!(w.out, "result {v}")?,
                    Err(_) => writeln!(w.out, "error")?,
                }
            } else if op["mode"].as_str().unwrap() == "update" {
                match interaction.update(&mut w, op["ignore"].as_bool().unwrap(), 1_000_000) {
                    Ok(v) => writeln!(w.out, "result {v}")?,
                    Err(_) => writeln!(w.out, "error")?,
                }
            } else {
                match interaction.apply_current(
                    &mut w,
                    FluidTagId(op["tag"].as_u64().unwrap() as u32),
                    number(&op["speed"]),
                ) {
                    Ok(()) => writeln!(w.out, "result ok")?,
                    Err(_) => writeln!(w.out, "error")?,
                }
            }
            writeln!(w.out, "trackers {}", interaction.trackers().len())?;
            for t in interaction.trackers() {
                writeln!(
                    w.out,
                    "tracker {} {:016x} {}",
                    t.fluid.index() as u8,
                    f64_bits(t.height),
                    t.eyes_inside
                )?;
            }
            for (tag, c) in interaction.currents() {
                writeln!(
                    w.out,
                    "current {} {:016x} {} {}",
                    tag.0,
                    f64_bits(c.height),
                    show(c.accumulated),
                    c.count
                )?;
            }
            for t in 0..4 {
                let tag = FluidTagId(t);
                let height = interaction.fluid_height(&w, tag);
                let inside = interaction.is_in_fluid(&w, tag);
                let eyes = interaction.is_eye_in_fluid(&w, tag);
                writeln!(w.out, "tag {t} {:016x} {inside} {eyes}", f64_bits(height))?;
            }
            writeln!(w.out, "final {}", show(w.velocity))?;
            writeln!(w.out, "queries {}", w.calls)?;
            tick = w.tick;
            if initial.is_some() {
                writeln!(
                    w.out,
                    "tick {} {} {:016x}",
                    tick.touch,
                    tick.first,
                    f64_bits(tick.fall)
                )?;
            }
        }
    }
    Ok(())
}
