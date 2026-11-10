mod common;

use std::path::Path;
use std::time::Duration;

use common::blocks::{CHILDREN, server, steady};
use common::hand::{NAMES, Names, capture};
use common::samples::{LEVEL_ID, waypoints};
use common::{CLIENT, SERVER, TempDir, save, start, stop};
use horizon_perf::bench_diff::BenchReply;
use horizon_perf::cli::{Env, run};
use horizon_perf::diff::DiffEnvelope;
use horizon_perf::query::Row;
use horizon_perf::watch::Event;
use horizon_perf::wire::Envelope;
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// A client capture whose start and stop carry every field `meta` prints, with frames on the render thread.
fn client_fields() -> Vec<Field> {
    let text = |id: u32, text: &str| {
        Field::String(StringEntry {
            side: CLIENT,
            id,
            text: text.into(),
        })
    };
    let mut fields = vec![Field::Start(Start {
        side: CLIENT,
        kind: Kind::Client as i32,
        trigger: Trigger::Key as i32,
        started_unix_ms: 1_794_580_324_000,
        uptime_s: 12.3456,
        build: Some(Build {
            version: "0.42.1".into(),
            commit: "1d63e0a5a3".into(),
            profile: "release".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            features: vec!["audio".into(), "gamepad".into()],
            symbols: Symbols::Dwarf as i32,
        }),
        machine: Some(Machine {
            os: "linux".into(),
            os_version: "6.19".into(),
            arch: "x86_64".into(),
            cpu: "Test CPU".into(),
            cpu_threads: 16,
            cpu_cores: 8,
            ram_mb: 32768,
            cgroup_cpu_cores: Some(3.5),
            cgroup_memory_mb: Some(4096),
            gpu: "Test GPU".into(),
        }),
        options: Some(Options {
            hz: 99,
            threads: ThreadSelection::All as i32,
            ticks_over_ms: Some(2.5),
            wall: true,
            timeline: true,
            gpu: GpuMode::Passes as i32,
            samples: true,
            window_s: Some(60.0),
        }),
        settings: Some(Settings {
            view_distance: Some(12),
            simulation_distance: Some(8),
            max_players: Some(20),
            network_compression_threshold: Some(-1),
        }),
        mods: vec![
            Mod {
                id: "alpha".into(),
                version: "1.2.3".into(),
                kind: ModKind::Content as i32,
                crate_name: String::new(),
            },
            Mod {
                id: "beta".into(),
                version: "0.1.0".into(),
                kind: ModKind::Native as i32,
                crate_name: "beta_mod".into(),
            },
        ],
        same_process: true,
        players: 1,
        world_fs_used_mb: 512,
        world_fs_total_mb: 4096,
        ..Default::default()
    })];
    fields.push(text(1, "render"));
    fields.push(text(2, "frame"));
    fields.push(text(3, "chunk_upload"));
    for block in 0..2u32 {
        fields.push(Field::SpanBlock(SpanBlock {
            side: CLIENT,
            thread: 1,
            block,
            t0: 5.0 * f64::from(block),
            block_s: 5.0,
            units: 300,
            span: vec![2, 3],
            parent: vec![0, 2],
            wall_ms: vec![3000.0 + 90.0 * block as f32, 450.0],
            cpu_ms: vec![2400.0, 300.0],
            seen: vec![300, 120],
        }));
    }
    for t in 0..10 {
        fields.push(Field::Health(HealthSample {
            side: CLIENT,
            t: f64::from(t),
            fps: Some(100.0 + t as f32),
            frame_ms_p50: Some(9.5),
            frame_ms_p95: Some(14.25),
            frame_ms_p99: Some(20.0),
            frame_ms_max: Some(31.0),
            frame_cpu_ms_p50: Some(6.0),
            frame_gpu_ms_p50: Some(7.5),
            rss_mb: 1500.5,
            ..Default::default()
        }));
    }
    fields.push(Field::Slow(SlowEvent {
        side: CLIENT,
        t: 4.0,
        unit: Unit::Frame as i32,
        counter: 400,
        wall_ms: 80.5,
        cpu_ms: 70.25,
        top: vec![SpanCost {
            name: 3,
            wall_ms: 40.0,
            cpu_ms: 30.0,
            depth: 1,
        }],
    }));
    fields.push(Field::Stop(Stop {
        side: CLIENT,
        t: 10.0,
        reason: StopReason::WindowCap as i32,
        overhead: Some(Overhead {
            spans_ms_per_tick: 0.0123,
            spans_share: 0.00123,
            sampler_share_of_thread: 0.000321,
            writer_core_share: 0.0042,
            dropped_samples: 3,
            dropped_messages: 1,
            timeline_stopped_t: Some(9.5),
            ..Default::default()
        }),
    }));
    fields
}

