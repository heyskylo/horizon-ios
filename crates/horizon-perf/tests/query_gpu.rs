mod common;

use common::{CLIENT, SERVER, TempDir, meta_rows, query, rows, save, start, stop};
use horizon_perf::query::Row;
use horizon_perf::query::rows::{GpuDeviceRow, GpuPassRow};
use horizon_perf::wire::{
    Envelope, GpuBackendName, GpuFragmentUnitName, GpuPartsModeName, GpuSourceName, Unit,
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
    let rows = rows(reply);
    let Row::GpuDevice(device) = &rows[0] else {
        panic!("the first row is the device: {rows:?}");
    };
    let passes = rows[1..]
        .iter()
        .map(|r| match r {
            Row::GpuPass(p) => p,
            other => panic!("not a pass: {other:?}"),
        })
        .collect();
    (device, passes)
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
