mod common;

use common::hand::{NAMES, Names, capture};
use common::{SERVER, TempDir, query, rows, save, start, stop};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;
use serde_json::{Value, json};

fn hand_file(dir: &TempDir) -> String {
    save(dir, "hand.hzperf", &capture(&Names::new(&NAMES)))
}

fn row<'a>(reply: &'a Value, id: &str) -> &'a Value {
    rows(reply)
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {reply}"))
}

#[test]
fn series_resample_spans_health_and_counters_into_steps() {
    let dir = TempDir::new("series");
    let file = hand_file(&dir);
    let reply = query(&["series", "span:tick_lan", "h:players", "x:streaming.updates{kind=light}", "--every", "4", &file]);
    let rows = rows(&reply);
    // tick_lan blocks: 150 ms over 5 ticks (t0 0) and 250 over 5 (t0 5); a step of 4 s holds block 0 alone, then block 5.
    assert_eq!(rows[0]["field"], "s:tick_lan");
    assert_eq!(rows[0]["t"], json!([0.0, 4.0, 8.0]));
    assert_eq!(rows[0]["values"], json!([30.0, 50.0, null]));
    // Two players on every second of the capture.
    assert_eq!(rows[1]["field"], "h:players");
    assert_eq!(rows[1]["values"], json!([2.0, 2.0, 2.0]));
    // A running total 0, 100, 300, 600, 1000 at t = 0, 2, 4, 6, 8: (300 - 0) / 4 s and (1000 - 300) / 4 s; nothing ends in (8, 10].
    assert_eq!(rows[2]["field"], "x:streaming.updates{kind=light}");
    assert_eq!(rows[2]["unit"], "per_s");
    assert_eq!(rows[2]["values"], json!([75.0, 175.0, null]));
    // A bare name finds the same counter.
    let bare = query(&["series", "streaming.updates", "--every", "4", &file]);
    assert_eq!(bare["rows"][0]["values"], rows[2]["values"]);
}

#[test]
fn an_unknown_series_field_lists_the_health_fields() {
    let dir = TempDir::new("series-unknown");
    let reply = query(&["series", "h:nonsense", &hand_file(&dir)]);
    assert_eq!(reply["ok"], false);
    assert!(reply["candidates"].as_array().unwrap().iter().any(|c| c["id"] == "h:mspt_p95"));
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
            samples: (0..90).map(|i| health(if i < 45 { 100.0 } else { 200.0 })).collect(),
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
    let reply = query(&["series", "h:mspt_p95", "--from", "-900", "--every", "300", &file]);
    assert_eq!(reply["rows"][0]["t"], json!([-780.0, -480.0, -180.0]));
    // [-780, -480): 100 throughout. [-480, -180): 30 s of 100 and 270 s of 200. [-180, 120): 180 s of 200 and 120 s of live 50.
    assert_eq!(reply["rows"][0]["values"], json!([100.0, 190.0, 140.0]));
    let live_only = query(&["series", "h:mspt_p95", "--every", "60", &file]);
    assert_eq!(live_only["rows"][0]["values"], json!([50.0, 50.0]));
}

#[test]
fn world_ranks_dimensions_entities_and_sampled_costs() {
    let dir = TempDir::new("world");
    let file = hand_file(&dir);
    let dims = query(&["world", &file]);
    assert_eq!(
        rows(&dims),
        &vec![
            json!({"id": "d:overworld", "name": "overworld", "chunks": 100, "entities": 8, "block_entities": 2}),
            json!({"id": "d:the_nether", "name": "the_nether", "chunks": 10, "entities": 1, "block_entities": 0}),
        ]
    );
    let entities = query(&["world", "--by", "entity", &file]);
    assert_eq!(
        rows(&entities),
        &vec![
            json!({"id": "e:zombie", "name": "zombie", "count": 6}),
            json!({"id": "e:item", "name": "item", "count": 3}),
        ]
    );
    let nether = query(&["world", "--by", "entity", "--where", "dim=the_nether", &file]);
    assert_eq!(rows(&nether), &vec![json!({"id": "e:zombie", "name": "zombie", "count": 1})]);
    assert_eq!(rows(&query(&["world", "--by", "block_entity", &file]))[0]["id"], "b:hopper");
    let costs = query(&["world", "--by", "cost", &file]);
    let ranked: Vec<(String, f64)> = rows(&costs)
        .iter()
        .map(|r| (format!("{}@{}", r["id"].as_str().unwrap(), r["dimension"].as_str().unwrap()), r["ms_per_tick"].as_f64().unwrap()))
        .collect();
    assert_eq!(
        ranked,
        [("e:zombie@d:overworld".to_string(), 3.0), ("e:item@d:overworld".to_string(), 2.0), ("e:zombie@d:the_nether".to_string(), 0.5)]
    );
}

