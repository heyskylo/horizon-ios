use std::collections::HashMap;
use std::hint::black_box;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use horizon_perf_format::v1::{
    Kind as CaptureKind, SampleProfile, Side, Start, Stop, StringEntry, Unit,
};
use horizon_perf_format::{CaptureWriter, Field};
use horizon_threads::profile::{self, Aggregate, Config, Profile};
use horizon_threads::sampler::{self, Context, Kind, Record, Stats};
use horizon_threads::unwind::MAX_FRAMES;
use horizon_threads::work;

use super::*;

#[inline(never)]
fn alpha() {
    black_box(1);
}

#[inline(never)]
fn beta() {
    black_box(2);
}

#[inline(never)]
fn gamma() {
    black_box(3);
}

fn at(f: fn(), offset: usize) -> usize {
    f as *const () as usize + offset
}

fn record(kind: Kind, ctx: Context, value: u64, stack: &[usize]) -> Record {
    let mut frames = [0; MAX_FRAMES];
    frames[..stack.len()].copy_from_slice(stack);
    Record {
        kind,
        depth: stack.len() as u8,
        ctx,
        value,
        frames,
    }
}

fn profile_of(agg: Aggregate, config: Config) -> Profile {
    Profile {
        agg,
        config,
        stats: Stats::default(),
        elapsed: Duration::from_secs(10),
        started: SystemTime::now(),
        writer_cpu_ns: 0,
    }
}

struct Converted {
    samples: SampleProfile,
    strings: Vec<StringEntry>,
}

impl Converted {
    fn text(&self, id: u32) -> &str {
        match id {
            0 => "",
            id => &self.strings.iter().find(|s| s.id == id).unwrap().text,
        }
    }

    fn function(&self, row: u32) -> &str {
        self.text(self.samples.functions.as_ref().unwrap().name[row as usize])
    }

    /// Function names from the root down to `node`.
    fn path(&self, node: u32) -> Vec<&str> {
        let nodes = self.samples.nodes.as_ref().unwrap();
        let mut path = vec![self.function(nodes.function[node as usize])];
        let mut at = node as usize;
        while nodes.thread_root[at] == 0 {
            at = nodes.parent[at] as usize;
            path.push(self.function(nodes.function[at]));
        }
        path.reverse();
        path
    }

    /// Samples per entity name, optionally only the slow ones.
    fn by_entity(&self, slow_only: bool) -> HashMap<String, u64> {
        let (w, c) = (
            self.samples.weights.as_ref().unwrap(),
            self.samples.contexts.as_ref().unwrap(),
        );
        let mut by = HashMap::new();
        for i in 0..w.node.len() {
            if slow_only && !w.slow[i] {
                continue;
            }
            let entity = self.text(c.entity[w.context[i] as usize]).to_owned();
            *by.entry(entity).or_default() += w.count[i];
        }
        by
    }
}

fn convert(profile: &Profile) -> Converted {
    let mut interner = Interner::default();
    let samples = sample_profile(
        profile,
        None,
        Side::Server,
        Unit::Tick,
        &|id| (id == 7).then(|| "minecraft:hopper".to_owned()),
        &mut interner,
    );
    Converted {
        samples,
        strings: interner.take_new(Side::Server),
    }
}

fn short(name: &str) -> &str {
    name.rsplit("::").next().unwrap()
}

