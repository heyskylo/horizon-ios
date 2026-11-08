#![forbid(unsafe_code)]
//! Runs tools/test_emitter_particles_parity.py's emitters through
//! `particles::on_block_faces`, `particles::along_axis` and
//! `particles::item_use_particles`, plus the tracking emitter queue.
use horizon_client::{
    Result,
    assets::Assets,
    blocks::BlockFacts,
    collisions,
    particles::{
        self, ParticleEngine, ParticleLevel, ParticleOptions, ParticleType, TrackingBody,
        TrackingTarget,
    },
    world::World,
};
use horizon_core::random::{LegacyRandom, RandomSource};
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

fn d(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn record(kind: &str, p: [f64; 3], v: [f64; 3]) -> String {
    format!(
        "+ {kind} {} {} {} {} {} {}",
        d(p[0]),
        d(p[1]),
        d(p[2]),
        d(v[0]),
        d(v[1]),
        d(v[2])
    )
}

fn tracking_body(frame: &Value) -> TrackingBody {
    TrackingBody {
        feet: std::array::from_fn(|i| frame["pos"][i].as_f64().expect("frame position")),
        width: f64::from(frame["width"].as_f64().expect("frame width") as f32),
        height: f64::from(frame["height"].as_f64().expect("frame height") as f32),
    }
}

fn tracking_case(
    engine: &mut ParticleEngine,
    level: &ParticleLevel,
    case: &Value,
    out: &mut impl Write,
) -> Result<()> {
    let target = TrackingTarget::Entity(1);
    let particle = ParticleType::from_name(case["particle"].as_str().ok_or("particle")?)
        .ok_or("tracking particle type")?;
    engine.clear();
    engine.seed_randoms(case["seed"].as_i64().ok_or("seed")?, 0, 0);
    engine.requested = Some(Vec::new());
    for (tick, frame) in case["frames"]
        .as_array()
        .ok_or("frames")?
        .iter()
        .enumerate()
    {
        writeln!(out, "tick {tick}")?;
        if tick == 0 {
            let options = ParticleOptions::simple(particle);
            let body = tracking_body(frame);
            if particle == ParticleType::TotemOfUndying {
                engine.tracking_emitter_for(options, target, body, 30);
            } else {
                engine.tracking_emitter(options, target, body);
            }
        } else {
            engine.tick(level, |id| {
                (id == target && !frame.is_null()).then(|| tracking_body(frame))
            });
        }
        for (options, p, v) in engine.requested.as_mut().unwrap().drain(..) {
            writeln!(out, "{}", record(options.ty.name(), p, v))?;
        }
    }
    writeln!(out, "next {}", engine.next_randoms()[0])?;
    Ok(())
}

pub fn main() -> Result<()> {
    let args: Vec<String> = crate::args().collect();
    let root = Path::new(".local/horizon-client").join(horizon_core::VANILLA_VERSION);
    let assets = Assets::load(&root)?;
    let facts = BlockFacts::load(&root, assets.registry)?;
    let collision = collisions::load(&root, assets.registry)?;
    let world = World::new(facts.air, 0);
    let level = ParticleLevel {
        world: &world,
        facts: &facts,
        collision: &collision,
        player: None,
    };
    let mut engine = ParticleEngine::new(&root, &Default::default(), &Default::default());
    let mut out = BufWriter::new(File::create(&args[2])?);
    for (n, line) in BufReader::new(File::open(&args[1])?).lines().enumerate() {
        let c: Value = serde_json::from_str(&line?)?;
        writeln!(out, "== {n}")?;
        if c["kind"] == "tracking" {
            tracking_case(&mut engine, &level, &c, &mut out)?;
            continue;
        }
        let mut random = LegacyRandom::new(c["seed"].as_i64().ok_or("seed")?);
        let mut lines = Vec::new();
        if c["kind"] == "item_use" {
            let p: [f64; 3] = std::array::from_fn(|i| c["pos"][i].as_f64().unwrap_or(0.0));
            let f = |k: &str| c[k].as_f64().unwrap_or(0.0) as f32;
            particles::item_use_particles(
                &mut random,
                f("pitch"),
                f("yaw"),
                p,
                p[1] + f64::from(f("eye")),
                c["count"].as_u64().unwrap_or(0) as u32,
                |p, v| lines.push(record("item", p, v)),
            );
        } else {
            let p: [i32; 3] = std::array::from_fn(|i| c["pos"][i].as_i64().unwrap_or(0) as i32);
            match c["axis"].as_i64() {
                Some(axis @ 0..=2) => {
                    particles::along_axis(&mut random, axis as usize, p, (10, 19), |p, v| {
                        lines.push(record("electric_spark", p, v))
                    });
                }
                _ => {
                    particles::on_block_faces(&mut random, p, |p, v| {
                        lines.push(record("wax_on", p, v))
                    });
                }
            }
        }
        for l in lines {
            writeln!(out, "{l}")?;
        }
        writeln!(out, "next {}", random.next_long())?;
    }
    out.flush()?;
    Ok(())
}