#[test]
fn net_gives_rates_per_player_over_the_time_each_was_on() {
    let dir = TempDir::new("net");
    let reply = query(&["net", &hand_file(&dir)]);
    let all = row(&reply, "n:all");
    // 10,240 + 20,480 + 5,120 bytes out in 10 s = 3.5 KiB/s; 1,024 + 1,024 + 512 in = 0.25 KiB/s.
    assert_eq!((all["out_bytes"].clone(), all["out_kib_s"].clone(), all["in_kib_s"].clone()), (json!(35_840), json!(3.5), json!(0.25)));
    // Player 1 was on for 10 s: 30,720 B = 3.0 KiB/s, peak block 20,480 B / 5 s = 4.0 KiB/s.
    let one = row(&reply, "n:p1");
    assert_eq!((one["out_kib_s"].clone(), one["in_kib_s"].clone(), one["peak_out_kib_s"].clone()), (json!(3.0), json!(0.2), json!(4.0)));
    assert_eq!(one["packets_out"], 300);
    // Player 2 joined at 5 s: 5,120 B in 5 s = 1.0 KiB/s.
    let two = row(&reply, "n:p2");
    assert_eq!((two["out_kib_s"].clone(), two["in_kib_s"].clone(), two["joined_t"].clone()), (json!(1.0), json!(0.1), json!(5.0)));
    let order: Vec<&str> = rows(&reply).iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(order, ["n:all", "n:p1", "n:p2"]);
    // Only the second block: player 1 moved 20,480 B in 5 s.
    let late = query(&["net", "--from", "5", &hand_file(&dir)]);
    assert_eq!(row(&late, "n:p1")["out_kib_s"], 4.0);
}

#[test]
fn meta_lists_each_side_with_its_start_stop_and_section_counts() {
    let dir = TempDir::new("meta");
    let reply = query(&["meta", &hand_file(&dir)]);
    let meta = &rows(&reply)[0];
    assert_eq!(meta["side"], "server");
    assert_eq!(meta["kind"], "dedicated");
    assert_eq!(meta["trigger"], "console");
    assert_eq!(meta["build"]["version"], "0.42.1");
    assert_eq!(meta["stop"]["reason"], "normal");
    assert_eq!(meta["seconds"], 10.0);
    assert_eq!(meta["sections"]["slow"], 3);
    assert_eq!(meta["sections"]["slow_history"], 1);
    assert_eq!(meta["sections"]["span_blocks"], 2);
    assert_eq!(meta["sections"]["health"], 10);
    assert_eq!(meta["sections"]["memory"], 2);
}

#[test]
fn marks_and_warnings_are_filtered_by_time_and_worded() {
    let dir = TempDir::new("marks");
    let file = hand_file(&dir);
    let marks = query(&["marks", &file]);
    let listed: Vec<(f64, &str)> = rows(&marks).iter().map(|m| (m["t"].as_f64().unwrap(), m["name"].as_str().unwrap())).collect();
    assert_eq!(listed, [(2.0, "warmup done"), (6.5, "after the fix")]);
    let after = query(&["marks", "--from", "mark:after the fix", &file]);
    assert_eq!(rows(&after).len(), 1);
    let unknown = query(&["marks", "--from", "mark:nope", &file]);
    assert_eq!(unknown["ok"], false);
    assert!(unknown["error"].as_str().unwrap().contains("warmup done"));

    let warnings = query(&["warnings", &file]);
    let w = &rows(&warnings)[0];
    assert_eq!(w["code"], "WARNING_CODE_TIMELINE_CAPPED");
    assert_eq!(w["text"], "the timeline hit its cap after 60 s (256.0 MiB written)");
}

