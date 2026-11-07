mod common;

use std::collections::BTreeMap;

use common::blocks::{CHILDREN, server, steady};
use common::{TempDir, save};
use horizon_bench::compare::{Ab, Paired, Verdict as PairVerdict};
use horizon_bench::{App, Frames, Hardware, Kind as BenchKind, Report};
use horizon_perf::bench_diff::{AbSection, BenchReply};
use horizon_perf::cli::{CliError, Env, run};
use horizon_perf::diff::{DiffEnvelope, DiffOptions, diff};
use horizon_perf::wire::{Role, Unit, Verdict};
use horizon_perf::{Baseline, SummaryOptions, read_capture, summarize};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

fn capture(fields: &[Field]) -> Capture {
    read_capture(&common::write_capture(fields).bytes).unwrap()
}

fn compare(a: &[Field], b: &[Field]) -> horizon_perf::diff::Diff {
    diff(&capture(a), &capture(b), &DiffOptions::default()).unwrap()
}

#[test]
fn a_capture_diffed_with_itself_is_within_noise_everywhere() {
    let fields = server(&CHILDREN, &steady(12, 0), "release");
    let d = compare(&fields, &fields);
    assert!(d.rows.len() >= 7, "{:?}", d.rows);
    for row in &d.rows {
        assert_eq!(row.verdict, Verdict::WithinNoise, "{row:?}");
        assert_eq!(row.effect, Some(0.0), "{row:?}");
        assert_eq!(row.blocks, [12, 12]);
    }
    assert_eq!(d.warnings, Vec::<String>::new());
}

#[test]
fn two_ms_more_on_one_span_is_slower_with_two_inside_the_interval_and_the_rest_unchanged() {
    let a = server(&CHILDREN, &steady(12, 0), "release");
    let mut rows_b = steady(12, 5);
    for row in &mut rows_b {
        row[0] += 2.0;
    }
    let d = compare(&a, &server(&CHILDREN, &rows_b, "release"));
    let lan = d.row("s:tick_lan").unwrap();
    assert_eq!(lan.verdict, Verdict::Slower);
    let [lo, hi] = lan.ci.unwrap();
    assert!(0.0 < lo && lo <= 2.0 && 2.0 <= hi, "CI [{lo}, {hi}]");
    assert!((lan.effect.unwrap() - 2.0).abs() < 1e-3, "{:?}", lan.effect);
    // The blocks' jitter has median 0.025, so the medians are 40.025 and 42.025.
    assert!((lan.a.unwrap() - 40.025).abs() < 1e-3 && (lan.b.unwrap() - 42.025).abs() < 1e-3);
    for id in ["s:streaming", "s:spawn_natural"] {
        let row = d.row(id).unwrap();
        assert_eq!(row.verdict, Verdict::WithinNoise, "{id}: {row:?}");
        assert!(row.ci.unwrap()[0] <= 0.0 && 0.0 <= row.ci.unwrap()[1]);
    }
    let text = d.text("a.hzperf", "b.hzperf");
    let line = |id: &str| {
        text.lines()
            .find(|l| l.starts_with(id))
            .unwrap()
            .to_string()
    };
    assert!(line("s:tick_lan").ends_with("slower"), "{text}");
    assert!(line("s:streaming").ends_with("~ within noise"), "{text}");
    assert!(
        text.starts_with(
            "A a.hzperf  B b.hzperf  (12 vs 12 blocks of 5 s, server tick thread, ms/tick)\n"
        ),
        "{text}"
    );
}

