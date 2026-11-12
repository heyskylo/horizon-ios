#![allow(dead_code)]

pub mod blocks;
pub mod bots;
pub mod hand;
pub mod samples;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use horizon_perf::Entry;
use horizon_perf::query::Row;
use horizon_perf::query::rows::{
    CostRow, CountRow, DimensionRow, FrameRow, HotRow, MarkRow, MemoryDiffRow, MemoryRow, MetaRow,
    NetPlayer, NetTotal, SeriesRow, SpanRow, SplitRow, TickRow, TreeRow, WarningRow,
};
use horizon_perf::wire::Envelope;
use horizon_perf_format::v1::*;
use horizon_perf_format::{CaptureWriter, Field};

pub const SERVER: i32 = Side::Server as i32;
pub const CLIENT: i32 = Side::Client as i32;

/// A `Write` whose bytes stay readable through a clone while a writer owns it.
#[derive(Clone, Default)]
pub struct Shared(pub Arc<Mutex<Vec<u8>>>);

impl Shared {
    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// xorshift64*: reproducible draws without a dependency.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A scratch folder removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "horizon-perf-test-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn start(side: i32, kind: Kind) -> Field {
    Field::Start(Start {
        side,
        kind: kind as i32,
        started_unix_ms: 1_794_580_324_000,
        ..Default::default()
    })
}

pub fn stop(side: i32, t: f64) -> Field {
    Field::Stop(Stop {
        side,
        t,
        reason: StopReason::Normal as i32,
        ..Default::default()
    })
}

pub fn health(side: i32, t: f64, tps: f32) -> Field {
    Field::Health(HealthSample {
        side,
        t,
        tps: Some(tps),
        ..Default::default()
    })
}

/// A two-sided capture with every kind of live field, the stops last, server first.
pub fn synthetic_fields() -> Vec<Field> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut fields = vec![start(SERVER, Kind::Dedicated), start(CLIENT, Kind::Client)];
    for i in 0..300u64 {
        let side = if i % 2 == 0 { SERVER } else { CLIENT };
        let t = i as f64 / 2.0;
        fields.push(match rng.below(6) {
            0 => Field::String(StringEntry {
                side,
                id: i as u32,
                text: format!("span.{i}.{}", rng.next()),
            }),
            1 => Field::Mark(Mark {
                side,
                t,
                name: format!("mark {}", rng.below(1000)),
            }),
            2 => Field::Warning(Warning {
                side,
                t,
                code: WarningCode::TimelineCapped as i32,
                args: vec![t, 1e6],
            }),
            3 => Field::Counters(CounterSample {
                side,
                t,
                ids: (0..8).collect(),
                values: (0..8).map(|_| rng.below(1 << 40) as f64).collect(),
            }),
            4 => Field::Slow(SlowEvent {
                side,
                t,
                unit: Unit::Tick as i32,
                counter: i,
                wall_ms: 250.0 + rng.below(500) as f32,
                top: (0..8)
                    .map(|d| SpanCost {
                        name: d,
                        wall_ms: 10.0,
                        cpu_ms: 9.0,
                        depth: d,
                    })
                    .collect(),
                ..Default::default()
            }),
            _ => health(side, t, 20.0 - rng.below(40) as f32 / 10.0),
        });
    }
    fields.push(stop(SERVER, 150.0));
    fields.push(stop(CLIENT, 150.0));
    fields
}

pub struct Written {
    pub bytes: Vec<u8>,
    /// The file length after the format field and after each field, each flushed.
    pub flushed_at: Vec<usize>,
}

pub fn write_capture(fields: &[Field]) -> Written {
    let out = Shared::default();
    let mut writer = CaptureWriter::new(out.clone()).unwrap();
    let mut flushed_at = vec![out.bytes().len()];
    for field in fields {
        writer.write(field.clone()).unwrap();
        flushed_at.push(out.bytes().len());
    }
    writer.finish().unwrap();
    Written {
        bytes: out.bytes(),
        flushed_at,
    }
}

pub fn expected_entries(fields: &[Field]) -> Vec<Entry> {
    std::iter::once(Entry::Format(1))
        .chain(fields.iter().cloned().map(Entry::Field))
        .collect()
}

/// Writes `fields` as a finished capture file and returns its path.
pub fn save(dir: &TempDir, name: &str, fields: &[Field]) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, write_capture(fields).bytes).unwrap();
    path.to_string_lossy().into_owned()
}

/// Runs `horizon-perf query ...` and returns the reply it printed, whether the query succeeded or not.
pub fn query(parts: &[&str]) -> Envelope<Row> {
    let env = horizon_perf::cli::Env {
        horizon_home: None,
        poll: std::time::Duration::from_millis(2),
    };
    let mut args = vec!["query".to_string()];
    args.extend(parts.iter().map(|s| s.to_string()));
    let mut out = Vec::new();
    let _ = horizon_perf::cli::run(&args, &env, &mut out);
    serde_json::from_slice(&out)
        .unwrap_or_else(|e| panic!("not a reply ({e}): {}", String::from_utf8_lossy(&out)))
}

/// The rows of a reply that succeeded.
pub fn rows(reply: &Envelope<Row>) -> &[Row] {
    assert!(reply.ok, "{reply:?}");
    reply.rows()
}

macro_rules! rows_of {
    ($name:ident, $variant:ident, $ty:ty) => {
        pub fn $name(reply: &Envelope<Row>) -> Vec<&$ty> {
            rows(reply)
                .iter()
                .map(|row| match row {
                    Row::$variant(row) => row,
                    other => panic!("not a {}: {other:?}", stringify!($variant)),
                })
                .collect()
        }
    };
}

rows_of!(span_rows, Span, SpanRow);
rows_of!(tick_rows, Tick, TickRow);
rows_of!(series_rows, Series, SeriesRow);
rows_of!(dimension_rows, Dimension, DimensionRow);
rows_of!(count_rows, Count, CountRow);
rows_of!(cost_rows, Cost, CostRow);
rows_of!(meta_rows, Meta, MetaRow);
rows_of!(memory_rows, Memory, MemoryRow);
rows_of!(memory_diff_rows, MemoryDiff, MemoryDiffRow);
rows_of!(warning_rows, Warning, WarningRow);
rows_of!(mark_rows, Mark, MarkRow);
rows_of!(hot_rows, Hot, HotRow);
rows_of!(tree_rows, Tree, TreeRow);
rows_of!(split_rows, Split, SplitRow);
rows_of!(frame_rows, Frame, FrameRow);

/// The row of every player together, which `net` lists first.
pub fn net_total(reply: &Envelope<Row>) -> &NetTotal {
    match &rows(reply)[0] {
        Row::NetTotal(total) => total,
        other => panic!("not the total: {other:?}"),
    }
}

/// The `net` rows of single players, in the order listed.
pub fn net_players(reply: &Envelope<Row>) -> Vec<&NetPlayer> {
    rows(reply)
        .iter()
        .filter_map(|row| match row {
            Row::NetPlayer(player) => Some(player),
            _ => None,
        })
        .collect()
}

pub fn net_player<'a>(reply: &'a Envelope<Row>, id: &str) -> &'a NetPlayer {
    net_players(reply)
        .into_iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("no row {id} in {reply:?}"))
}

/// The one row with this id.
pub fn span<'a>(reply: &'a Envelope<Row>, id: &str) -> &'a SpanRow {
    span_rows(reply)
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id} in {reply:?}"))
}

pub fn tick<'a>(reply: &'a Envelope<Row>, id: &str) -> &'a TickRow {
    tick_rows(reply)
        .into_iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("no row {id} in {reply:?}"))
}
