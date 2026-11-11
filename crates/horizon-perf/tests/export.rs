mod common;

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use common::samples::*;
use common::{CLIENT, SERVER, TempDir, save, start, stop};
use horizon_perf::cli::{CliError, Env, run};
use horizon_perf::export::perfetto::{MetaName, Phase, TraceEvent};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

fn env() -> Env {
    Env {
        horizon_home: None,
        poll: Duration::from_millis(2),
    }
}

fn export(parts: &[&str]) -> Result<Vec<u8>, CliError> {
    let mut args = vec!["export".to_string()];
    args.extend(parts.iter().map(|s| s.to_string()));
    let mut out = Vec::new();
    run(&args, &env(), &mut out).map(|()| out)
}

fn text(parts: &[&str]) -> String {
    String::from_utf8(export(parts).expect("export")).unwrap()
}

fn failure(parts: &[&str]) -> String {
    match export(parts) {
        Err(CliError::Failed(m) | CliError::Usage(m)) => m,
        Ok(out) => panic!("export succeeded: {}", String::from_utf8_lossy(&out)),
    }
}

fn read_varint(b: &[u8], at: &mut usize) -> u64 {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let byte = b[*at];
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return v;
        }
        shift += 7;
    }
}

/// Splits a message into (field, varint or bytes).
fn fields(b: &[u8]) -> Vec<(u64, u64, &[u8])> {
    let (mut at, mut out) = (0, Vec::new());
    while at < b.len() {
        let key = read_varint(b, &mut at);
        match key & 7 {
            0 => out.push((key >> 3, read_varint(b, &mut at), &b[0..0])),
            2 => {
                let len = read_varint(b, &mut at) as usize;
                out.push((key >> 3, 0, &b[at..at + len]));
                at += len;
            }
            wire => panic!("unexpected wire type {wire}"),
        }
    }
    out
}

fn packed(b: &[u8]) -> Vec<u64> {
    let (mut at, mut out) = (0, Vec::new());
    while at < b.len() {
        out.push(read_varint(b, &mut at));
    }
    out
}

struct Sample {
    locations: Vec<u64>,
    values: Vec<u64>,
    labels: Vec<(String, String)>,
}

struct Pprof {
    strings: Vec<String>,
    sample_types: Vec<(String, String)>,
    samples: Vec<Sample>,
    function_names: HashMap<u64, u64>,
    location_function: HashMap<u64, u64>,
    time_nanos: u64,
    duration_nanos: u64,
    period_type: (String, String),
    period: u64,
}

fn decode(gz: &[u8]) -> Pprof {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_end(&mut raw)
        .expect("gunzip");
    let top = fields(&raw);
    let strings: Vec<String> = top
        .iter()
        .filter(|f| f.0 == 6)
        .map(|f| String::from_utf8(f.2.to_vec()).unwrap())
        .collect();
    let s = |i: u64| strings[i as usize].clone();
    let value_type = |body: &[u8]| {
        let f = fields(body);
        (s(f[0].1), s(f[1].1))
    };
    let mut p = Pprof {
        sample_types: Vec::new(),
        samples: Vec::new(),
        function_names: HashMap::new(),
        location_function: HashMap::new(),
        time_nanos: 0,
        duration_nanos: 0,
        period_type: Default::default(),
        period: 0,
        strings: strings.clone(),
    };
    for (field, value, body) in top {
        match field {
            1 => p.sample_types.push(value_type(body)),
            2 => {
                let mut sample = Sample {
                    locations: vec![],
                    values: vec![],
                    labels: vec![],
                };
                for (field, _, b) in fields(body) {
                    match field {
                        1 => sample.locations = packed(b),
                        2 => sample.values = packed(b),
                        3 => {
                            let l = fields(b);
                            sample.labels.push((s(l[0].1), s(l[1].1)));
                        }
                        _ => {}
                    }
                }
                p.samples.push(sample);
            }
            5 => {
                let f = fields(body);
                p.function_names.insert(f[0].1, f[1].1);
            }
            4 => {
                let f = fields(body);
                let line = fields(f.iter().find(|x| x.0 == 4).unwrap().2);
                p.location_function.insert(f[0].1, line[0].1);
            }
            9 => p.time_nanos = value,
            10 => p.duration_nanos = value,
            11 => p.period_type = value_type(body),
            12 => p.period = value,
            _ => {}
        }
    }
    p
}

