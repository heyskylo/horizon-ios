use super::*;
use horizon_perf_format::v1::{Capture, Kind as K, Side as S, StopReason as R};
use std::fs;

mod timeline_capture;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("hz-server-perf-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options(window_s: f64) -> CaptureOptions {
    CaptureOptions {
        window_s: Some(window_s),
        trigger: Trigger::Console,
        start: Start {
            players: 3,
            settings: Some(settings(10, 8, 20, 256)),
            ..Start::default()
        },
        samples: false,
        sampler: SamplerOptions::default(),
        block_names: Vec::new(),
        levels: Vec::new(),
        wire: Vec::new(),
        gpu: GpuMode::Unspecified,
        timeline: None,
        file: None,
    }
}

fn span(name: &'static str, ms: f32) -> Span {
    Span {
        name,
        ms,
        cpu_ms: ms,
        depth: 0,
    }
}

fn stop(perf: &mut ServerPerf, t: f64, reason: R) -> Option<std::io::Result<Saved>> {
    perf.stop_capture(t, reason, CaptureEnd::default())
}

fn text(capture: &Capture, id: u32) -> &str {
    &capture.strings.iter().find(|s| s.id == id).unwrap().text
}

/// `per_s` ticks a second from second `from` to `to`; each `(index, wall_ms, span)` is a slow tick.
fn run(perf: &mut ServerPerf, from: u32, to: u32, per_s: u32, slow: &[(u32, f32, &'static str)]) {
    for i in 0..(to - from) * per_s {
        let t = f64::from(from) + f64::from(i) / f64::from(per_s);
        let (wall, detail) = match slow.iter().find(|s| s.0 == i) {
            Some(&(_, wall, name)) => (
                wall,
                Some(TickDetail {
                    cpu_ms: Some(280.0),
                    spans: vec![span("tick_lan", 4.0), span(name, wall - 20.0)],
                }),
            ),
            None => (10.0, None),
        };
        perf.tick(t, wall, detail, WorldCounts::default, Vec::new);
    }
}

fn read(path: &Path) -> Capture {
    horizon_perf::read_capture(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn three_hundred_slow_ticks_in_ten_seconds_leave_the_last_256_in_the_capture() {
    let scratch = Scratch::new("ring");
    let mut perf = ServerPerf::new(0.0, 250.0);
    // Tick k (1-based) takes 299 + k ms, so every one is slow.
    let slow: Vec<_> = (0..300)
        .map(|i| (i, 300.0 + i as f32, "tick_mobs"))
        .collect();
    run(&mut perf, 0, 10, 30, &slow);
    let ring: Vec<_> = perf.slow_records().map(|r| r.counter).collect();
    assert_eq!(ring, (45..=300).collect::<Vec<u64>>());
    perf.start_capture(10.0, &scratch.0, options(1.0)).unwrap();
    run(&mut perf, 10, 11, 10, &[]);
    let saved = stop(&mut perf, 11.0, R::Normal).unwrap().unwrap();
    let c = read(&saved.path);
    let history: Vec<_> = c
        .slow_history
        .iter()
        .map(|e| (e.counter, e.wall_ms))
        .collect();
    let expected: Vec<_> = (45..=300u64).map(|k| (k, 299.0 + k as f32)).collect();
    assert_eq!(history, expected);
    let newest = &c.slow_history[255];
    let top: Vec<_> = newest
        .top
        .iter()
        .map(|s| (text(&c, s.name), s.wall_ms))
        .collect();
    assert_eq!(top, [("tick_mobs", 579.0), ("tick_lan", 4.0)]);
    assert!(c.slow.is_empty(), "no tick of the capture itself was slow");
}

#[test]
fn a_forced_server_reads_back_with_its_tick_count_and_tps() {
    let scratch = Scratch::new("forced");
    let mut perf = ServerPerf::new(0.0, 250.0);
    // 3 s at 10 TPS before the capture, one slow tick (the 6th) among them.
    run(&mut perf, 0, 3, 10, &[(5, 400.0, "tick_mobs")]);
    perf.start_capture(3.0, &scratch.0, options(5.0)).unwrap();
    assert!(perf.capturing());
    // 5 s at 20 TPS; ticks 7, 33 and 61 take 300 ms.
    run(
        &mut perf,
        3,
        8,
        20,
        &[
            (7, 300.0, "tick_mobs"),
            (33, 300.0, "tick_chunks"),
            (61, 300.0, "tick_mobs"),
        ],
    );
    let saved = stop(&mut perf, 8.0, R::Normal).unwrap().unwrap();
    assert!(saved.deleted.is_empty());
    assert!(!perf.capturing());

    let c = read(&saved.path);
    assert_eq!(c.format, 1);
    let start = &c.start[0];
    assert_eq!(start.side(), S::Server);
    assert_eq!(start.kind(), K::Dedicated);
    assert_eq!(start.trigger(), Trigger::Console);
    assert_eq!(start.players, 3);
    assert_eq!(start.options.as_ref().unwrap().window_s, Some(5.0));
    assert_eq!(start.settings.as_ref().unwrap().view_distance, Some(10));

    assert_eq!(c.health.len(), 5);
    for (i, h) in c.health.iter().enumerate() {
        assert_eq!(h.t, i as f64);
        assert_eq!(h.tps, Some(20.0), "second {i}");
        assert_eq!(h.mspt_p50, Some(10.0));
    }
    let ticks: f32 = c.health.iter().filter_map(|h| h.tps).sum();
    assert_eq!(ticks, 100.0);
    let maxes: Vec<_> = c.health.iter().map(|h| h.mspt_max).collect();
    assert_eq!(
        maxes,
        [
            Some(300.0),
            Some(300.0),
            Some(10.0),
            Some(300.0),
            Some(10.0)
        ]
    );

    let history = &c.history[0];
    assert_eq!(history.step_s, 1.0);
    assert_eq!(history.t0, -3.0);
    assert_eq!(history.samples.len(), 3);
    assert!(history.samples.iter().all(|s| s.tps == Some(10.0)));

    let counters: Vec<_> = c.slow.iter().map(|e| e.counter).collect();
    assert_eq!(counters, [30 + 8, 30 + 34, 30 + 62]);
    assert!((c.slow[1].t - 1.65).abs() < 1e-9);
    let top: Vec<_> = c.slow[1]
        .top
        .iter()
        .map(|s| (text(&c, s.name), s.wall_ms))
        .collect();
    assert_eq!(top, [("tick_chunks", 280.0), ("tick_lan", 4.0)]);
    assert_eq!(c.slow_history.len(), 1);
    assert_eq!(c.slow_history[0].counter, 6);
    assert_eq!(c.slow_history[0].t, -2.5);
    assert_eq!(text(&c, c.slow_history[0].top[0].name), "tick_mobs");

    assert_eq!(c.stop.len(), 1);
    assert_eq!(c.stop[0].reason(), R::Normal);
    assert_eq!(c.stop[0].t, 5.0);
}

#[test]
fn a_capture_started_mid_second_leaves_out_the_partial_second() {
    let scratch = Scratch::new("partial");
    let mut perf = ServerPerf::new(0.0, 250.0);
    run(&mut perf, 0, 3, 20, &[]);
    perf.start_capture(3.5, &scratch.0, options(5.0)).unwrap();
    for i in 0..80 {
        perf.tick(
            3.5 + f64::from(i) / 20.0,
            10.0,
            None,
            WorldCounts::default,
            Vec::new,
        );
    }
    let saved = stop(&mut perf, 7.5, R::WindowCap).unwrap().unwrap();
    let c = read(&saved.path);
    // The second 3 to 4 began before the capture and is left out; the whole seconds 4, 5 and 6 are in.
    let ts: Vec<_> = c.health.iter().map(|h| h.t).collect();
    assert_eq!(ts, [0.5, 1.5, 2.5]);
    assert_eq!(c.stop[0].reason(), R::WindowCap);
}

#[test]
fn counters_are_reported_with_their_names_and_totals() {
    let scratch = Scratch::new("counters");
    let hits = horizon_perf_format::counter("s1a.test", &[("kind", "light")]);
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(3.0)).unwrap();
    hits.add(7);
    run(&mut perf, 0, 3, 20, &[]);
    let saved = stop(&mut perf, 3.0, R::Normal).unwrap().unwrap();
    let c = read(&saved.path);
    let def = c
        .counter_defs
        .iter()
        .find(|d| text(&c, d.name) == "s1a.test")
        .expect("the counter is defined");
    assert_eq!(text(&c, def.labels[0].key), "kind");
    assert_eq!(text(&c, def.labels[0].value), "light");
    let last = c.counters.last().unwrap();
    let at = last.ids.iter().position(|&id| id == def.id).unwrap();
    assert!(last.values[at] >= 7.0);
}

#[test]
fn finishing_removes_the_oldest_capture_beyond_thirty_and_keeps_a_renamed_one() {
    let scratch = Scratch::new("retention");
    let old: Vec<PathBuf> = (0..30)
        .map(|i| scratch.0.join(files::file_name(1_000_000 + i * 60)))
        .collect();
    for path in &old {
        fs::write(path, b"x").unwrap();
    }
    let renamed = scratch.0.join("profile-19700101-000000Z-mine.hzperf");
    fs::write(&renamed, b"x").unwrap();
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(1.0)).unwrap();
    run(&mut perf, 0, 1, 20, &[]);
    let saved = stop(&mut perf, 1.0, R::Normal).unwrap().unwrap();
    assert_eq!(saved.deleted, [old[0].clone()]);
    assert!(saved.path.exists() && renamed.exists() && old[1].exists());
    let line = files::saved_line(&saved.path);
    assert!(line.contains(saved.path.to_str().unwrap()), "{line}");
}

#[test]
fn a_second_capture_cannot_start_and_stopping_with_none_running_is_none() {
    let scratch = Scratch::new("single");
    let mut perf = ServerPerf::new(0.0, 250.0);
    assert!(stop(&mut perf, 1.0, R::Normal).is_none());
    perf.start_capture(0.0, &scratch.0, options(1.0)).unwrap();
    assert!(perf.start_capture(0.0, &scratch.0, options(1.0)).is_err());
    stop(&mut perf, 0.0, R::Shutdown).unwrap().unwrap();
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[test]
fn a_slow_tick_without_detail_has_no_spans_and_no_invented_cpu_time() {
    let scratch = Scratch::new("nodetail");
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(5.0)).unwrap();
    perf.tick(0.5, 400.0, None, WorldCounts::default, Vec::new);
    let saved = stop(&mut perf, 1.0, R::Normal).unwrap().unwrap();
    let c = read(&saved.path);
    assert_eq!(c.slow.len(), 1);
    assert_eq!(c.slow[0].wall_ms, 400.0);
    assert_eq!(c.slow[0].cpu_ms, 0.0);
    assert!(c.slow[0].top.is_empty());
}

#[test]
fn a_capture_without_the_samples_flag_has_no_samples_field() {
    let scratch = Scratch::new("nosamples");
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(5.0)).unwrap();
    run(&mut perf, 0, 1, 20, &[]);
    let saved = stop(&mut perf, 1.0, R::Normal).unwrap().unwrap();
    let c = read(&saved.path);
    assert!(c.samples.is_empty());
    assert!(!c.start[0].options.as_ref().unwrap().samples);
}

#[cfg(target_os = "linux")]
mod sampled {
    use super::*;
    use horizon_threads::{sampler, work};
    use std::collections::HashMap;
    use std::hint::black_box;

    #[inline(never)]
    fn spin_unit(cpu_ms: u64) {
        let until = sampler::thread_cpu_ns() + cpu_ms * 1_000_000;
        let mut x = 1u64;
        while sampler::thread_cpu_ns() < until {
            for _ in 0..1 << 14 {
                x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
            }
        }
        black_box(x);
    }

    #[test]
    fn a_sampled_capture_reads_back_with_the_entity_shares_of_the_work() {
        let _turn = crate::server_perf::SAMPLED_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let scratch = Scratch::new("samples");
        let mut perf = ServerPerf::new(0.0, 250.0);
        let on = CaptureOptions {
            samples: true,
            block_names: vec!["minecraft:air".into()],
            ..options(5.0)
        };
        perf.start_capture(0.0, &scratch.0, on).unwrap();
        let (a, b) = (work::entity_id("test:a"), work::entity_id("test:b"));
        for unit in 0..4u32 {
            work::tick_start();
            work::set_entity(if unit < 3 { a } else { b });
            let started = sampler::thread_cpu_ns();
            spin_unit(40);
            work::clear_work();
            let ns = sampler::thread_cpu_ns() - started;
            work::tick_end(ns);
            perf.tick(
                f64::from(unit) * 0.05,
                ns as f32 / 1e6,
                None,
                WorldCounts::default,
                Vec::new,
            );
        }
        let saved = stop(&mut perf, 1.0, R::Normal).unwrap().unwrap();
        let c = read(&saved.path);

        let options = c.start[0].options.as_ref().unwrap();
        assert!(options.samples && options.hz == sampler::DEFAULT_HZ);
        let overhead = c.stop[0].overhead.as_ref().expect("the sampler's cost");
        assert!(overhead.writer_core_share >= 0.0 && overhead.sampler_share_of_thread >= 0.0);

        let profile = &c.samples[0];
        assert_eq!(profile.side(), S::Server);
        assert_eq!(text(&c, profile.threads[0].name), "tick");
        assert_eq!(profile.threads[0].units, 4);
        let (w, ctx) = (
            profile.weights.as_ref().unwrap(),
            profile.contexts.as_ref().unwrap(),
        );
        let mut by: HashMap<&str, u64> = HashMap::new();
        for i in 0..w.node.len() {
            let entity = ctx.entity[w.context[i] as usize];
            *by.entry(if entity == 0 { "" } else { text(&c, entity) })
                .or_default() += w.count[i];
        }
        let total = (by["test:a"] + by["test:b"]) as f64;
        let share_a = by["test:a"] as f64 / total;
        let share_b = by["test:b"] as f64 / total;
        assert!(
            (share_a - 0.75).abs() < 0.1 && (share_b - 0.25).abs() < 0.1,
            "{by:?}"
        );

        let f = profile.functions.as_ref().unwrap();
        let mut seen = std::collections::HashSet::new();
        for row in 0..f.name.len() {
            assert!(
                seen.insert((f.name[row], f.file[row])),
                "{} twice",
                text(&c, f.name[row])
            );
        }
        let names: Vec<_> = f.name.iter().map(|&n| text(&c, n)).collect();
        assert!(names.iter().any(|n| n.ends_with("spin_unit")), "{names:?}");
    }
}

use super::spans::{BLOCK_S, HISTOGRAM_EDGES_MS};
use super::world::{LevelCounts, MobCost, MobCosts, WorkerJobs};
use horizon_perf_format::v1::SpanSummary;

fn nested(name: &'static str, ms: f32, cpu_ms: f32, depth: u8) -> Span {
    Span {
        name,
        ms,
        cpu_ms,
        depth,
    }
}

/// The spans of one tick as they are recorded: `tick` holds `tick_mobs` (twice, 1 + 2 ms) and `tick_lan` (1 ms).
fn tick_spans(wall_ms: f32) -> Vec<Span> {
    vec![
        nested("tick_mobs", 1.0, 0.5, 1),
        nested("tick_lan", 1.0, 1.0, 1),
        nested("tick_mobs", 2.0, 1.0, 1),
        nested("tick", wall_ms - 1.0, wall_ms - 2.0, 0),
    ]
}

fn name(c: &Capture, id: u32) -> &str {
    if id == 0 { "" } else { text(c, id) }
}

/// 120 ticks at 10 a second, the i-th taking `10 + i` ms, with spans.
fn span_capture(scratch: &Scratch) -> Capture {
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(12.0)).unwrap();
    for i in 0..120u32 {
        let wall = 10.0 + i as f32;
        let detail = TickDetail {
            cpu_ms: Some(wall - 1.0),
            spans: tick_spans(wall),
        };
        let t = f64::from(i) / 10.0;
        perf.tick(t, wall, Some(detail), WorldCounts::default, Vec::new);
    }
    let end = CaptureEnd {
        span_ns: 200.0,
        mob_ns: 50.0,
        mob_costs: MobCosts {
            costs: vec![MobCost {
                dimension: "minecraft:overworld".into(),
                kind: "minecraft:cow".into(),
                nanos: 3_000_000,
                mob_ticks: 40_000,
            }],
            sampled_ticks: 30,
        },
        ..CaptureEnd::default()
    };
    let saved = perf.stop_capture(12.0, R::Normal, end).unwrap().unwrap();
    read(&saved.path)
}

