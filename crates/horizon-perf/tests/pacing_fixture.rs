mod common;

use std::path::PathBuf;

use common::{CLIENT, query, start, stop, tick_rows, write_capture};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

const NAMES: [&str; 5] = ["update_world", "render", "chunk_upload", "tick", "submit"];

fn text(name: &str) -> u32 {
    NAMES.iter().position(|n| *n == name).unwrap() as u32 + 1
}

fn slow(counter: u64, t: f64, wall_ms: f32, cpu_ms: f32, top: &[(&str, f32, f32, u32)]) -> Field {
    let Field::SlowHistory(event) = history(counter, t, wall_ms, cpu_ms, top) else {
        unreachable!()
    };
    Field::Slow(event)
}

fn history(
    counter: u64,
    t: f64,
    wall_ms: f32,
    cpu_ms: f32,
    top: &[(&str, f32, f32, u32)],
) -> Field {
    Field::SlowHistory(SlowEvent {
        side: CLIENT,
        t,
        unit: Unit::Frame as i32,
        counter,
        wall_ms,
        cpu_ms,
        top: top
            .iter()
            .map(|&(name, wall_ms, cpu_ms, depth)| SpanCost {
                name: text(name),
                wall_ms,
                cpu_ms,
                depth,
            })
            .collect(),
    })
}

/// A client capture of five slow frames and one from before it whose spans are chosen by hand, the one `tools/pacing_bench.py`'s test reads.
fn fields() -> Vec<Field> {
    let mut fields = vec![start(CLIENT, Kind::Client)];
    for (i, name) in NAMES.iter().enumerate() {
        fields.push(Field::String(StringEntry {
            side: CLIENT,
            id: i as u32 + 1,
            text: (*name).into(),
        }));
    }
    fields.extend([
        history(
            30,
            -20.0,
            300.0,
            280.0,
            &[("update_world", 290.0, 270.0, 0)],
        ),
        slow(
            120,
            1.0,
            45.0,
            12.0,
            &[("render", 40.0, 10.0, 0), ("chunk_upload", 25.0, 6.0, 1)],
        ),
        slow(
            340,
            2.5,
            130.0,
            18.0,
            &[("submit", 110.0, 1.0, 0), ("tick", 12.0, 11.0, 0)],
        ),
        slow(
            410,
            4.0,
            62.5,
            60.0,
            &[("tick", 50.0, 49.0, 0), ("update_world", 8.0, 8.0, 0)],
        ),
        slow(
            500,
            6.0,
            21.0,
            5.0,
            &[("render", 15.0, 4.0, 0), ("update_world", 0.25, 0.25, 0)],
        ),
        slow(520, 7.0, 15.0, 5.0, &[("render", 12.0, 4.0, 0)]),
    ]);
    fields.push(stop(CLIENT, 10.0));
    fields
}

fn committed() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/pacing_slow_frames.hzperf")
}

#[test]
#[ignore = "rewrites tests/fixtures/pacing_slow_frames.hzperf"]
fn write_the_pacing_fixture() {
    std::fs::write(committed(), write_capture(&fields()).bytes).unwrap();
}

#[test]
fn the_committed_pacing_fixture_lists_its_slow_frames_slowest_first_with_the_ring_from_before_the_capture()
 {
    let path = committed();
    let reply = query(&[
        "ticks",
        path.to_str().unwrap(),
        "--side",
        "client",
        "--worst",
        "1000",
    ]);
    let frames: Vec<_> = tick_rows(&reply)
        .iter()
        .map(|t| (t.id.as_str(), t.wall_ms, t.cpu_ms))
        .collect();
    assert_eq!(
        frames,
        [
            ("k:30", 300.0, 280.0),
            ("k:340", 130.0, 18.0),
            ("k:410", 62.5, 60.0),
            ("k:120", 45.0, 12.0),
            ("k:500", 21.0, 5.0),
            ("k:520", 15.0, 5.0),
        ]
    );
    let before: Vec<_> = tick_rows(&reply).iter().map(|t| t.before_capture).collect();
    assert_eq!(before, [true, false, false, false, false, false]);
    let spans: Vec<_> = tick_rows(&reply)[1]
        .spans
        .iter()
        .map(|s| (s.0.as_str(), s.1, s.2, s.3))
        .collect();
    assert_eq!(spans, [("submit", 0, 110.0, 1.0), ("tick", 0, 12.0, 11.0)]);
}