impl Pprof {
    /// The function names of a sample's stack, root first.
    fn stack(&self, sample: &Sample) -> Vec<&str> {
        let mut names: Vec<&str> = sample
            .locations
            .iter()
            .map(|l| {
                let function = self.location_function[l];
                self.strings[self.function_names[&function] as usize].as_str()
            })
            .collect();
        names.reverse();
        names
    }

    fn total(&self, column: usize) -> u64 {
        self.samples.iter().map(|s| s.values[column]).sum()
    }

    fn has(&self, sample: &Sample, key: &str, value: &str) -> bool {
        sample.labels.iter().any(|(k, v)| k == key && v == value)
    }
}

fn standard(dir: &TempDir) -> String {
    save(dir, "samples.hzperf", &waypoints(1, 50).fields())
}

#[test]
fn pprof_decodes_and_its_totals_are_the_captures() {
    let dir = TempDir::new("pprof");
    let path = standard(&dir);
    let p = decode(&export(&[&path, "--pprof"]).unwrap());

    let capture = horizon_perf::read_capture(&std::fs::read(&path).unwrap()).unwrap();
    let weights = capture.samples[0].weights.as_ref().unwrap();
    let in_capture = |kind: u32| -> u64 {
        (0..weights.node.len())
            .filter(|&i| weights.kind[i] == kind)
            .map(|i| weights.count[i])
            .sum()
    };
    assert_eq!(in_capture(0), 120);
    assert_eq!(in_capture(1), 20);
    let types: Vec<_> = p.sample_types.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(types, ["cpu", "wall"]);
    assert_eq!(p.total(0), in_capture(0));
    assert_eq!(p.total(1), in_capture(1));
    assert_eq!(p.period_type, ("cpu".into(), "nanoseconds".into()));
    assert_eq!(p.period, 10_000_000);
    assert_eq!(p.time_nanos, 1_794_580_324_000 * 1_000_000);
    assert_eq!(p.duration_nanos, 10_000_000_000);
}

#[test]
fn pprof_samples_carry_their_stack_and_context_as_labels() {
    let dir = TempDir::new("pprof-labels");
    let p = decode(&export(&[&standard(&dir), "--pprof"]).unwrap());

    let slow: Vec<&Sample> = p
        .samples
        .iter()
        .filter(|s| p.has(s, "slow", "true"))
        .collect();
    assert_eq!(slow.len(), 1);
    assert_eq!(slow[0].values, [10, 0]);
    assert_eq!(
        p.stack(slow[0]),
        [MAIN, SERVER_TICK, LAN, LEVEL, UPDATE],
        "innermost frame is the first location"
    );
    for (key, value) in [
        ("entity", "zombie"),
        ("dim", "overworld"),
        ("chunk", "12,-4"),
        ("span", "tick_lan"),
        ("side", "server"),
        ("thread", "tick"),
    ] {
        assert!(p.has(slow[0], key, value), "{:?}", slow[0].labels);
    }
    let waiting: Vec<&Sample> = p
        .samples
        .iter()
        .filter(|s| p.has(s, "state", "waiting"))
        .collect();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].values, [0, 8]);
    let running: u64 = p
        .samples
        .iter()
        .filter(|s| p.has(s, "state", "running"))
        .map(|s| s.values[1])
        .sum();
    assert_eq!(running, 12);
    let context_free = p
        .samples
        .iter()
        .find(|s| p.stack(s) == [MAIN, SERVER_TICK] && s.values[0] == 10)
        .expect("the server tick's own samples");
    assert!(
        context_free
            .labels
            .iter()
            .all(|(k, _)| k == "side" || k == "thread"),
        "{:?}",
        context_free.labels
    );
}

const RENDER: &str = "horizon_client::game::Game::render";

