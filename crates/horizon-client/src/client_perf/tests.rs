use super::*;
use crate::spans::Span;
use horizon_perf_format::files;
use horizon_perf_format::v1::{Capture, Kind, Side, StopReason as R, Unit};
use std::fs;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("hz-client-perf-{tag}-{}", std::process::id()));
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

fn read(path: &Path) -> Capture {
    horizon_perf::read_capture(&fs::read(path).unwrap()).unwrap()
}

fn text(capture: &Capture, id: u32) -> &str {
    &capture.strings.iter().find(|s| s.id == id).unwrap().text
}

fn no_samples() -> StartOptions {
    StartOptions {
        samples: false,
        ..StartOptions::default()
    }
}

fn begin(perf: &mut ClientPerf, t: f64, dir: &Path, options: &StartOptions) -> PathBuf {
    perf.start(t, dir, Trigger::Ctl, options, Start::default(), Vec::new())
        .unwrap()
}

/// `per_s` frames a second from second `from` to `to`: 16 ms, CPU 5 ms, GPU 3 ms, and the first of each second takes `hitch` ms.
fn run(perf: &mut ClientPerf, from: u32, to: u32, per_s: u32, hitch: f32) {
    for i in 0..(to - from) * per_s {
        let t = f64::from(from) + f64::from(i) / f64::from(per_s);
        let wall = if i % per_s == 0 { hitch } else { 16.0 };
        let detail = (wall > SLOW_FRAME_MS).then(|| SlowDetail {
            cpu_ms: Some(wall - 30.0),
            spans: vec![Span {
                name: "terrain",
                ms: wall - 20.0,
                cpu_ms: wall - 40.0,
                depth: 0,
            }],
        });
        perf.frame(t, wall, 5.0, Some(3.0), detail, WorldCounts::default);
    }
}

#[test]
fn a_client_capture_reads_back_with_its_frames_marks_and_health() {
    let scratch = Scratch::new("readback");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    run(&mut perf, 0, 2, 60, 16.0);
    let path = begin(&mut perf, 2.0, &scratch.0, &no_samples());
    assert!(perf.capturing());
    assert!(path.starts_with(&scratch.0));
    // 4 s at 60 fps; the first frame of seconds 3 and 5 takes 80 ms.
    run(&mut perf, 2, 3, 60, 16.0);
    assert!(perf.mark(3.25, "loaded"));
    run(&mut perf, 3, 4, 60, 80.0);
    run(&mut perf, 4, 5, 60, 16.0);
    run(&mut perf, 5, 6, 60, 80.0);
    assert!(perf.stop(6.0, R::Normal));
    let saved = perf.take_saved(true).pop().unwrap().unwrap();
    assert_eq!(saved.path, path);
    assert!(!perf.capturing());

    let c = read(&saved.path);
    let start = &c.start[0];
    assert_eq!(start.side(), Side::Client);
    assert_eq!(start.kind(), Kind::Client);
    assert_eq!(start.trigger(), Trigger::Ctl);
    assert_eq!(start.options.as_ref().unwrap().window_s, None);
    assert!(!start.options.as_ref().unwrap().samples);

    let ts: Vec<_> = c.health.iter().map(|h| h.t).collect();
    assert_eq!(ts, [0.0, 1.0, 2.0, 3.0]);
    for h in &c.health {
        assert_eq!(h.side(), Side::Client);
        assert_eq!(h.fps, Some(60.0));
        assert_eq!(h.frame_ms_p50, Some(16.0));
        assert_eq!(h.frame_cpu_ms_p50, Some(5.0));
        assert_eq!(h.frame_gpu_ms_p50, Some(3.0));
        assert_eq!(h.tps, None);
    }
    let maxes: Vec<_> = c.health.iter().map(|h| h.frame_ms_max).collect();
    assert_eq!(maxes, [Some(16.0), Some(80.0), Some(16.0), Some(80.0)]);
    // 59 frames of 16 ms and one of 80 ms: the 95th percentile ranks 57th, the 99th ranks 60th.
    assert_eq!(c.health[1].frame_ms_p95, Some(16.0));
    assert_eq!(c.health[1].frame_ms_p99, Some(80.0));
    let frames: f32 = c.health.iter().filter_map(|h| h.fps).sum();
    assert_eq!(frames, 240.0);

    assert_eq!(c.marks.len(), 1);
    assert_eq!(c.marks[0].name, "loaded");
    assert_eq!(c.marks[0].t, 1.25);
    assert_eq!(c.marks[0].side(), Side::Client);

    assert_eq!(c.history[0].t0, -2.0);
    assert_eq!(c.history[0].samples.len(), 2);

    let slow: Vec<_> = c.slow.iter().map(|e| (e.unit(), e.wall_ms)).collect();
    assert_eq!(slow, [(Unit::Frame, 80.0), (Unit::Frame, 80.0)]);
    assert_eq!(c.slow[0].counter, 121 + 60);
    assert_eq!(text(&c, c.slow[0].top[0].name), "terrain");

    assert_eq!(c.stop[0].side(), Side::Client);
    assert_eq!(c.stop[0].t, 4.0);
    assert_eq!(c.stop[0].reason(), R::Normal);
}