fn row_sum(c: &Capture, span: &str, parent: &str) -> (f64, f64, u32) {
    let mut sum = (0.0, 0.0, 0);
    for b in &c.span_blocks {
        for i in 0..b.span.len() {
            if name(c, b.span[i]) == span && name(c, b.parent[i]) == parent {
                sum.0 += f64::from(b.wall_ms[i]);
                sum.1 += f64::from(b.cpu_ms[i]);
                sum.2 += b.seen[i];
            }
        }
    }
    sum
}

#[test]
fn span_blocks_are_per_five_seconds_and_name_the_parent() {
    let scratch = Scratch::new("blocks");
    let c = span_capture(&scratch);
    let blocks: Vec<_> = c
        .span_blocks
        .iter()
        .map(|b| (b.block, b.t0, b.block_s, b.units))
        .collect();
    assert_eq!(
        blocks,
        [
            (0, 0.0, BLOCK_S, 50),
            (1, 5.0, BLOCK_S, 50),
            (2, 10.0, BLOCK_S, 20)
        ]
    );
    assert!(c.span_blocks.iter().all(|b| b.side() == S::Server));
    assert_eq!(name(&c, c.span_blocks[0].thread), "Server thread");
    // Block 0 holds ticks 0 to 49: the tick spans sum to (10 + i - 1) ms, tick_mobs to 3 ms each, tick_lan to 1 ms each.
    let b = &c.span_blocks[0];
    let at = |span: &str, parent: &str| {
        (0..b.span.len())
            .find(|&i| name(&c, b.span[i]) == span && name(&c, b.parent[i]) == parent)
            .unwrap_or_else(|| panic!("no row {span} under {parent}"))
    };
    let mobs = at("tick_mobs", "tick");
    assert_eq!(
        (b.wall_ms[mobs], b.cpu_ms[mobs], b.seen[mobs]),
        (150.0, 75.0, 50)
    );
    let lan = at("tick_lan", "tick");
    assert_eq!((b.wall_ms[lan], b.seen[lan]), (50.0, 50));
    let tick = at("tick", "");
    let walls: f32 = (0..50).map(|i| 9.0 + i as f32).sum();
    assert_eq!((b.wall_ms[tick], b.seen[tick]), (walls, 50));
    assert_eq!(b.span.len(), 3);
    // Rows are costliest first.
    assert_eq!(b.span[0], b.span[tick]);
}

