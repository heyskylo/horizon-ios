mod common;

use common::hand::{NAMES, Names, capture};
use common::{
    SERVER, TempDir, cost_rows, count_rows, dimension_rows, mark_rows, memory_diff_rows,
    memory_rows, meta_rows, net_player, net_players, net_total, query, rows, save, series_rows,
    start, stop, warning_rows,
};
use horizon_perf::query::rows::{CountChange, CountRow, DimensionRow, MbChange};
use horizon_perf::wire::{
    Amount, ContextKey, KindName, MemoryUnit, OneOrMany, SeriesUnit, SideName, StopReasonName,
    TriggerName, Unit, WarningCodeName,
};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

fn hand_file(dir: &TempDir) -> String {
    save(dir, "hand.hzperf", &capture(&Names::new(&NAMES)))
}

#[test]
fn series_resample_spans_health_and_counters_into_steps() {
    let dir = TempDir::new("series");
    let file = hand_file(&dir);
    let reply = query(&[
        "series",
        "span:tick_lan",
        "h:players",
        "x:streaming.updates{kind=light}",
        "--every",
        "4",
        &file,
    ]);
    let rows = series_rows(&reply);
    // tick_lan blocks: 150 ms over 5 ticks (t0 0) and 250 over 5 (t0 5); a step of 4 s holds block 0 alone, then block 5.
    assert_eq!(rows[0].field, "s:tick_lan");
    assert_eq!(rows[0].t, [0.0, 4.0, 8.0]);
    assert_eq!(rows[0].values, [Some(30.0), Some(50.0), None]);
    // Two players on every second of the capture.
    assert_eq!(rows[1].field, "h:players");
    assert_eq!(rows[1].values, [Some(2.0), Some(2.0), Some(2.0)]);
    // A running total 0, 100, 300, 600, 1000 at t = 0, 2, 4, 6, 8: (300 - 0) / 4 s and (1000 - 300) / 4 s; nothing ends in (8, 10].
    assert_eq!(rows[2].field, "x:streaming.updates{kind=light}");
    assert_eq!(rows[2].unit, SeriesUnit::Counter("per_s".into()));
    assert_eq!(rows[2].values, [Some(75.0), Some(175.0), None]);
    // A bare name finds the same counter.
    let bare = query(&["series", "streaming.updates", "--every", "4", &file]);
    assert_eq!(series_rows(&bare)[0].values, rows[2].values);
}

#[test]
fn an_unknown_series_field_lists_the_health_fields() {
    let dir = TempDir::new("series-unknown");
    let reply = query(&["series", "h:nonsense", &hand_file(&dir)]);
    assert!(!reply.ok);
    assert!(reply.candidates.iter().any(|c| c.id == "h:mspt_p95"));
}

fn with_history() -> Vec<Field> {
    let health = |value: f32| HealthSample {
        mspt_p95: Some(value),
        ..Default::default()
    };
    let mut fields = vec![
        start(SERVER, Kind::Dedicated),
        Field::History(HealthWindow {
            side: SERVER,
            step_s: 10.0,
            t0: -900.0,
            samples: (0..90)
                .map(|i| health(if i < 45 { 100.0 } else { 200.0 }))
                .collect(),
        }),
    ];
    for t in 0..120 {
        fields.push(Field::Health(HealthSample {
            side: SERVER,
            t: f64::from(t),
            mspt_p95: Some(50.0),
            ..Default::default()
        }));
    }
    fields.push(stop(SERVER, 120.0));
    fields
}

#[test]
fn series_from_minus_900_reaches_into_the_history_tiers() {
    let dir = TempDir::new("series-history");
    let file = save(&dir, "history.hzperf", &with_history());
    // End is 120 s, so -900 starts at -780: three steps of 300 s.
    let reply = query(&[
        "series",
        "h:mspt_p95",
        "--from",
        "-900",
        "--every",
        "300",
        &file,
    ]);
    assert_eq!(series_rows(&reply)[0].t, [-780.0, -480.0, -180.0]);
    // [-780, -480): 100 throughout. [-480, -180): 30 s of 100 and 270 s of 200. [-180, 120): 180 s of 200 and 120 s of live 50.
    assert_eq!(
        series_rows(&reply)[0].values,
        [Some(100.0), Some(190.0), Some(140.0)]
    );
    let live_only = query(&["series", "h:mspt_p95", "--every", "60", &file]);
    assert_eq!(series_rows(&live_only)[0].values, [Some(50.0), Some(50.0)]);
}