/// Fields `follow --json` turns into every kind of event, on both sides.
fn watched_fields() -> Vec<Field> {
    vec![
        start(SERVER, Kind::Dedicated),
        start(CLIENT, Kind::Client),
        Field::String(StringEntry {
            side: SERVER,
            id: 5,
            text: "tick_lan".into(),
        }),
        Field::Health(HealthSample {
            side: SERVER,
            t: 1.0,
            tps: Some(19.7),
            mspt_avg: Some(41.3),
            mspt_p95: Some(55.1),
            mspt_max: Some(80.9),
            rss_mb: 1024.4,
            players: 3,
            ..Default::default()
        }),
        Field::Health(HealthSample {
            side: CLIENT,
            t: 1.0,
            fps: Some(144.3),
            frame_ms_p95: Some(9.1),
            rss_mb: 2048.7,
            ..Default::default()
        }),
        Field::Slow(SlowEvent {
            side: SERVER,
            t: 2.5,
            unit: Unit::Tick as i32,
            counter: 48812,
            wall_ms: 312.4,
            top: vec![
                SpanCost {
                    name: 5,
                    wall_ms: 180.2,
                    ..Default::default()
                },
                SpanCost {
                    name: 9,
                    wall_ms: 61.1,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        Field::Slow(SlowEvent {
            side: CLIENT,
            t: 2.6,
            unit: Unit::Frame as i32,
            counter: 7,
            wall_ms: 55.5,
            ..Default::default()
        }),
        Field::Mark(Mark {
            side: SERVER,
            t: 2.75,
            name: "after the fix".into(),
        }),
        Field::Warning(Warning {
            side: SERVER,
            t: 2.8,
            code: WarningCode::TimelineCapped as i32,
            args: vec![30.0, 1_048_576.0],
        }),
        stop(SERVER, 3.0),
        Field::Stop(Stop {
            side: CLIENT,
            t: 3.0,
            reason: StopReason::WindowCap as i32,
            ..Default::default()
        }),
    ]
}

fn bench_report(frames: &[(&str, u32, f64, Option<f64>)], ab: Option<&str>) -> String {
    let frames: Vec<String> = frames
        .iter()
        .map(|(phase, repeat, p50, cpu)| {
            let cpu = cpu.map_or(String::new(), |c| format!(r#","cpu_p50_ms":{c}"#));
            format!(
                r#"{{"phase":"{phase}","repeat":{repeat},"frames":600,"seconds":5.0,"avg_fps":100.0,"low_1pct_fps":80.0,"low_01pct_fps":70.0,"p50_ms":{p50},"p95_ms":{},"p99_ms":{},"p999_ms":{},"max_ms":{}{cpu},"histogram":[]}}"#,
                p50 * 1.5,
                p50 * 2.0,
                p50 * 3.0,
                p50 * 4.0
            )
        })
        .collect();
    let ab = ab.map_or(String::new(), |ab| format!(r#","ab":{ab}"#));
    format!(
        r#"{{"format":"horizon-benchmark-report","version":1,"kind":"game","created":"2026-11-07T10:00:00Z","app":{{"name":"horizon","version":"0.1"}},"hardware":{{}},"frames":[{}]{ab}}}"#,
        frames.join(",")
    )
}

const AB: &str = r#"{"switch":"gpu-culling","a":"off","b":"on","pairs":4,"block_s":4.0,"warmup_s":2.0,"first_block_outside_range":true,"result":{"gpu_p50_ms":{"pairs":4,"a":3.66,"b":5.63,"diff":1.96,"diff_pct":53.6,"ci95":[-0.5,3.0],"confidence":0.875,"noise":0.1,"verdict":"within_noise"},"cpu_p50_ms":{"pairs":4,"a":2.0,"b":2.0,"diff":0.0,"diff_pct":null,"ci95":null,"confidence":null,"noise":null,"verdict":"too_few_pairs"}}}"#;

struct Fixtures {
    _dir: TempDir,
    paths: Vec<(&'static str, String)>,
}

fn fixtures() -> Fixtures {
    let dir = TempDir::new("golden");
    let hand = capture(&Names::new(&NAMES));
    let mut slower = steady(12, 5);
    for row in &mut slower {
        row[0] += 2.0;
        row[2] -= 1.0;
    }
    let reports: Vec<(&str, String)> = vec![
        (
            "benchA.json",
            bench_report(&steady_frames("steady", 10.0, true), Some(AB)),
        ),
        (
            "benchB.json",
            bench_report(&steady_frames("steady", 12.0, true), None),
        ),
        (
            "benchC.json",
            bench_report(&steady_frames("steady", 10.0, true), None),
        ),
    ];
    let mut paths = vec![
        ("hand", save(&dir, "hand.hzperf", &hand)),
        ("hand2", save(&dir, "other.hzperf", &hand)),
        ("client", save(&dir, "client.hzperf", &client_fields())),
        (
            "base",
            save(
                &dir,
                "base.hzperf",
                &server(&CHILDREN, &steady(12, 0), "release"),
            ),
        ),
        (
            "head",
            save(&dir, "head.hzperf", &server(&CHILDREN, &slower, "fast")),
        ),
        ("watched", save(&dir, "watched.hzperf", &watched_fields())),
        (
            "samples",
            save(&dir, "samples.hzperf", &waypoints(1, 50).fields()),
        ),
        (
            "bare",
            save(
                &dir,
                "bare.hzperf",
                &[start(SERVER, Kind::Dedicated), stop(SERVER, 1.0)],
            ),
        ),
    ];
    for (name, text) in &reports {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        paths.push((
            match *name {
                "benchA.json" => "benchA",
                "benchB.json" => "benchB",
                _ => "benchC",
            },
            path.to_string_lossy().into_owned(),
        ));
    }
    Fixtures { _dir: dir, paths }
}

fn steady_frames(phase: &str, base: f64, cpu: bool) -> Vec<(&str, u32, f64, Option<f64>)> {
    [0.0, 0.1, -0.1, 0.2, -0.2, 0.0, 0.1, -0.1]
        .iter()
        .enumerate()
        .map(|(i, d)| {
            (
                phase,
                i as u32,
                base + d,
                (cpu && i % 2 == 0).then_some(base / 2.0),
            )
        })
        .collect()
}

const CASES: &[(&str, &[&str])] = &[
    ("spans", &["query", "spans", "{hand}"]),
    ("spans_slow", &["query", "spans", "--slow", "{hand}"]),
    ("spans_two", &["query", "spans", "{hand}", "{hand2}"]),
    (
        "spans_names_where",
        &[
            "query",
            "spans",
            "--names",
            "streaming,tick_lan",
            "--where",
            "span=streaming",
            "--where",
            "dim=overworld",
            "{hand}",
        ],
    ),
    (
        "spans_window",
        &[
            "query",
            "spans",
            "--from",
            "2",
            "--to",
            "mark:after the fix",
            "{hand}",
        ],
    ),
    (
        "spans_window_from_end",
        &["query", "spans", "--from", "-5", "{hand}"],
    ),
    (
        "net_window",
        &[
            "query", "net", "--from", "2", "--to", "8", "--limit", "1", "{hand}",
        ],
    ),
    (
        "world_where_two",
        &[
            "query",
            "world",
            "--where",
            "dim=overworld",
            "--where",
            "dimension=the_nether",
            "--by",
            "entity",
            "{hand}",
        ],
    ),
    (
        "spans_unknown_name",
        &["query", "spans", "--names", "nope", "{hand}"],
    ),
    (
        "spans_ambiguous_name",
        &["query", "spans", "--names", "t", "{hand}"],
    ),
    (
        "spans_client",
        &["query", "spans", "--side", "client", "{client}"],
    ),
    (
        "spans_slow_client",
        &["query", "spans", "--slow", "--side", "client", "{client}"],
    ),
    ("ticks", &["query", "ticks", "{hand}"]),
    (
        "ticks_worst_over",
        &["query", "ticks", "--worst", "2", "--over", "100", "{hand}"],
    ),
    (
        "ticks_client",
        &["query", "ticks", "--side", "client", "{client}"],
    ),
    ("tick", &["query", "tick", "k:101", "{hand}"]),
    (
        "hot_window",
        &["query", "hot", "--from", "6", "--to", "8", "{samples}"],
    ),
    ("memory_labels", &["query", "memory_labels", "{hand}"]),
    ("counters", &["query", "counters", "{hand}"]),
    ("tick_incomplete", &["query", "tick", "100", "{hand}"]),
    ("tick_missing", &["query", "tick", "k:55", "{hand}"]),
    ("tick_bad_id", &["query", "tick", "x", "{hand}"]),
    (
        "series",
        &[
            "query",
            "series",
            "span:tick_lan",
            "h:players",
            "x:streaming.updates{kind=light}",
            "streaming",
            "--every",
            "4",
            "{hand}",
        ],
    ),
    (
        "series_client",
        &[
            "query",
            "series",
            "h:fps",
            "h:frame_ms_p95",
            "span:chunk_upload",
            "--side",
            "client",
            "--thread",
            "render",
            "--every",
            "5",
            "{client}",
        ],
    ),
    (
        "series_unknown",
        &["query", "series", "h:nonsense", "{hand}"],
    ),
    (
        "series_unknown_span",
        &["query", "series", "span:nonsense", "{hand}"],
    ),
    (
        "series_unknown_counter",
        &["query", "series", "x:nonsense", "{hand}"],
    ),
    (
        "series_ambiguous",
        &["query", "series", "nonsense", "{hand}"],
    ),
    ("series_none", &["query", "series", "{hand}"]),
    ("world", &["query", "world", "{hand}"]),
    (
        "world_entity",
        &["query", "world", "--by", "entity", "{hand}"],
    ),
    (
        "world_block_entity",
        &[
            "query",
            "world",
            "--by",
            "block_entity",
            "--where",
            "dimension=overworld",
            "{hand}",
        ],
    ),
    ("world_cost", &["query", "world", "--by", "cost", "{hand}"]),
    ("net", &["query", "net", "{hand}"]),
    ("meta", &["query", "meta", "{hand}"]),
    ("meta_client", &["query", "meta", "{client}"]),
    ("memory", &["query", "memory", "{hand}"]),
    ("memory_label", &["query", "memory", "start", "{hand}"]),
    ("memory_diff", &["query", "memory", "--diff", "{hand}"]),
    (
        "memory_diff_labels",
        &[
            "query", "memory", "--diff", "start", "stop", "--min-mb", "0.5", "{hand}",
        ],
    ),
    (
        "memory_bad_label",
        &["query", "memory", "--diff", "start", "later", "{hand}"],
    ),
    ("marks", &["query", "marks", "{hand}"]),
    ("warnings", &["query", "warnings", "{hand}"]),
    ("hot", &["query", "hot", "--limit", "3", "{samples}"]),
    (
        "tree",
        &["query", "tree", LEVEL_ID, "--min-pct", "10", "{samples}"],
    ),
    ("callers", &["query", "callers", LEVEL_ID, "{samples}"]),
    ("callees", &["query", "callees", LEVEL_ID, "{samples}"]),
    (
        "split",
        &["query", "split", LEVEL_ID, "--by", "chunk", "{samples}"],
    ),
    ("group", &["query", "group", "--by", "entity", "{samples}"]),
    ("frames", &["query", "frames", LEVEL_ID, "{samples}"]),
    ("meta_samples", &["query", "meta", "{samples}"]),
    ("hot_without_samples", &["query", "hot", "{hand}"]),
    ("unanswered_op", &["query", "samples", "{hand}"]),
    ("unknown_op", &["query", "nonsense", "{hand}"]),
    ("bare_world", &["query", "world", "{bare}"]),
    (
        "side_missing",
        &["query", "spans", "--side", "client", "{hand}"],
    ),
    ("two_files_one_op", &["query", "ticks", "{hand}", "{hand2}"]),
    ("diff", &["diff", "{base}", "{head}", "--json"]),
    (
        "diff_limited",
        &[
            "diff", "{base}", "{head}", "--json", "--limit", "5", "--min-ms", "1",
        ],
    ),
    ("diff_same", &["diff", "{base}", "{base}", "--json"]),
    ("bench_ab", &["diff", "{benchA}", "{benchB}", "--json"]),
    ("bench_frames", &["diff", "{benchC}", "{benchB}", "--json"]),
    ("follow", &["follow", "{watched}", "--json"]),
];

fn run_case(args: &[&str], paths: &[(&'static str, String)]) -> String {
    let args: Vec<String> = args
        .iter()
        .map(|a| {
            paths.iter().fold((*a).to_string(), |a, (key, path)| {
                a.replace(&format!("{{{key}}}"), path)
            })
        })
        .collect();
    let env = Env {
        horizon_home: None,
        poll: Duration::from_millis(2),
    };
    let mut out = Vec::new();
    let _ = run(&args, &env, &mut out);
    String::from_utf8(out).unwrap()
}

fn golden_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.json"))
}

/// The wire shape a case prints, one value per line.
#[derive(Clone, Copy)]
enum Shape {
    Query,
    Diff,
    Bench,
    Follow,
}

impl Shape {
    fn of(name: &str, args: &[&str]) -> Shape {
        match (args[0], name.starts_with("bench")) {
            ("follow", _) => Shape::Follow,
            ("diff", true) => Shape::Bench,
            ("diff", false) => Shape::Diff,
            _ => Shape::Query,
        }
    }

    /// Parses `line` into its type and serializes it back.
    fn round_trip(self, line: &str) -> String {
        fn back<T: Serialize + DeserializeOwned>(line: &str) -> String {
            let typed: T = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"));
            serde_json::to_string(&typed).unwrap()
        }
        match self {
            Shape::Query => back::<Envelope<Row>>(line),
            Shape::Diff => back::<DiffEnvelope>(line),
            Shape::Bench => back::<BenchReply>(line),
            Shape::Follow => back::<Event>(line),
        }
    }
}

fn values(text: &str) -> Vec<Value> {
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

#[test]
fn every_reply_is_the_json_it_was_before_the_types() {
    let fx = fixtures();
    for (name, args) in CASES {
        let golden = std::fs::read_to_string(golden_path(name)).unwrap();
        let out = run_case(args, &fx.paths);
        assert_eq!(values(&out), values(&golden), "{name}");
    }
}

#[test]
fn golden_replies_parse_into_the_types_and_serialize_back_to_themselves() {
    for (name, args) in CASES {
        let golden = std::fs::read_to_string(golden_path(name)).unwrap();
        let shape = Shape::of(name, args);
        let back: Vec<Value> = golden
            .lines()
            .map(|l| serde_json::from_str(&shape.round_trip(l)).unwrap())
            .collect();
        assert_eq!(back, values(&golden), "{name}");
    }
}

#[test]
fn what_the_cli_prints_is_what_its_types_serialize_to() {
    let fx = fixtures();
    for (name, args) in CASES {
        let out = run_case(args, &fx.paths);
        let shape = Shape::of(name, args);
        for line in out.lines() {
            assert_eq!(shape.round_trip(line), line, "{name}");
        }
    }
}