#[test]
fn block_sums_equal_the_span_totals() {
    let scratch = Scratch::new("totals");
    let c = span_capture(&scratch);
    assert_eq!(c.spans.len(), 1);
    let s: &SpanSummary = &c.spans[0];
    assert_eq!((s.side(), s.unit(), s.count), (S::Server, Unit::Tick, 120));
    assert_eq!(name(&c, s.thread), "Server thread");
    assert_eq!(s.spans.len(), 3);
    for total in &s.spans {
        let (span, parent) = (name(&c, total.name), name(&c, total.parent));
        let (wall, cpu, seen) = row_sum(&c, span, parent);
        assert!(
            (wall - total.wall_ms).abs() < 1e-2,
            "{span}: {wall} {}",
            total.wall_ms
        );
        assert!(
            (cpu - total.cpu_ms).abs() < 1e-2,
            "{span}: {cpu} {}",
            total.cpu_ms
        );
        assert_eq!(u64::from(seen), total.seen, "{span}");
    }
    let find = |span: &str| {
        s.spans
            .iter()
            .find(|t| name(&c, t.name) == span)
            .unwrap_or_else(|| panic!("no total for {span}"))
    };
    // 120 ticks: tick_mobs 3 ms and 1.5 cpu ms each, tick_lan 1 ms; tick is 10 + i - 1 ms, so 9 * 120 + 7140.
    let (mobs, lan, tick) = (find("tick_mobs"), find("tick_lan"), find("tick"));
    assert_eq!(
        (mobs.wall_ms, mobs.cpu_ms, mobs.seen, mobs.depth),
        (360.0, 180.0, 120, 1)
    );
    assert_eq!((lan.wall_ms, lan.seen), (120.0, 120));
    assert_eq!(
        (tick.wall_ms, tick.parent, tick.depth),
        (9.0 * 120.0 + 7140.0, 0, 0)
    );
    assert_eq!(name(&c, mobs.parent), "tick");
    // The units' own time: 10 + i wall, 9 + i cpu.
    assert_eq!(s.wall_ms_total, 10.0 * 120.0 + 7140.0);
    assert_eq!(s.cpu_ms_total, 9.0 * 120.0 + 7140.0);
}