fn dimension(
    id: &str,
    name: &str,
    chunks: u64,
    entities: u64,
    block_entities: u64,
) -> DimensionRow {
    DimensionRow {
        id: id.into(),
        name: name.into(),
        chunks,
        entities,
        block_entities,
    }
}

fn count(id: &str, name: &str, count: u64) -> CountRow {
    CountRow {
        id: id.into(),
        name: name.into(),
        count,
    }
}

#[test]
fn world_ranks_dimensions_entities_and_sampled_costs() {
    let dir = TempDir::new("world");
    let file = hand_file(&dir);
    let dims = query(&["world", &file]);
    assert_eq!(
        dimension_rows(&dims),
        [
            &dimension("d:overworld", "overworld", 100, 8, 2),
            &dimension("d:the_nether", "the_nether", 10, 1, 0),
        ]
    );
    assert_eq!(dims.unit, Some(Unit::Count));
    let entities = query(&["world", "--by", "entity", &file]);
    assert_eq!(
        count_rows(&entities),
        [&count("e:zombie", "zombie", 6), &count("e:item", "item", 3)]
    );
    let nether = query(&[
        "world",
        "--by",
        "entity",
        "--where",
        "dim=the_nether",
        &file,
    ]);
    assert_eq!(count_rows(&nether), [&count("e:zombie", "zombie", 1)]);
    assert_eq!(
        count_rows(&query(&["world", "--by", "block_entity", &file]))[0].id,
        "b:hopper"
    );
    let costs = query(&["world", "--by", "cost", &file]);
    assert_eq!(costs.unit, Some(Unit::MsPerTick));
    let ranked: Vec<(String, f64)> = cost_rows(&costs)
        .iter()
        .map(|r| (format!("{}@{}", r.id, r.dimension), r.ms_per_tick))
        .collect();
    assert_eq!(
        ranked,
        [
            ("e:zombie@d:overworld".to_string(), 3.0),
            ("e:item@d:overworld".to_string(), 2.0),
            ("e:zombie@d:the_nether".to_string(), 0.5)
        ]
    );
}

#[test]
fn net_gives_rates_per_player_over_the_time_each_was_on() {
    let dir = TempDir::new("net");
    let reply = query(&["net", &hand_file(&dir)]);
    let all = net_total(&reply);
    assert_eq!(all.id, "n:all");
    // 10,240 + 20,480 + 5,120 bytes out in 10 s = 3.5 KiB/s; 1,024 + 1,024 + 512 in = 0.25 KiB/s.
    assert_eq!(
        (all.out_bytes, all.out_kib_s, all.in_kib_s),
        (35_840, 3.5, 0.25)
    );
    // Player 1 was on for 10 s: 30,720 B = 3.0 KiB/s, peak block 20,480 B / 5 s = 4.0 KiB/s.
    let one = net_player(&reply, "n:p1");
    assert_eq!(
        (one.out_kib_s, one.in_kib_s, one.peak_out_kib_s),
        (3.0, 0.2, 4.0)
    );
    assert_eq!(one.packets_out, 300);
    // Player 2 joined at 5 s: 5,120 B in 5 s = 1.0 KiB/s.
    let two = net_player(&reply, "n:p2");
    assert_eq!((two.out_kib_s, two.in_kib_s, two.joined_t), (1.0, 0.1, 5.0));
    let order: Vec<&str> = net_players(&reply).iter().map(|p| p.id.as_str()).collect();
    assert_eq!(order, ["n:p1", "n:p2"]);
    // Only the second block: player 1 moved 20,480 B in 5 s.
    let late = query(&["net", "--from", "5", &hand_file(&dir)]);
    assert_eq!(net_player(&late, "n:p1").out_kib_s, 4.0);
}

