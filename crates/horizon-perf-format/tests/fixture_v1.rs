//! `tests/fixtures/v1.hzperf` was written once and is never regenerated: every later build must decode it.

use std::io::Read;

use flate2::read::GzDecoder;
use horizon_perf_format::v1::*;
use prost::Message;

const SERVER: i32 = Side::Server as i32;
const CLIENT: i32 = Side::Client as i32;

fn health(side: i32, t: f64) -> HealthSample {
    HealthSample {
        side,
        t,
        ..Default::default()
    }
}

fn expected() -> Capture {
    Capture {
        format: 1,
        start: vec![
            Start {
                side: SERVER,
                kind: Kind::Dedicated as i32,
                trigger: Trigger::Console as i32,
                started_unix_ms: 1_794_580_324_000,
                uptime_s: 3600.5,
                build: Some(Build {
                    version: "0.42.0".into(),
                    commit: "c2a2acb5e1".into(),
                    profile: "release".into(),
                    target: "x86_64-unknown-linux-gnu".into(),
                    features: vec!["sampler".into()],
                    symbols: Symbols::Dwarf as i32,
                }),
                machine: Some(Machine {
                    os: "linux".into(),
                    os_version: "6.12".into(),
                    arch: "x86_64".into(),
                    cpu: "AMD Ryzen 9 7900X".into(),
                    cpu_threads: 24,
                    cpu_cores: 12,
                    ram_mb: 64_000,
                    cgroup_cpu_cores: Some(8.0),
                    cgroup_memory_mb: Some(16_384),
                    ..Default::default()
                }),
                options: Some(Options {
                    hz: 250,
                    threads: ThreadSelection::All as i32,
                    wall: true,
                    samples: true,
                    window_s: Some(60.0),
                    ..Default::default()
                }),
                settings: Some(Settings {
                    view_distance: Some(10),
                    simulation_distance: Some(8),
                    max_players: Some(20),
                    network_compression_threshold: Some(256),
                }),
                game_rules: Some(GameRules::default()),
                horizon_options: Some(HorizonOptions::default()),
                mods: vec![Mod {
                    id: "lantern_paths".into(),
                    version: "1.0.0".into(),
                    kind: ModKind::Content as i32,
                    ..Default::default()
                }],
                players: 3,
                world_fs_used_mb: 1200,
                world_fs_total_mb: 500_000,
                ..Default::default()
            },
            Start {
                side: CLIENT,
                kind: Kind::Client as i32,
                trigger: Trigger::Ctl as i32,
                started_unix_ms: 1_794_580_324_250,
                machine: Some(Machine {
                    gpu: "AMD Radeon RX 7800 XT".into(),
                    ..Default::default()
                }),
                options: Some(Options {
                    hz: 250,
                    gpu: GpuMode::Passes as i32,
                    timeline: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        strings: vec![
            StringEntry {
                side: SERVER,
                id: 1,
                text: "tick_lan".into(),
            },
            StringEntry {
                side: SERVER,
                id: 2,
                text: "streaming.cap".into(),
            },
            StringEntry {
                side: SERVER,
                id: 3,
                text: "kind".into(),
            },
            StringEntry {
                side: SERVER,
                id: 4,
                text: "light".into(),
            },
            StringEntry {
                side: CLIENT,
                id: 1,
                text: "render".into(),
            },
        ],
        history: vec![HealthWindow {
            side: SERVER,
            step_s: 1.0,
            t0: -3.0,
            samples: vec![
                HealthSample {
                    tps: Some(20.0),
                    mspt_avg: Some(12.5),
                    ..health(SERVER, -3.0)
                },
                HealthSample {
                    tps: Some(19.5),
                    mspt_avg: Some(48.25),
                    ..health(SERVER, -2.0)
                },
                HealthSample {
                    tps: Some(20.0),
                    mspt_avg: Some(11.0),
                    ..health(SERVER, -1.0)
                },
            ],
        }],
        slow_history: vec![SlowEvent {
            side: SERVER,
            t: -2.0,
            unit: Unit::Tick as i32,
            counter: 71_980,
            wall_ms: 260.0,
            cpu_ms: 250.5,
            top: vec![SpanCost {
                name: 1,
                wall_ms: 240.0,
                cpu_ms: 236.0,
                depth: 0,
            }],
        }],
        marks: vec![Mark {
            side: CLIENT,
            t: 1.5,
            name: "before boss".into(),
        }],
        warnings: vec![Warning {
            side: SERVER,
            t: 2.0,
            code: WarningCode::TimelineCapped as i32,
            args: vec![60.0, 268_435_456.0],
        }],
        health: vec![
            HealthSample {
                tps: Some(20.0),
                mspt_avg: Some(12.5),
                mspt_p50: Some(12.0),
                mspt_p95: Some(18.75),
                mspt_max: Some(31.0),
                cpu_process_cores: 2.5,
                cpu_system_cores: 6.25,
                rss_mb: 3072.0,
                physical_used_mb: 20_480.0,
                allocs_per_s: Some(1_250_000.0),
                players: 3,
                chunks: 4410,
                entities: 812,
                ..health(SERVER, 1.0)
            },
            HealthSample {
                fps: Some(144.0),
                frame_ms_p50: Some(6.875),
                frame_ms_p95: Some(9.5),
                frame_ms_p99: Some(12.25),
                frame_ms_max: Some(16.0),
                frame_cpu_ms_p50: Some(4.5),
                frame_gpu_ms_p50: Some(5.75),
                rss_mb: 2048.0,
                ..health(CLIENT, 1.0)
            },
        ],
        counter_defs: vec![CounterDef {
            side: SERVER,
            id: 1,
            name: 2,
            labels: vec![Label { key: 3, value: 4 }],
            unit: 0,
            kind: CounterKind::Sum as i32,
        }],
        counters: vec![CounterSample {
            side: SERVER,
            t: 1.0,
            ids: vec![1],
            values: vec![42.0],
        }],
        slow: vec![SlowEvent {
            side: SERVER,
            t: 2.0,
            unit: Unit::Tick as i32,
            counter: 72_001,
            wall_ms: 312.5,
            cpu_ms: 290.0,
            top: vec![
                SpanCost {
                    name: 1,
                    wall_ms: 300.0,
                    cpu_ms: 280.0,
                    depth: 0,
                },
                SpanCost {
                    name: 1,
                    wall_ms: 120.0,
                    cpu_ms: 118.0,
                    depth: 1,
                },
            ],
        }],
        stop: vec![
            Stop {
                side: CLIENT,
                t: 2.25,
                reason: StopReason::Shutdown as i32,
                overhead: Some(Overhead::default()),
            },
            Stop {
                side: SERVER,
                t: 2.5,
                reason: StopReason::Normal as i32,
                overhead: Some(Overhead {
                    spans_ms_per_tick: 0.0625,
                    spans_share: 0.005,
                    writer_core_share: 0.0013,
                    dropped_messages: 2,
                    timeline_stopped_t: Some(2.0),
                    ..Default::default()
                }),
            },
        ],
        ..Default::default()
    }
}

#[test]
fn the_frozen_v1_fixture_decodes_to_its_listed_values() {
    let file = include_bytes!("fixtures/v1.hzperf");
    assert_eq!(&file[..2], &[0x1f, 0x8b], "a gzip stream");
    let mut inflated = Vec::new();
    GzDecoder::new(&file[..])
        .read_to_end(&mut inflated)
        .unwrap();
    assert_eq!(Capture::decode(&inflated[..]).unwrap(), expected());
}