/// The standard server tree plus a client whose `LEVEL` is the same function and whose furnace samples carry a block.
fn both_sides(dir: &TempDir) -> String {
    let server = waypoints(1, 50);
    let mut client = Builder::new(30, &[15, 15]);
    client.side = CLIENT;
    client.thread = "render".into();
    for name in [MAIN, SERVER_TICK, LAN, LEVEL] {
        client.func(name);
    }
    let furnace = client.block_ctx("furnace", "overworld", "render_blocks");
    let main = client.node(None, MAIN);
    let render = client.node(Some(main), RENDER);
    let level = client.node(Some(render), LEVEL);
    client.weight(render, 7, W::default());
    client.weight(
        level,
        5,
        W {
            ctx: furnace,
            ..W::default()
        },
    );
    client.weight(
        level,
        3,
        W {
            ctx: furnace,
            state: 2,
            ..W::default()
        },
    );
    let mut fields = vec![start(SERVER, Kind::Dedicated), start(CLIENT, Kind::Client)];
    fields.extend(server.sections());
    fields.extend(client.sections());
    fields.extend([stop(SERVER, 10.0), stop(CLIENT, 10.0)]);
    save(dir, "both.hzperf", &fields)
}

fn side_total(p: &Pprof, side: &str, column: usize) -> u64 {
    p.samples
        .iter()
        .filter(|s| p.has(s, "side", side))
        .map(|s| s.values[column])
        .sum()
}

#[test]
fn pprof_of_both_sides_shares_functions_and_labels_each_sample_with_its_side() {
    let dir = TempDir::new("pprof-both");
    let p = decode(&export(&[&both_sides(&dir), "--pprof"]).unwrap());

    assert_eq!(
        (side_total(&p, "server", 0), side_total(&p, "server", 1)),
        (120, 20)
    );
    assert_eq!(
        (side_total(&p, "client", 0), side_total(&p, "client", 1)),
        (12, 3)
    );
    assert_eq!((p.total(0), p.total(1)), (132, 23));
    for s in &p.samples {
        let sides = s.labels.iter().filter(|(k, _)| k == "side").count();
        assert_eq!(sides, 1, "{:?}", s.labels);
    }

    let mut names: Vec<&str> = p
        .function_names
        .values()
        .map(|&i| p.strings[i as usize].as_str())
        .collect();
    names.sort_unstable();
    let distinct = names.len();
    names.dedup();
    assert_eq!(
        names.len(),
        distinct,
        "a function on both sides is one function"
    );
    assert_eq!(distinct, 9, "the server's eight and the client's render");

    let innermost = |side: &str, stack: &[&str]| -> u64 {
        let sample = p
            .samples
            .iter()
            .find(|s| p.has(s, "side", side) && p.stack(s) == stack)
            .expect("sample");
        sample.locations[0]
    };
    assert_eq!(
        innermost("server", &[MAIN, SERVER_TICK, LAN, LEVEL]),
        innermost("client", &[MAIN, RENDER, LEVEL]),
    );

    let furnace: Vec<&Sample> = p
        .samples
        .iter()
        .filter(|s| p.has(s, "block", "furnace"))
        .collect();
    assert_eq!(furnace.len(), 2);
    for s in &furnace {
        for (key, value) in [
            ("side", "client"),
            ("thread", "render"),
            ("dim", "overworld"),
            ("span", "render_blocks"),
        ] {
            assert!(p.has(s, key, value), "{:?}", s.labels);
        }
        assert_eq!(p.stack(s), [MAIN, RENDER, LEVEL]);
    }
    let mut values: Vec<_> = furnace.iter().map(|s| s.values.clone()).collect();
    values.sort();
    assert_eq!(values, [vec![0, 3], vec![5, 0]]);
    assert!(p.has(
        furnace.iter().find(|s| s.values[1] == 3).unwrap(),
        "state",
        "waiting"
    ));
    assert!(
        p.samples
            .iter()
            .filter(|s| p.has(s, "side", "server"))
            .all(|s| !s.labels.iter().any(|(k, _)| k == "block")),
        "the server's contexts name no block"
    );
}

#[test]
fn pprof_side_filter_keeps_one_side_and_its_functions() {
    let dir = TempDir::new("pprof-side");
    let path = both_sides(&dir);
    let p = decode(&export(&[&path, "--pprof", "--side", "client"]).unwrap());
    assert_eq!((p.total(0), p.total(1)), (12, 3));
    assert!(p.samples.iter().all(|s| p.has(s, "side", "client")));
    assert_eq!(p.function_names.len(), 3);
    let p = decode(&export(&[&path, "--pprof", "--side", "server"]).unwrap());
    assert_eq!((p.total(0), p.total(1)), (120, 20));
    assert!(p.samples.iter().all(|s| p.has(s, "side", "server")));
}