#[test]
fn meta_lists_each_side_with_its_start_stop_and_section_counts() {
    let dir = TempDir::new("meta");
    let reply = query(&["meta", &hand_file(&dir)]);
    let meta = meta_rows(&reply)[0];
    let start = meta.start.as_ref().unwrap();
    assert_eq!(meta.side, SideName::Server);
    assert_eq!(start.kind, KindName::Dedicated);
    assert_eq!(start.trigger, TriggerName::Console);
    assert_eq!(start.build.as_ref().unwrap().version, "0.42.1");
    assert_eq!(meta.stop.as_ref().unwrap().reason, StopReasonName::Normal);
    assert_eq!(meta.seconds, 10.0);
    assert!(meta.timing.is_none());
    assert_eq!(meta.sections.slow, 3);
    assert_eq!(meta.sections.slow_history, 1);
    assert_eq!(meta.sections.span_blocks, 2);
    assert_eq!(meta.sections.health, 10);
    assert_eq!(meta.sections.memory, 2);
}

#[test]
fn marks_and_warnings_are_filtered_by_time_and_worded() {
    let dir = TempDir::new("marks");
    let file = hand_file(&dir);
    let marks = query(&["marks", &file]);
    let listed: Vec<(f64, &str)> = mark_rows(&marks)
        .iter()
        .map(|m| (m.t, m.name.as_str()))
        .collect();
    assert_eq!(listed, [(2.0, "warmup done"), (6.5, "after the fix")]);
    let after = query(&["marks", "--from", "mark:after the fix", &file]);
    assert_eq!(rows(&after).len(), 1);
    let unknown = query(&["marks", "--from", "mark:nope", &file]);
    assert!(!unknown.ok);
    assert!(unknown.error.unwrap().contains("warmup done"));

    let warnings = query(&["warnings", &file]);
    let w = warning_rows(&warnings)[0];
    assert_eq!(w.code, WarningCodeName::TimelineCapped);
    assert_eq!(
        w.text,
        "the timeline hit its cap after 60 s (256.0 MiB written)"
    );
}

#[test]
fn memory_lists_the_last_snapshot_and_diffs_two() {
    let dir = TempDir::new("memory");
    let file = hand_file(&dir);
    let last = query(&["memory", &file]);
    let paths: Vec<&str> = memory_rows(&last).iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "heap_by_tag_mb[chunk columns]",
            "process.resident_mb",
            "heap_by_tag_mb[lod]",
            "client_columns.columns"
        ]
    );
    assert_eq!(memory_rows(&last)[0].id, "m:heap_by_tag_mb[chunk columns]");
    assert_eq!(memory_rows(&last)[0].unit, MemoryUnit::Mb);
    assert_eq!(memory_rows(&last)[3].unit, MemoryUnit::Count);
    let start = query(&["memory", "start", &file]);
    assert_eq!(memory_rows(&start)[0].value, Amount::Float(2000.0));

    let diff = query(&["memory", "--diff", &file]);
    let row = memory_diff_rows(&diff)[0];
    assert_eq!((row.from.as_str(), row.to.as_str()), ("start", "stop"));
    assert_eq!(row.seconds, 10.0);
    assert_eq!(row.resident_mb, Some([1000.0, 1100.0, 100.0]));
    assert_eq!(
        row.grew_mb,
        [MbChange(
            "heap_by_tag_mb[chunk columns]".into(),
            2000.0,
            2096.0,
            96.0
        )]
    );
    assert_eq!(row.shrank_mb, []);
    assert_eq!(
        row.counts,
        [CountChange(
            "client_columns.columns".into(),
            Amount::Int(100),
            Amount::Int(160),
            Amount::Int(60)
        )]
    );
    assert!(row.only_in_a.is_empty() && row.only_in_b.is_empty());
    assert_eq!(
        query(&["memory", "--diff", "start", "stop", &file]).rows,
        diff.rows
    );
    let nope = query(&["memory", "--diff", "start", "later", &file]);
    assert!(!nope.ok);
    assert!(nope.error.unwrap().contains("start, stop"));
}

