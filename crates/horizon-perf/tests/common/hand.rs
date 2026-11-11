//! A small server capture whose every number is chosen so a test can do the arithmetic by hand.

use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

use super::{SERVER, stop};

pub const NAMES: [&str; 19] = [
    "tick",
    "tick_lan",
    "streaming",
    "spawn_natural",
    "overworld",
    "the_nether",
    "zombie",
    "item",
    "hopper",
    "generate",
    "light",
    "start",
    "stop",
    "process.resident_mb",
    "heap_by_tag_mb[chunk columns]",
    "heap_by_tag_mb[lod]",
    "columns.columns",
    "streaming.updates",
    "kind",
];

/// String ids assigned in the order given, from 1.
pub struct Names(pub Vec<String>);

impl Names {
    pub fn new(order: &[&str]) -> Self {
        Self(order.iter().map(|s| s.to_string()).collect())
    }

    pub fn id(&self, name: &str) -> u32 {
        self.0
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| panic!("unknown name {name}")) as u32
            + 1
    }

    pub fn fields(&self) -> Vec<Field> {
        self.0
            .iter()
            .enumerate()
            .map(|(i, text)| {
                Field::String(StringEntry {
                    side: SERVER,
                    id: i as u32 + 1,
                    text: text.clone(),
                })
            })
            .collect()
    }
}

fn cost(n: &Names, name: &str, wall_ms: f32, cpu_ms: f32, depth: u32) -> SpanCost {
    SpanCost {
        name: n.id(name),
        wall_ms,
        cpu_ms,
        depth,
    }
}

fn event(t: f64, counter: u64, wall_ms: f32, cpu_ms: f32, top: Vec<SpanCost>) -> SlowEvent {
    SlowEvent {
        side: SERVER,
        t,
        unit: Unit::Tick as i32,
        counter,
        wall_ms,
        cpu_ms,
        top,
    }
}

fn slow(t: f64, counter: u64, wall_ms: f32, cpu_ms: f32, top: Vec<SpanCost>) -> Field {
    Field::Slow(event(t, counter, wall_ms, cpu_ms, top))
}

fn block(n: &Names, index: u32, t0: f64, wall: [f32; 3], cpu: [f32; 3], seen: [u32; 3]) -> Field {
    let spans = ["tick_lan", "streaming", "spawn_natural"];
    Field::SpanBlock(SpanBlock {
        side: SERVER,
        thread: n.id("tick"),
        block: index,
        t0,
        block_s: 5.0,
        units: 5,
        span: spans.iter().map(|s| n.id(s)).collect(),
        parent: vec![0, n.id("tick_lan"), n.id("tick_lan")],
        wall_ms: wall.to_vec(),
        cpu_ms: cpu.to_vec(),
        seen: seen.to_vec(),
    })
}

fn snapshot(n: &Names, label: &str, t: f64, rows: &[(&str, f64)]) -> Field {
    Field::Memory(MemorySnapshot {
        side: SERVER,
        t,
        label: n.id(label),
        path: rows.iter().map(|r| n.id(r.0)).collect(),
        value_mb: rows.iter().map(|r| r.1).collect(),
    })
}