#[test]
fn a_stack_becomes_a_root_to_leaf_chain_with_its_context() {
    let entity = work::entity_id("test:zombie");
    let ctx = Context {
        tick: 1,
        entity_type: entity,
        block: 7,
        dim: 2,
        chunk: (-3, 9),
        ..Context::default()
    };
    let mut agg = Aggregate::new(50_000_000);
    for _ in 0..5 {
        agg.add(
            "tick",
            &record(Kind::Cpu, ctx, 0, &[at(alpha, 4), at(gamma, 3)]),
        );
    }
    agg.add("tick", &record(Kind::Tick, ctx, 9_000_000, &[]));
    agg.finish();
    let c = convert(&profile_of(agg, Config::default()));
    let w = c.samples.weights.as_ref().unwrap();
    assert_eq!(
        (w.node.len(), w.count[0], w.state[0], w.slow[0]),
        (1, 5, 0, false)
    );
    let path: Vec<_> = c.path(w.node[0]).into_iter().map(short).collect();
    assert_eq!(path, ["gamma", "alpha"]);
    let nodes = c.samples.nodes.as_ref().unwrap();
    assert_eq!(nodes.thread_root, [1, 0]);
    let ctxs = c.samples.contexts.as_ref().unwrap();
    let row = w.context[0] as usize;
    assert_eq!(c.text(ctxs.entity[row]), "test:zombie");
    assert_eq!(c.text(ctxs.block[row]), "minecraft:hopper");
    assert_eq!(c.text(ctxs.dim[row]), "the_nether");
    assert_eq!((ctxs.chunk_x[row], ctxs.chunk_z[row]), (-3, 9));
    assert_eq!(c.text(c.samples.threads[0].name), "tick");
    assert_eq!(
        (c.samples.threads[0].units, c.samples.threads[0].slow_units),
        (1, 0)
    );
}

#[test]
fn addresses_of_one_function_share_one_row() {
    let ctx = Context::default();
    let mut agg = Aggregate::new(50_000_000);
    for (inner, count) in [(4, 2), (8, 3)] {
        for _ in 0..count {
            agg.add(
                "tick",
                &record(Kind::Cpu, ctx, 0, &[at(alpha, inner), at(gamma, 3)]),
            );
        }
    }
    agg.add(
        "tick",
        &record(Kind::Cpu, ctx, 0, &[at(beta, 4), at(gamma, 3)]),
    );
    agg.finish();
    let c = convert(&profile_of(agg, Config::default()));
    let f = c.samples.functions.as_ref().unwrap();
    let names: Vec<_> = (0..f.name.len() as u32)
        .map(|row| short(c.function(row)))
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, ["alpha", "beta", "gamma"]);
    let w = c.samples.weights.as_ref().unwrap();
    let on_alpha: u64 = (0..w.node.len())
        .filter(|&i| short(c.path(w.node[i]).last().unwrap()) == "alpha")
        .map(|i| w.count[i])
        .sum();
    assert_eq!(on_alpha, 5);
    let n = c.samples.nodes.as_ref().unwrap();
    let alphas: Vec<usize> = (0..n.function.len())
        .filter(|&i| short(c.function(n.function[i])) == "alpha")
        .collect();
    assert_eq!(alphas.len(), 1, "one node per (parent, function)");
    assert_eq!(
        short(c.function(n.function[n.parent[alphas[0]] as usize])),
        "gamma"
    );
    let weight_on_alpha: u64 = (0..w.node.len())
        .filter(|&i| w.node[i] as usize == alphas[0])
        .map(|i| w.count[i])
        .sum();
    assert_eq!(weight_on_alpha, 5);
    let alpha_row = names.iter().position(|n| *n == "alpha").unwrap();
    assert!(f.addr[alpha_row] > 0, "a module-relative address is kept");
}

#[test]
fn time_blocks_and_ticks_per_block_follow_when_they_were_added() {
    let ctx = |tick| Context {
        tick,
        ..Context::default()
    };
    let mut agg = Aggregate::new(50_000_000);
    for (block, tick, samples) in [(0, 1, 2u64), (1, 2, 3), (1, 3, 1), (3, 4, 4)] {
        agg.set_time_block(block);
        for _ in 0..samples {
            agg.add("tick", &record(Kind::Cpu, ctx(tick), 0, &[at(alpha, 1)]));
        }
        agg.add("tick", &record(Kind::Tick, ctx(tick), 1_000_000, &[]));
    }
    agg.finish();
    let config = Config {
        block_s: 2.5,
        ..Config::default()
    };
    let c = convert(&profile_of(agg, config));
    let w = c.samples.weights.as_ref().unwrap();
    let mut per_block = [0u64; 4];
    for i in 0..w.node.len() {
        per_block[w.block[i] as usize] += w.count[i];
    }
    assert_eq!(per_block, [2, 4, 0, 4]);
    assert_eq!(c.samples.block_s, 2.5);
    assert_eq!(c.samples.block_units[0].units, [1, 2, 0, 1]);
    assert_eq!(c.samples.threads[0].units, 4);
}

