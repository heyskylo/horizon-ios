use std::hint::black_box;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::agg::{MAX_KEYS, MAX_NODES};
use super::*;
use crate::sampler::{self, Context, Kind, Record};
use crate::unwind::MAX_FRAMES;

fn cpu_ns() -> u64 {
    sampler::thread_cpu_ns()
}

#[inline(never)]
fn spin_fast(cpu_ms: u64) {
    spin(cpu_ms);
    black_box(0);
}

#[inline(never)]
fn spin_slow(cpu_ms: u64) {
    spin(cpu_ms);
    black_box(1);
}

#[inline(always)]
fn spin(cpu_ms: u64) {
    let (until, mut x) = (cpu_ns() + cpu_ms * 1_000_000, 1u64);
    while cpu_ns() < until {
        for _ in 0..1 << 16 {
            x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
    }
    black_box(x);
}

fn within(addr: usize, f: usize) -> bool {
    (f..f + 512).contains(&addr)
}

/// True when the trie path of `key` passes through an address inside `f`.
fn path_has(agg: &Aggregate, key: &Key, f: fn(u64)) -> bool {
    let f = f as *const () as usize;
    agg.path(key.node)
        .iter()
        .any(|&n| matches!(agg.frame(n), Frame::Addr(a) if within(a as usize + 1, f) || within(a as usize, f)))
}

fn config(wall: bool) -> Config {
    Config {
        sampler: sampler::Config {
            hz: 1000,
            ring_samples: 512,
            unwind: sampler::Unwind::StackCopy,
            stack_copy_kib: 16,
            wall,
        },
        ticks_over_ms: 50.0,
        ..Config::default()
    }
}

/// Runs `body` on a thread opted in as `name`, profiled for exactly its duration.
fn profiled(name: &'static str, config: Config, body: fn()) -> (Profile, Status) {
    let (ready, wait) = mpsc::channel();
    let (go, start) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        assert!(sampler::sampled_thread(name));
        ready.send(()).unwrap();
        start.recv().unwrap();
        body();
    });
    wait.recv().unwrap();
    let session = super::start(Config {
        threads: vec![name.into()],
        ..config
    })
    .expect("start profiling");
    go.send(()).unwrap();
    let mut mid = session.status();
    while !worker.is_finished() {
        thread::sleep(Duration::from_millis(20));
        mid = session.status();
    }
    worker.join().unwrap();
    (session.stop().expect("stop profiling"), mid)
}

fn synthetic(kind: Kind, tick: u64, value: u64, stack: &[usize]) -> Record {
    let mut frames = [0; MAX_FRAMES];
    frames[..stack.len()].copy_from_slice(stack);
    Record {
        kind,
        depth: stack.len() as u8,
        ctx: Context {
            tick,
            entity_type: 7,
            chunk: (-3, 9),
            ..Context::default()
        },
        value,
        frames,
    }
}

#[test]
fn only_samples_of_ticks_over_the_threshold_are_slow() {
    let mut agg = Aggregate::new(50_000_000);
    for tick in 1..=6u64 {
        for _ in 0..(tick * 3) {
            agg.add(
                "tick",
                &synthetic(Kind::Cpu, tick, 0, &[0x1000 + tick as usize, 0x2000]),
            );
        }
        let ns = if tick == 4 { 80_000_000 } else { 9_000_000 };
        agg.add("tick", &synthetic(Kind::Tick, tick, ns, &[]));
    }
    agg.add("tick", &synthetic(Kind::Cpu, 7, 0, &[0x1007, 0x2000]));
    agg.finish();
    let slow: u64 = agg
        .counts
        .iter()
        .filter(|(k, _)| k.slow)
        .map(|(_, n)| n)
        .sum();
    let all: u64 = agg.counts.values().sum();
    assert_eq!((slow, all), (12, 3 * 21 + 1));
    assert!(
        agg.counts
            .iter()
            .filter(|(k, _)| k.slow)
            .all(|(k, _)| { matches!(agg.frame(k.node), Frame::Addr(a) if a == 0x1004) })
    );
    let ticks = &agg.ticks[&0];
    assert_eq!((ticks.count, ticks.slow, ticks.max_ns), (6, 1, 80_000_000));
    assert_eq!(ticks.worst[0], (4, 80_000_000));
}

#[test]
fn return_addresses_resolve_inside_the_calling_instruction() {
    let mut agg = Aggregate::new(u64::MAX);
    agg.add("t", &synthetic(Kind::Cpu, 0, 0, &[0x5000, 0x6000, 0x7000]));
    agg.finish();
    let key = *agg.counts.keys().next().unwrap();
    let frames: Vec<_> = agg.path(key.node).iter().map(|&n| agg.frame(n)).collect();
    assert_eq!(
        frames,
        [
            Frame::Thread(0),
            Frame::Addr(0x6fff),
            Frame::Addr(0x5fff),
            Frame::Addr(0x5000)
        ]
    );
    assert_eq!((key.entity, key.chunk), (7, (-3, 9)));
}