#[test]
fn health_samples_carry_the_process_gauges_and_this_seconds_counts() {
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    let counts = || WorldCounts {
        players: 3,
        chunks: 40,
        entities: 7,
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let mut t = 0.0;
    let sample = loop {
        perf.frame(t, 16.0, 5.0, None, None, counts);
        t += 1.0;
        let (samples, _) = perf.health().samples_since(0, 0.0);
        if let Some(s) = samples.into_iter().find(|s| s.rss_mb > 0.0) {
            break s;
        }
        assert!(Instant::now() < deadline, "no sample carried an rss_mb");
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_eq!((sample.players, sample.chunks, sample.entities), (3, 40, 7));
}

#[test]
fn the_first_second_of_a_capture_counts_only_frames_after_it_began() {
    let scratch = Scratch::new("midsecond");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    run(&mut perf, 0, 3, 30, 16.0);
    begin(&mut perf, 3.5, &scratch.0, &no_samples());
    for i in 0..90 {
        perf.frame(
            3.5 + f64::from(i) / 30.0,
            16.0,
            5.0,
            None,
            None,
            WorldCounts::default,
        );
    }
    perf.stop(6.5, R::Normal);
    let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
    let ts: Vec<_> = c.health.iter().map(|h| h.t).collect();
    assert_eq!(ts, [0.5, 1.5]);
    assert!(c.health.iter().all(|h| h.frame_gpu_ms_p50.is_none()));
}

#[test]
fn a_window_ends_the_capture_and_the_longest_one_is_a_cap() {
    let scratch = Scratch::new("window");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    let options = StartOptions {
        seconds: Some(2),
        ..no_samples()
    };
    begin(&mut perf, 0.0, &scratch.0, &options);
    assert_eq!(perf.remaining_s(0.5), Some(1.5));
    run(&mut perf, 0, 4, 10, 16.0);
    assert!(!perf.capturing(), "the 2 s window ended the capture");
    let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
    assert_eq!(c.stop[0].reason(), R::Normal);
    assert_eq!(c.stop[0].t, 2.0);
    assert_eq!(c.start[0].options.as_ref().unwrap().window_s, Some(2.0));

    begin(&mut perf, 10.0, &scratch.0, &no_samples());
    perf.frame(609.9, 16.0, 5.0, None, None, WorldCounts::default);
    assert!(perf.capturing());
    perf.frame(610.0, 16.0, 5.0, None, None, WorldCounts::default);
    assert!(!perf.capturing());
    let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
    assert_eq!(c.stop[0].reason(), R::WindowCap);
    assert_eq!(c.stop[0].t, 600.0);
}

#[test]
fn one_capture_runs_at_a_time_and_nothing_marks_or_stops_without_one() {
    let scratch = Scratch::new("single");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    assert!(!perf.mark(1.0, "early"));
    assert!(!perf.stop(1.0, R::Normal));
    assert!(perf.status(1.0).is_none());
    begin(&mut perf, 0.0, &scratch.0, &no_samples());
    let again = perf.start(
        0.0,
        &scratch.0,
        Trigger::Key,
        &no_samples(),
        Start::default(),
        Vec::new(),
    );
    assert!(again.is_err());
    assert!(perf.stop(0.0, R::Shutdown));
    perf.take_saved(true).pop().unwrap().unwrap();
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
}

#[test]
fn status_grows_with_the_capture() {
    let scratch = Scratch::new("status");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    let path = begin(&mut perf, 1.0, &scratch.0, &no_samples());
    let first = perf.status(1.0).unwrap();
    assert_eq!(first.path, path);
    run(&mut perf, 1, 6, 30, 16.0);
    perf.mark(6.0, "later");
    let second = perf.status(6.5).unwrap();
    assert_eq!(second.elapsed_s, 5.5);
    let waited = Instant::now();
    while perf.status(6.5).unwrap().bytes == 0 {
        assert!(waited.elapsed().as_secs() < 5, "the writer wrote nothing");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    perf.stop(7.0, R::Normal);
    perf.take_saved(true).pop().unwrap().unwrap();
    assert!(perf.status(7.0).is_none());
}

#[test]
fn finishing_applies_the_retention_of_thirty_files() {
    let scratch = Scratch::new("retention");
    let old: Vec<PathBuf> = (0..30)
        .map(|i| scratch.0.join(files::file_name(1_000_000 + i * 60)))
        .collect();
    for path in &old {
        fs::write(path, b"x").unwrap();
    }
    let renamed = scratch.0.join("profile-19700101-000000Z-mine.hzperf");
    fs::write(&renamed, b"x").unwrap();
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    begin(&mut perf, 0.0, &scratch.0, &no_samples());
    perf.stop(0.0, R::Normal);
    let saved = perf.take_saved(true).pop().unwrap().unwrap();
    assert_eq!(saved.deleted, [old[0].clone()]);
    assert!(renamed.exists() && saved.path.exists());
}

#[test]
fn captures_go_to_the_profile_folder_or_the_game_homes_reports() {
    let home = Path::new("/game/home");
    assert_eq!(reports_dir(None, home), Path::new("/game/home/reports"));
    assert_eq!(
        reports_dir(Some(OsStr::new("")), home),
        Path::new("/game/home/reports")
    );
    assert_eq!(
        reports_dir(Some(OsStr::new("/tmp/out")), home),
        Path::new("/tmp/out")
    );
}

#[test]
fn the_median_of_a_second_is_the_nearest_rank_and_the_buffer_empties() {
    let mut v = vec![9.0, 1.0, 5.0, 3.0];
    assert_eq!(median(&mut v), Some(3.0));
    assert!(v.is_empty());
    assert_eq!(median(&mut v), None);
    assert_eq!(median(&mut vec![7.0]), Some(7.0));
}

#[test]
fn the_messages_the_game_shows_are_translation_keys_with_one_argument_each() {
    let en: std::collections::HashMap<String, String> =
        serde_json::from_str(include_str!("../../assets/horizon/lang/en_us.json")).unwrap();
    for key in [
        "horizon.perf.saved",
        "horizon.perf.open_in_browser",
        "horizon.perf.error",
    ] {
        let text = en
            .get(key)
            .unwrap_or_else(|| panic!("{key} is not in en_us.json"));
        assert_eq!(text.matches("%s").count(), 1, "{key}: {text}");
    }
    assert!(en.contains_key("horizon.perf.started"));
}

#[cfg(target_os = "linux")]
mod sampled {
    use super::*;
    use horizon_perf_format::v1::Symbols;
    use horizon_threads::{sampler, work};

    #[inline(never)]
    fn spin_frame(cpu_ms: u64) {
        let until = sampler::thread_cpu_ns() + cpu_ms * 1_000_000;
        let mut x = 1u64;
        while sampler::thread_cpu_ns() < until {
            for _ in 0..1 << 14 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
        }
        std::hint::black_box(x);
    }

    #[test]
    fn the_render_thread_samples_count_the_frames_the_run_made() {
        let scratch = Scratch::new("samples");
        let mut perf = ClientPerf::new(SLOW_FRAME_MS);
        let on = StartOptions {
            hz: Some(500),
            ..StartOptions::default()
        };
        let start = Start {
            build: Some(server_perf::build()),
            ..Start::default()
        };
        perf.start(0.0, &scratch.0, Trigger::Env, &on, start, Vec::new())
            .unwrap();
        for frame in 0..7u32 {
            work::tick_start();
            let started = sampler::thread_cpu_ns();
            spin_frame(30);
            let ns = sampler::thread_cpu_ns() - started;
            work::tick_end(ns);
            perf.frame(
                f64::from(frame) * 0.1,
                ns as f32 / 1e6,
                ns as f32 / 1e6,
                None,
                None,
                WorldCounts::default,
            );
        }
        perf.stop(0.7, R::Normal);
        let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
        let samples = &c.samples[0];
        assert_eq!(samples.side(), Side::Client);
        let thread = &samples.threads[0];
        assert_eq!(text(&c, thread.name), "render");
        assert_eq!(thread.unit(), Unit::Frame);
        assert_eq!(thread.units, 7);
        let weighed: u64 = samples.weights.as_ref().unwrap().count.iter().sum();
        assert!(
            weighed > 20,
            "30 ms frames at 500 Hz left {weighed} samples"
        );
        let options = c.start[0].options.as_ref().unwrap();
        assert!(options.samples);
        assert_eq!(options.hz, 500);
        assert_ne!(
            c.start[0].build.as_ref().unwrap().symbols(),
            Symbols::Unspecified
        );
        assert!(c.stop[0].overhead.is_some());
    }
}
