mod common;

use common::{CLIENT, SERVER, TempDir, meta_rows, query, rows, save, start, stop};
use horizon_perf::query::Row;
use horizon_perf::query::rows::{GpuDeviceRow, GpuFrameRow, GpuPassRow, GpuSummaryRow, MsStats};
use horizon_perf::wire::{
    BoundName, Envelope, GpuBackendName, GpuFragmentUnitName, GpuPartsModeName, GpuPassSplitName,
    GpuSourceName, Unit,
};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

fn text(id: u32, text: &str) -> Field {
    Field::String(StringEntry {
        side: CLIENT,
        id,
        text: text.into(),
    })
}

fn counts(vertex: u64, fragment: u64) -> Option<GpuCounts> {
    Some(GpuCounts {
        vertex_shader_invocations: vertex,
        clipper_invocations: 2_050_000,
        clipper_primitives_out: 1_200_000,
        fragment_shader_invocations: fragment,
    })
}

fn device(unit: GpuFragmentUnit) -> Option<GpuDevice> {
    Some(GpuDevice {
        adapter: "Test GPU".into(),
        backend: GpuBackend::Vulkan as i32,
        driver: "Test driver 1.2".into(),
        parts_mode: GpuPartsMode::InsidePasses as i32,
        counters: true,
        fragment_unit: unit as i32,
        pass_split: GpuPassSplit::DrawGroups as i32,
    })
}

const NAMES: [&str; 6] = [
    "World",
    "GPU culling",
    "cull",
    "HUD",
    "before translucent",
    "Visibility IDs",
];

fn id(name: &str) -> u32 {
    1 + NAMES.iter().position(|n| *n == name).unwrap() as u32
}

fn fields(unit: GpuFragmentUnit) -> Vec<Field> {
    let mut fields = vec![start(CLIENT, Kind::Client)];
    fields.extend(NAMES.iter().enumerate().map(|(i, n)| text(i as u32 + 1, n)));
    fields.push(Field::Gpu(GpuPasses {
        side: CLIENT,
        device: device(unit),
        source: GpuSource::Horizon as i32,
        passes: vec![
            GpuPass {
                name: id("HUD"),
                ms: Some(0.5),
                parts: Vec::new(),
            },
            GpuPass {
                name: id("World"),
                ms: Some(1.5),
                parts: Vec::new(),
            },
            GpuPass {
                name: id("GPU culling"),
                ms: Some(0.25),
                parts: vec![GpuPart {
                    name: id("cull"),
                    ms: 0.125,
                }],
            },
        ],
        counters: vec![
            GpuCounter {
                pass: id("World"),
                total: counts(4_100_000, 6_300_000),
                pixels: 2_073_600,
                parts: vec![GpuPartCounts {
                    name: id("before translucent"),
                    counts: counts(3_000_000, 5_100_000),
                }],
            },
            GpuCounter {
                pass: id("Visibility IDs"),
                total: counts(10, 20),
                pixels: 100,
                parts: Vec::new(),
            },
        ],
    }));
    fields.push(Field::Gpu(GpuPasses {
        side: CLIENT,
        device: device(unit),
        source: GpuSource::ShaderPack as i32,
        passes: vec![GpuPass {
            name: id("HUD"),
            ms: Some(4.0),
            parts: Vec::new(),
        }],
        counters: Vec::new(),
    }));
    fields.push(stop(CLIENT, 10.0));
    fields
}

fn file(tag: &str, unit: GpuFragmentUnit) -> (TempDir, String) {
    let dir = TempDir::new(tag);
    let path = save(&dir, "run.hzperf", &fields(unit));
    (dir, path)
}

fn split(reply: &Envelope<Row>) -> (&GpuDeviceRow, Vec<&GpuPassRow>) {
    let (device, _, passes) = split_all(reply);
    (device, passes)
}

fn split_all(reply: &Envelope<Row>) -> (&GpuDeviceRow, &GpuSummaryRow, Vec<&GpuPassRow>) {
    let rows = rows(reply);
    let Row::GpuDevice(device) = &rows[0] else {
        panic!("the first row is the device: {rows:?}");
    };
    let Row::GpuSummary(summary) = &rows[1] else {
        panic!("the second row is the summary: {rows:?}");
    };
    let passes = rows[2..]
        .iter()
        .map(|r| match r {
            Row::GpuPass(p) => p,
            other => panic!("not a pass: {other:?}"),
        })
        .collect();
    (device, summary, passes)
}

