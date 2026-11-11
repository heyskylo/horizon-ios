use super::*;
use crate::spans::Span;
use horizon_perf_format::files;
use horizon_perf_format::v1::{Capture, GpuMode, Kind, Side, StopReason as R, Unit, WarningCode};
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
        let detail = (wall > SLOW_FRAME_MS).then(|| TickDetail {
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

fn benchmark_report() -> horizon_bench::Report {
    let app = horizon_bench::App {
        name: "horizon-client".into(),
        version: "26.3.1".into(),
        ..Default::default()
    };
    let mut report = horizon_bench::Report::with_hardware(
        horizon_bench::Kind::Game,
        app,
        horizon_bench::Hardware::default(),
    );
    report.conditions.scenario = Some("travel".into());
    report.metric("startup.title_ms", &[100.0, 120.0]);
    report
}

#[test]
fn a_benchmark_report_given_at_stop_is_in_the_capture_verbatim() {
    let scratch = Scratch::new("bench");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    run(&mut perf, 0, 2, 60, 16.0);
    let options = no_samples();
    perf.start(
        2.0,
        &scratch.0,
        Trigger::Benchmark,
        &options,
        Start::default(),
        Vec::new(),
    )
    .unwrap();
    run(&mut perf, 2, 4, 60, 16.0);
    let report = benchmark_report();
    let result = bench_result(report.to_json()).unwrap();
    assert!(perf.stop_with(4.0, R::Normal, None, vec![result]));
    let saved = perf.take_saved(true).pop().unwrap().unwrap();

    let c = read(&saved.path);
    assert_eq!(c.start[0].trigger(), Trigger::Benchmark);
    let [bench] = &c.bench[..] else {
        panic!("one report: {:?}", c.bench);
    };
    assert_eq!(bench.side(), Side::Client);
    assert_eq!(bench.format, "horizon-benchmark-report");
    assert_eq!(bench.report_json, report.to_json());
    assert_eq!(c.stop[0].t, 2.0);
}

#[test]
fn a_benchmark_reports_local_paths_and_label_stay_out_of_the_capture() {
    let scratch = Scratch::new("bench-local");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    run(&mut perf, 0, 2, 60, 16.0);
    perf.start(
        2.0,
        &scratch.0,
        Trigger::Benchmark,
        &no_samples(),
        Start::default(),
        Vec::new(),
    )
    .unwrap();
    run(&mut perf, 2, 4, 60, 16.0);
    let mut report = benchmark_report();
    report.label = Some("someone's laptop".into());
    report.local = vec!["/home/someone/logs/latest.log".into()];
    let result = bench_result(report.to_json()).unwrap();
    assert!(perf.stop_with(4.0, R::Normal, None, vec![result]));
    let saved = perf.take_saved(true).pop().unwrap().unwrap();

    let embedded = &read(&saved.path).bench[0].report_json;
    assert!(!embedded.contains("someone"), "{embedded}");
    assert!(!embedded.contains("latest.log"), "{embedded}");
    let mut expected = report.clone();
    expected.label = None;
    expected.local.clear();
    assert_eq!(*embedded, expected.to_json());
}

#[test]
fn only_a_benchmark_report_can_be_embedded() {
    assert!(bench_result("{\"format\":\"something-else\"}".into()).is_err());
    assert!(bench_result("not json".into()).is_err());
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
fn a_trace_of_two_seconds_writes_a_timeline_capture_at_its_file() {
    let _timeline = crate::server_perf::timeline::test_lock();
    let scratch = Scratch::new("trace");
    let reports = scratch.0.join("reports");
    fs::create_dir_all(&reports).unwrap();
    for day in 1..=31 {
        fs::write(reports.join(files::file_name(86_400 * day)), b"old").unwrap();
    }
    let file = scratch.0.join("somewhere").join("frame.hzperf");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    let options = StartOptions {
        seconds: Some(2),
        timeline: true,
        file: Some(file.clone()),
        ..no_samples()
    };
    let started = begin(&mut perf, 0.0, &reports, &options);
    assert_eq!(started, file);
    let render = || {
        std::thread::Builder::new()
            .name("render".into())
            .spawn(|| crate::spans::time("draw", || crate::spans::time("terrain", || ())))
            .unwrap()
            .join()
            .unwrap();
    };
    for i in 0..40 {
        render();
        perf.frame(
            f64::from(i) * 0.1,
            16.0,
            5.0,
            None,
            None,
            WorldCounts::default,
        );
    }
    assert!(!perf.capturing(), "the 2 s window ended the capture");
    let saved = perf.take_saved(true).pop().unwrap().unwrap();

    assert_eq!(saved.path, file);
    assert!(
        saved.deleted.is_empty(),
        "a capture of its own path is no one's to trim"
    );
    assert_eq!(fs::read_dir(&reports).unwrap().count(), 31);
    let c = read(&file);
    assert_eq!(c.start[0].options.as_ref().unwrap().window_s, Some(2.0));
    assert!(c.start[0].options.as_ref().unwrap().timeline);
    let chunks: Vec<_> = c
        .timeline
        .iter()
        .filter(|chunk| text(&c, chunk.thread) == "render")
        .collect();
    assert_eq!(chunks.len(), 2, "a chunk for each of the two seconds");
    let names: Vec<_> = chunks
        .iter()
        .flat_map(|chunk| chunk.name.iter().map(|&id| text(&c, id)))
        .collect();
    assert_eq!(names.iter().filter(|&&n| n == "draw").count(), 21);
    assert_eq!(names.iter().filter(|&&n| n == "terrain").count(), 21);
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.side() == Side::Client && chunk.depth.iter().all(|&d| d <= 1))
    );
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

fn adapter() -> wgpu::AdapterInfo {
    wgpu::AdapterInfo {
        name: "Test GPU".into(),
        vendor: 0,
        device: 0,
        device_type: wgpu::DeviceType::DiscreteGpu,
        device_pci_bus_id: String::new(),
        driver: "NVIDIA".into(),
        driver_info: "570.1".into(),
        backend: wgpu::Backend::Vulkan,
        subgroup_min_size: 0,
        subgroup_max_size: 0,
        transient_saves_memory: None,
        limit_bucket: None,
    }
}

/// The World pass of a 1920 x 1080 frame that shaded 6,300,000 pixels, and a HUD pass.
fn readout() -> GpuReadout {
    use crate::gfx::pack_timing::{
        Counters, DetailSupport, FragmentUnit, PartsMode, PassCounters, Readings,
    };
    GpuReadout {
        adapter: adapter(),
        own: Some(Readings {
            passes: vec![("World".into(), 1.5), ("HUD".into(), 0.25)],
            parts: Vec::new(),
            counters: vec![(
                "World".into(),
                PassCounters {
                    total: Counters {
                        fragment_shader_invocations: 6_300_000,
                        ..Counters::default()
                    },
                    pixels: 2_073_600,
                    parts: Vec::new(),
                },
            )],
            support: DetailSupport {
                parts: PartsMode::InsidePasses,
                encoder_stamps: true,
                counters: true,
                fragment_unit: Some(FragmentUnit::Pixel),
            },
        }),
        pack: None,
    }
}

fn gpu_options(gpu: GpuSwitch) -> StartOptions {
    StartOptions {
        gpu,
        ..no_samples()
    }
}

fn gpu_query(path: &Path) -> Vec<horizon_perf::query::Row> {
    let q = horizon_perf::query::Query::new(horizon_perf::wire::Op::Gpu);
    let answer = horizon_perf::ops::query(&[path.to_owned()], &q).unwrap();
    assert!(answer.error.is_none(), "{:?}", answer.error);
    answer.envelope.rows().to_vec()
}

#[test]
fn a_gpu_capture_reads_back_its_pass_times_and_fragments_per_pixel() {
    use horizon_perf::query::Row;
    let scratch = Scratch::new("gpu");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    begin(&mut perf, 0.0, &scratch.0, &gpu_options(GpuSwitch::Detail));
    run(&mut perf, 0, 2, 30, 16.0);
    assert!(perf.stop_with_gpu(2.0, R::Normal, Some(readout())));
    let saved = perf.take_saved(true).pop().unwrap().unwrap();

    let c = read(&saved.path);
    assert_eq!(c.start[0].options.as_ref().unwrap().gpu(), GpuMode::Detail);
    assert!(c.warnings.is_empty());
    let rows = gpu_query(&saved.path);
    let Row::GpuDevice(device) = &rows[0] else {
        panic!("{rows:?}");
    };
    assert_eq!(
        (device.adapter.as_str(), device.driver.as_str()),
        ("Test GPU", "NVIDIA 570.1")
    );
    assert!(matches!(&rows[1], Row::GpuSummary(_)), "{rows:?}");
    let passes: Vec<_> = rows[2..]
        .iter()
        .map(|r| match r {
            Row::GpuPass(p) => p,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        passes
            .iter()
            .map(|p| (p.name.as_str(), p.ms))
            .collect::<Vec<_>>(),
        [("World", Some(1.5)), ("HUD", Some(0.25))]
    );
    // 6,300,000 / 2,073,600 = 3.038..., shown to two decimals.
    assert_eq!(
        passes[0].counters.as_ref().unwrap().fragments_per_pixel,
        Some(3.04)
    );
}

#[test]
fn timed_frames_go_out_in_blocks_and_the_end_section_takes_their_median() {
    use crate::gfx::pack_timing::{TimedFrame, TimedFrames};
    use horizon_perf::query::Row;
    let scratch = Scratch::new("gpu-frames");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    begin(&mut perf, 0.0, &scratch.0, &gpu_options(GpuSwitch::Passes));
    // 25 frames 0.1 s apart, World 1..=25 ms: blocks close after each second and at the stop.
    for i in 1..=25u32 {
        let frames = TimedFrames {
            own: vec![TimedFrame {
                at: Instant::now(),
                total_ms: Some(i as f32 + 1.0),
                passes: vec![("World".into(), i as f32), ("HUD".into(), 0.25)],
                parts: Vec::new(),
            }],
            pack: Vec::new(),
        };
        perf.gpu_frames(f64::from(i) / 10.0, 4.0, &frames);
    }
    assert!(perf.stop_with_gpu(2.6, R::Normal, Some(readout())));
    let saved = perf.take_saved(true).pop().unwrap().unwrap();
    let c = read(&saved.path);
    assert_eq!(c.gpu_frames.len(), 3);
    let frames: Vec<_> = c.gpu_frames.iter().flat_map(|b| &b.frames).collect();
    assert_eq!(frames.len(), 25);
    assert!((frames[9].t - 1.0).abs() < 0.01, "{}", frames[9].t);
    let cpu: usize = c.gpu_frames.iter().map(|b| b.cpu_frame_ms.len()).sum();
    assert_eq!(cpu, 25);
    let rows = gpu_query(&saved.path);
    let Row::GpuSummary(summary) = &rows[1] else {
        panic!("{rows:?}");
    };
    // Totals 2..=26: nearest rank 13 of 25 is 14; the CPU's 4 ms is under 14 / 1.2.
    assert_eq!(summary.gpu_frame.p50, Some(14.0));
    assert_eq!(summary.bound, Some(horizon_perf::wire::BoundName::Gpu));
    let world = rows[2..].iter().find_map(|r| match r {
        Row::GpuPass(p) if p.name == "World" => Some(p),
        _ => None,
    });
    // The capture's median (the 13th of 25), not the readout's 1.5.
    assert_eq!(world.unwrap().ms, Some(13.0));
}

#[test]
fn a_gpu_capture_with_no_timings_says_so_and_one_without_gpu_ignores_them() {
    let scratch = Scratch::new("gpu-none");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    begin(&mut perf, 0.0, &scratch.0, &gpu_options(GpuSwitch::Passes));
    perf.stop(1.0, R::Normal);
    let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
    assert!(c.gpu.is_empty());
    assert_eq!(c.start[0].options.as_ref().unwrap().gpu(), GpuMode::Passes);
    assert_eq!(c.warnings.len(), 1);
    assert_eq!(c.warnings[0].code(), WarningCode::GpuUnavailable);
    assert_eq!((c.warnings[0].side(), c.warnings[0].t), (Side::Client, 1.0));

    begin(&mut perf, 10.0, &scratch.0, &no_samples());
    perf.stop_with_gpu(11.0, R::Normal, Some(readout()));
    let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
    assert!(c.gpu.is_empty() && c.warnings.is_empty());
    assert_eq!(c.start[0].options.as_ref().unwrap().gpu(), GpuMode::Off);
}

#[test]
fn the_window_asks_for_the_timings_once_when_it_ends_the_capture() {
    let scratch = Scratch::new("gpu-window");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    let options = StartOptions {
        seconds: Some(1),
        ..gpu_options(GpuSwitch::Passes)
    };
    begin(&mut perf, 0.0, &scratch.0, &options);
    let mut asked = 0;
    for i in 0..30 {
        perf.frame_with_gpu(
            f64::from(i) / 20.0,
            16.0,
            5.0,
            None,
            None,
            WorldCounts::default,
            || {
                asked += 1;
                Some(readout())
            },
        );
    }
    assert!(!perf.capturing());
    assert_eq!(asked, 1);
    let c = read(&perf.take_saved(true).pop().unwrap().unwrap().path);
    assert_eq!(c.gpu.len(), 1);
    assert_eq!(c.gpu[0].passes.len(), 2);
}

#[test]
fn the_timing_variables_ask_for_passes_and_the_detail_one_for_more() {
    let set = Some(std::ffi::OsStr::new("1"));
    assert_eq!(env_gpu(None, None), GpuSwitch::Off);
    assert_eq!(env_gpu(set, None), GpuSwitch::Passes);
    assert_eq!(env_gpu(None, set), GpuSwitch::Detail);
    assert_eq!(env_gpu(set, set), GpuSwitch::Detail);
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
fn the_messages_the_game_shows_are_translation_keys_with_the_arguments_the_code_fills() {
    let en: std::collections::HashMap<String, String> =
        serde_json::from_str(include_str!("../../assets/horizon/lang/en_us.json")).unwrap();
    for (key, holes) in [
        ("horizon.perf.saved", 2),
        ("horizon.perf.open_failed", 2),
        ("horizon.perf.upload_failed", 2),
        ("horizon.perf.shared", 2),
        ("horizon.perf.not_found", 1),
        ("horizon.perf.error", 1),
    ] {
        let text = en
            .get(key)
            .unwrap_or_else(|| panic!("{key} is not in en_us.json"));
        assert_eq!(text.matches("%s").count(), holes, "{key}: {text}");
    }
    assert!(en.contains_key("horizon.perf.started"));
}

/// Server ticks, 20 a second, from second `from` up to but not including `to`.
fn tick_server(server: &mut ServerPerf, from: f64, to: f64) {
    for i in 0..((to - from) * 20.0).round() as u32 {
        let t = from + f64::from(i) / 20.0;
        server.tick(t, 10.0, None, WorldCounts::default, Wire::new);
    }
}

fn server_options() -> CaptureOptions {
    CaptureOptions {
        window_s: None,
        trigger: Trigger::Ctl,
        start: Start {
            same_process: true,
            ..Start::default()
        },
        samples: false,
        sampler: SamplerOptions::default(),
        block_names: Vec::new(),
        levels: Vec::new(),
        wire: Wire::new(),
        gpu: GpuMode::Unspecified,
        timeline: None,
        file: None,
    }
}

#[test]
fn a_single_player_capture_holds_both_sides_whole_and_on_one_clock() {
    let scratch = Scratch::new("both");
    let mut client = ClientPerf::new(SLOW_FRAME_MS);
    let mut server = ServerPerf::with_role(Role::INTEGRATED, 0.0, 250.0);
    run(&mut client, 0, 2, 20, 16.0);
    tick_server(&mut server, 0.0, 2.0);
    let path = begin(&mut client, 2.0, &scratch.0, &no_samples());

    let join = client.join().unwrap();
    assert_eq!(join.path, path);
    assert_eq!(join.trigger, Trigger::Ctl);
    assert_eq!(join.options, no_samples());
    assert_eq!(join.ends.unwrap().1, R::WindowCap);
    let origin = join.origin.duration_since(client.epoch).as_secs_f64();
    assert!((origin - 2.0).abs() < 1e-6, "{origin}");

    // The server thread takes the request up in the middle of second 4.
    run(&mut client, 2, 5, 20, 16.0);
    tick_server(&mut server, 2.0, 4.5);
    server
        .join_capture(4.5, origin, join.sender, join.path, server_options())
        .unwrap();
    tick_server(&mut server, 4.5, 7.0);
    assert!(server.mark(5.25, "from the server"));
    assert!(client.mark(5.25, "from the client"));
    run(&mut client, 5, 7, 20, 16.0);

    assert!(server.begin_stop(7.0, R::Normal, CaptureEnd::default()));
    assert!(client.stop(7.0, R::Normal));
    let joined = server.take_saved(true).pop().unwrap().unwrap();
    assert_eq!(joined.path, path);
    assert!(joined.deleted.is_empty());
    let c = read(&client.take_saved(true).pop().unwrap().unwrap().path);

    assert_eq!((c.start.len(), c.stop.len()), (2, 2));
    let start = |side| c.start.iter().find(|s| s.side() == side).unwrap();
    assert_eq!(start(Side::Client).kind(), Kind::Client);
    assert_eq!(start(Side::Server).kind(), Kind::Integrated);
    assert!(start(Side::Server).same_process);
    for side in [Side::Client, Side::Server] {
        let stop = c.stop.iter().find(|s| s.side() == side).unwrap();
        assert_eq!((stop.t, stop.reason()), (5.0, R::Normal), "{side:?}");
    }

    // Both sides count from the client's start (2 s); the server joined mid-second 4 and skips that partial second.
    let seconds = |side| -> Vec<f64> {
        c.health
            .iter()
            .filter(|h| h.side() == side)
            .map(|h| h.t)
            .collect()
    };
    assert_eq!(seconds(Side::Client), [0.0, 1.0, 2.0, 3.0, 4.0]);
    assert_eq!(seconds(Side::Server), [3.0, 4.0]);
    for h in &c.health {
        match h.side() {
            Side::Server => assert_eq!((h.tps, h.fps), (Some(20.0), None)),
            _ => assert_eq!((h.fps, h.tps), (Some(20.0), None)),
        }
    }
    let mark = |name: &str| c.marks.iter().find(|m| m.name == name).unwrap();
    assert_eq!(
        (mark("from the server").side(), mark("from the server").t),
        (Side::Server, 3.25)
    );
    assert_eq!(
        (mark("from the client").side(), mark("from the client").t),
        (Side::Client, 3.25)
    );

    let options = horizon_perf::SummaryOptions::default();
    let report = horizon_perf::summarize(&c, "single player", &options).unwrap();
    let (server_at, client_at) = (report.find("integrated server"), report.find(": client"));
    assert!(server_at.is_some() && client_at.is_some(), "{report}");
    assert!(server_at < client_at, "the server comes first:\n{report}");
}

#[test]
fn a_server_that_joins_late_counts_from_the_captures_start_not_its_own() {
    let scratch = Scratch::new("late");
    let mut client = ClientPerf::new(SLOW_FRAME_MS);
    // The server's clock reads 0 when the client's reads 10.
    let mut server = ServerPerf::with_role(Role::INTEGRATED, 0.0, 250.0);
    run(&mut client, 0, 3, 10, 16.0);
    begin(&mut client, 3.0, &scratch.0, &no_samples());
    let join = client.join().unwrap();
    let origin = join.origin.duration_since(client.epoch).as_secs_f64() - 10.0;
    assert!((origin + 7.0).abs() < 1e-6, "{origin}");
    tick_server(&mut server, 0.0, 4.0);
    server
        .join_capture(4.0, origin, join.sender, join.path, server_options())
        .unwrap();
    tick_server(&mut server, 4.0, 5.0);
    assert!(server.begin_stop(5.0, R::Shutdown, CaptureEnd::default()));
    assert!(client.stop(15.0, R::Normal));
    server.take_saved(true).pop().unwrap().unwrap();
    let c = read(&client.take_saved(true).pop().unwrap().unwrap().path);
    let stop = |side| c.stop.iter().find(|s| s.side() == side).unwrap();
    assert_eq!(
        (stop(Side::Server).t, stop(Side::Server).reason()),
        (12.0, R::Shutdown)
    );
    assert_eq!(
        (stop(Side::Client).t, stop(Side::Client).reason()),
        (12.0, R::Normal)
    );
    let server_seconds: Vec<_> = c
        .health
        .iter()
        .filter(|h| h.side() == Side::Server)
        .map(|h| h.t)
        .collect();
    assert_eq!(server_seconds, [11.0]);
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
        let _turn = server_perf::SAMPLED_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    #[test]
    fn each_side_samples_its_own_thread_and_side_client_selects_the_render_thread() {
        let _turn = server_perf::SAMPLED_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use horizon_perf::request::QueryRequest;
        use horizon_perf::wire::{Op, SideName};
        let scratch = Scratch::new("sides");
        let mut client = ClientPerf::new(SLOW_FRAME_MS);
        let on = StartOptions {
            hz: Some(500),
            ..StartOptions::default()
        };
        let path = client
            .start(
                0.0,
                &scratch.0,
                Trigger::Env,
                &on,
                Start::default(),
                Vec::new(),
            )
            .unwrap();
        let join = client.join().unwrap();
        let server = std::thread::spawn(move || {
            let mut perf = ServerPerf::with_role(Role::INTEGRATED, 0.0, 250.0);
            let options = CaptureOptions {
                samples: true,
                sampler: SamplerOptions {
                    hz: Some(500),
                    ..SamplerOptions::default()
                },
                ..server_options()
            };
            perf.join_capture(0.0, 0.0, join.sender, join.path, options)
                .unwrap();
            for tick in 0..7u32 {
                work::tick_start();
                let started = sampler::thread_cpu_ns();
                spin_frame(30);
                let ns = sampler::thread_cpu_ns() - started;
                work::tick_end(ns);
                perf.tick(
                    f64::from(tick) * 0.1,
                    ns as f32 / 1e6,
                    None,
                    WorldCounts::default,
                    Wire::new,
                );
            }
            perf.begin_stop(0.7, R::Normal, CaptureEnd::default());
            perf.take_saved(true).pop().unwrap().unwrap();
        });
        for frame in 0..7u32 {
            work::tick_start();
            let started = sampler::thread_cpu_ns();
            spin_frame(30);
            let ns = sampler::thread_cpu_ns() - started;
            work::tick_end(ns);
            let wall = ns as f32 / 1e6;
            let t = f64::from(frame) * 0.1;
            client.frame(t, wall, wall, None, None, WorldCounts::default);
        }
        client.stop(0.7, R::Normal);
        server.join().unwrap();
        let saved = client.take_saved(true).pop().unwrap().unwrap();
        assert_eq!(saved.path, path);

        let c = read(&path);
        let threads: Vec<_> = c
            .samples
            .iter()
            .map(|s| {
                let name = c
                    .strings
                    .iter()
                    .find(|e| e.side() == s.side() && e.id == s.threads[0].name)
                    .unwrap();
                (s.side(), name.text.as_str(), s.threads[0].units)
            })
            .collect();
        assert!(
            threads.contains(&(Side::Client, "render", 7)),
            "{threads:?}"
        );
        assert!(threads.contains(&(Side::Server, "tick", 7)), "{threads:?}");
        assert_eq!(c.stop.len(), 2);

        let hot = |side| {
            let request = QueryRequest {
                side: Some(side),
                ..QueryRequest::default()
            };
            let q = request.into_query(Op::Hot, Vec::new()).unwrap();
            let answer = horizon_perf::ops::query(std::slice::from_ref(&path), &q).unwrap();
            assert_eq!(answer.error, None);
            answer.envelope.selection.unwrap().thread
        };
        assert_eq!(hot(SideName::Client).as_deref(), Some("render"));
        assert_eq!(hot(SideName::Server).as_deref(), Some("tick"));
    }
}

#[test]
fn a_refused_server_request_is_a_warning_at_its_time_in_the_client_side() {
    let scratch = Scratch::new("server-warning");
    let mut perf = ClientPerf::new(SLOW_FRAME_MS);
    run(&mut perf, 0, 2, 60, 16.0);
    assert!(
        !perf.warn(2.0, WarningCode::ServerDenied),
        "no capture runs"
    );
    begin(&mut perf, 2.0, &scratch.0, &no_samples());
    run(&mut perf, 2, 3, 60, 16.0);
    assert!(perf.warn(3.0, WarningCode::ServerNotHorizon));
    run(&mut perf, 3, 5, 60, 16.0);
    assert!(perf.warn(5.0, WarningCode::ServerAlreadyRunning));
    assert!(perf.stop(5.0, R::Normal));
    let saved = perf.take_saved(true).pop().unwrap().unwrap();

    let c = read(&saved.path);
    let seen: Vec<_> = c
        .warnings
        .iter()
        .map(|w| (w.side(), w.t, w.code()))
        .collect();
    assert_eq!(
        seen,
        [
            (Side::Client, 1.0, WarningCode::ServerNotHorizon),
            (Side::Client, 3.0, WarningCode::ServerAlreadyRunning),
        ]
    );
}