#[test]
fn a_drifting_baseline_raises_the_drift_warning_and_a_steady_one_does_not() {
    let drifting: Vec<Vec<f64>> = (0..12)
        .map(|i| vec![40.0 + 0.5 * i as f64, 6.0, 4.0])
        .collect();
    let a = server(&CHILDREN, &drifting, "release");
    let b = server(&CHILDREN, &steady(12, 4), "release");
    let d = compare(&a, &b);
    let drift: Vec<&String> = d.warnings.iter().filter(|w| w.contains("drifts")).collect();
    assert_eq!(drift.len(), 1, "{:?}", d.warnings);
    // 0.5 ms more per 5 s block is 0.1 ms/s on the MSPT.
    assert!(
        drift[0].starts_with("A's MSPT drifts upward over its window (Theil–Sen +0.100 ms/s)"),
        "{}",
        drift[0]
    );
    assert!(
        compare(&b, &a)
            .warnings
            .iter()
            .any(|w| w.starts_with("B's MSPT drifts upward"))
    );
    assert_eq!(compare(&b, &b).warnings, Vec::<String>::new());
}

#[test]
fn spans_on_one_side_only_are_new_or_gone_and_few_blocks_give_no_verdict() {
    let a = server(&CHILDREN, &steady(12, 0), "release");
    let renamed = ["tick_lan", "streaming", "generate"];
    let d = compare(&a, &server(&renamed, &steady(12, 0), "release"));
    assert_eq!(d.row("s:spawn_natural").unwrap().verdict, Verdict::Gone);
    assert_eq!(d.row("s:generate").unwrap().verdict, Verdict::New);
    assert_eq!(d.row("s:generate").unwrap().effect, None);

    let few = compare(
        &server(&CHILDREN, &steady(5, 0), "release"),
        &server(&CHILDREN, &steady(5, 0), "release"),
    );
    assert!(few.rows.iter().all(|r| r.verdict == Verdict::TooFewBlocks));
    assert!(few.text("a", "b").contains("? too few blocks"));
}

#[test]
fn more_than_32_blocks_merge_into_longer_ones_with_the_same_values() {
    let d = compare(
        &server(&CHILDREN, &steady(40, 0), "release"),
        &server(&CHILDREN, &steady(40, 0), "release"),
    );
    assert_eq!(d.blocks, [20, 20]);
    assert_eq!(d.block_s, 10.0);
    let lan = d.row("s:tick_lan").unwrap();
    assert_eq!(lan.blocks, [20, 20]);
    // Pairs of blocks sum their wall time and ticks, so the rate stays near 40 ms.
    assert!((lan.a.unwrap() - 40.0).abs() < 0.3, "{:?}", lan.a);
}

