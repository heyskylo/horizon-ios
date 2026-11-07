mod common;

use common::hand::{NAMES, Names, capture};
use common::{bots, write_capture};
use horizon_perf::{Error, SummaryOptions, read_capture, summarize};
use horizon_perf_format::v1::Side;

const NAME: &str = "profile-20261107-143204Z.hzperf";

fn text(fields: &[horizon_perf_format::Field], options: &SummaryOptions) -> String {
    let capture = read_capture(&write_capture(fields).bytes).unwrap();
    summarize(&capture, NAME, options).unwrap()
}

fn budget(tokens: usize) -> SummaryOptions {
    SummaryOptions {
        budget_tokens: tokens,
        ..Default::default()
    }
}

fn tokens(text: &str) -> usize {
    text.len() / 4
}

/// The words of a row, so a test does not depend on column padding.
fn words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

fn line_starting<'a>(text: &'a str, start: &str) -> &'a str {
    text.lines()
        .find(|l| l.trim_start().starts_with(start))
        .unwrap_or_else(|| panic!("no line starting {start:?} in\n{text}"))
}

#[test]
fn the_bot_server_summary_has_every_section_and_the_spec_example_numbers() {
    let out = text(&bots::capture(), &SummaryOptions::default());
    assert!(tokens(&out) <= 1500, "{} tokens\n{out}", tokens(&out));
    assert!(
        tokens(&out) > 600,
        "the default is the full report: {} tokens",
        tokens(&out)
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        "profile-20261107-143204Z.hzperf: dedicated server, 120.0 s, Horizon 0.42.1 (1d63e0a5a3) release, Linux x86_64, 24 threads"
    );
    // The histogram's buckets above the 50 ms edge hold 640 + 480 + 32 of the 1,152 ticks.
    assert!(
        lines[1].contains("1,152 of 1,152 ticks over 50 ms"),
        "{}",
        lines[1]
    );

    // ms/tick, % of the mean 104.3 ms tick, cpu, waited, id: the §5.3 table.
    assert!(lines.iter().any(|l| l.starts_with("tick phases")));
    assert_eq!(
        words(line_starting(&out, "tick_lan")),
        ["tick_lan", "41.3", "39.6", "40.0", "1.3", "[s:tick_lan]"]
    );
    assert_eq!(
        words(line_starting(&out, "level_overworld")),
        [
            "level_overworld",
            "38.9",
            "37.3",
            "38.6",
            "0.3",
            "[s:level_overworld]"
        ]
    );
    assert_eq!(
        words(line_starting(&out, "tick_blocks")),
        [
            "tick_blocks",
            "14.1",
            "13.5",
            "14.0",
            "0.1",
            "[s:tick_blocks]"
        ]
    );
    // A child sits two spaces deeper than its parent.
    let indent =
        |name: &str| line_starting(&out, name).len() - line_starting(&out, name).trim_start().len();
    assert_eq!(indent("tick_blocks"), indent("level_overworld") + 2);

    // The worst tick is 312.4 ms at 5.0 s; its culprits are below the root span.
    let worst = line_starting(&out, "worst");
    assert!(worst.starts_with("worst   312.4 ms at 5.0 s ("), "{worst}");
    assert!(worst.ends_with("[k:47000]"), "{worst}");

    // 200 players: out = sum of the blocks / 120 s; the biggest sender is named.
    assert!(
        line_starting(&out, "net").contains("per player out p50"),
        "{out}"
    );
    assert!(line_starting(&out, "net").contains("max"), "{out}");
    // The biggest leaf of memory, and what it grew by over the window (2104 * 0.04 = +84).
    let memory = line_starting(&out, "memory");
    assert!(
        memory.starts_with("memory  chunk columns 2.1 GB (+84)"),
        "{memory}"
    );
    assert!(
        memory.contains("[m:heap_by_tag_mb[chunk columns]]"),
        "{memory}"
    );
    assert!(
        line_starting(&out, "world").starts_with("world   overworld 58,000 chunks 844 entities"),
        "{out}"
    );
    assert_eq!(
        line_starting(&out, "workers"),
        "workers generate 2.31 cores · light 0.88 · decorate 0.52 · mesh 0.00 (server)"
    );
    assert_eq!(
        line_starting(&out, "cost"),
        "cost    spans 0.61 ms/tick (0.60% of the tick thread) · sampler 0.05% of the tick thread · writer 0.13% of a core · 0 samples dropped"
    );
    assert!(
        line_starting(&out, "warn")
            .ends_with("the timeline hit its cap after 60 s (256.0 MiB written)")
    );
    assert!(line_starting(&out, "note").contains("no names, UUIDs, addresses, seed or world name"));
    let last = out.lines().last().unwrap();
    assert!(
        last.starts_with("next    horizon-perf query ticks --worst 5"),
        "{last}"
    );
    assert!(last.contains("s:tick_lan"), "{last}");
}

