mod common;

use common::samples::*;
use common::{TempDir, query, save, tick};

#[test]
fn a_fast_tick_reports_the_functions_of_the_second_around_it() {
    let dir = TempDir::new("tick-fast");
    let file = save(&dir, "ticks.hzperf", &waypoints_with_ticks());
    let reply = query(&["tick", "k:8", &file]);
    let row = tick(&reply, "k:8");
    // Block 0 (0 to 5 s) holds 85 CPU samples of 10 ms over 25 ticks.
    assert_eq!((row.samples_from, row.samples_to), (Some(0.0), Some(5.0)));
    let ranked: Vec<(&str, f64)> = row.hot.iter().map(|h| (h.id.as_str(), h.self_ms)).collect();
    assert_eq!(ranked[0], (LEVEL_ID, 10.0));
    assert_eq!(row.hot.len(), 5);
}

#[test]
fn a_slow_tick_keeps_only_slow_samples() {
    let dir = TempDir::new("tick-slow");
    let file = save(&dir, "ticks.hzperf", &waypoints_with_ticks());
    let reply = query(&["tick", "k:7", &file]);
    let row = tick(&reply, "k:7");
    let ids: Vec<&str> = row.hot.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, [UPDATE_ID]);
    assert_eq!(row.hot[0].samples, 10);
}

#[test]
fn a_tick_in_a_capture_without_samples_has_no_hot_rows() {
    let dir = TempDir::new("tick-none");
    let file = save(
        &dir,
        "hand.hzperf",
        &common::hand::capture(&common::hand::Names::new(&common::hand::NAMES)),
    );
    let reply = query(&["tick", "k:101", &file]);
    let row = tick(&reply, "k:101");
    assert!(row.hot.is_empty() && row.samples_from.is_none() && row.samples_to.is_none());
}