#[test]
fn a_sample_with_no_stack_hangs_off_an_unknown_root() {
    let mut agg = Aggregate::new(50_000_000);
    agg.add("tick", &record(Kind::Cpu, Context::default(), 0, &[]));
    agg.finish();
    let c = convert(&profile_of(agg, Config::default()));
    let w = c.samples.weights.as_ref().unwrap();
    assert_eq!(c.path(w.node[0]), ["[unknown]"]);
}

#[test]
fn wall_samples_carry_their_state_and_a_second_kind() {
    let mut agg = Aggregate::new(50_000_000);
    let ctx = Context::default();
    agg.add("tick", &record(Kind::Cpu, ctx, 0, &[at(alpha, 1)]));
    agg.add("tick", &record(Kind::Wall, ctx, 1, &[at(alpha, 1)]));
    agg.add("tick", &record(Kind::Wall, ctx, 0, &[at(beta, 1)]));
    agg.finish();
    let config = Config {
        sampler: sampler::Config {
            hz: 200,
            ..sampler::Config::default()
        },
        ..Config::default()
    };
    let c = convert(&profile_of(agg, config));
    assert_eq!(c.samples.kinds.len(), 2);
    assert_eq!(c.samples.kinds[0].period, 5.0);
    let w = c.samples.weights.as_ref().unwrap();
    let mut seen: Vec<_> = (0..w.node.len())
        .map(|i| {
            (
                short(c.path(w.node[i]).last().unwrap()).to_owned(),
                w.kind[i],
                w.state[i],
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("alpha".into(), 0, 0),
            ("alpha".into(), 1, 1),
            ("beta".into(), 1, 2)
        ]
    );
}

#[inline(never)]
fn spin_fast() {
    spin(8);
    black_box(0);
}

#[inline(never)]
fn spin_slow() {
    spin(120);
    black_box(1);
}

fn spin(cpu_ms: u64) {
    let until = sampler::thread_cpu_ns() + cpu_ms * 1_000_000;
    let mut x = 1u64;
    while sampler::thread_cpu_ns() < until {
        for _ in 0..1 << 14 {
            x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
    }
    black_box(x);
}

fn live_config() -> Config {
    Config {
        sampler: sampler::Config {
            hz: 1000,
            ring_samples: 512,
            unwind: sampler::Unwind::StackCopy,
            stack_copy_kib: 16,
            wall: false,
        },
        ticks_over_ms: 50.0,
        ..Config::default()
    }
}

/// Runs `body` on a thread opted in as `name`, profiled for exactly its duration.
fn profiled(name: &'static str, body: fn()) -> Profile {
    let (ready, wait) = mpsc::channel();
    let (go, start) = mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        assert!(sampler::sampled_thread(name));
        ready.send(()).unwrap();
        start.recv().unwrap();
        body();
    });
    wait.recv().unwrap();
    let session = profile::start(Config {
        threads: vec![name.into()],
        ..live_config()
    })
    .expect("start profiling");
    go.send(()).unwrap();
    worker.join().unwrap();
    session.stop().expect("stop profiling")
}

fn entity_units() {
    let (a, b) = (work::entity_id("test:a"), work::entity_id("test:b"));
    for unit in 0..4 {
        work::tick_start();
        work::set_entity(if unit < 3 { a } else { b });
        let started = sampler::thread_cpu_ns();
        spin(40);
        work::clear_work();
        work::tick_end(sampler::thread_cpu_ns() - started);
    }
}

#[test]
fn three_units_of_a_and_one_of_b_split_the_samples_three_to_one() {
    let c = convert(&profiled("perf-capture-entities", entity_units));
    let by = c.by_entity(false);
    let total = (by["test:a"] + by["test:b"]) as f64;
    let (a, b) = (by["test:a"] as f64 / total, by["test:b"] as f64 / total);
    assert!((a - 0.75).abs() < 0.1 && (b - 0.25).abs() < 0.1, "{by:?}");
    assert!(total > 100.0, "{total} samples");
    assert_eq!(c.samples.threads[0].units, 4);
}

fn ticks_with_one_slow() {
    for tick in 0..6 {
        work::tick_start();
        let started = sampler::thread_cpu_ns();
        if tick == 3 {
            spin_slow();
        } else {
            spin_fast();
        }
        work::tick_end(sampler::thread_cpu_ns() - started);
    }
}

