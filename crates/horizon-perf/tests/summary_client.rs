mod common;

use common::{CLIENT, start, stop, write_capture};
use horizon_perf::{SummaryOptions, read_capture, summarize};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

fn string(id: u32, text: &str) -> Field {
    Field::String(StringEntry {
        side: CLIENT,
        id,
        text: text.into(),
    })
}

#[test]
fn a_client_summary_counts_frames_not_ticks() {
    let fields = vec![
        start(CLIENT, Kind::Client),
        string(1, "render"),
        string(2, "draw"),
        Field::Spans(SpanSummary {
            side: CLIENT,
            thread: 1,
            unit: Unit::Frame as i32,
            count: 100,
            spans: vec![SpanTotal {
                name: 2,
                parent: 0,
                depth: 0,
                wall_ms: 500.0,
                cpu_ms: 400.0,
                seen: 100,
            }],
            wall_ms_total: 1000.0,
            ..Default::default()
        }),
        stop(CLIENT, 5.0),
    ];
    let capture = read_capture(&write_capture(&fields).bytes).unwrap();
    let out = summarize(&capture, "run.hzperf", &SummaryOptions::default()).unwrap();
    assert!(out.contains("frame phases"), "{out}");
    assert!(out.contains("ms/frame") && out.contains("%frame"), "{out}");
    // 500 ms over 100 frames of a 10 ms mean frame: 5.0 ms, 50% of it, 4.0 ms cpu, 1.0 ms waited.
    let row = out
        .lines()
        .find(|l| l.trim_start().starts_with("draw"))
        .unwrap();
    assert_eq!(
        row.split_whitespace().collect::<Vec<_>>(),
        ["draw", "5.0", "50.0", "4.0", "1.0", "[s:draw]"]
    );
    assert!(out.lines().last().unwrap().contains("h:frame_ms_p95"));
}