#[test]
fn folded_of_both_sides_starts_each_line_with_its_side() {
    let dir = TempDir::new("folded-both");
    let path = both_sides(&dir);
    let out = text(&[&path, "--folded"]);
    let sum = |prefix: &str| -> u64 {
        out.lines()
            .filter(|l| l.starts_with(prefix))
            .map(|l| l.rsplit_once(' ').unwrap().1.parse::<u64>().unwrap())
            .sum()
    };
    assert_eq!((sum("server;tick;"), sum("client;render;")), (120, 12));
    assert_eq!(out.lines().count(), 7 + 2);

    let client = format!("client;render;{MAIN};{RENDER}");
    let only = text(&[&path, "--folded", "--side", "client"]);
    assert_eq!(only, format!("{client} 7\n{client};{LEVEL} 5\n"));
    let wall = text(&[&path, "--folded", "--side", "client", "--select", "wall"]);
    assert_eq!(wall, format!("{client};{LEVEL};[waiting] 3\n"));
}

#[test]
fn pprof_period_keeps_sub_millisecond_periods_and_drops_conflicting_ones() {
    let dir = TempDir::new("pprof-period");
    let mut fine = waypoints(1, 50);
    fine.period_ms = 0.25;
    let p = decode(&export(&[&save(&dir, "fine.hzperf", &fine.fields()), "--pprof"]).unwrap());
    assert_eq!(p.period_type, ("cpu".into(), "nanoseconds".into()));
    assert_eq!(p.period, 250_000);

    let mut fields = waypoints(1, 50).fields();
    for field in &mut fields {
        if let Field::Samples(profile) = field {
            profile.kinds[1].period = 20.0;
        }
    }
    let p = decode(&export(&[&save(&dir, "mixed.hzperf", &fields), "--pprof"]).unwrap());
    assert_eq!((p.period_type, p.period), (Default::default(), 0));
}

#[test]
fn pprof_slow_keeps_only_slow_samples_and_thread_picks_one() {
    let dir = TempDir::new("pprof-slow");
    let path = standard(&dir);
    let p = decode(&export(&[&path, "--pprof", "--slow"]).unwrap());
    assert_eq!((p.total(0), p.total(1)), (10, 0));
    let p = decode(&export(&[&path, "--pprof", "--thread", "tick"]).unwrap());
    assert_eq!(p.total(0), 120);
    let message = failure(&[&path, "--pprof", "--thread", "render"]);
    assert!(message.contains("no thread named render") && message.contains("tick"));
}

#[test]
fn folded_lines_sum_to_the_samples_and_match_the_hand_folded_stacks() {
    let dir = TempDir::new("folded");
    let path = standard(&dir);
    let out = text(&[&path, "--folded"]);
    let stack = |frames: &[&str]| format!("server;tick;{}", frames.join(";"));
    let mut expected = vec![
        format!("{} 10", stack(&[MAIN, SERVER_TICK])),
        format!("{} 40", stack(&[MAIN, SERVER_TICK, LAN, LEVEL])),
        format!("{} 30", stack(&[MAIN, SERVER_TICK, LAN, LEVEL, UPDATE])),
        format!("{} 10", stack(&[MAIN, SERVER_TICK, LAN, LEVEL, TEAM])),
        format!("{} 20", stack(&[MAIN, SERVER_TICK, LAN, TRACK])),
        format!("{} 5", stack(&[MAIN, SERVER_TICK, BLOCKS, LEVEL])),
        format!("{} 5", stack(&[MAIN, SERVER_TICK, BLOCKS, LEVEL, UPDATE])),
    ];
    expected.sort();
    assert_eq!(out.lines().collect::<Vec<_>>(), expected);
    let counted: u64 = out
        .lines()
        .map(|l| l.rsplit_once(' ').unwrap().1.parse::<u64>().unwrap())
        .sum();
    assert_eq!(counted, 120);
}

