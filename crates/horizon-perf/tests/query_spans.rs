mod common;

use common::hand::{NAMES, Names, capture};
use common::{TempDir, query, rows, save, span, span_rows, tick, tick_rows};
use horizon_perf::query::rows::TickSpanRow;
use horizon_perf::wire::{Candidate, Unit};
use horizon_perf_format::Field;

fn hand_file(dir: &TempDir) -> String {
    save(dir, "hand.hzperf", &capture(&Names::new(&NAMES)))
}

/// A single-capture row's `values`, `cpu`, `waited` and `seen`.
fn figures(row: &horizon_perf::query::rows::SpanRow) -> (f64, f64, f64, u64) {
    (
        row.values[0].unwrap(),
        row.cpu[0].unwrap(),
        row.waited[0].unwrap(),
        row.seen[0].unwrap(),
    )
}

#[test]
fn spans_without_a_window_read_the_summary_totals_per_tick() {
    let dir = TempDir::new("spans");
    let reply = query(&["spans", &hand_file(&dir)]);
    assert_eq!(reply.unit, Some(Unit::MsPerTick));
    assert_eq!(reply.selection.as_ref().unwrap().ticks, Some(10));
    // tick_lan: 400 ms wall, 380 cpu over 10 ticks.
    assert_eq!(figures(span(&reply, "s:tick_lan")), (40.0, 38.0, 2.0, 10));
    let streaming = span(&reply, "s:streaming");
    assert_eq!(
        (streaming.values[0], streaming.cpu[0], streaming.waited[0]),
        (Some(10.0), Some(6.0), Some(4.0))
    );
    assert_eq!(streaming.parent.as_deref(), Some("s:tick_lan"));
    assert_eq!(span(&reply, "s:spawn_natural").seen, [Some(4)]);
    let order: Vec<&str> = span_rows(&reply).iter().map(|r| r.name.as_str()).collect();
    assert_eq!(order, ["tick_lan", "streaming", "spawn_natural"]);
}

#[test]
fn a_time_window_sums_only_the_blocks_that_start_inside_it() {
    let dir = TempDir::new("spans-window");
    let file = hand_file(&dir);
    // Block 1 (t0 = 5, 5 ticks): tick_lan 250 ms wall, 240 cpu; streaming 40 / 20; spawn_natural 10 / 10 in 2 ticks.
    let late = query(&["spans", "--from", "5", &file]);
    let lan = span(&late, "s:tick_lan");
    assert_eq!(
        (lan.values[0], lan.cpu[0], lan.waited[0]),
        (Some(50.0), Some(48.0), Some(2.0))
    );
    let streaming = span(&late, "s:streaming");
    assert_eq!(
        (streaming.values[0], streaming.waited[0]),
        (Some(8.0), Some(4.0))
    );
    assert_eq!(span(&late, "s:spawn_natural").seen, [Some(2)]);
    assert_eq!(late.selection.as_ref().unwrap().from, Some(5.0));
    // Block 0 (t0 = 0): tick_lan 150 / 140 over 5 ticks.
    let early = query(&["spans", "--to", "5", &file]);
    let lan = span(&early, "s:tick_lan");
    assert_eq!((lan.values[0], lan.cpu[0]), (Some(30.0), Some(28.0)));
    // Negative bounds count back from the end: -5 is t = 5.
    assert_eq!(query(&["spans", "--from", "-5", &file]).rows, late.rows);
}

#[test]
fn slow_spans_give_the_wall_cpu_and_waited_of_slowticks_py_by_hand() {
    let dir = TempDir::new("spans-slow");
    let reply = query(&["spans", "--slow", &hand_file(&dir)]);
    assert_eq!(reply.unit, Some(Unit::MsPerSlowTick));
    // Slow ticks at t = 3, 6, 9; tick_lan wall (180 + 100 + 50) / 3, cpu (170 + 90 + 50) / 3.
    assert_eq!(
        figures(span(&reply, "s:tick_lan")),
        (110.0, 103.333, 6.667, 3)
    );
    // streaming wall (60 + 40) / 3, cpu (50 + 40) / 3, in 2 of the 3.
    assert_eq!(
        figures(span(&reply, "s:streaming")),
        (33.333, 30.0, 3.333, 2)
    );
    // spawn_natural wall 30 / 3, cpu 10 / 3, in 1 of the 3.
    assert_eq!(
        figures(span(&reply, "s:spawn_natural")),
        (10.0, 3.333, 6.667, 1)
    );
}

#[test]
fn a_negative_from_brings_the_slow_ring_from_before_the_capture_in() {
    let dir = TempDir::new("spans-ring");
    let reply = query(&["spans", "--slow", "--from", "-60", &hand_file(&dir)]);
    // Four slow ticks now: tick_lan wall (120 + 330) / 4, cpu (100 + 310) / 4; streaming wall 100 / 4.
    let lan = span(&reply, "s:tick_lan");
    assert_eq!(
        (lan.values[0], lan.cpu[0], lan.waited[0]),
        (Some(112.5), Some(102.5), Some(10.0))
    );
    assert_eq!(span(&reply, "s:streaming").values, [Some(25.0)]);
    // Only the last 2 s: the tick at t = 9.
    let late = query(&["spans", "--slow", "--from", "-2", &hand_file(&dir)]);
    assert_eq!(span(&late, "s:tick_lan").values, [Some(50.0)]);
}