#[test]
fn only_the_slow_ticks_function_is_in_the_slow_weights() {
    let c = convert(&profiled("perf-capture-slow", ticks_with_one_slow));
    let w = c.samples.weights.as_ref().unwrap();
    let (mut slow, mut fast) = (0, 0);
    for i in 0..w.node.len() {
        let names: Vec<_> = c.path(w.node[i]).into_iter().map(short).collect();
        if w.slow[i] {
            slow += w.count[i];
            assert!(
                names.contains(&"spin_slow") && !names.contains(&"spin_fast"),
                "{names:?}"
            );
        } else {
            fast += w.count[i];
            assert!(!names.contains(&"spin_slow"), "{names:?}");
        }
    }
    assert!(slow > 60 && fast > 20, "{slow} slow, {fast} other");
    assert_eq!(
        (c.samples.threads[0].units, c.samples.threads[0].slow_units),
        (6, 1)
    );
}

#[test]
fn overhead_counts_lost_samples_and_the_writers_share_of_a_core() {
    let mut agg = Aggregate::new(1);
    agg.lost = 4;
    let mut profile = profile_of(agg, Config::default());
    profile.stats.dropped = 3;
    profile.stats.samples = 1000;
    profile.stats.handler_ns = Some(2_000.0);
    profile.writer_cpu_ns = 50_000_000;
    let o = overhead(&profile, None);
    assert_eq!(o.dropped_samples, 7);
    assert!((o.sampler_share_of_thread - 0.0002).abs() < 1e-7);
    assert!((o.writer_core_share - 0.005).abs() < 1e-6);
}

#[test]
fn crates_come_from_the_first_path_segment() {
    for (name, krate) in [
        (
            "horizon_world::level::tick::random_tick_at",
            "horizon_world",
        ),
        (
            "<horizon_world::X as core::ops::Drop>::drop",
            "horizon_world",
        ),
        ("core::ptr::drop_in_place<alloc::vec::Vec<u8>>", "core"),
        ("clone3", ""),
        ("[horizon-server 0x4f2a]", "horizon-server"),
        ("[unmapped 0x7ffd]", ""),
    ] {
        assert_eq!(crate_of(name), krate, "{name}");
    }
}

fn ask(
    file: &std::path::Path,
    parts: &[&str],
) -> horizon_perf::wire::Envelope<horizon_perf::query::Row> {
    let env = horizon_perf::cli::Env {
        horizon_home: None,
        poll: Duration::from_millis(2),
    };
    let mut args = vec!["query".to_owned()];
    args.extend(parts.iter().map(|p| (*p).to_owned()));
    args.push(file.to_string_lossy().into_owned());
    let mut out = Vec::new();
    if let Err(e) = horizon_perf::cli::run(&args, &env, &mut out) {
        panic!("{e:?}");
    }
    serde_json::from_slice(&out)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out)))
}

