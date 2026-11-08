mod common;

use common::samples::*;
use common::write_capture;
use horizon_perf::{SummaryOptions, read_capture, summarize};

fn summary(options: &SummaryOptions) -> String {
    let fields = waypoints(1, 50).fields();
    let capture = read_capture(&write_capture(&fields).bytes).unwrap();
    summarize(&capture, "run.hzperf", options).unwrap()
}

fn row(text: &str, start: &str) -> Vec<String> {
    text.lines()
        .find(|l| l.trim_start().starts_with(start))
        .unwrap_or_else(|| panic!("no row {start} in\n{text}"))
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[test]
fn hot_functions_list_self_and_total_per_tick_with_their_ids() {
    let text = summary(&SummaryOptions::default());
    assert!(
        text.contains("hot functions, tick thread, CPU, all ticks"),
        "{text}"
    );
    // 45 own and 90 inclusive samples of 10 ms over 50 ticks.
    assert_eq!(
        row(&text, "Waypoints::tick_level"),
        ["Waypoints::tick_level", "9.00", "18.00", "[f:yg3ymj0d]"]
    );
    assert_eq!(
        row(&text, "Hub::track_entities"),
        ["Hub::track_entities", "4.00", "4.00", "[f:bvz9skmm]"]
    );
    let order: Vec<usize> = [
        "Waypoints::tick_level",
        "Waypoints::update_or_create",
        "Hub::track_entities",
    ]
    .iter()
    .map(|n| text.find(n).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "ranked by self time");
}

#[test]
fn the_sampled_splits_follow_with_their_ids() {
    let text = summary(&SummaryOptions::default());
    assert!(
        text.contains(
            "by entity type (sampled ms/tick): zombie 13.0 [e:zombie] · item 5.0 [e:item]"
        ),
        "{text}"
    );
    assert!(
        text.contains("by chunk (sampled ms/tick): overworld/12,-4 10.0 [c:overworld/12,-4]"),
        "{text}"
    );
    assert!(!text.contains("by block type"), "no sample carried a block");
}

#[test]
fn slow_summaries_rank_the_slow_ticks_samples() {
    let text = summary(&SummaryOptions {
        slow: true,
        ..Default::default()
    });
    assert!(
        text.contains("hot functions, tick thread, CPU, slow ticks"),
        "{text}"
    );
    assert_eq!(
        row(&text, "Waypoints::update_or_create"),
        [
            "Waypoints::update_or_create",
            "20.00",
            "20.00",
            "[f:57zvmm9s]"
        ]
    );
}

#[test]
fn next_points_at_the_callers_of_the_hottest_function() {
    let text = summary(&SummaryOptions::default());
    let next = text.lines().last().unwrap();
    assert!(next.starts_with("next"), "{text}");
    assert!(
        next.contains("horizon-perf query callers f:yg3ymj0d"),
        "{next}"
    );
}

#[test]
fn a_small_budget_cuts_rows_from_the_bottom_and_keeps_the_last_line() {
    let full = summary(&SummaryOptions::default());
    let cut = summary(&SummaryOptions {
        budget_tokens: 90,
        ..Default::default()
    });
    assert!(cut.len() < full.len());
    assert_eq!(cut.lines().last(), full.lines().last());
    let rows = |t: &str| t.lines().filter(|l| l.contains("[f:")).count();
    assert!(rows(&cut) < rows(&full));
    assert!(
        cut.contains("Waypoints::tick_level"),
        "the top row survives"
    );
}