#[test]
fn the_histogram_and_the_sixteen_worst_ticks_are_kept_with_their_trees() {
    let scratch = Scratch::new("worst");
    let c = span_capture(&scratch);
    let s = &c.spans[0];
    let timing = s.timing.as_ref().unwrap();
    assert_eq!(timing.count, 120);
    assert_eq!(timing.slowest_1pct_ms, 128.5);
    assert_eq!(timing.slowest_01pct_ms, 129.0);
    let h = s.histogram.as_ref().unwrap();
    assert_eq!(h.edges_ms, HISTOGRAM_EDGES_MS);
    assert_eq!(h.counts.len(), h.edges_ms.len() + 1);
    // Walls are 10..=129 ms; the bucket i holds [edges[i-1], edges[i]).
    let expect = |lo: f32, hi: f32| {
        (10..130)
            .filter(|&w| lo <= w as f32 && (w as f32) < hi)
            .count() as u64
    };
    let mut lower = 0.0;
    for (i, &count) in h.counts.iter().enumerate() {
        let upper = h.edges_ms.get(i).copied().unwrap_or(f32::INFINITY);
        assert_eq!(count, expect(lower, upper), "bucket {i}");
        lower = upper;
    }
    assert_eq!(h.counts.iter().sum::<u64>(), 120);
    assert_eq!(s.worst.len(), 16);
    let walls: Vec<f32> = s.worst.iter().map(|w| w.wall_ms).collect();
    let expected: Vec<f32> = (114..130).rev().map(|w| w as f32).collect();
    assert_eq!(walls, expected);
    let worst = &s.worst[0];
    assert_eq!((worst.counter, worst.t, worst.cpu_ms), (120, 11.9, 128.0));
    let tree: Vec<_> = worst
        .spans
        .iter()
        .map(|n| (name(&c, n.name), n.depth, n.wall_ms, n.cpu_ms))
        .collect();
    assert_eq!(
        tree,
        [
            ("tick", 0, 128.0, 127.0),
            ("tick_mobs", 1, 3.0, 1.5),
            ("tick_lan", 1, 1.0, 1.0),
        ]
    );
}