#[test]
fn folded_selects_slow_wall_and_running_samples() {
    let dir = TempDir::new("folded-select");
    let path = standard(&dir);
    let level = format!("server;tick;{MAIN};{SERVER_TICK};{LAN};{LEVEL}");
    let slow = text(&[&path, "--folded", "--slow"]);
    assert_eq!(slow, format!("{level};{UPDATE} 10\n"));
    let wall = text(&[&path, "--folded", "--select", "wall"]);
    assert_eq!(wall, format!("{level} 12\n{level};[waiting] 8\n"));
    let running = text(&[&path, "--folded", "--select", "running"]);
    assert_eq!(running, format!("{level} 12\n"));
    assert!(failure(&[&path, "--folded", "--select", "alloc"]).contains("no alloc samples"));
    assert!(failure(&[&path, "--folded", "--select", "live"]).contains("no live samples"));
}

#[test]
fn exports_name_what_the_capture_lacks_and_demand_one_format() {
    let dir = TempDir::new("errors");
    let path = standard(&dir);
    assert!(failure(&[&path, "--perfetto"]).contains("no timeline"));
    assert!(failure(&[&path, "--folded", "--side", "client"]).contains("no client side"));
    assert!(failure(&[&path]).contains("exactly one of"));
    assert!(failure(&[&path, "--folded", "--pprof"]).contains("exactly one of"));
    let bare = save(
        &dir,
        "bare.hzperf",
        &[start(SERVER, Kind::Dedicated), stop(SERVER, 1.0)],
    );
    assert!(failure(&[&bare, "--pprof"]).contains("no samples"));
}

#[test]
fn out_writes_the_file_and_stdout_gets_the_summary_line() {
    let dir = TempDir::new("out");
    let path = standard(&dir);
    let target = dir.path().join("flame.folded");
    let target = target.to_string_lossy().into_owned();
    let said = text(&[&path, "--folded", "--out", &target]);
    let written = std::fs::read_to_string(&target).unwrap();
    assert_eq!(said, format!("wrote {target} ({} bytes)\n", written.len()));
    assert_eq!(written, text(&[&path, "--folded"]));
}

fn string(side: i32, id: u32, text: &str) -> Field {
    Field::String(StringEntry {
        side,
        id,
        text: text.into(),
    })
}

/// A chunk of one thread from rows of (name id, start, duration, depth).
fn chunk(side: i32, thread: u32, rows: &[(u32, u64, u32, u32)]) -> Field {
    Field::Timeline(TimelineChunk {
        side,
        thread,
        name: rows.iter().map(|r| r.0).collect(),
        start_us: rows.iter().map(|r| r.1).collect(),
        dur_us: rows.iter().map(|r| r.2).collect(),
        depth: rows.iter().map(|r| r.3).collect(),
    })
}

/// Server thread "tick" in two chunks, a second server thread, and a client thread; ids 1..=3 are thread names.
fn timeline_fields() -> Vec<Field> {
    let (tick, lan, level, blocks) = (4, 5, 6, 7);
    vec![
        start(SERVER, Kind::Dedicated),
        start(CLIENT, Kind::Client),
        string(SERVER, 1, "tick"),
        string(SERVER, 2, "worker"),
        string(SERVER, tick, "tick"),
        string(SERVER, lan, "tick_lan"),
        string(SERVER, level, "level"),
        string(SERVER, blocks, "tick_blocks"),
        string(CLIENT, 3, "render"),
        string(CLIENT, 4, "frame"),
        Field::Mark(Mark {
            side: SERVER,
            t: 0.5,
            name: "lag".into(),
        }),
        chunk(
            SERVER,
            1,
            &[
                (tick, 0, 50_000, 0),
                (lan, 1_000, 20_000, 1),
                (level, 1_000, 5_000, 2),
                (blocks, 30_000, 10_000, 1),
            ],
        ),
        chunk(SERVER, 1, &[(tick, 1_000_000, 40_000, 0)]),
        chunk(SERVER, 2, &[(level, 2_000, 7_000, 0)]),
        chunk(CLIENT, 3, &[(4, 100, 16_000, 0)]),
        stop(SERVER, 2.0),
        stop(CLIENT, 2.0),
    ]
}

fn trace(parts: &[&str]) -> Vec<TraceEvent> {
    serde_json::from_slice(&export(parts).expect("export")).expect("a JSON array of events")
}