#[test]
fn memory_lists_the_last_snapshot_and_diffs_two() {
    let dir = TempDir::new("memory");
    let file = hand_file(&dir);
    let last = query(&["memory", &file]);
    let paths: Vec<&str> = rows(&last).iter().map(|r| r["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["heap_by_tag_mb[chunk columns]", "process.resident_mb", "heap_by_tag_mb[lod]", "client_columns.columns"]);
    assert_eq!(rows(&last)[0]["id"], "m:heap_by_tag_mb[chunk columns]");
    let start = query(&["memory", "start", &file]);
    assert_eq!(rows(&start)[0]["value"], 2000.0);

    let diff = query(&["memory", "--diff", &file]);
    assert_eq!(
        rows(&diff)[0],
        json!({
            "from": "start", "to": "stop", "seconds": 10.0,
            "resident_mb": [1000.0, 1100.0, 100.0],
            "grew_mb": [["heap_by_tag_mb[chunk columns]", 2000.0, 2096.0, 96.0]],
            "shrank_mb": [],
            "counts": [["client_columns.columns", 100, 160, 60]],
            "only_in_a": [], "only_in_b": []
        })
    );
    assert_eq!(query(&["memory", "--diff", "start", "stop", &file])["rows"], diff["rows"]);
    let nope = query(&["memory", "--diff", "start", "later", &file]);
    assert_eq!(nope["ok"], false);
    assert!(nope["error"].as_str().unwrap().contains("start, stop"));
}

/// Spec D's documented `memory-diff` example, with snapshots that produce it.
#[test]
fn memory_diff_reproduces_spec_ds_documented_example() {
    let names = ["a", "b", "process.resident_mb", "heap_by_tag_mb[lod_reduce]", "renderer.cpu.copies_mb", "client_columns.columns", "heap_by_tag_mb[Atlas packing]"];
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
    let diff = query(&["memory", "--diff", "a", "b", &save(&dir, "d.hzperf", &fields)]);
    let row = &rows(&diff)[0];
    for key in ["resident_mb", "grew_mb", "shrank_mb", "counts", "only_in_a", "only_in_b"] {
        let expected = json!({
            "resident_mb": [1410.2, 1688.0, 277.8],
            "grew_mb": [["heap_by_tag_mb[lod_reduce]", 120.4, 410.2, 289.8]],
            "shrank_mb": [["renderer.cpu.copies_mb", 80.0, 0.0, -80.0]],
            "counts": [["client_columns.columns", 1089, 1200, 111]],
            "only_in_a": [],
            "only_in_b": ["heap_by_tag_mb[Atlas packing]"]
        });
        assert_eq!(row[key], expected[key], "{key}");
    }
    assert_eq!(row["seconds"], 612.4);
    let wide = query(&["memory", "--diff", "a", "b", "--min-mb", "500", &save(&dir, "d.hzperf", &fields)]);
    assert_eq!(rows(&wide)[0]["grew_mb"], json!([]));
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
        &["series", "span:streaming", "h:players", "streaming.updates", "--every", "5"],
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
        let mut with = |file: &str| {
            let mut args: Vec<&str> = parts.to_vec();
            args.push(file);
            query(&args)
        };
        let (x, y) = (with(&a), with(&b));
        assert!(!rows(&x).is_empty(), "{parts:?} gave no rows");
        assert_eq!(x["rows"], y["rows"], "{parts:?}");
    }
}

#[test]
fn a_sample_query_a_wrong_op_and_a_missing_section_are_errors_in_json() {
    let dir = TempDir::new("errors");
    let file = hand_file(&dir);
    let samples = query(&["hot", &file]);
    assert_eq!((samples["ok"].clone(), samples["op"].clone()), (json!(false), json!("hot")));
    assert!(samples["error"].as_str().unwrap().contains("samples"));
    assert_eq!(query(&["nonsense", &file])["ok"], false);
    let bare = save(&dir, "bare.hzperf", &[start(SERVER, Kind::Dedicated), stop(SERVER, 1.0)]);
    for op in ["world", "net", "memory", "spans"] {
        assert_eq!(query(&[op, &bare])["ok"], false, "{op}");
    }
    let client = query(&["spans", "--side", "client", &file]);
    assert_eq!(client["ok"], false);
    assert!(client["error"].as_str().unwrap().contains("no client side"));
}

#[test]
fn the_envelope_names_the_capture_the_selection_and_the_unit() {
    let dir = TempDir::new("envelope");
    let reply = query(&["spans", "--where", "span=streaming", "--where", "span=tick_lan", &hand_file(&dir)]);
    assert_eq!(reply["captures"][0]["file"], "hand.hzperf");
    assert_eq!(reply["captures"][0]["sides"], json!(["server"]));
    assert_eq!(reply["captures"][0]["kind"], "dedicated");
    assert_eq!(reply["captures"][0]["seconds"], 10.0);
    assert_eq!(reply["selection"]["side"], "server");
    assert_eq!(reply["selection"]["thread"], "tick");
    assert_eq!(reply["selection"]["where"], json!({"span": ["streaming", "tick_lan"]}));
    assert_eq!(reply["selection"]["to"], 10.0);
}