#[test]
fn a_converted_profile_written_to_a_file_reads_back_through_the_queries() {
    let ctx = Context::default();
    let mut agg = Aggregate::new(50_000_000);
    for _ in 0..5 {
        agg.add(
            "tick",
            &record(Kind::Cpu, ctx, 0, &[at(alpha, 4), at(gamma, 3)]),
        );
    }
    for _ in 0..2 {
        agg.add(
            "tick",
            &record(Kind::Cpu, ctx, 0, &[at(beta, 4), at(gamma, 3)]),
        );
    }
    for _ in 0..2 {
        agg.add("tick", &record(Kind::Tick, ctx, 9_000_000, &[]));
    }
    for _ in 0..3 {
        agg.add("worker", &record(Kind::Cpu, ctx, 0, &[at(beta, 4)]));
    }
    agg.add("worker", &record(Kind::Tick, ctx, 9_000_000, &[]));
    agg.finish();
    let config = Config {
        sampler: sampler::Config {
            hz: 100,
            ..sampler::Config::default()
        },
        ..Config::default()
    };
    let c = convert(&profile_of(agg, config));
    let mut fields = vec![Field::Start(Start {
        side: Side::Server as i32,
        kind: CaptureKind::Dedicated as i32,
        ..Default::default()
    })];
    fields.extend(c.strings.into_iter().map(Field::String));
    fields.push(Field::Samples(c.samples));
    fields.push(Field::Stop(Stop {
        side: Side::Server as i32,
        t: 10.0,
        ..Default::default()
    }));
    let dir = std::env::temp_dir().join(format!("horizon-render-perf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("real.hzperf");
    let mut writer = CaptureWriter::new(std::fs::File::create(&file).unwrap()).unwrap();
    for field in fields {
        writer.write(field).unwrap();
    }
    writer.finish().unwrap();

    // 10 ms samples: alpha has 5 over the tick thread's 2 ticks.
    let hot = ask(&file, &["hot"]);
    let alpha = hot.rows().iter().find_map(|r| match r {
        horizon_perf::query::Row::Hot(h) if h.full.ends_with("alpha") => Some(h),
        _ => None,
    });
    assert_eq!(alpha.map(|h| (h.self_ms, h.samples)), Some((25.0, 5)));
    let worker = ask(&file, &["split", "t:worker", "--by", "thread"]);
    let sel = worker.selection.as_ref().unwrap();
    assert_eq!(
        (sel.thread.as_deref(), sel.ticks, sel.samples),
        (Some("worker"), Some(1), Some(3))
    );
    let values: Vec<_> = worker
        .rows()
        .iter()
        .map(|r| match r {
            horizon_perf::query::Row::Split(s) => (s.key.clone(), s.value),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(values, [(Some("t:worker".to_owned()), 30.0)]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn heap_samples_share_the_tables_without_the_allocators_own_frames() {
    let mut cpu = Aggregate::new(50_000_000);
    cpu.add(
        "tick",
        &record(
            Kind::Cpu,
            Context::default(),
            0,
            &[at(alpha, 4), at(gamma, 3)],
        ),
    );
    cpu.finish();
    let mut heap = Aggregate::new(0);
    let inside = HeapProfile::bytes as *const () as usize + 4;
    let on_tick = heap.thread_index("tick");
    let key = heap.stack_key(
        on_tick,
        profile::State::Alloc,
        &Context::default(),
        &[inside, at(beta, 4), at(gamma, 3)],
    );
    heap.add_weight(key, 3000);
    heap.add_weight(
        profile::Key {
            state: profile::State::Live,
            ..key
        },
        1000,
    );
    let on_worker = heap.thread_index("worker");
    let key = heap.stack_key(
        on_worker,
        profile::State::Live,
        &Context::default(),
        &[inside, at(alpha, 4)],
    );
    heap.add_weight(key, 500);
    let heap = HeapProfile {
        agg: heap,
        interval: 4096,
        samples: 3,
        live_samples: 2,
        sample_ns: 0,
        freed: 2000,
        elapsed: Duration::from_secs(10),
    };
    let mut interner = Interner::default();
    let samples = sample_profile(
        &profile_of(cpu, Config::default()),
        Some(&heap),
        Side::Server,
        Unit::Tick,
        &|_| None,
        &mut interner,
    );
    let c = Converted {
        samples,
        strings: interner.take_new(Side::Server),
    };
    let sources: Vec<_> = c
        .samples
        .kinds
        .iter()
        .map(|k| (k.source(), k.period))
        .collect();
    assert_eq!(
        sources,
        [
            (SampleSource::Cpu, 4.0),
            (SampleSource::Alloc, 1.0),
            (SampleSource::Live, 1.0)
        ]
    );
    let names: Vec<_> = c.samples.threads.iter().map(|t| c.text(t.name)).collect();
    assert_eq!(names, ["tick", "worker"]);
    let w = c.samples.weights.as_ref().unwrap();
    let mut rows: Vec<_> = (0..w.node.len())
        .map(|i| {
            let path: Vec<_> = c.path(w.node[i]).into_iter().map(short).collect();
            (w.kind[i], w.thread[i], path.join(">"), w.count[i])
        })
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            (0, 0, "gamma>alpha".to_owned(), 1),
            (1, 0, "gamma>beta".to_owned(), 3000),
            (2, 0, "gamma>beta".to_owned(), 1000),
            (2, 1, "alpha".to_owned(), 500),
        ]
    );
    let o = overhead(
        &profile_of(Aggregate::new(0), Config::default()),
        Some(&heap),
    );
    assert_eq!(o.heap_samples, 3);
}