#[test]
fn the_medians_come_back_per_pass_biggest_first_with_the_device() {
    let (_dir, path) = file("medians", GpuFragmentUnit::Pixel);
    let reply = query(&["gpu", &path]);
    assert_eq!(reply.unit, Some(Unit::MsPerFrame));
    let (device, passes) = split(&reply);
    assert_eq!(device.id, "g:device");
    assert_eq!(device.adapter, "Test GPU");
    assert_eq!(device.backend, GpuBackendName::Vulkan);
    assert_eq!(device.driver, "Test driver 1.2");
    assert_eq!(device.parts_mode, GpuPartsModeName::InsidePasses);
    assert!(device.counters);
    assert_eq!(device.fragment_unit, GpuFragmentUnitName::Pixel);

    let listed: Vec<_> = passes
        .iter()
        .map(|p| (p.name.as_str(), p.source, p.ms))
        .collect();
    assert_eq!(
        listed,
        [
            ("World", GpuSourceName::Horizon, Some(1.5)),
            ("HUD", GpuSourceName::Horizon, Some(0.5)),
            ("GPU culling", GpuSourceName::Horizon, Some(0.25)),
            ("Visibility IDs", GpuSourceName::Horizon, None),
            ("HUD", GpuSourceName::ShaderPack, Some(4.0)),
        ]
    );
    // 1.5 + 0.5 + 0.25 = 2.25 ms of Horizon's own passes; the pack's one pass is all of its own.
    assert_eq!(passes[0].share, Some(0.666667));
    assert_eq!(passes[4].share, Some(1.0));
    assert_eq!(passes[3].share, None);
    assert_eq!(passes[0].id, "g:World");
    let culling = &passes[2].parts;
    assert_eq!((culling[0].name.as_str(), culling[0].ms), ("cull", 0.125));
}

#[test]
fn fragments_per_pixel_is_the_counters_vector_of_the_world_pass() {
    let (_dir, path) = file("overdraw", GpuFragmentUnit::Pixel);
    let reply = query(&["gpu", "World", &path]);
    let (_, passes) = split(&reply);
    assert_eq!(passes.len(), 1);
    let counters = passes[0].counters.as_ref().unwrap();
    assert_eq!(counters.total.fragment_shader_invocations, 6_300_000);
    assert_eq!(counters.pixels, 2_073_600);
    // 6,300,000 shaded pixels over a 1920 x 1080 target.
    assert_eq!(counters.fragments_per_pixel, Some(3.04));
    assert_eq!(counters.parts[0].name, "before translucent");
    assert_eq!(
        counters.parts[0].counts.vertex_shader_invocations,
        3_000_000
    );
}

#[test]
fn a_quad_counter_stands_for_four_pixels_and_an_unknown_unit_gives_none() {
    let per_pixel = |unit, pass: &str| {
        let (_dir, path) = file("unit", unit);
        let reply = query(&["gpu", pass, &path]);
        let (_, passes) = split(&reply);
        passes[0].counters.as_ref().unwrap().fragments_per_pixel
    };
    // 20 invocations over a target of 100 pixels: 0.2 as pixels, 0.8 as quads.
    assert_eq!(
        per_pixel(GpuFragmentUnit::Pixel, "Visibility IDs"),
        Some(0.2)
    );
    assert_eq!(
        per_pixel(GpuFragmentUnit::Quad, "Visibility IDs"),
        Some(0.8)
    );
    assert_eq!(
        per_pixel(GpuFragmentUnit::Unspecified, "Visibility IDs"),
        None
    );
}

#[test]
fn passes_are_picked_by_name_or_id_and_an_unknown_one_lists_the_passes() {
    let (_dir, path) = file("pick", GpuFragmentUnit::Pixel);
    let by_id = query(&["gpu", "g:GPU culling", &path]);
    assert_eq!(split(&by_id).1[0].name, "GPU culling");
    let unknown = query(&["gpu", "Shadows", &path]);
    assert!(!unknown.ok);
    let error = unknown.error.unwrap();
    assert!(
        error.contains("Shadows") && error.contains("GPU culling"),
        "{error}"
    );
    let limited = query(&["gpu", "--limit", "2", &path]);
    assert_eq!(split(&limited).1.len(), 2);
}