#[test]
fn a_budget_of_600_tokens_cuts_rows_from_the_bottom_and_keeps_the_rank_order() {
    let fields = bots::capture();
    let full = text(&fields, &SummaryOptions::default());
    let cut = text(&fields, &budget(600));
    assert!(tokens(&cut) <= 600, "{} tokens\n{cut}", tokens(&cut));
    assert!(cut.len() < full.len());

    // Every kept line is a whole line of the full report, in the same order.
    let full_lines: Vec<&str> = full.lines().collect();
    let mut at = 0;
    for line in cut.lines() {
        at += full_lines[at..]
            .iter()
            .position(|l| *l == line)
            .unwrap_or_else(|| panic!("{line:?} is not a later line of the full report"))
            + 1;
    }
    // The header and the last line always survive.
    assert_eq!(cut.lines().next(), full.lines().next());
    assert_eq!(cut.lines().last(), full.lines().last());

    // The phase rows left are the biggest ones, still in the table's order: nothing cut is bigger than anything kept.
    let phase_rows = |t: &str| -> Vec<String> {
        t.lines()
            .skip_while(|l| !l.starts_with("tick phases"))
            .skip(1)
            .take_while(|l| l.starts_with("  ") && l.contains("[s:"))
            .map(str::to_string)
            .collect()
    };
    let ms = |row: &String| -> f64 { words(row)[1].parse().unwrap() };
    let (all, kept) = (phase_rows(&full), phase_rows(&cut));
    assert!(
        !kept.is_empty() && kept.len() < all.len(),
        "{} of {}",
        kept.len(),
        all.len()
    );
    let smallest_kept = kept.iter().map(ms).fold(f64::MAX, f64::min);
    let biggest_cut = all
        .iter()
        .filter(|r| !kept.contains(r))
        .map(ms)
        .fold(0.0, f64::max);
    assert!(
        biggest_cut <= smallest_kept,
        "cut {biggest_cut} ms but kept {smallest_kept} ms"
    );
    // Top-level spans keep their order: biggest first.
    let top: Vec<f64> = kept
        .iter()
        .filter(|l| !l.starts_with("    "))
        .map(ms)
        .collect();
    assert!(top.windows(2).all(|w| w[0] >= w[1]), "{top:?}");
    // Every section keeps its first row at this budget.
    for section in ["worst", "net", "memory", "world", "workers", "cost"] {
        assert!(
            cut.lines().any(|l| l.starts_with(section)),
            "{section} missing from\n{cut}"
        );
    }
}

#[test]
fn the_slow_option_summarizes_the_slow_ticks_only() {
    let out = text(
        &capture(&Names::new(&NAMES)),
        &SummaryOptions {
            slow: true,
            ..Default::default()
        },
    );
    // Slow ticks at 3, 6 and 9 s: tick_lan averages 110 ms of a 216.7 ms slow tick, 103.3 of it cpu.
    assert!(out.contains("slow tick phases"), "{out}");
    assert_eq!(
        words(line_starting(&out, "tick_lan")),
        ["tick_lan", "110.0", "50.8", "103.3", "6.7", "[s:tick_lan]"]
    );
}

#[test]
fn a_thread_that_does_not_exist_is_refused_with_the_threads_there_are() {
    let capture = read_capture(&write_capture(&capture(&Names::new(&NAMES))).bytes).unwrap();
    let options = SummaryOptions {
        thread: Some("render".into()),
        ..Default::default()
    };
    match summarize(&capture, NAME, &options) {
        Err(Error::NoSuchThread { detail }) => assert!(detail.contains("tick"), "{detail}"),
        other => panic!("{other:?}"),
    }
    let client = SummaryOptions {
        side: Some(Side::Client),
        ..Default::default()
    };
    assert_eq!(
        summarize(&capture, NAME, &client),
        Err(Error::NoSuchSide { side: Side::Client })
    );
}
