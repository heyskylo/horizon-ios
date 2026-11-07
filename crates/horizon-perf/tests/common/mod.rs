#![allow(dead_code)]

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use horizon_perf::Entry;
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