#[test]
fn a_sixty_second_session_at_one_khz_stays_bounded() {
    let mut agg = Aggregate::new(50_000_000);
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let samples = 60 * 1000;
    for i in 0..samples {
        let stack: Vec<usize> = (0..MAX_FRAMES)
            .map(|_| 0x1000 + (next() % 400) as usize * 16)
            .collect();
        let mut record = synthetic(Kind::Cpu, i / 20, 0, &stack);
        record.ctx.chunk = ((next() % 5000) as i32, (next() % 5000) as i32);
        agg.add("tick", &record);
        if i % 20 == 19 {
            agg.add("tick", &synthetic(Kind::Tick, i / 20, 1_000_000, &[]));
        }
    }
    agg.finish();
    let counted: u64 = agg.counts.values().sum();
    assert!(agg.nodes() <= MAX_NODES, "{} trie nodes", agg.nodes());
    assert!(
        agg.counts.len() <= MAX_KEYS,
        "{} counters",
        agg.counts.len()
    );
    assert_eq!(
        counted + agg.lost,
        samples,
        "every sample is counted or reported lost"
    );
    assert!(
        agg.truncated > 0,
        "random 64-deep stacks must reach the node cap"
    );
}

#[test]
fn a_realistic_session_needs_few_nodes() {
    let mut agg = Aggregate::new(50_000_000);
    for i in 0..60_000usize {
        let stack = [0x1000 + i % 3, 0x2000 + i % 2, 0x3000, 0x4000];
        agg.add("tick", &synthetic(Kind::Cpu, 0, 0, &stack));
    }
    agg.finish();
    assert!(
        agg.nodes() < 30,
        "{} nodes for 6 distinct stacks",
        agg.nodes()
    );
    assert_eq!(agg.counts.values().sum::<u64>(), 60_000);
}

fn ticks_with_one_slow() {
    for tick in 1..=6u64 {
        sampler::set_tick(tick);
        let started = cpu_ns();
        if tick == 4 {
            spin_slow(120);
        } else {
            spin_fast(8);
        }
        sampler::tick_end(cpu_ns() - started);
    }
}

#[test]
fn a_session_keeps_the_slow_tick_apart_and_reports_status_while_running() {
    let (profile, mid) = profiled("p-ticks", config(false), ticks_with_one_slow);
    let agg = &profile.agg;
    let slow: Vec<_> = agg
        .counts
        .iter()
        .filter(|(k, _)| k.slow && k.state == State::Cpu)
        .collect();
    let slow_n: u64 = slow.iter().map(|(_, n)| **n).sum();
    assert!(slow_n > 60, "{slow_n} slow samples");
    assert!(
        slow.iter().all(|(k, _)| path_has(agg, k, spin_slow)),
        "a slow sample outside spin_slow"
    );
    let fast: Vec<_> = agg
        .counts
        .iter()
        .filter(|(k, _)| !k.slow && k.state == State::Cpu)
        .collect();
    assert!(fast.iter().all(|(k, _)| !path_has(agg, k, spin_slow)));
    assert!(fast.iter().map(|(_, n)| **n).sum::<u64>() > 20);
    assert_eq!((agg.ticks[&0].count, agg.ticks[&0].slow), (6, 1));
    assert!(
        mid.cpu_samples > 0,
        "the writer drained during the session: {mid:?}"
    );
    assert!(profile.writer_cpu_ns > 0 && profile.stats.samples > 100);
}

fn sleeps() {
    thread::sleep(Duration::from_millis(250));
}

#[test]
fn wall_samples_of_a_sleeping_thread_are_waiting() {
    let (profile, _) = profiled("p-wall-sleep", config(true), sleeps);
    let agg = &profile.agg;
    let (waiting, running) = (
        agg.total(State::Waiting, false),
        agg.total(State::Running, false),
    );
    assert!(waiting > 60, "{waiting} waiting samples in 250 ms at 1 kHz");
    assert!(
        running * 10 <= waiting,
        "{running} running vs {waiting} waiting"
    );
    assert!(
        agg.counts
            .keys()
            .filter(|k| k.state == State::Waiting)
            .all(|k| {
                agg.path(k.node)
                    .iter()
                    .any(|&n| matches!(agg.frame(n), Frame::Wall(State::Waiting)))
            })
    );
}

fn spins() {
    spin_fast(200);
}

#[test]
fn a_wall_session_also_records_cpu_samples_without_mixing_them() {
    let (profile, _) = profiled("p-wall-spin", config(true), spins);
    let agg = &profile.agg;
    let (cpu, running, waiting) = (
        agg.total(State::Cpu, false),
        agg.total(State::Running, false),
        agg.total(State::Waiting, false),
    );
    assert!(cpu > 100 && running > 100, "cpu {cpu} running {running}");
    assert!(
        waiting * 5 <= running,
        "{waiting} waiting vs {running} running"
    );
}
