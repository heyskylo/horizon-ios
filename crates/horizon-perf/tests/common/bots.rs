//! A 200-bot dedicated server capture of the size S1b will write: 120 s, 1152 ticks, 40 spans, 200 players.

use std::collections::HashMap;

use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

use super::{Rng, SERVER, stop};

const TICKS: u64 = 1152;
/// Top-level spans, then the children of level_overworld, as (name, parent, ms per tick, cpu ms per tick).
const SPANS: [(&str, &str, f64, f64); 8] = [
    ("tick_lan", "", 41.3, 40.0),
    ("level_overworld", "", 38.9, 38.6),
    ("tick_blocks", "level_overworld", 14.1, 14.0),
    ("tick_mobs", "level_overworld", 9.8, 9.8),
    ("streaming", "level_overworld", 6.2, 5.1),
    ("spawn_natural", "level_overworld", 3.9, 3.9),
    ("lan_waypoints", "", 12.4, 12.4),
    ("level_the_nether", "", 2.1, 2.1),
];
pub const MEAN_TICK_MS: f64 = 104.3;

struct Interner {
    ids: HashMap<String, u32>,
    order: Vec<String>,
}

impl Interner {
    fn id(&mut self, text: &str) -> u32 {
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        self.order.push(text.to_string());
        let id = self.order.len() as u32;
        self.ids.insert(text.to_string(), id);
        id
    }
}

fn span_names() -> Vec<(String, String, f64, f64)> {
    let mut spans: Vec<(String, String, f64, f64)> = SPANS
        .iter()
        .map(|s| (s.0.to_string(), s.1.to_string(), s.2, s.3))
        .collect();
    for i in 0..32 {
        let parent = if i % 2 == 0 {
            "tick_lan"
        } else {
            "level_overworld"
        };
        let ms = 0.9 - f64::from(i) * 0.02;
        spans.push((
            format!("child_span_{i:02}"),
            parent.to_string(),
            ms,
            ms * 0.9,
        ));
    }
    spans
}