#[test]
fn recording_costs_are_priced_against_the_tick() {
    let scratch = Scratch::new("overhead");
    let c = span_capture(&scratch);
    let o = c.stop[0].overhead.as_ref().unwrap();
    // 4 spans a tick at 200 ns is 0.0008 ms against a mean tick of 10 + 59.5 ms.
    let mean = 69.5;
    assert!(
        (f64::from(o.spans_ms_per_tick) - 0.0008).abs() < 1e-7,
        "{}",
        o.spans_ms_per_tick
    );
    assert!((f64::from(o.spans_share) - 0.0008 / mean).abs() < 1e-7);
    // 40,000 sampled mobs over 120 ticks at 50 ns.
    let mob_ms = 40_000.0 / 120.0 * 50.0 / 1e6;
    assert!((f64::from(o.mob_timing_ms_per_tick) - mob_ms).abs() < 1e-6);
    assert!((f64::from(o.mob_timing_share) - mob_ms / mean).abs() < 1e-6);
}

#[test]
fn spans_inside_other_spans_keep_their_parents_and_a_root_under_two_parents_is_two_rows() {
    let scratch = Scratch::new("parents");
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(1.0)).unwrap();
    // `shared` runs under `a` and under `b`; `deep` runs under `shared` under `a`.
    let raw = vec![
        nested("deep", 1.0, 1.0, 2),
        nested("shared", 2.0, 2.0, 1),
        nested("a", 3.0, 3.0, 0),
        nested("shared", 4.0, 4.0, 1),
        nested("b", 5.0, 5.0, 0),
    ];
    let detail = TickDetail {
        cpu_ms: None,
        spans: raw,
    };
    perf.tick(0.5, 9.0, Some(detail), WorldCounts::default, Vec::new);
    let saved = stop(&mut perf, 1.0, R::Normal).unwrap().unwrap();
    let c = read(&saved.path);
    assert_eq!(row_sum(&c, "shared", "a"), (2.0, 2.0, 1));
    assert_eq!(row_sum(&c, "shared", "b"), (4.0, 4.0, 1));
    assert_eq!(row_sum(&c, "deep", "shared"), (1.0, 1.0, 1));
    assert_eq!(c.span_blocks[0].span.len(), 5);
    let tree: Vec<_> = c.spans[0].worst[0]
        .spans
        .iter()
        .map(|n| (name(&c, n.name), n.depth, n.wall_ms))
        .collect();
    assert_eq!(
        tree,
        [
            ("b", 0, 5.0),
            ("shared", 1, 4.0),
            ("a", 0, 3.0),
            ("shared", 1, 2.0),
            ("deep", 2, 1.0),
        ]
    );
    // Without a thread CPU clock a tick's CPU time is its wall time.
    assert_eq!(c.spans[0].cpu_ms_total, 9.0);
}

/// Connection 1 is on from the start; connection 2 joins at 3 s and leaves after 8 s.
fn wire_at(t: f64) -> traffic::Wire {
    let s = t.floor() as u64;
    let mut wire = vec![(1, (1000 + 100 * s, 2000 + 1000 * s))];
    if (3..=8).contains(&s) {
        wire.push((2, (10 * (s - 2), 30 * (s - 2))));
    }
    wire
}

#[test]
fn network_bytes_are_per_player_per_block_by_join_order() {
    let scratch = Scratch::new("net");
    let mut perf = ServerPerf::new(0.0, 250.0);
    let mut start = options(12.0);
    start.wire = wire_at(0.0);
    perf.start_capture(0.0, &scratch.0, start).unwrap();
    for i in 0..24u32 {
        let t = f64::from(i) / 2.0;
        perf.tick(t, 10.0, None, WorldCounts::default, || wire_at(t));
    }
    let end = CaptureEnd {
        wire: wire_at(12.0),
        ..CaptureEnd::default()
    };
    let saved = perf.stop_capture(12.0, R::Normal, end).unwrap().unwrap();
    let c = read(&saved.path);
    assert_eq!(c.net.len(), 1);
    let net = &c.net[0];
    assert_eq!((net.side(), net.block_s), (S::Server, BLOCK_S));
    assert_eq!(net.players.len(), 2);
    let (first, second) = (&net.players[0], &net.players[1]);
    assert_eq!((first.index, first.joined_t, first.left_t), (1, 0.0, None));
    // Samples at 1 s steps: 1 to 4 in block 0, 5 to 9 in block 1, 10 to 12 in block 2.
    assert_eq!(first.bytes_in, [400, 500, 300]);
    assert_eq!(first.bytes_out, [4000, 5000, 3000]);
    assert_eq!(
        (second.index, second.joined_t, second.left_t),
        (2, 3.0, Some(9.0))
    );
    assert_eq!(second.bytes_in, [20, 40, 0]);
    assert_eq!(second.bytes_out, [60, 120, 0]);
    assert!(first.packets_in.is_empty() && first.packets_out.is_empty());
}