fn complete(events: &[TraceEvent]) -> Vec<(&str, u32, u32, u64, u32)> {
    events
        .iter()
        .filter(|e| e.ph == Phase::Complete)
        .map(|e| (e.name.as_str(), e.pid, e.tid, e.ts.unwrap(), e.dur.unwrap()))
        .collect()
}

#[test]
fn perfetto_events_have_the_recorded_times_and_nest_by_containment() {
    let dir = TempDir::new("perfetto");
    let path = save(&dir, "timeline.hzperf", &timeline_fields());
    let events = trace(&[&path, "--perfetto"]);

    let spans = complete(&events);
    assert_eq!(
        spans,
        [
            ("tick", 1, 1, 0, 50_000),
            ("tick_lan", 1, 1, 1_000, 20_000),
            ("level", 1, 1, 1_000, 5_000),
            ("tick_blocks", 1, 1, 30_000, 10_000),
            ("tick", 1, 1, 1_000_000, 40_000),
            ("level", 1, 2, 2_000, 7_000),
            ("frame", 2, 1, 100, 16_000),
        ]
    );
    let depth = |i: usize| {
        let (_, pid, tid, ts, dur) = spans[i];
        spans
            .iter()
            .enumerate()
            .filter(|&(j, &(_, p, t, s, d))| {
                j != i && (p, t) == (pid, tid) && s <= ts && s + u64::from(d) >= ts + u64::from(dur)
            })
            .count()
    };
    assert_eq!(
        (0..4).map(depth).collect::<Vec<_>>(),
        [0, 1, 2, 1],
        "the spans nest as the recorded depths say"
    );
}

#[test]
fn perfetto_names_processes_threads_and_marks() {
    let dir = TempDir::new("perfetto-meta");
    let path = save(&dir, "timeline.hzperf", &timeline_fields());
    let events = trace(&[&path, "--perfetto"]);

    let meta = |name: MetaName| -> Vec<(u32, u32, String)> {
        events
            .iter()
            .filter(|e| e.ph == Phase::Metadata && e.name == name.as_str())
            .map(|e| (e.pid, e.tid, e.args.clone().unwrap().name))
            .collect()
    };
    assert_eq!(
        meta(MetaName::ProcessName),
        [(1, 0, "server".to_string()), (2, 0, "client".to_string())]
    );
    assert_eq!(
        meta(MetaName::ThreadName),
        [
            (1, 1, "tick".to_string()),
            (1, 2, "worker".to_string()),
            (2, 1, "render".to_string()),
        ]
    );
    let marks: Vec<_> = events.iter().filter(|e| e.ph == Phase::Instant).collect();
    assert_eq!(marks.len(), 1);
    assert_eq!(
        (marks[0].name.as_str(), marks[0].pid, marks[0].ts),
        ("lag", 1, Some(500_000))
    );
}

#[test]
fn perfetto_side_and_thread_narrow_the_trace() {
    let dir = TempDir::new("perfetto-filter");
    let path = save(&dir, "timeline.hzperf", &timeline_fields());
    let client = trace(&[&path, "--perfetto", "--side", "client"]);
    assert_eq!(complete(&client), [("frame", 2, 1, 100, 16_000)]);
    assert!(client.iter().all(|e| e.pid == 2), "no server mark or track");
    let worker = trace(&[&path, "--perfetto", "--thread", "worker"]);
    assert_eq!(complete(&worker), [("level", 1, 1, 2_000, 7_000)]);
    let message = failure(&[&path, "--perfetto", "--thread", "audio"]);
    assert!(message.contains("no thread named audio") && message.contains("worker"));
}

#[test]
fn perfetto_puts_a_parent_before_its_child_that_starts_with_it() {
    let dir = TempDir::new("perfetto-order");
    let fields = vec![
        start(SERVER, Kind::Dedicated),
        string(SERVER, 1, "tick"),
        string(SERVER, 2, "outer"),
        string(SERVER, 3, "inner"),
        chunk(SERVER, 1, &[(3, 10, 5, 1), (2, 10, 50, 0)]),
        stop(SERVER, 1.0),
    ];
    let path = save(&dir, "order.hzperf", &fields);
    let names: Vec<String> = trace(&[&path, "--perfetto"])
        .into_iter()
        .filter(|e| e.ph == Phase::Complete)
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["outer", "inner"]);
}