pub fn capture() -> Vec<Field> {
    let mut n = Interner {
        ids: HashMap::new(),
        order: Vec::new(),
    };
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut body: Vec<Field> = Vec::new();
    let tick = n.id("tick");

    body.push(Field::Start(Start {
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
        machine: Some(Machine {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu_threads: 24,
            ..Default::default()
        }),
        players: 200,
        ..Default::default()
    }));

    for (tier, (step, t0, count)) in [
        (1.0f32, -600.0, 600usize),
        (10.0, -3600.0, 360),
        (60.0, -86_400.0, 1440),
        (900.0, -604_800.0, 672),
    ]
    .into_iter()
    .enumerate()
    {
        let samples = (0..count)
            .map(|_| HealthSample {
                tps: Some(9.0 + rng.below(30) as f32 / 10.0),
                mspt_avg: Some(100.0 + rng.below(40) as f32),
                cpu_process_cores: 3.0,
                rss_mb: 5600.0 + tier as f32 * 10.0,
                ..Default::default()
            })
            .collect();
        body.push(Field::History(HealthWindow {
            side: SERVER,
            step_s: step,
            t0,
            samples,
        }));
    }

    for t in 0..120 {
        body.push(Field::Health(HealthSample {
            side: SERVER,
            t: f64::from(t),
            tps: Some(9.6),
            mspt_avg: Some(104.3),
            mspt_p50: Some(98.7),
            mspt_p95: Some(141.3),
            mspt_max: Some(312.4),
            cpu_process_cores: 3.1,
            cpu_system_cores: 7.9,
            rss_mb: 5600.0 + f32::from(t as u8) * 1.8,
            players: 200,
            chunks: 61_842,
            entities: 9_310,
            ..Default::default()
        }));
    }

    let spans = span_names();
    let name_id = |n: &mut Interner, s: &str| if s.is_empty() { 0 } else { n.id(s) };
    let mut totals = Vec::new();
    for (name, parent, ms, cpu) in &spans {
        let depth = if parent.is_empty() { 0 } else { 1 };
        totals.push(SpanTotal {
            name: n.id(name),
            parent: name_id(&mut n, parent),
            depth,
            wall_ms: ms * TICKS as f64,
            cpu_ms: cpu * TICKS as f64,
            seen: TICKS,
        });
    }
    let worst: Vec<WorstUnit> = (0..16u32)
        .map(|i| {
            let wall = 312.4 - f64::from(i) * 9.0;
            WorstUnit {
                t: 5.0 + f64::from(i) * 7.0,
                counter: 47_000 + u64::from(i) * 13,
                wall_ms: wall as f32,
                cpu_ms: (wall * 0.93) as f32,
                spans: spans
                    .iter()
                    .take(10)
                    .enumerate()
                    .map(|(k, (name, parent, ..))| SpanCost {
                        name: n.id(name),
                        wall_ms: (wall / (k as f64 + 1.8)) as f32,
                        cpu_ms: (wall / (k as f64 + 2.2)) as f32,
                        depth: u32::from(!parent.is_empty()),
                    })
                    .collect(),
            }
        })
        .collect();
    body.push(Field::Spans(SpanSummary {
        timing: None,
        side: SERVER,
        thread: tick,
        unit: Unit::Tick as i32,
        count: TICKS,
        spans: totals,
        histogram: Some(Histogram {
            edges_ms: vec![25.0, 50.0, 100.0, 200.0],
            counts: vec![0, 0, 640, 480, 32],
        }),
        worst,
        wall_ms_total: MEAN_TICK_MS * TICKS as f64,
        cpu_ms_total: 98.0 * TICKS as f64,
    }));

    for block in 0..24u32 {
        let units = 48;
        body.push(Field::SpanBlock(SpanBlock {
            side: SERVER,
            thread: tick,
            block,
            t0: f64::from(block) * 5.0,
            block_s: 5.0,
            units,
            span: spans.iter().map(|s| n.id(&s.0)).collect(),
            parent: spans.iter().map(|s| name_id(&mut n, &s.1)).collect(),
            wall_ms: spans
                .iter()
                .map(|s| (s.2 * f64::from(units)) as f32)
                .collect(),
            cpu_ms: spans
                .iter()
                .map(|s| (s.3 * f64::from(units)) as f32)
                .collect(),
            seen: spans.iter().map(|_| units).collect(),
        }));
    }

    for i in 0..100u64 {
        body.push(Field::Slow(SlowEvent {
            side: SERVER,
            t: i as f64 * 1.1,
            unit: Unit::Tick as i32,
            counter: 40_000 + i,
            wall_ms: 250.0 + rng.below(60) as f32,
            cpu_ms: 200.0,
            top: spans
                .iter()
                .take(8)
                .map(|s| SpanCost {
                    name: n.id(&s.0),
                    wall_ms: 30.0,
                    cpu_ms: 25.0,
                    depth: u32::from(!s.1.is_empty()),
                })
                .collect(),
        }));
    }

    let players: Vec<PlayerTraffic> = (1..=200u32)
        .map(|index| PlayerTraffic {
            index,
            joined_t: 0.0,
            left_t: None,
            bytes_in: (0..24).map(|_| 1_000 + rng.below(8_000)).collect(),
            bytes_out: (0..24)
                .map(|_| 600_000 + rng.below(900_000) + u64::from(index) * 100)
                .collect(),
            packets_in: (0..24).map(|_| 100).collect(),
            packets_out: (0..24).map(|_| 1_500).collect(),
        })
        .collect();
    body.push(Field::Net(NetTraffic {
        side: SERVER,
        block_s: 5.0,
        players,
    }));

    for (label, t, scale) in [("start", 0.0, 1.0), ("stop", 120.0, 1.04)] {
        let mut rows: Vec<(String, f64)> = vec![("process.resident_mb".into(), 5600.0 * scale)];
        for tag in 0..60 {
            rows.push((
                format!("heap_by_tag_mb[tag {tag:02}]"),
                (2104.0 - f64::from(tag) * 30.0).max(1.0) * scale,
            ));
        }
        rows.push(("heap_by_tag_mb[chunk columns]".into(), 2104.0 * scale));
        rows.push(("client_columns.columns".into(), 61_000.0 * scale));
        body.push(Field::Memory(MemorySnapshot {
            side: SERVER,
            t,
            label: n.id(label),
            path: rows.iter().map(|r| n.id(&r.0)).collect(),
            value_mb: rows.iter().map(|r| r.1).collect(),
        }));
    }

    let types = [
        "zombie", "item", "villager", "skeleton", "creeper", "cow", "sheep", "pig",
    ];
    let dim = |n: &mut Interner, name: &str, chunks: u32| DimensionCounts {
        dimension: n.id(name),
        chunks,
        entities: types
            .iter()
            .map(|t| TypeCount {
                r#type: n.id(t),
                count: 100 + t.len() as u32,
            })
            .collect(),
        block_entities: vec![TypeCount {
            r#type: n.id("hopper"),
            count: 400,
        }],
    };
    body.push(Field::World(WorldSnapshot {
        side: SERVER,
        t: 120.0,
        label: n.id("stop"),
        dims: vec![
            dim(&mut n, "overworld", 58_000),
            dim(&mut n, "the_nether", 3_200),
            dim(&mut n, "the_end", 642),
        ],
        entity_costs: types
            .iter()
            .enumerate()
            .map(|(i, t)| EntityCost {
                dimension: n.id("overworld"),
                entity: n.id(t),
                ms_per_tick: 3.1 - i as f32 * 0.35,
                mobs_ticked_per_tick: 100.0,
                us_per_mob: 31.0,
            })
            .collect(),
    }));
    body.push(Field::Workers(Workers {
        side: SERVER,
        kinds: [
            ("generate", 2.31),
            ("light", 0.88),
            ("decorate", 0.52),
            ("mesh", 0.0),
        ]
        .iter()
        .map(|(k, c)| WorkerKind {
            kind: n.id(k),
            jobs: 500,
            cpu_cores: *c,
            ms_per_job: 4.0,
        })
        .collect(),
    }));
    body.push(Field::Mark(Mark {
        side: SERVER,
        t: 60.0,
        name: "bots joined".into(),
    }));
    body.push(Field::Warning(Warning {
        side: SERVER,
        t: 90.0,
        code: WarningCode::TimelineCapped as i32,
        args: vec![60.0, 268_435_456.0],
    }));
    let Field::Stop(mut end) = stop(SERVER, 120.0) else {
        unreachable!()
    };
    end.overhead = Some(Overhead {
        spans_ms_per_tick: 0.61,
        spans_share: 0.006,
        sampler_share_of_thread: 0.0005,
        writer_core_share: 0.0013,
        ..Default::default()
    });
    body.push(Field::Stop(end));

    let mut fields: Vec<Field> = Vec::new();
    fields.push(body.remove(0));
    fields.extend(n.order.iter().enumerate().map(|(i, text)| {
        Field::String(StringEntry {
            side: SERVER,
            id: i as u32 + 1,
            text: text.clone(),
        })
    }));
    fields.extend(body);
    fields
}
