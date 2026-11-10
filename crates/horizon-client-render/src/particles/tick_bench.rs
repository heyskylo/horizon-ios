//! The particle stress scene's load on `ParticleEngine`, outside the game: `cargo test --profile fast
//! -p horizon-client-render --lib particle_tick_cost -- --ignored --nocapture` (prepared resources;
//! `PARTICLE_BENCH_SECS` sets the run, default 5). It spawns what tools/stress_scenes.py's `particles`
//! scene makes each tick (flames, cosy smoke, end rods, TNT debris) over its stone floor and times the
//! spawns and the engine tick apart, once the engine holds as many particles as it will.
use super::*;
use crate::{blocks::BlockFacts, chunk::Chunk, collisions, world::World};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FLOOR_Y: i32 = 289;

/// A cheap, repeatable stream of numbers in [0, 1).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn signed(&mut self) -> f64 {
        self.next() * 2.0 - 1.0
    }
}

type Emitter = (ParticleType, usize, [f64; 3], [f64; 3], f64);

fn spawn_tick(engine: &mut ParticleEngine, rng: &mut Lcg, types: &[Emitter], rockets: usize) {
    for _ in 0..rockets {
        // `summon firework_rocket` with a large ball of three colours and a trail, bursting 18 ticks up.
        let explosion = FireworkExplosion {
            shape: FireworkShape::LargeBall,
            colors: vec![0xFF0000, 0x00FF00, 0x0000FF],
            fade: Vec::new(),
            trail: true,
            twinkle: false,
        };
        let at = [-5.5 + rng.next() * 12.0, f64::from(FLOOR_Y + 20), 20.5];
        engine.fireworks(at, [0.0, 0.1, 0.0], vec![explosion], engine.camera, true);
    }
    for &(ty, count, center, delta, speed) in types {
        let options = ParticleOptions::simple(ty);
        for _ in 0..count {
            let p = [
                center[0] + rng.signed() * delta[0],
                center[1] + rng.signed() * delta[1],
                center[2] + rng.signed() * delta[2],
            ];
            let v = [
                rng.signed() * speed,
                rng.signed() * speed,
                rng.signed() * speed,
            ];
            engine.add(&options, p, v);
        }
    }
}

#[test]
#[ignore = "a measurement, not a check"]
fn particle_tick_cost() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/horizon-client")
        .join(horizon_core::VANILLA_VERSION);
    if !root.join("cached-collision.nbt").exists() {
        eprintln!("no prepared resources: skipped");
        return;
    }
    let assets = crate::assets::Assets::load(&root).unwrap();
    let facts = BlockFacts::load(&root, assets.registry).unwrap();
    let collision = collisions::load(&root, assets.registry).unwrap();
    let mut world = World::new(facts.air, 0);
    let floor = assets
        .registry
        .default_state("minecraft:smooth_stone")
        .unwrap();
    for cz in -1..=3 {
        for cx in -2..=2 {
            world
                .chunks
                .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
        }
    }
    for z in 2..=40 {
        for x in -24..=24 {
            world.set(x, FLOOR_Y, z, floor).unwrap();
        }
    }
    let level = ParticleLevel {
        world: &world,
        facts: &facts,
        collision: &collision,
        player: None,
        cached_chunk: std::cell::Cell::new(None),
    };
    let ty = |name: &str| ParticleType::from_name(name).unwrap();
    let (fx, fy, fz) = (0.5, f64::from(FLOOR_Y + 5), 20.5);
    let tnt = [0.5, f64::from(FLOOR_Y + 24), 26.5];
    // The scene's emitters, per tick, and TNT's debris (poof and smoke).
    let flame: Emitter = (ty("flame"), 320, [fx, fy, fz], [8.0, 4.0, 8.0], 0.01);
    let cosy: Emitter = (
        ty("campfire_cosy_smoke"),
        8,
        [fx, fy, fz],
        [8.0, 4.0, 8.0],
        0.0,
    );
    let rod: Emitter = (ty("end_rod"), 80, [fx, fy, fz], [8.0, 4.0, 8.0], 0.05);
    let poof: Emitter = (ty("poof"), 258, tnt, [3.0, 3.0, 3.0], 0.4);
    let smoke: Emitter = (ty("smoke"), 238, tnt, [3.0, 3.0, 3.0], 0.4);
    let cases: [(&str, Vec<Emitter>, usize); 7] = [
        ("flame 320/tick", vec![flame], 0),
        ("cosy smoke 8/tick", vec![cosy], 0),
        ("end rod 80/tick", vec![rod], 0),
        ("poof 258/tick", vec![poof], 0),
        ("smoke 238/tick", vec![smoke], 0),
        ("3 rockets/tick", vec![], 3),
        ("the whole scene", vec![flame, cosy, rod, poof, smoke], 3),
    ];
    let secs: f64 = std::env::var("PARTICLE_BENCH_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    for (name, types, rockets) in cases {
        let mut engine = ParticleEngine::new(Path::new(""), &HashMap::new(), &HashSet::new());
        engine.camera = [0.5, 298.0, 0.5];
        let mut rng = Lcg(1234);
        for _ in 0..600 {
            spawn_tick(&mut engine, &mut rng, &types, rockets);
            engine.tick(&level);
        }
        let (mut spawn, mut tick, mut ticks, mut alive) =
            (Duration::ZERO, Duration::ZERO, 0u64, 0usize);
        let end = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < end {
            let t = Instant::now();
            spawn_tick(&mut engine, &mut rng, &types, rockets);
            spawn += t.elapsed();
            let t = Instant::now();
            engine.tick(&level);
            tick += t.elapsed();
            alive += engine.len();
            ticks += 1;
        }
        let per = |d: Duration| d.as_secs_f64() * 1000.0 / ticks as f64;
        let alive = alive / ticks as usize;
        eprintln!(
            "{name:20} alive {alive:6} | per tick: spawn {:6.3} ms, engine tick {:6.3} ms | {:5.0} ns per particle",
            per(spawn),
            per(tick),
            per(tick) * 1.0e6 / alive.max(1) as f64
        );
    }
}