#[test]
fn min_ms_hides_small_rows_and_the_limit_says_how_many_it_cut() {
    let fields = server(&CHILDREN, &steady(12, 0), "release");
    let d = diff(
        &capture(&fields),
        &capture(&fields),
        &DiffOptions {
            min_ms: 5.0,
            ..Default::default()
        },
    )
    .unwrap();
    let ids: Vec<&str> = d.rows.iter().map(|r| r.id.as_str()).collect();
    assert!(
        ids.contains(&"s:streaming") && !ids.contains(&"s:spawn_natural"),
        "{ids:?}"
    );
    let limited = diff(
        &capture(&fields),
        &capture(&fields),
        &DiffOptions {
            limit: 4,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(limited.rows.len(), 4);
    assert!(
        limited
            .warnings
            .iter()
            .any(|w| w.starts_with("3 more rows")),
        "{:?}",
        limited.warnings
    );
}

#[test]
fn builds_of_different_kinds_are_called_out() {
    let rows = steady(12, 0);
    let d = compare(
        &server(&CHILDREN, &rows, "release"),
        &server(&CHILDREN, &rows, "dev"),
    );
    assert!(
        d.warnings
            .contains(&"A is a release build and B a dev build".to_string())
    );
}

#[test]
fn json_rows_have_the_documented_shape() {
    let a = server(&CHILDREN, &steady(12, 0), "release");
    let mut rows_b = steady(12, 5);
    for row in &mut rows_b {
        row[0] += 2.0;
    }
    let reply =
        compare(&a, &server(&CHILDREN, &rows_b, "release")).envelope("a.hzperf", "b.hzperf");
    assert!(reply.ok);
    assert_eq!(reply.unit, Some(Unit::MsPerTick));
    let row = reply.rows().iter().find(|r| r.id == "s:tick_lan").unwrap();
    assert_eq!(row.a, Some(40.025));
    assert_eq!(row.b, Some(42.025));
    assert_eq!(row.diff, Some(2.0));
    assert_eq!(row.diff_pct, Some(5.0));
    assert_eq!(row.blocks, [12, 12]);
    assert_eq!(row.verdict, Verdict::Slower);
    assert!(row.confidence.unwrap() >= 0.95);
    assert!(row.ci95.is_some());
}

fn summary_text(a: &[Field], b: &[Field], options: SummaryOptions) -> String {
    let base = Baseline {
        name: "before.hzperf".into(),
        capture: capture(a),
    };
    summarize(
        &capture(b),
        "after.hzperf",
        &SummaryOptions {
            baseline: Some(base),
            ..options
        },
    )
    .unwrap()
}

#[test]
fn summary_with_a_baseline_adds_deltas_and_verdicts_to_the_phase_table() {
    let a = server(&CHILDREN, &steady(12, 0), "release");
    let renamed = ["tick_lan", "streaming", "generate"];
    let mut rows_b = steady(12, 5);
    for row in &mut rows_b {
        row[0] += 2.0;
    }
    let text = summary_text(
        &a,
        &server(&renamed, &rows_b, "release"),
        SummaryOptions::default(),
    );
    assert!(
        text.contains("baseline: before.hzperf, same build kind, 12 vs 12 blocks of 5 s"),
        "{text}"
    );
    let words = |start: &str| -> Vec<String> {
        text.lines()
            .find(|l| l.trim_start().starts_with(start))
            .unwrap_or_else(|| panic!("no {start} in\n{text}"))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    assert_eq!(
        words("tick_lan"),
        ["tick_lan", "42.0", "+2.00", "slower", "[s:tick_lan]"]
    );
    assert_eq!(words("streaming")[3], "~");
    assert_eq!(words("generate")[3], "new");
    let gone = words("spawn_natural");
    assert_eq!((gone[1].as_str(), gone[3].as_str()), ("-", "gone"));
    assert!(gone[2].starts_with("-4.0"), "{gone:?}");
    assert!(text.contains("vs base h:mspt_avg"), "{text}");
}

#[test]
fn summary_baseline_of_another_build_kind_or_without_the_side_says_so() {
    let rows = steady(12, 0);
    let text = summary_text(
        &server(&CHILDREN, &rows, "dev"),
        &server(&CHILDREN, &rows, "release"),
        SummaryOptions::default(),
    );
    assert!(
        text.contains("different build kind (dev vs release)"),
        "{text}"
    );
    let client_only = vec![
        Field::Start(Start {
            side: Side::Client as i32,
            kind: Kind::Client as i32,
            ..Default::default()
        }),
        common::stop(Side::Client as i32, 1.0),
    ];
    let text = summary_text(
        &client_only,
        &server(&CHILDREN, &rows, "release"),
        SummaryOptions::default(),
    );
    assert!(
        text.contains("baseline: before.hzperf, cannot compare: capture A has no server side"),
        "{text}"
    );
}

fn env() -> Env {
    Env {
        horizon_home: None,
        poll: std::time::Duration::from_millis(2),
    }
}

fn cli(parts: &[&str]) -> Result<String, CliError> {
    let args: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    run(&args, &env(), &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn the_diff_and_summary_commands_read_files() {
    let dir = TempDir::new("diff-cli");
    let a = save(
        &dir,
        "a.hzperf",
        &server(&CHILDREN, &steady(12, 0), "release"),
    );
    let mut rows_b = steady(12, 5);
    for row in &mut rows_b {
        row[0] += 2.0;
    }
    let b = save(&dir, "b.hzperf", &server(&CHILDREN, &rows_b, "release"));
    let text = cli(&["diff", &a, &b, "--min-ms", "5"]).unwrap();
    assert!(
        text.starts_with("A a.hzperf  B b.hzperf  (12 vs 12 blocks of 5 s"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.starts_with("s:tick_lan") && l.ends_with("slower"))
    );
    assert!(!text.contains("s:spawn_natural"));
    let reply: DiffEnvelope =
        serde_json::from_str(&cli(&["diff", &a, &b, "--json"]).unwrap()).unwrap();
    assert_eq!(reply.captures[1].file, "b.hzperf");
    let summary = cli(&["summary", &b, "--baseline", &a]).unwrap();
    assert!(
        summary.contains("baseline: a.hzperf, same build kind"),
        "{summary}"
    );
    assert!(matches!(cli(&["diff", &a]), Err(CliError::Usage(_))));
    assert!(matches!(
        cli(&["diff", &a, &b, "--side", "client"]),
        Err(CliError::Failed(m)) if m.contains("capture A has no client side")
    ));
}

fn frames(phase: &str, values: &[f64], cpu: bool) -> Vec<Frames> {
    values
        .iter()
        .enumerate()
        .map(|(repeat, &p50)| Frames {
            phase: phase.into(),
            repeat: repeat as u32,
            frames: 600,
            seconds: 5.0,
            avg_fps: 100.0,
            low_1pct_fps: 80.0,
            low_01pct_fps: 70.0,
            p50_ms: p50,
            p95_ms: p50 * 1.5,
            p99_ms: p50 * 2.0,
            p999_ms: p50 * 3.0,
            max_ms: p50 * 4.0,
            cpu_p50_ms: (cpu && repeat % 2 == 0).then_some(p50 / 2.0),
            gpu_p50_ms: None,
            histogram: Vec::new(),
        })
        .collect()
}

fn report(frames: Vec<Frames>, ab: Option<Ab>) -> String {
    let app = App {
        name: "horizon".into(),
        version: "0.1".into(),
        ..Default::default()
    };
    let mut r = Report::with_hardware(BenchKind::Game, app, Hardware::default());
    r.frames = frames;
    r.ab = ab;
    r.to_json()
}

#[test]
fn benchmark_reports_with_an_ab_print_its_verdict_unchanged_and_never_recompute() {
    // The frames below are 2 ms apart and would be slower in any recomputation; the stored verdict says otherwise.
    let paired = |a, b, diff, diff_pct, ci95, confidence, noise, verdict| Paired {
        pairs: 4,
        a,
        b,
        diff,
        diff_pct,
        ci95,
        confidence,
        noise,
        verdict,
    };
    let ab = Ab {
        switch: "gpu-culling".into(),
        a: "off".into(),
        b: "on".into(),
        pairs: 4,
        block_s: 4.0,
        warmup_s: 2.0,
        first_block_outside_range: false,
        result: BTreeMap::from([
            (
                "gpu_p50_ms".to_string(),
                paired(
                    3.66,
                    5.63,
                    1.96,
                    Some(53.6),
                    Some([-0.5, 3.0]),
                    Some(0.875),
                    Some(0.1),
                    PairVerdict::WithinNoise,
                ),
            ),
            (
                "cpu_p50_ms".to_string(),
                paired(
                    2.0,
                    2.0,
                    0.0,
                    Some(0.0),
                    None,
                    None,
                    None,
                    PairVerdict::TooFewPairs,
                ),
            ),
        ]),
    };
    let dir = TempDir::new("bench-ab");
    let (a, b) = (dir.path().join("a.json"), dir.path().join("b.json"));
    std::fs::write(
        &a,
        report(frames("ab", &[10.0; 8], false), Some(ab.clone())),
    )
    .unwrap();
    std::fs::write(&b, report(frames("ab", &[12.0; 8], false), None)).unwrap();
    let (a, b) = (
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    );
    let text = cli(&["diff", &a, &b]).unwrap();
    let line = |metric: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with(metric))
            .unwrap()
            .to_string()
    };
    assert!(line("gpu_p50_ms").ends_with("within_noise"), "{text}");
    assert!(line("gpu_p50_ms").contains("CI [-0.5, 3.0]"), "{text}");
    assert!(
        line("cpu_p50_ms").ends_with("no CI  too_few_pairs"),
        "{text}"
    );
    assert!(!text.contains("bench:"), "no rows are recomputed:\n{text}");
    assert!(
        text.contains("warning: report B has no A/B result"),
        "{text}"
    );
    let reply: BenchReply =
        serde_json::from_str(&cli(&["diff", &a, &b, "--json"]).unwrap()).unwrap();
    assert_eq!(
        reply.ab,
        [AbSection {
            report: Role::A,
            ab
        }]
    );
}

#[test]
fn benchmark_reports_without_an_ab_compare_the_repeats_of_each_phase() {
    let dir = TempDir::new("bench-frames");
    let base = [10.0, 10.1, 9.9, 10.2, 9.8, 10.0, 10.1, 9.9];
    let slower: Vec<f64> = base.iter().map(|v| v + 2.0).collect();
    let a = dir.path().join("a.json");
    let b = dir.path().join("b.json");
    let mut first = frames("steady", &base, true);
    first.extend(frames("short", &[5.0, 5.0, 5.0], false));
    let mut second = frames("steady", &slower, true);
    second.extend(frames("short", &[8.0, 8.0, 8.0], false));
    std::fs::write(&a, report(first, None)).unwrap();
    std::fs::write(&b, report(second, None)).unwrap();
    let (a, b) = (
        a.to_string_lossy().into_owned(),
        b.to_string_lossy().into_owned(),
    );
    let reply: BenchReply =
        serde_json::from_str(&cli(&["diff", &a, &b, "--json"]).unwrap()).unwrap();
    let rows = &reply.rows;
    let find = |id: &str| {
        rows.iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("{id} in {reply:?}"))
    };
    assert_eq!(find("bench:steady.p50_ms").verdict, Verdict::Slower);
    assert_eq!(find("bench:steady.p50_ms").blocks, [8, 8]);
    assert_eq!(find("bench:steady.p95_ms").verdict, Verdict::Slower);
    assert_eq!(find("bench:short.p50_ms").verdict, Verdict::TooFewBlocks);
    assert!(
        rows.iter().all(|r| r.id != "bench:steady.cpu_p50_ms"),
        "cpu is on every other repeat only"
    );
}

#[test]
fn a_capture_and_a_report_cannot_be_compared_and_a_non_report_json_is_refused() {
    let dir = TempDir::new("bench-mixed");
    let capture_path = save(
        &dir,
        "a.hzperf",
        &server(&CHILDREN, &steady(12, 0), "release"),
    );
    let bench = dir.path().join("b.json");
    std::fs::write(&bench, report(frames("p", &[1.0; 6], false), None)).unwrap();
    let bench = bench.to_string_lossy().into_owned();
    assert!(matches!(
        cli(&["diff", &capture_path, &bench]),
        Err(CliError::Failed(m)) if m.contains("cannot compare a capture with a benchmark report")
    ));
    let other = dir.path().join("c.json");
    std::fs::write(&other, "{\"format\": \"something-else\"}").unwrap();
    let other = other.to_string_lossy().into_owned();
    assert!(matches!(
        cli(&["diff", &other, &other]),
        Err(CliError::Failed(m)) if m.contains("not a benchmark report")
    ));
    assert!(matches!(
        cli(&["summary", &capture_path, "--baseline", &bench]),
        Err(CliError::Failed(m)) if m.contains("needs a capture")
    ));
}
