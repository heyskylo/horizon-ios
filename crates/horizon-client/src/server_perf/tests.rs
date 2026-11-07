use super::*;
use horizon_perf_format::v1::{Capture, Kind as K, Side as S, StopReason as R};
use std::fs;

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
        window_s,
        trigger: Trigger::Console,
        start: Start {
            players: 3,
            settings: Some(settings(10, 8, 20, 256)),
            ..Start::default()
        },
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
                Some(SlowDetail {
                    cpu_ms: Some(280.0),
                    spans: vec![span("tick_lan", 4.0), span(name, wall - 20.0)],
                }),
            ),
            None => (10.0, None),
        };
        perf.tick(t, wall, detail, WorldCounts::default);
    }
}

fn read(path: &Path) -> Capture {
    horizon_perf::read_capture(&fs::read(path).unwrap()).unwrap()
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
    let saved = perf.stop_capture(8.0, R::Normal).unwrap().unwrap();
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
        perf.tick(3.5 + f64::from(i) / 20.0, 10.0, None, WorldCounts::default);
    }
    let saved = perf.stop_capture(7.5, R::WindowCap).unwrap().unwrap();
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
    let saved = perf.stop_capture(3.0, R::Normal).unwrap().unwrap();
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
    let saved = perf.stop_capture(1.0, R::Normal).unwrap().unwrap();
    assert_eq!(saved.deleted, [old[0].clone()]);
    assert!(saved.path.exists() && renamed.exists() && old[1].exists());
    let line = files::saved_line(&saved.path);
    assert!(line.contains(saved.path.to_str().unwrap()), "{line}");
}

#[test]
fn a_second_capture_cannot_start_and_stopping_with_none_running_is_none() {
    let scratch = Scratch::new("single");
    let mut perf = ServerPerf::new(0.0, 250.0);
    assert!(perf.stop_capture(1.0, R::Normal).is_none());
    perf.start_capture(0.0, &scratch.0, options(1.0)).unwrap();
    assert!(perf.start_capture(0.0, &scratch.0, options(1.0)).is_err());
    perf.stop_capture(0.0, R::Shutdown).unwrap().unwrap();
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[test]
fn a_slow_tick_without_detail_has_no_spans_and_no_invented_cpu_time() {
    let scratch = Scratch::new("nodetail");
    let mut perf = ServerPerf::new(0.0, 250.0);
    perf.start_capture(0.0, &scratch.0, options(5.0)).unwrap();
    perf.tick(0.5, 400.0, None, WorldCounts::default);
    let saved = perf.stop_capture(1.0, R::Normal).unwrap().unwrap();
    let c = read(&saved.path);
    assert_eq!(c.slow.len(), 1);
    assert_eq!(c.slow[0].wall_ms, 400.0);
    assert_eq!(c.slow[0].cpu_ms, 0.0);
    assert!(c.slow[0].top.is_empty());
}
