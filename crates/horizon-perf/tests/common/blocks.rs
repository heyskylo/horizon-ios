//! Server captures of 5 s span blocks with a known jitter, for the diff and golden tests.

use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

use super::{SERVER, stop};

/// Per-tick jitter in ms; it sums to 0 so a block mean is the base value.
pub const JITTER: [f64; 12] = [
    0.0, 0.3, -0.2, 0.1, -0.3, 0.2, 0.15, -0.15, 0.25, -0.25, 0.05, -0.15,
];

pub fn jitter(i: usize, shift: usize) -> f64 {
    JITTER[(i + shift) % JITTER.len()]
}

const TICKS_PER_BLOCK: f64 = 100.0;

/// A server capture of 5 s blocks: `children[c]` ran `rows[i][c]` ms per tick in block i, under a root that adds 10 ms.
pub fn server(children: &[&str], rows: &[Vec<f64>], profile: &str) -> Vec<Field> {
    let mut names = vec!["tick".to_string()];
    names.extend(children.iter().map(|c| c.to_string()));
    let id = |name: &str| names.iter().position(|n| n == name).unwrap() as u32 + 1;
    let mut fields = vec![Field::Start(Start {
        side: SERVER,
        kind: Kind::Dedicated as i32,
        started_unix_ms: 1_794_580_324_000,
        build: Some(Build {
            profile: profile.into(),
            ..Default::default()
        }),
        ..Default::default()
    })];
    for (i, text) in names.iter().enumerate() {
        fields.push(Field::String(StringEntry {
            side: SERVER,
            id: i as u32 + 1,
            text: text.clone(),
        }));
    }
    for (block, row) in rows.iter().enumerate() {
        let root = row.iter().sum::<f64>() + 10.0;
        let per_block = |ms: f64| (ms * TICKS_PER_BLOCK) as f32;
        fields.push(Field::SpanBlock(SpanBlock {
            side: SERVER,
            thread: id("tick"),
            block: block as u32,
            t0: 5.0 * block as f64,
            block_s: 5.0,
            units: TICKS_PER_BLOCK as u32,
            span: std::iter::once(id("tick"))
                .chain(children.iter().map(|c| id(c)))
                .collect(),
            parent: std::iter::once(0)
                .chain(children.iter().map(|_| id("tick")))
                .collect(),
            wall_ms: std::iter::once(per_block(root))
                .chain(row.iter().map(|&v| per_block(v)))
                .collect(),
            cpu_ms: std::iter::once(per_block(root))
                .chain(row.iter().map(|&v| per_block(v)))
                .collect(),
            seen: vec![TICKS_PER_BLOCK as u32; children.len() + 1],
        }));
        for second in 0..5 {
            fields.push(Field::Health(HealthSample {
                side: SERVER,
                t: 5.0 * block as f64 + f64::from(second),
                tps: Some(20.0),
                mspt_avg: Some(root as f32),
                mspt_p50: Some(root as f32),
                mspt_p95: Some(root as f32 + 3.0),
                ..Default::default()
            }));
        }
    }
    fields.push(stop(SERVER, 5.0 * rows.len() as f64));
    fields
}

pub const CHILDREN: [&str; 3] = ["tick_lan", "streaming", "spawn_natural"];

/// The baseline run: tick_lan 40 ms, streaming 6 ms, spawn_natural 4 ms, each with its own jitter.
pub fn steady(blocks: usize, shift: usize) -> Vec<Vec<f64>> {
    (0..blocks)
        .map(|i| {
            vec![
                40.0 + jitter(i, shift),
                6.0 + jitter(i, shift + 3),
                4.0 + jitter(i, shift + 7),
            ]
        })
        .collect()
}