#[test]
fn a_capture_without_gpu_says_how_to_record_it() {
    let dir = TempDir::new("none");
    let path = save(
        &dir,
        "none.hzperf",
        &[start(SERVER, Kind::Dedicated), stop(SERVER, 5.0)],
    );
    let reply = query(&["gpu", &path]);
    assert!(!reply.ok);
    assert!(reply.error.unwrap().contains("--gpu"));
}

#[test]
fn meta_counts_the_gpu_sections_and_the_summary_names_the_top_passes() {
    let (_dir, path) = file("meta", GpuFragmentUnit::Pixel);
    let meta = query(&["meta", &path]);
    assert_eq!(meta_rows(&meta)[0].sections.gpu, 2);
    let summary = horizon_perf::ops::summary(
        std::path::Path::new(&path),
        &horizon_perf::summary::SummaryOptions::default(),
    )
    .unwrap();
    let line = summary
        .lines()
        .find(|l| l.starts_with("gpu"))
        .unwrap_or_else(|| panic!("no gpu line in {summary}"));
    assert!(
        line.contains("World 1.50 ms 3.04 frag/px [g:World]"),
        "{line}"
    );
}

#[test]
fn a_capture_from_before_frames_reads_with_empty_statistics_and_no_series() {
    let (_dir, path) = file("old", GpuFragmentUnit::Pixel);
    let reply = query(&["gpu", &path]);
    let (device, summary, passes) = split_all(&reply);
    assert_eq!(device.pass_split, GpuPassSplitName::DrawGroups);
    assert_eq!(summary.gpu_frame, MsStats::default());
    assert_eq!(summary.cpu_frame, MsStats::default());
    assert_eq!(summary.bound, None);
    let world = passes.iter().find(|p| p.name == "World").unwrap();
    assert_eq!(
        (world.ms, world.samples, world.p50, world.max),
        (Some(1.5), 0, None, None)
    );
    assert_eq!(world.share, Some(0.666667));
    let series = query(&["gpu_frames", &path]);
    assert!(rows(&series).is_empty());
}

fn frame(t: f64, total: f32, world: f32, sky: Option<f32>) -> GpuFrame {
    GpuFrame {
        source: GpuSource::Horizon as i32,
        t,
        total_ms: Some(total),
        passes: vec![
            GpuPass {
                name: id("World"),
                ms: Some(world),
                parts: sky
                    .map(|ms| GpuPart {
                        name: id("cull"),
                        ms,
                    })
                    .into_iter()
                    .collect(),
            },
            GpuPass {
                name: id("HUD"),
                ms: Some(0.5),
                parts: Vec::new(),
            },
        ],
    }
}

/// Ten timed frames 0.5 s apart from t = 1, the second block's written backwards, twenty CPU frames (1..=20 ms) and the end section.
fn timed_fields() -> Vec<Field> {
    let totals = [4.0, 1.0, 3.0, 2.0, 5.0, 10.0, 6.0, 7.0, 8.0, 9.0];
    let worlds = [2.0, 1.0, 2.0, 1.0, 3.0, 6.0, 3.0, 4.0, 4.0, 5.0];
    let parts = [
        Some(0.5),
        None,
        Some(0.25),
        None,
        None,
        Some(0.75),
        None,
        None,
        Some(1.0),
        None,
    ];
    let frames: Vec<GpuFrame> = (0..10)
        .map(|i| frame(1.0 + i as f64 * 0.5, totals[i], worlds[i], parts[i]))
        .collect();
    let cpu: Vec<f32> = (1..=20).map(|ms| ms as f32).collect();
    let mut fields = vec![start(CLIENT, Kind::Client)];
    fields.extend(NAMES.iter().enumerate().map(|(i, n)| text(i as u32 + 1, n)));
    fields.push(Field::GpuFrames(GpuFrames {
        side: CLIENT,
        frames: frames[..4].to_vec(),
        cpu_frame_ms: cpu[..8].to_vec(),
    }));
    fields.push(Field::GpuFrames(GpuFrames {
        side: CLIENT,
        frames: frames[4..].iter().rev().cloned().collect(),
        cpu_frame_ms: cpu[8..].to_vec(),
    }));
    fields.push(Field::Slow(SlowEvent {
        side: CLIENT,
        t: 2.01,
        unit: horizon_perf_format::v1::Unit::Frame as i32,
        counter: 7,
        wall_ms: 20.0,
        cpu_ms: 3.0,
        top: Vec::new(),
    }));
    fields.push(Field::Gpu(GpuPasses {
        side: CLIENT,
        device: device(GpuFragmentUnit::Pixel),
        source: GpuSource::Horizon as i32,
        passes: vec![
            GpuPass {
                name: id("World"),
                ms: Some(3.0),
                parts: vec![GpuPart {
                    name: id("cull"),
                    ms: 0.75,
                }],
            },
            GpuPass {
                name: id("HUD"),
                ms: Some(0.5),
                parts: Vec::new(),
            },
        ],
        counters: Vec::new(),
    }));
    fields.push(stop(CLIENT, 10.0));
    fields
}

