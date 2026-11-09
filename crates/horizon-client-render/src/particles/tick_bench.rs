//! The particle stress scene's load on `ParticleEngine`, outside the game.
//!
//! - `parallel_tick_matches_serial_tick`: the engine ticking on several threads (`tick_all`) leaves
//!   every particle, the engine's random and its sounds exactly as ticking them one after another.
//! - `particle_tick_cost` (`cargo test --profile fast -p horizon-client-render --lib
//!   particle_tick_cost -- --ignored --nocapture`, prepared resources; `PARTICLE_BENCH_SECS` sets
//!   the run, default 2): spawns what tools/stress_scenes.py's `particles` scene makes each tick
//!   (flames, cosy smoke, end rods, TNT debris, rockets) over its stone floor and times the spawns
//!   and the engine tick apart, once the engine holds as many particles as it will.
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

/// What to spawn each tick: a type, how many, around where, how far, how fast.
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
        engine.fireworks(at, [0.0, 0.1, 0.0], vec![explosion], engine.camera);
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

/// The scene's emitters, per tick: flames, cosy smoke, end rods, and TNT's debris (poof and smoke).
fn emitters() -> [Emitter; 5] {
    let ty = |name: &str| ParticleType::from_name(name).unwrap();
    let (fx, fy, fz) = (0.5, f64::from(FLOOR_Y + 5), 20.5);
    let tnt = [0.5, f64::from(FLOOR_Y + 24), 26.5];
    [
        (ty("flame"), 320, [fx, fy, fz], [8.0, 4.0, 8.0], 0.01),
        (
            ty("campfire_cosy_smoke"),
            8,
            [fx, fy, fz],
            [8.0, 4.0, 8.0],
            0.0,
        ),
        (ty("end_rod"), 80, [fx, fy, fz], [8.0, 4.0, 8.0], 0.05),
        (ty("poof"), 258, tnt, [3.0, 3.0, 3.0], 0.4),
        (ty("smoke"), 238, tnt, [3.0, 3.0, 3.0], 0.4),
    ]
}

/// Runs `f` over the stress scene's world (a stone floor at y 289 under the particles). Skipped
/// without prepared resources.
fn with_scene(f: impl FnOnce(&ParticleLevel)) {
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
    f(&ParticleLevel {
        world: &world,
        facts: &facts,
        collision: &collision,
        player: None,
        cached_chunk: std::cell::Cell::new(None),
    });
}

fn engine(parallel_min: usize, threads: usize) -> ParticleEngine {
    let mut engine = ParticleEngine::new(Path::new(""), &HashMap::new(), &HashSet::new());
    engine.camera = [0.5, 298.0, 0.5];
    engine.parallel_min = parallel_min;
    engine.parallel_threads = threads;
    engine
}

/// Every live particle's fields, as text.
fn state(engine: &ParticleEngine) -> Vec<String> {
    engine
        .particles
        .iter()
        .map(|p| format!("{:?}", engine.snapshot(p)))
        .collect()
}

#[test]
fn parallel_tick_matches_serial_tick() {
    with_scene(|level| {
        let types = emitters();
        // One engine ticks every particle in turn; the other on three threads. Same seeds, same spawns.
        let mut serial = engine(usize::MAX, 1);
        let mut threaded = engine(0, 3);
        for e in [&mut serial, &mut threaded] {
            e.seed_randoms(7, 8, 9);
        }
        let (mut a, mut b) = (Lcg(99), Lcg(99));
        for tick in 0..90 {
            spawn_tick(&mut serial, &mut a, &types, 3);
            spawn_tick(&mut threaded, &mut b, &types, 3);
            serial.tick(level);
            threaded.tick(level);
            assert_eq!(serial.len(), threaded.len(), "tick {tick}");
            if tick % 10 == 9 {
                assert_eq!(state(&serial), state(&threaded), "tick {tick}");
            }
        }
        assert!(serial.len() > 1000, "{} particles", serial.len());
        assert_eq!(serial.sounds.len(), threaded.sounds.len());
        assert_eq!(serial.next_randoms(), threaded.next_randoms());
    });
}

/// Spawns and ticks `types` once the engine is full, for `secs`, and prints the cost per tick.
fn measure(
    level: &ParticleLevel,
    name: &str,
    (types, rockets): (&[Emitter], usize),
    (parallel_min, threads): (usize, usize),
    secs: f64,
) {
    let mut engine = engine(parallel_min, threads);
    let mut rng = Lcg(1234);
    for _ in 0..600 {
        spawn_tick(&mut engine, &mut rng, types, rockets);
        engine.tick(level);
    }
    let (mut spawn, mut tick, mut ticks, mut alive) =
        (Duration::ZERO, Duration::ZERO, 0u64, 0usize);
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end {
        let t = Instant::now();
        spawn_tick(&mut engine, &mut rng, types, rockets);
        spawn += t.elapsed();
        let t = Instant::now();
        engine.tick(level);
        tick += t.elapsed();
        alive += engine.len();
        ticks += 1;
    }
    let per = |d: Duration| d.as_secs_f64() * 1000.0 / ticks as f64;
    let alive = alive / ticks as usize;
    eprintln!(
        "{name:24} alive {alive:6} | per tick: spawn {:6.3} ms, engine tick {:6.3} ms | {:5.0} ns per particle",
        per(spawn),
        per(tick),
        per(tick) * 1.0e6 / alive.max(1) as f64
    );
}

#[test]
#[ignore = "a measurement, not a check"]
fn particle_tick_cost() {
    with_scene(|level| {
        let [flame, cosy, rod, poof, smoke] = emitters();
        let all = [flame, cosy, rod, poof, smoke];
        let secs: f64 = std::env::var("PARTICLE_BENCH_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2.0);
        let one = (usize::MAX, 1);
        eprintln!("default threads: {}", parallel_threads());
        for (name, types, rockets) in [
            ("flame 320/tick", vec![flame], 0),
            ("cosy smoke 8/tick", vec![cosy], 0),
            ("end rod 80/tick", vec![rod], 0),
            ("poof 258/tick", vec![poof], 0),
            ("smoke 238/tick", vec![smoke], 0),
            ("3 rockets/tick", vec![], 3),
        ] {
            measure(level, name, (&types, rockets), one, secs);
        }
        measure(level, "whole scene, 1 thread", (&all, 3), one, secs);
        for threads in [2, 3, 4, 6] {
            measure(
                level,
                &format!("whole scene, {threads} threads"),
                (&all, 3),
                (PARALLEL_MIN, threads),
                secs,
            );
        }
    });
}