/// 10 s, 10 ticks of 50 ms. The slow ticks 100, 101 and 102 and the ring's tick 90 are the `slowticks.py` case.
pub fn capture(n: &Names) -> Vec<Field> {
    let mut f = vec![Field::Start(Start {
        side: SERVER,
        kind: Kind::Dedicated as i32,
        trigger: Trigger::Console as i32,
        started_unix_ms: 1_794_580_324_000,
        build: Some(Build {
            version: "0.42.1".into(),
            commit: "1d63e0a5a3".into(),
            profile: "release".into(),
            ..Default::default()
        }),
        ..Default::default()
    })];
    f.extend(n.fields());
    f.push(Field::SlowHistory(event(
        -30.0,
        90,
        120.0,
        100.0,
        vec![cost(n, "tick_lan", 120.0, 100.0, 0)],
    )));
    for t in 0..10 {
        f.push(Field::Health(HealthSample {
            side: SERVER,
            t: f64::from(t),
            tps: Some(20.0),
            mspt_p95: Some(60.0),
            players: 2,
            ..Default::default()
        }));
    }
    f.push(Field::CounterDef(CounterDef {
        side: SERVER,
        id: 1,
        name: n.id("streaming.updates"),
        labels: vec![Label {
            key: n.id("kind"),
            value: n.id("light"),
        }],
        unit: 0,
        kind: CounterKind::Sum as i32,
    }));
    for (t, total) in [
        (0.0, 0.0),
        (2.0, 100.0),
        (4.0, 300.0),
        (6.0, 600.0),
        (8.0, 1000.0),
    ] {
        f.push(Field::Counters(CounterSample {
            side: SERVER,
            t,
            ids: vec![1],
            values: vec![total],
        }));
    }
    f.push(Field::Mark(Mark {
        side: SERVER,
        t: 2.0,
        name: "warmup done".into(),
    }));
    f.push(Field::Mark(Mark {
        side: SERVER,
        t: 6.5,
        name: "after the fix".into(),
    }));
    f.push(block(
        n,
        0,
        0.0,
        [150.0, 60.0, 10.0],
        [140.0, 40.0, 10.0],
        [5, 5, 2],
    ));
    f.push(block(
        n,
        1,
        5.0,
        [250.0, 40.0, 10.0],
        [240.0, 20.0, 10.0],
        [5, 5, 2],
    ));
    f.push(slow(
        3.0,
        100,
        300.0,
        280.0,
        vec![
            cost(n, "tick_lan", 180.0, 170.0, 0),
            cost(n, "streaming", 60.0, 50.0, 1),
        ],
    ));
    f.push(slow(
        6.0,
        101,
        200.0,
        150.0,
        vec![
            cost(n, "tick_lan", 100.0, 90.0, 0),
            cost(n, "streaming", 40.0, 40.0, 1),
            cost(n, "spawn_natural", 30.0, 10.0, 1),
        ],
    ));
    f.push(slow(
        9.0,
        102,
        150.0,
        150.0,
        vec![cost(n, "tick_lan", 50.0, 50.0, 0)],
    ));
    f.push(Field::Warning(Warning {
        side: SERVER,
        t: 8.0,
        code: WarningCode::TimelineCapped as i32,
        args: vec![60.0, 268_435_456.0],
    }));
    f.push(snapshot(
        n,
        "start",
        0.0,
        &[
            ("process.resident_mb", 1000.0),
            ("heap_by_tag_mb[chunk columns]", 2000.0),
            ("heap_by_tag_mb[lod]", 400.0),
            ("columns.columns", 100.0),
        ],
    ));
    f.push(snapshot(
        n,
        "stop",
        10.0,
        &[
            ("process.resident_mb", 1100.0),
            ("heap_by_tag_mb[chunk columns]", 2096.0),
            ("heap_by_tag_mb[lod]", 400.0),
            ("columns.columns", 160.0),
        ],
    ));
    f.push(Field::Net(NetTraffic {
        side: SERVER,
        block_s: 5.0,
        players: vec![
            PlayerTraffic {
                index: 1,
                joined_t: 0.0,
                left_t: None,
                bytes_in: vec![1024, 1024],
                bytes_out: vec![10_240, 20_480],
                packets_in: vec![10, 10],
                packets_out: vec![100, 200],
                name: "Alex".into(),
                brand: "vanilla".into(),
                view_distance: 12,
            },
            PlayerTraffic {
                index: 2,
                joined_t: 5.0,
                left_t: None,
                bytes_in: vec![0, 512],
                bytes_out: vec![0, 5_120],
                packets_in: vec![0, 4],
                packets_out: vec![0, 50],
                ..PlayerTraffic::default()
            },
        ],
    }));
    let dim = |name: &str, chunks: u32, entities: &[(&str, u32)], blocks: &[(&str, u32)]| {
        DimensionCounts {
            dimension: n.id(name),
            chunks,
            entities: entities
                .iter()
                .map(|e| TypeCount {
                    r#type: n.id(e.0),
                    count: e.1,
                })
                .collect(),
            block_entities: blocks
                .iter()
                .map(|e| TypeCount {
                    r#type: n.id(e.0),
                    count: e.1,
                })
                .collect(),
        }
    };
    f.push(Field::World(WorldSnapshot {
        side: SERVER,
        t: 10.0,
        label: n.id("stop"),
        dims: vec![
            dim(
                "overworld",
                100,
                &[("zombie", 5), ("item", 3)],
                &[("hopper", 2)],
            ),
            dim("the_nether", 10, &[("zombie", 1)], &[]),
        ],
        entity_costs: vec![
            EntityCost {
                dimension: n.id("overworld"),
                entity: n.id("zombie"),
                ms_per_tick: 3.0,
                mobs_ticked_per_tick: 5.0,
                us_per_mob: 600.0,
            },
            EntityCost {
                dimension: n.id("the_nether"),
                entity: n.id("zombie"),
                ms_per_tick: 0.5,
                mobs_ticked_per_tick: 1.0,
                us_per_mob: 500.0,
            },
            EntityCost {
                dimension: n.id("overworld"),
                entity: n.id("item"),
                ms_per_tick: 2.0,
                mobs_ticked_per_tick: 3.0,
                us_per_mob: 666.0,
            },
        ],
    }));
    f.push(Field::Workers(Workers {
        side: SERVER,
        kinds: vec![
            WorkerKind {
                kind: n.id("light"),
                jobs: 10,
                cpu_cores: 0.5,
                ms_per_job: 2.0,
            },
            WorkerKind {
                kind: n.id("generate"),
                jobs: 20,
                cpu_cores: 2.0,
                ms_per_job: 4.0,
            },
        ],
    }));
    f.push(Field::Spans(SpanSummary {
        timing: None,
        side: SERVER,
        thread: n.id("tick"),
        unit: Unit::Tick as i32,
        count: 10,
        spans: vec![
            SpanTotal {
                name: n.id("tick_lan"),
                parent: 0,
                depth: 0,
                wall_ms: 400.0,
                cpu_ms: 380.0,
                seen: 10,
            },
            SpanTotal {
                name: n.id("streaming"),
                parent: n.id("tick_lan"),
                depth: 1,
                wall_ms: 100.0,
                cpu_ms: 60.0,
                seen: 10,
            },
            SpanTotal {
                name: n.id("spawn_natural"),
                parent: n.id("tick_lan"),
                depth: 1,
                wall_ms: 20.0,
                cpu_ms: 20.0,
                seen: 4,
            },
        ],
        histogram: Some(Histogram {
            edges_ms: vec![25.0, 50.0, 100.0],
            counts: vec![0, 6, 3, 1],
        }),
        worst: vec![
            WorstUnit {
                t: 4.0,
                counter: 7,
                wall_ms: 312.4,
                cpu_ms: 290.1,
                spans: vec![
                    cost(n, "tick", 312.4, 290.1, 0),
                    cost(n, "tick_lan", 180.2, 170.0, 1),
                    cost(n, "streaming", 61.0, 50.0, 1),
                ],
            },
            WorstUnit {
                t: 6.0,
                counter: 101,
                wall_ms: 200.0,
                cpu_ms: 150.0,
                spans: vec![
                    cost(n, "tick", 200.0, 150.0, 0),
                    cost(n, "tick_lan", 100.0, 90.0, 1),
                    cost(n, "streaming", 40.0, 40.0, 1),
                    cost(n, "spawn_natural", 30.0, 10.0, 1),
                ],
            },
        ],
        wall_ms_total: 500.0,
        cpu_ms_total: 460.0,
    }));
    f.push(stop(SERVER, 10.0));
    f
}