/// Spec D's documented `memory-diff` example, with snapshots that produce it.
#[test]
fn memory_diff_reproduces_spec_ds_documented_example() {
    let names = [
        "a",
        "b",
        "process.resident_mb",
        "heap_by_tag_mb[lod_reduce]",
        "renderer.cpu.copies_mb",
        "client_columns.columns",
        "heap_by_tag_mb[Atlas packing]",
    ];
    let n = common::hand::Names::new(&names);
    let snap = |label: &str, t: f64, rows: &[(&str, f64)]| {
        Field::Memory(MemorySnapshot {
            side: SERVER,
            t,
            label: n.id(label),
            path: rows.iter().map(|r| n.id(r.0)).collect(),
            value_mb: rows.iter().map(|r| r.1).collect(),
        })
    };
    let mut fields = vec![start(SERVER, Kind::Dedicated)];
    fields.extend(n.fields());
    fields.push(snap(
        "a",
        100.0,
        &[
            ("process.resident_mb", 1410.2),
            ("heap_by_tag_mb[lod_reduce]", 120.4),
            ("renderer.cpu.copies_mb", 80.0),
            ("client_columns.columns", 1089.0),
        ],
    ));
    fields.push(snap(
        "b",
        712.4,
        &[
            ("process.resident_mb", 1688.0),
            ("heap_by_tag_mb[lod_reduce]", 410.2),
            ("renderer.cpu.copies_mb", 0.0),
            ("client_columns.columns", 1200.0),
            ("heap_by_tag_mb[Atlas packing]", 0.5),
        ],
    ));
    fields.push(stop(SERVER, 713.0));
    let dir = TempDir::new("memory-d");
    let diff = query(&[
        "memory",
        "--diff",
        "a",
        "b",
        &save(&dir, "d.hzperf", &fields),
    ]);
    let row = memory_diff_rows(&diff)[0];
    assert_eq!(row.resident_mb, Some([1410.2, 1688.0, 277.8]));
    assert_eq!(
        row.grew_mb,
        [MbChange(
            "heap_by_tag_mb[lod_reduce]".into(),
            120.4,
            410.2,
            289.8
        )]
    );
    assert_eq!(
        row.shrank_mb,
        [MbChange("renderer.cpu.copies_mb".into(), 80.0, 0.0, -80.0)]
    );
    assert_eq!(
        row.counts,
        [CountChange(
            "client_columns.columns".into(),
            Amount::Int(1089),
            Amount::Int(1200),
            Amount::Int(111)
        )]
    );
    assert!(row.only_in_a.is_empty());
    assert_eq!(row.only_in_b, ["heap_by_tag_mb[Atlas packing]"]);
    assert_eq!(row.seconds, 612.4);
    let wide = query(&[
        "memory",
        "--diff",
        "a",
        "b",
        "--min-mb",
        "500",
        &save(&dir, "d.hzperf", &fields),
    ]);
    assert!(memory_diff_rows(&wide)[0].grew_mb.is_empty());
}

#[test]
fn ids_are_the_same_for_captures_whose_string_tables_are_in_a_different_order() {
    let dir = TempDir::new("ids");
    let forward = Names::new(&NAMES);
    let mut reversed_order: Vec<&str> = NAMES.to_vec();
    reversed_order.reverse();
    let backward = Names::new(&reversed_order);
    assert_ne!(forward.id("tick_lan"), backward.id("tick_lan"));
    let a = save(&dir, "a.hzperf", &capture(&forward));
    let b = save(&dir, "b.hzperf", &capture(&backward));
    let commands: [&[&str]; 14] = [
        &["spans"],
        &["spans", "--slow"],
        &["ticks"],
        &["tick", "k:101"],
        &[
            "series",
            "span:streaming",
            "h:players",
            "streaming.updates",
            "--every",
            "5",
        ],
        &["world"],
        &["world", "--by", "cost"],
        &["world", "--by", "entity"],
        &["memory"],
        &["memory", "--diff"],
        &["marks"],
        &["warnings"],
        &["net"],
        &["meta"],
    ];
    for parts in commands {
        let with = |file: &str| {
            let mut args: Vec<&str> = parts.to_vec();
            args.push(file);
            query(&args)
        };
        let (x, y) = (with(&a), with(&b));
        assert!(!rows(&x).is_empty(), "{parts:?} gave no rows");
        assert_eq!(x.rows, y.rows, "{parts:?}");
    }
}