#[test]
fn names_pick_spans_by_id_exact_or_unique_part_and_an_ambiguous_one_lists_candidates() {
    let dir = TempDir::new("spans-names");
    let file = hand_file(&dir);
    let by_part = query(&["spans", "--names", "stream", &file]);
    assert_eq!(span_rows(&by_part).len(), 1);
    assert_eq!(span_rows(&by_part)[0].id, "s:streaming");
    let two = query(&["spans", "--names", "s:spawn_natural,tick_lan", &file]);
    assert_eq!(rows(&two).len(), 2);

    let ambiguous = query(&["spans", "--names", "n", &file]);
    assert!(!ambiguous.ok);
    let ids: Vec<&str> = ambiguous.candidates.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["s:tick_lan", "s:streaming", "s:spawn_natural"]);

    let unknown = query(&["spans", "--names", "zzz", &file]);
    assert!(!unknown.ok);
    assert!(unknown.error.unwrap().contains("zzz"));
}

#[test]
fn several_captures_make_one_column_each_and_a_span_one_lacks_is_null() {
    let dir = TempDir::new("spans-columns");
    let first = hand_file(&dir);
    let mut fields = capture(&Names::new(&NAMES));
    for field in &mut fields {
        if let Field::Spans(summary) = field {
            summary.spans.truncate(1);
            summary.spans[0].wall_ms = 200.0;
            summary.spans[0].cpu_ms = 190.0;
        }
    }
    let second = save(&dir, "second.hzperf", &fields);
    let reply = query(&["spans", &first, &second]);
    assert_eq!(reply.captures.len(), 2);
    let lan = span(&reply, "s:tick_lan");
    assert_eq!(lan.values, [Some(40.0), Some(20.0)]);
    assert_eq!(lan.waited, [Some(2.0), Some(1.0)]);
    assert_eq!(span(&reply, "s:spawn_natural").values, [Some(2.0), None]);
    let refused = query(&["ticks", &first, &second]);
    assert!(!refused.ok);
}

#[test]
fn ticks_list_the_worst_with_their_trees_and_prefer_a_whole_tree_over_the_top_eight() {
    let dir = TempDir::new("ticks");
    let file = hand_file(&dir);
    let reply = query(&["ticks", &file]);
    let ids: Vec<&str> = tick_rows(&reply).iter().map(|r| r.id.as_str()).collect();
    // 312.4 (worst), 300, 200, 150, and the ring's 120 from before the capture.
    assert_eq!(ids, ["k:7", "k:100", "k:101", "k:102", "k:90"]);
    assert_eq!(
        tick_rows(&reply)[0].spans[1],
        TickSpanRow("tick_lan".into(), 1, 180.2, 170.0)
    );
    assert!(tick_rows(&reply)[0].complete);
    assert!(!tick(&reply, "k:100").complete);
    assert!(
        tick(&reply, "k:101").complete,
        "the worst list has tick 101's whole tree"
    );
    assert!(tick(&reply, "k:90").before_capture);

    let two = query(&["ticks", "--worst", "2", &file]);
    let ids: Vec<&str> = tick_rows(&two).iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["k:7", "k:100"]);
    let over = query(&["ticks", "--over", "160", &file]);
    assert_eq!(rows(&over).len(), 3);
    let inside = query(&["ticks", "--from", "0", &file]);
    assert_eq!(rows(&inside).len(), 4, "the ring's tick is before 0");
}

#[test]
fn one_tick_by_id_gives_its_span_tree_and_an_unknown_one_lists_the_worst() {
    let dir = TempDir::new("tick");
    let file = hand_file(&dir);
    let whole = query(&["tick", "k:101", &file]);
    let span = |name: &str, depth, wall, cpu| TickSpanRow(name.into(), depth, wall, cpu);
    assert_eq!(
        tick_rows(&whole)[0].spans,
        [
            span("tick", 0, 200.0, 150.0),
            span("tick_lan", 1, 100.0, 90.0),
            span("streaming", 1, 40.0, 40.0),
            span("spawn_natural", 1, 30.0, 10.0),
        ]
    );
    assert_eq!(whole.warnings, Some(Vec::new()));
    let top_eight = query(&["tick", "100", &file]);
    assert_eq!(tick_rows(&top_eight)[0].wall_ms, 300.0);
    assert_eq!(top_eight.warnings.unwrap().len(), 1);

    let missing = query(&["tick", "k:55", &file]);
    assert!(!missing.ok);
    assert_eq!(missing.candidates[0], Candidate::new("k:7", "312.4 ms"));
}