#[test]
fn frame_statistics_are_nearest_rank_percentiles_and_shares_of_the_frame_p50() {
    let dir = TempDir::new("frames");
    let path = save(&dir, "frames.hzperf", &timed_fields());
    let reply = query(&["gpu", &path]);
    let (_, summary, passes) = split_all(&reply);
    // Totals 1..=10: rank ceil(0.5 x 10) = 5, ceil(0.95 x 10) = ceil(0.99 x 10) = 10.
    let stats = |samples, p50, p95, p99, max| MsStats {
        samples,
        p50: Some(p50),
        p95: Some(p95),
        p99: Some(p99),
        max: Some(max),
    };
    assert_eq!(summary.gpu_frame, stats(10, 5.0, 10.0, 10.0, 10.0));
    // CPU frames 1..=20: ranks 10, 19 and 20.
    assert_eq!(summary.cpu_frame, stats(20, 10.0, 19.0, 20.0, 20.0));
    // 10 ms of CPU against 5 ms of GPU: at least 1.2 times, so the CPU bounds the frames.
    assert_eq!(summary.bound, Some(BoundName::Cpu));
    assert!(summary.bound_rule.contains("1.2"));

    let names: Vec<_> = passes.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["World", "HUD"]);
    let world = passes[0];
    // World's times sorted: 1 1 2 2 3 3 4 4 5 6.
    assert_eq!(
        (world.samples, world.p50, world.p95, world.p99, world.max),
        (10, Some(3.0), Some(6.0), Some(6.0), Some(6.0))
    );
    assert_eq!((world.ms, world.share), (Some(3.0), Some(0.6)));
    assert_eq!(passes[1].share, Some(0.1));
    // The part was timed in four frames: 0.25 0.5 0.75 1.0.
    let part = &world.parts[0];
    assert_eq!(part.name, "cull");
    assert_eq!(
        (part.samples, part.p50, part.p95, part.max, part.ms),
        (4, Some(0.5), Some(1.0), Some(1.0), 0.75)
    );
    assert_eq!(part.share, Some(0.1));
}

#[test]
fn the_frame_series_is_in_time_order_and_tags_the_frame_inside_a_slow_one() {
    let dir = TempDir::new("series");
    let path = save(&dir, "series.hzperf", &timed_fields());
    let reply = query(&["gpu_frames", "--from", "1.5", "--to", "3.5", &path]);
    assert_eq!(reply.unit, Some(Unit::MsPerFrame));
    let got: Vec<&GpuFrameRow> = rows(&reply)
        .iter()
        .map(|r| match r {
            Row::GpuFrame(f) => f,
            other => panic!("not a frame: {other:?}"),
        })
        .collect();
    let seen: Vec<_> = got
        .iter()
        .map(|f| (f.t, f.total_ms, f.slow, f.slow_wall_ms))
        .collect();
    // The slow frame ended at 2.01 after 20 ms: it began at 1.99, so it holds the frame timed at 2.0.
    assert_eq!(
        seen,
        [
            (1.5, Some(1.0), false, None),
            (2.0, Some(3.0), true, Some(20.0)),
            (2.5, Some(2.0), false, None),
            (3.0, Some(5.0), false, None),
            (3.5, Some(10.0), false, None),
        ]
    );
    let slow = got[1];
    assert_eq!(slow.source, GpuSourceName::Horizon);
    assert_eq!(slow.passes[0].name, "World");
    assert_eq!(slow.passes[0].ms, Some(2.0));
    assert_eq!(slow.passes[0].parts[0].name, "cull");
    assert_eq!(slow.passes[0].parts[0].ms, 0.25);
    assert_eq!(slow.passes[1].name, "HUD");
    assert!(got[0].passes[0].parts.is_empty());
}