#[test]
fn a_sample_query_a_wrong_op_and_a_missing_section_are_errors_in_json() {
    let dir = TempDir::new("errors");
    let file = hand_file(&dir);
    let samples = query(&["hot", &file]);
    assert!(!samples.ok);
    assert_eq!(
        samples.op,
        horizon_perf::wire::OpName::Query(horizon_perf::wire::Op::Hot)
    );
    assert!(samples.error.unwrap().contains("samples"));
    let wrong = query(&["nonsense", &file]);
    assert!(!wrong.ok);
    assert_eq!(
        wrong.op,
        horizon_perf::wire::OpName::Other("nonsense".into())
    );
    let bare = save(
        &dir,
        "bare.hzperf",
        &[start(SERVER, Kind::Dedicated), stop(SERVER, 1.0)],
    );
    for op in ["world", "net", "memory", "spans"] {
        assert!(!query(&[op, &bare]).ok, "{op}");
    }
    let client = query(&["spans", "--side", "client", &file]);
    assert!(!client.ok);
    assert!(client.error.unwrap().contains("no client side"));
}

#[test]
fn the_envelope_names_the_capture_the_selection_and_the_unit() {
    let dir = TempDir::new("envelope");
    let reply = query(&[
        "spans",
        "--where",
        "span=streaming",
        "--where",
        "span=tick_lan",
        &hand_file(&dir),
    ]);
    assert_eq!(reply.captures[0].file, "hand.hzperf");
    assert_eq!(reply.captures[0].sides, [SideName::Server]);
    assert_eq!(reply.captures[0].kind, KindName::Dedicated);
    assert_eq!(reply.captures[0].seconds, 10.0);
    let selection = reply.selection.unwrap();
    assert_eq!(selection.side, Some(SideName::Server));
    assert_eq!(selection.thread.as_deref(), Some("tick"));
    assert_eq!(
        selection.where_.get(&ContextKey::Span),
        Some(&OneOrMany::Many(vec![
            "streaming".into(),
            "tick_lan".into()
        ]))
    );
    assert_eq!(selection.to, Some(10.0));
}

#[test]
fn meta_exposes_capture_wide_tails_with_server_capacity_capped_at_twenty() {
    let dir = TempDir::new("timing");
    for (side, kind, expected) in [
        (SERVER, Kind::Dedicated, 20.0),
        (common::CLIENT, Kind::Client, 40.0),
    ] {
        let fields = vec![
            start(side, kind),
            Field::Spans(SpanSummary {
                side,
                thread: 1,
                count: 1000,
                timing: Some(TimingSummary {
                    count: 1000,
                    slowest_1pct_ms: 25.0,
                    slowest_01pct_ms: 100.0,
                }),
                ..Default::default()
            }),
            stop(side, 10.0),
        ];
        let file = save(&dir, &format!("timing-{side}.hzperf"), &fields);
        let reply = query(&["meta", &file]);
        let timing = meta_rows(&reply)[0].timing.as_ref().unwrap();
        assert_eq!(timing.count, 1000);
        assert_eq!(timing.low_1pct, expected);
        assert_eq!(timing.low_01pct, 10.0);
        assert_eq!(timing.slowest_1pct_ms, 25.0);
        assert_eq!(timing.slowest_01pct_ms, 100.0);
    }
}

#[test]
fn meta_never_reports_partial_or_invalid_tails_as_capture_wide_lows() {
    let dir = TempDir::new("invalid-timing");
    for (index, timing) in [
        TimingSummary {
            count: 999,
            slowest_1pct_ms: 10.0,
            slowest_01pct_ms: 100.0,
        },
        TimingSummary {
            count: 1000,
            slowest_1pct_ms: f64::NAN,
            slowest_01pct_ms: 100.0,
        },
        TimingSummary {
            count: 1000,
            slowest_1pct_ms: 10.0,
            slowest_01pct_ms: 0.0,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let fields = vec![
            start(SERVER, Kind::Dedicated),
            Field::Spans(SpanSummary {
                side: SERVER,
                thread: 1,
                count: 1000,
                timing: Some(timing),
                ..Default::default()
            }),
            stop(SERVER, 10.0),
        ];
        let file = save(&dir, &format!("invalid-{index}.hzperf"), &fields);
        let reply = query(&["meta", &file]);
        assert!(meta_rows(&reply)[0].timing.is_none());
    }
}