fn level(
    dimension: &str,
    chunks: u32,
    entities: &[(&str, u32)],
    blocks: &[(&str, u32)],
) -> LevelCounts {
    let owned = |v: &[(&str, u32)]| v.iter().map(|&(k, n)| (k.to_owned(), n)).collect();
    LevelCounts {
        dimension: dimension.to_owned(),
        chunks,
        entities: owned(entities),
        block_entities: owned(blocks),
    }
}

#[test]
fn the_world_is_counted_at_the_start_and_the_stop_and_mobs_and_workers_are_costed() {
    let scratch = Scratch::new("world");
    let mut perf = ServerPerf::new(0.0, 250.0);
    let mut start = options(60.0);
    start.levels = vec![level(
        "minecraft:overworld",
        100,
        &[("minecraft:cow", 5)],
        &[],
    )];
    perf.start_capture(0.0, &scratch.0, start).unwrap();
    perf.tick(1.0, 10.0, None, WorldCounts::default, Vec::new);
    let end = CaptureEnd {
        levels: vec![
            level(
                "minecraft:overworld",
                441,
                &[("minecraft:cow", 5), ("minecraft:villager", 200)],
                &[("minecraft:chest", 7)],
            ),
            level("minecraft:the_nether", 9, &[], &[]),
        ],
        mob_costs: MobCosts {
            costs: vec![
                MobCost {
                    dimension: "minecraft:overworld".into(),
                    kind: "minecraft:villager".into(),
                    nanos: 10 * 200 * 25_000,
                    mob_ticks: 10 * 200,
                },
                MobCost {
                    dimension: "minecraft:the_end".into(),
                    kind: "minecraft:enderman".into(),
                    nanos: 10 * 40 * 2_000,
                    mob_ticks: 10 * 40,
                },
            ],
            sampled_ticks: 10,
        },
        jobs: vec![
            WorkerJobs {
                kind: "load".into(),
                nanos: 0,
                count: 0,
            },
            WorkerJobs {
                kind: "generate".into(),
                nanos: 30_000_000_000,
                count: 6_000,
            },
            WorkerJobs {
                kind: "light".into(),
                nanos: 3_000_000_000,
                count: 6_000,
            },
        ],
        ..CaptureEnd::default()
    };
    let saved = perf.stop_capture(60.0, R::Normal, end).unwrap().unwrap();
    let c = read(&saved.path);
    assert_eq!(c.world.len(), 2);
    let (before, after) = (&c.world[0], &c.world[1]);
    assert_eq!((name(&c, before.label), before.t), ("start", 0.0));
    assert_eq!((name(&c, after.label), after.t), ("stop", 60.0));
    assert!(before.entity_costs.is_empty());
    assert_eq!(before.dims.len(), 1);
    assert_eq!(
        (before.dims[0].chunks, before.dims[0].entities[0].count),
        (100, 5)
    );
    let counts = |d: &horizon_perf_format::v1::DimensionCounts| {
        (
            name(&c, d.dimension).to_owned(),
            d.chunks,
            d.entities
                .iter()
                .map(|e| (name(&c, e.r#type).to_owned(), e.count))
                .collect::<Vec<_>>(),
            d.block_entities
                .iter()
                .map(|e| (name(&c, e.r#type).to_owned(), e.count))
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        counts(&after.dims[0]),
        (
            "minecraft:overworld".to_owned(),
            441,
            vec![
                ("minecraft:villager".to_owned(), 200),
                ("minecraft:cow".to_owned(), 5)
            ],
            vec![("minecraft:chest".to_owned(), 7)],
        )
    );
    assert_eq!(after.dims[1].chunks, 9);
    // 10 sampled ticks of 200 villagers at 25 us: 5 ms a tick; 40 endermen at 2 us: 0.08 ms.
    let costs: Vec<_> = after
        .entity_costs
        .iter()
        .map(|e| {
            (
                name(&c, e.dimension),
                name(&c, e.entity),
                e.ms_per_tick,
                e.mobs_ticked_per_tick,
                e.us_per_mob,
            )
        })
        .collect();
    assert_eq!(
        costs,
        [
            (
                "minecraft:overworld",
                "minecraft:villager",
                5.0,
                200.0,
                25.0
            ),
            ("minecraft:the_end", "minecraft:enderman", 0.08, 40.0, 2.0),
        ]
    );
    // 30 CPU seconds of generating over 60 s is half a core, 5 ms a job; load ran no job.
    let workers = &c.workers[0];
    assert_eq!(workers.kinds.len(), 2);
    let kinds: Vec<_> = workers
        .kinds
        .iter()
        .map(|k| (name(&c, k.kind), k.jobs, k.cpu_cores, k.ms_per_job))
        .collect();
    assert_eq!(
        kinds,
        [("generate", 6_000, 0.5, 5.0), ("light", 6_000, 0.05, 0.5)]
    );
}

#[test]
fn every_string_is_defined_before_the_field_that_uses_it() {
    use horizon_perf::{Entry, Follower};
    use horizon_perf_format::Field;
    let scratch = Scratch::new("strings");
    let mut perf = ServerPerf::new(0.0, 250.0);
    let mut start = options(12.0);
    start.levels = vec![level(
        "minecraft:overworld",
        1,
        &[("minecraft:cow", 1)],
        &[],
    )];
    perf.start_capture(0.0, &scratch.0, start).unwrap();
    for i in 0..120u32 {
        let detail = TickDetail {
            cpu_ms: None,
            spans: tick_spans(20.0),
        };
        perf.tick(
            f64::from(i) / 10.0,
            20.0,
            Some(detail),
            WorldCounts::default,
            Vec::new,
        );
    }
    let end = CaptureEnd {
        levels: vec![level(
            "minecraft:overworld",
            2,
            &[("minecraft:pig", 1)],
            &[],
        )],
        jobs: vec![WorkerJobs {
            kind: "light".into(),
            nanos: 1,
            count: 1,
        }],
        ..CaptureEnd::default()
    };
    let saved = perf.stop_capture(12.0, R::Normal, end).unwrap().unwrap();
    let entries = Follower::new()
        .push(&fs::read(&saved.path).unwrap())
        .unwrap();
    let mut defined = std::collections::HashSet::new();
    let mut checked = 0;
    for entry in entries {
        let Entry::Field(field) = entry else { continue };
        let used: Vec<u32> = match &field {
            Field::String(m) => {
                defined.insert(m.id);
                continue;
            }
            Field::SpanBlock(b) => b
                .span
                .iter()
                .chain(&b.parent)
                .chain([&b.thread])
                .copied()
                .collect(),
            Field::Spans(s) => [s.thread]
                .into_iter()
                .chain(s.spans.iter().flat_map(|t| [t.name, t.parent]))
                .chain(s.worst.iter().flat_map(|w| w.spans.iter().map(|n| n.name)))
                .collect(),
            Field::Workers(w) => w.kinds.iter().map(|k| k.kind).collect(),
            Field::World(w) => [w.label]
                .into_iter()
                .chain(w.dims.iter().flat_map(|d| {
                    [d.dimension]
                        .into_iter()
                        .chain(d.entities.iter().map(|e| e.r#type))
                        .chain(d.block_entities.iter().map(|e| e.r#type))
                }))
                .collect(),
            _ => continue,
        };
        for id in used.into_iter().filter(|&id| id != 0) {
            assert!(
                defined.contains(&id),
                "string {id} used before it was defined, in {field:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 20, "{checked}");
}

#[test]
fn horizon_perf_queries_read_back_the_spans_the_network_and_the_world() {
    use horizon_perf::cli::{Env, run};
    use horizon_perf::query::Row;
    use horizon_perf::wire::Envelope;
    let scratch = Scratch::new("queries");
    let mut perf = ServerPerf::new(0.0, 250.0);
    let mut start = options(12.0);
    start.wire = wire_at(0.0);
    start.levels = vec![level(
        "minecraft:overworld",
        100,
        &[("minecraft:cow", 5)],
        &[("minecraft:chest", 2)],
    )];
    perf.start_capture(0.0, &scratch.0, start).unwrap();
    for i in 0..24u32 {
        let t = f64::from(i) / 2.0;
        let detail = TickDetail {
            cpu_ms: Some(9.0),
            spans: tick_spans(10.0),
        };
        perf.tick(t, 10.0, Some(detail), WorldCounts::default, || wire_at(t));
    }
    let end = CaptureEnd {
        wire: wire_at(12.0),
        levels: vec![level(
            "minecraft:overworld",
            441,
            &[("minecraft:cow", 5), ("minecraft:pig", 2)],
            &[],
        )],
        ..CaptureEnd::default()
    };
    let saved = perf.stop_capture(12.0, R::Normal, end).unwrap().unwrap();
    let env = Env {
        horizon_home: None,
        poll: std::time::Duration::from_millis(2),
    };
    let rows = |op: &str| -> Vec<Row> {
        let mut out = Vec::new();
        let path = saved.path.to_str().unwrap().to_owned();
        run(&["query".into(), op.into(), path], &env, &mut out).unwrap();
        let reply: Envelope<Row> = serde_json::from_slice(&out).unwrap();
        assert!(reply.ok, "{op}: {:?}", reply.error);
        reply.rows.unwrap()
    };

    // Every tick took 10 ms and ran `tick` for 9 of them, `tick_mobs` for 3 and `tick_lan` for 1.
    let spans: Vec<_> = rows("spans")
        .into_iter()
        .map(|r| match r {
            Row::Span(s) => (s.name, s.values[0], s.cpu[0], s.waited[0], s.seen[0]),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        spans,
        [
            ("tick".to_owned(), Some(9.0), Some(8.0), Some(1.0), Some(24)),
            (
                "tick_mobs".to_owned(),
                Some(3.0),
                Some(1.5),
                Some(1.5),
                Some(24)
            ),
            (
                "tick_lan".to_owned(),
                Some(1.0),
                Some(1.0),
                Some(0.0),
                Some(24)
            ),
        ]
    );

    // Player 1 moved 1,000 bytes out and 100 in each second of the 12; player 2 was on from 3 to 8.
    let net = rows("net");
    let Row::NetTotal(all) = &net[0] else {
        panic!("{:?}", net[0])
    };
    assert_eq!(
        (all.players, all.out_bytes, all.in_bytes),
        (2, 12_180, 1_260)
    );
    let players: Vec<_> = net[1..]
        .iter()
        .map(|r| match r {
            Row::NetPlayer(p) => (p.index, p.joined_t, p.left_t, p.out_bytes, p.in_bytes),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        players,
        [(1, 0.0, None, 12_000, 1_200), (2, 3.0, Some(9.0), 180, 60)]
    );

    // The query shows the world as it stood at the stop.
    let world = rows("world");
    let Row::Dimension(overworld) = &world[0] else {
        panic!("{:?}", world[0])
    };
    assert_eq!(
        (
            overworld.name.as_str(),
            overworld.chunks,
            overworld.entities,
            overworld.block_entities
        ),
        ("minecraft:overworld", 441, 7, 0)
    );
}

fn start_of(
    scratch: &Scratch,
    rules: &crate::gamerules::GameRules,
    far: u32,
    lod: u32,
    anticheat: bool,
) -> Start {
    let mut perf = ServerPerf::new(0.0, 250.0);
    let mut o = options(1.0);
    o.start.game_rules = Some(game_rules(rules));
    o.start.horizon_options = Some(horizon_options(far, lod, anticheat));
    perf.start_capture(0.0, &scratch.0, o).unwrap();
    let saved = stop(&mut perf, 1.0, R::Normal).unwrap().unwrap();
    read(&saved.path).start.remove(0)
}

#[test]
fn the_game_rules_read_back_as_set_and_a_default_world_reads_its_defaults() {
    use crate::gamerules::{GameRules as Rules, Value};
    use horizon_perf_format::v1::GameRules;
    let scratch = Scratch::new("rules");
    let rules = |sets: &[(&str, Value)]| {
        let mut r = Rules::default();
        for &(name, value) in sets {
            assert!(r.set(name, value), "{name}");
        }
        r
    };
    let expect = |tick, cram, fire, time, grief, mobs, monsters, spawners| GameRules {
        random_tick_speed: Some(tick),
        max_entity_cramming: Some(cram),
        fire_spread_radius_around_player: Some(fire),
        advance_time: Some(time),
        mob_griefing: Some(grief),
        spawn_mobs: Some(mobs),
        spawn_monsters: Some(monsters),
        spawner_blocks_work: Some(spawners),
    };
    let rules_of = |r: &Rules| start_of(&scratch, r, 0, 0, false).game_rules.unwrap();

    // Vanilla's defaults are non-zero and true: a misspelt name would read 0 and false.
    assert_eq!(
        rules_of(&Rules::default()),
        expect(3, 24, 128, true, true, true, true, true)
    );
    let all = rules(&[
        ("random_tick_speed", Value::Int(9)),
        ("max_entity_cramming", Value::Int(5)),
        ("fire_spread_radius_around_player", Value::Int(-1)),
        ("advance_time", Value::Bool(false)),
        ("mob_griefing", Value::Bool(false)),
        ("spawn_mobs", Value::Bool(false)),
        ("spawn_monsters", Value::Bool(false)),
        ("spawner_blocks_work", Value::Bool(false)),
    ]);
    assert_eq!(
        rules_of(&all),
        expect(9, 5, -1, false, false, false, false, false)
    );
    // One switch at a time, so two switches cannot be swapped unseen.
    let one = |name| rules_of(&rules(&[(name, Value::Bool(false))]));
    assert_eq!(
        one("advance_time"),
        expect(3, 24, 128, false, true, true, true, true)
    );
    assert_eq!(
        one("mob_griefing"),
        expect(3, 24, 128, true, false, true, true, true)
    );
    assert_eq!(
        one("spawn_mobs"),
        expect(3, 24, 128, true, true, false, true, true)
    );
    assert_eq!(
        one("spawn_monsters"),
        expect(3, 24, 128, true, true, true, false, true)
    );
    assert_eq!(
        one("spawner_blocks_work"),
        expect(3, 24, 128, true, true, true, true, false)
    );
}

#[test]
fn the_horizon_options_read_back_as_given() {
    use horizon_perf_format::v1::HorizonOptions;
    let scratch = Scratch::new("horizon-options");
    let rules = crate::gamerules::GameRules::default();
    let on = start_of(&scratch, &rules, 24, 12, true).horizon_options;
    assert_eq!(
        on,
        Some(HorizonOptions {
            max_view_distance: Some(24),
            lod_distance: Some(12),
            anticheat: Some(true),
        })
    );
    let off = start_of(&scratch, &rules, 0, 0, false).horizon_options;
    assert_eq!(
        off,
        Some(HorizonOptions {
            max_view_distance: Some(0),
            lod_distance: Some(0),
            anticheat: Some(false),
        })
    );
}

#[test]
fn memory_snapshots_keep_their_labels_paths_values_and_capture_time() {
    let scratch = Scratch::new("memory-snapshot");
    let mut perf = ServerPerf::new(0.0, 250.0);
    let rows = vec![
        ("process.resident_mb".to_owned(), 12.5),
        ("gpu.resident_mb".to_owned(), 2.0),
    ];
    assert!(!perf.memory_snapshot(0.0, "idle", &rows));
    perf.start_capture(10.0, &scratch.0, options(5.0)).unwrap();
    assert!(perf.memory_snapshot(10.25, "start", &rows));
    let saved = stop(&mut perf, 11.0, R::Normal).unwrap().unwrap();
    let capture = read(&saved.path);
    assert_eq!(capture.memory.len(), 1);
    let snapshot = &capture.memory[0];
    assert_eq!(snapshot.t, 0.25);
    assert_eq!(text(&capture, snapshot.label), "start");
    assert_eq!(
        snapshot
            .path
            .iter()
            .map(|&id| text(&capture, id))
            .collect::<Vec<_>>(),
        ["process.resident_mb", "gpu.resident_mb"]
    );
    assert_eq!(snapshot.value_mb, [12.5, 2.0]);
    assert!(!perf.memory_snapshot(11.5, "after", &rows));
}
