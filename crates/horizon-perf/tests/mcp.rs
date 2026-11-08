mod common;

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::blocks::{CHILDREN, server, steady};
use common::hand::{NAMES, Names, capture};
use common::samples::waypoints;
use common::{TempDir, save, synthetic_fields, write_capture};
use horizon_bench::{App, Frames, Hardware, Kind as BenchKind, Report};
use horizon_perf::cli::{Env, run};
use horizon_perf::mcp::tools::{
    DiffArgs, DiffToolReply, FollowArgs, FollowReply, ListArgs, ListReply, ListState, QueryArgs,
    SummaryArgs,
};
use horizon_perf::mcp::{RpcId, handle};
use horizon_perf::request::{BoundArg, QueryRequest};
use horizon_perf::watch::Event;
use horizon_perf::wire::{ContextKey, KindName, Select, SideName};
use horizon_perf_format::Field;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

fn env(home: Option<&std::path::Path>) -> Env {
    Env {
        horizon_home: home.map(Into::into),
        poll: Duration::from_millis(2),
    }
}

#[derive(Deserialize)]
struct Rpc {
    id: Option<RpcId>,
    result: Option<Box<RawValue>>,
    error: Option<RpcFault>,
}

#[derive(Deserialize)]
struct RpcFault {
    code: i32,
    message: String,
}

#[derive(Deserialize)]
struct Called {
    content: Vec<Text>,
    #[serde(rename = "isError")]
    is_error: bool,
}

#[derive(Deserialize)]
struct Text {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Deserialize)]
struct Listed {
    tools: Vec<ToolView>,
}

#[derive(Deserialize)]
struct ToolView {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: SchemaView,
}

#[derive(Deserialize)]
struct SchemaView {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    properties: BTreeMap<String, SchemaView>,
    #[serde(default)]
    required: Vec<String>,
    #[serde(default, rename = "enum")]
    choices: Vec<String>,
    #[serde(default, rename = "anyOf")]
    any_of: Vec<SchemaView>,
}

fn request(id: i64, method: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

fn rpc(line: &str, env: &Env) -> Rpc {
    let reply = handle(line, env).expect("a request is answered");
    serde_json::from_str(&reply).unwrap_or_else(|e| panic!("not a reply ({e}): {reply}"))
}

/// A tool call's text and whether it is flagged as an error.
fn call_json(tool: &str, arguments: &str, env: &Env) -> (String, bool) {
    let params = format!(r#"{{"name":"{tool}","arguments":{arguments}}}"#);
    let reply = rpc(&request(1, "tools/call", &params), env);
    assert!(reply.error.is_none(), "{}", reply.error.unwrap().message);
    let called: Called = serde_json::from_str(reply.result.unwrap().get()).unwrap();
    assert_eq!(called.content.len(), 1);
    assert_eq!(called.content[0].kind, "text");
    (
        called.content.into_iter().next().unwrap().text,
        called.is_error,
    )
}

fn call<A: Serialize>(tool: &str, arguments: &A, env: &Env) -> (String, bool) {
    call_json(tool, &serde_json::to_string(arguments).unwrap(), env)
}

fn cli(parts: &[&str], env: &Env) -> String {
    let args: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let _ = run(&args, env, &mut out);
    String::from_utf8(out).unwrap()
}

fn keys_of<T: Serialize>(value: &T) -> Vec<String> {
    let text = serde_json::to_string(value).unwrap();
    let map: BTreeMap<String, Box<RawValue>> = serde_json::from_str(&text).unwrap();
    map.into_keys().collect()
}

fn names(schema: &SchemaView) -> Vec<String> {
    schema.properties.keys().cloned().collect()
}

fn every_filter() -> QueryRequest {
    QueryRequest {
        side: Some(SideName::Server),
        thread: Some("tick".into()),
        select: Some(Select::Cpu),
        slow: true,
        where_: BTreeMap::from([(ContextKey::Entity, vec!["zombie".into()])]),
        from: Some(BoundArg::Seconds(0.0)),
        to: Some(BoundArg::Text("mark:end".into())),
        limit: Some(1),
        names: vec!["tick".into()],
        worst: Some(1),
        over: Some(1.0),
        every: Some(1.0),
        diff: true,
        min_mb: Some(1.0),
        by: Some("self".into()),
        matching: Some("x".into()),
        depth: Some(1),
        min_pct: Some(1.0),
        self_share: true,
    }
}

#[test]
fn tools_list_names_the_five_tools_with_schemas_that_match_their_arguments() {
    let reply = rpc(&request(1, "tools/list", "{}"), &env(None));
    let listed: Listed = serde_json::from_str(reply.result.unwrap().get()).unwrap();
    let tools: BTreeMap<&str, &ToolView> =
        listed.tools.iter().map(|t| (t.name.as_str(), t)).collect();
    assert_eq!(
        listed
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        [
            "perf_summary",
            "perf_query",
            "perf_diff",
            "perf_list",
            "perf_follow"
        ]
    );
    for tool in &listed.tools {
        assert!(!tool.description.is_empty(), "{}", tool.name);
        assert_eq!(tool.input_schema.kind.as_deref(), Some("object"));
    }
    let summary = SummaryArgs {
        file: Some(String::new()),
        baseline: Some(String::new()),
        budget: Some(1),
        side: Some(SideName::Client),
        thread: Some(String::new()),
        slow: true,
    };
    let query = QueryArgs {
        op: "hot".into(),
        args: vec![String::new()],
        files: vec![String::new()],
        filters: every_filter(),
    };
    let diff = DiffArgs {
        a: String::new(),
        b: String::new(),
        min_ms: Some(1.0),
        side: Some(SideName::Client),
        thread: Some(String::new()),
        limit: Some(1),
    };
    let list = ListArgs {
        dir: Some(String::new()),
    };
    let follow = FollowArgs {
        file: Some(String::new()),
        cursor: Some(1),
    };
    assert_eq!(
        names(&tools["perf_summary"].input_schema),
        keys_of(&summary)
    );
    assert_eq!(names(&tools["perf_query"].input_schema), keys_of(&query));
    assert_eq!(names(&tools["perf_diff"].input_schema), keys_of(&diff));
    assert_eq!(names(&tools["perf_list"].input_schema), keys_of(&list));
    assert_eq!(names(&tools["perf_follow"].input_schema), keys_of(&follow));
    let q = &tools["perf_query"].input_schema;
    assert_eq!(q.required, ["op"]);
    assert_eq!(
        names(&q.properties["filters"]),
        keys_of(&every_filter()),
        "one schema property per filter"
    );
    assert_eq!(
        q.properties["op"].choices,
        [
            "spans", "ticks", "tick", "series", "world", "net", "meta", "memory", "marks",
            "warnings", "hot", "tree", "callers", "callees", "split", "group", "frames", "gpu",
            "bench"
        ]
    );
    assert_eq!(
        q.properties["filters"].properties["select"].choices.len(),
        6
    );
    assert_eq!(
        q.properties["filters"].properties["from"].any_of.len(),
        2,
        "a bound is seconds or text"
    );
    assert_eq!(tools["perf_diff"].input_schema.required, ["a", "b"]);
    for tool in ["perf_summary", "perf_list", "perf_follow"] {
        assert!(tools[tool].input_schema.required.is_empty(), "{tool}");
    }
}

#[test]
fn perf_query_returns_byte_identical_json_to_the_cli() {
    let dir = TempDir::new("mcp-query");
    let spans = save(&dir, "spans.hzperf", &capture(&Names::new(&NAMES)));
    let samples = save(&dir, "samples.hzperf", &waypoints(1, 50).fields());
    let env = env(None);
    let cases: Vec<(Vec<&str>, QueryArgs)> = vec![
        (
            vec!["query", "spans", "--slow", "--limit", "1", &spans],
            QueryArgs {
                op: "spans".into(),
                files: vec![spans.clone()],
                filters: QueryRequest {
                    slow: true,
                    limit: Some(1),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        (
            vec!["query", "ticks", "--worst", "2", "--from", "0", &spans],
            QueryArgs {
                op: "ticks".into(),
                files: vec![spans.clone()],
                filters: QueryRequest {
                    worst: Some(2),
                    from: Some(BoundArg::Seconds(0.0)),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        (
            vec![
                "query",
                "hot",
                "--by",
                "total",
                "--where",
                "entity=zombie,entity=item",
                &samples,
            ],
            QueryArgs {
                op: "hot".into(),
                files: vec![samples.clone()],
                filters: QueryRequest {
                    by: Some("total".into()),
                    where_: BTreeMap::from([(
                        ContextKey::Entity,
                        vec!["zombie".into(), "item".into()],
                    )]),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        (
            vec![
                "query", "group", "--by", "entity", "--select", "wall", &samples,
            ],
            QueryArgs {
                op: "group".into(),
                files: vec![samples.clone()],
                filters: QueryRequest {
                    by: Some("entity".into()),
                    select: Some(Select::Wall),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        (
            vec!["query", "tick", "k:55", &spans],
            QueryArgs {
                op: "tick".into(),
                args: vec!["k:55".into()],
                files: vec![spans.clone()],
                ..Default::default()
            },
        ),
        (
            vec!["query", "bogus", &spans],
            QueryArgs {
                op: "bogus".into(),
                files: vec![spans.clone()],
                ..Default::default()
            },
        ),
    ];
    for (parts, args) in &cases {
        let expected = cli(parts, &env);
        let (text, is_error) = call("perf_query", args, &env);
        assert_eq!(text + "\n", expected, "{parts:?}");
        assert!(expected.starts_with(r#"{"ok":"#));
        assert_eq!(
            is_error,
            expected.starts_with(r#"{"ok":false"#),
            "{parts:?}"
        );
    }
    assert!(
        call("perf_query", &cases[4].1, &env).1,
        "a failed query is flagged"
    );
}

#[test]
fn perf_query_without_files_reads_the_newest_finished_capture_of_the_reports_folder() {
    let home = TempDir::new("mcp-home");
    let reports = home.path().join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    std::fs::write(
        reports.join("profile-20261107-143204Z.hzperf"),
        write_capture(&capture(&Names::new(&NAMES))).bytes,
    )
    .unwrap();
    let env = env(Some(home.path()));
    let args = QueryArgs {
        op: "meta".into(),
        ..Default::default()
    };
    let (text, is_error) = call("perf_query", &args, &env);
    assert!(!is_error, "{text}");
    assert_eq!(text + "\n", cli(&["query", "meta"], &env));
}

#[test]
fn a_path_that_is_not_a_capture_is_refused_even_when_it_holds_one() {
    let dir = TempDir::new("mcp-refuse");
    let good = save(&dir, "run.hzperf", &capture(&Names::new(&NAMES)));
    let disguised = dir.path().join("run.bin");
    std::fs::copy(&good, &disguised).unwrap();
    let disguised = disguised.to_string_lossy().into_owned();
    let env = env(None);
    let refused = |(text, is_error): (String, bool)| {
        assert!(is_error, "{text}");
        assert!(text.starts_with("refusing "), "{text}");
        assert!(text.contains("only"), "{text}");
    };
    refused(call(
        "perf_query",
        &QueryArgs {
            op: "meta".into(),
            files: vec![disguised.clone()],
            ..Default::default()
        },
        &env,
    ));
    refused(call(
        "perf_query",
        &QueryArgs {
            op: "meta".into(),
            files: vec![good.clone(), "/etc/passwd".into()],
            ..Default::default()
        },
        &env,
    ));
    refused(call(
        "perf_summary",
        &SummaryArgs {
            file: Some(disguised.clone()),
            ..Default::default()
        },
        &env,
    ));
    refused(call(
        "perf_summary",
        &SummaryArgs {
            file: Some(good.clone()),
            baseline: Some(disguised.clone()),
            ..Default::default()
        },
        &env,
    ));
    refused(call(
        "perf_follow",
        &FollowArgs {
            file: Some(format!("{good}.bak")),
            ..Default::default()
        },
        &env,
    ));
    refused(call(
        "perf_diff",
        &DiffArgs {
            a: good.clone(),
            b: disguised.clone(),
            ..Default::default()
        },
        &env,
    ));
    let (text, is_error) = call(
        "perf_summary",
        &SummaryArgs {
            file: Some(good),
            ..Default::default()
        },
        &env,
    );
    assert!(!is_error && text.contains("tick_lan"), "{text}");
}

#[test]
fn perf_summary_is_the_text_the_cli_prints() {
    let dir = TempDir::new("mcp-summary");
    let file = save(&dir, "run.hzperf", &capture(&Names::new(&NAMES)));
    let env = env(None);
    let args = SummaryArgs {
        file: Some(file.clone()),
        budget: Some(400),
        slow: true,
        ..Default::default()
    };
    let (text, is_error) = call("perf_summary", &args, &env);
    assert!(!is_error);
    assert_eq!(
        text,
        cli(&["summary", &file, "--budget", "400", "--slow"], &env)
    );
}

fn frames(values: &[f64]) -> Vec<Frames> {
    values
        .iter()
        .enumerate()
        .map(|(repeat, &p50)| Frames {
            phase: "steady".into(),
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
            cpu_p50_ms: None,
            gpu_p50_ms: None,
            histogram: Vec::new(),
        })
        .collect()
}

fn report(values: &[f64]) -> String {
    let app = App {
        name: "horizon".into(),
        version: "0.1".into(),
        ..Default::default()
    };
    let mut r = Report::with_hardware(BenchKind::Game, app, Hardware::default());
    r.frames = frames(values);
    r.to_json()
}

#[test]
fn perf_diff_gives_the_text_table_and_the_json_rows_of_the_cli() {
    let dir = TempDir::new("mcp-diff");
    let a = save(
        &dir,
        "a.hzperf",
        &server(&CHILDREN, &steady(12, 0), "release"),
    );
    let b = save(
        &dir,
        "b.hzperf",
        &server(&CHILDREN, &steady(12, 3), "release"),
    );
    let env = env(None);
    let args = DiffArgs {
        a: a.clone(),
        b: b.clone(),
        min_ms: Some(0.5),
        ..Default::default()
    };
    let (text, is_error) = call("perf_diff", &args, &env);
    assert!(!is_error, "{text}");
    let reply: DiffToolReply = serde_json::from_str(&text).unwrap();
    assert_eq!(reply.text, cli(&["diff", &a, &b, "--min-ms", "0.5"], &env));
    let json = serde_json::to_string(&reply.reply).unwrap() + "\n";
    assert_eq!(
        json,
        cli(&["diff", &a, &b, "--min-ms", "0.5", "--json"], &env)
    );
    let (ra, rb) = (dir.path().join("a.json"), dir.path().join("b.json"));
    std::fs::write(&ra, report(&[1.0; 8])).unwrap();
    std::fs::write(&rb, report(&[2.0; 8])).unwrap();
    let (ra, rb) = (
        ra.to_string_lossy().into_owned(),
        rb.to_string_lossy().into_owned(),
    );
    let (text, is_error) = call(
        "perf_diff",
        &DiffArgs {
            a: ra.clone(),
            b: rb.clone(),
            ..Default::default()
        },
        &env,
    );
    assert!(!is_error, "benchmark reports are accepted: {text}");
    let reply: DiffToolReply = serde_json::from_str(&text).unwrap();
    assert_eq!(reply.text, cli(&["diff", &ra, &rb], &env));
    let (text, is_error) = call(
        "perf_diff",
        &DiffArgs {
            a: a.clone(),
            b: rb,
            ..Default::default()
        },
        &env,
    );
    assert!(is_error && text.contains("cannot compare"), "{text}");
}

#[test]
fn perf_list_describes_each_capture_with_its_state_sides_kind_seconds_and_size() {
    let dir = TempDir::new("mcp-list");
    let done = write_capture(&synthetic_fields());
    std::fs::write(dir.path().join("done.hzperf"), &done.bytes).unwrap();
    let cut = done.flushed_at[10];
    std::fs::write(dir.path().join("growing.hzperf"), &done.bytes[..cut]).unwrap();
    std::fs::write(dir.path().join("bad.hzperf"), b"not gzip at all").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"ignored").unwrap();
    let (text, is_error) = call(
        "perf_list",
        &ListArgs {
            dir: Some(dir.path().to_string_lossy().into_owned()),
        },
        &env(None),
    );
    assert!(!is_error, "{text}");
    let reply: ListReply = serde_json::from_str(&text).unwrap();
    let rows: BTreeMap<&str, _> = reply
        .captures
        .iter()
        .map(|r| (r.file.as_str(), r))
        .collect();
    assert_eq!(
        rows.keys().copied().collect::<Vec<_>>(),
        ["bad.hzperf", "done.hzperf", "growing.hzperf"]
    );
    let done_row = rows["done.hzperf"];
    assert_eq!(done_row.state, ListState::Finished);
    assert_eq!(done_row.size, Some(done.bytes.len() as u64));
    assert_eq!(
        done_row.path,
        dir.path().join("done.hzperf").display().to_string()
    );
    assert!(done_row.modified_unix_s.is_some());
    let sides: Vec<_> = done_row
        .sides
        .iter()
        .map(|s| (s.side, s.kind, s.seconds))
        .collect();
    assert_eq!(
        sides,
        [
            (SideName::Server, KindName::Dedicated, Some(150.0)),
            (SideName::Client, KindName::Client, Some(150.0))
        ]
    );
    let growing = rows["growing.hzperf"];
    assert_eq!(growing.state, ListState::Writing);
    assert_eq!(growing.size, Some(cut as u64));
    assert!(growing.sides.iter().all(|s| s.seconds.is_none()));
    assert_eq!(rows["bad.hzperf"].state, ListState::Unreadable);
    assert!(rows["bad.hzperf"].error.is_some());
}

fn follow(path: &str, cursor: Option<u64>, env: &Env) -> FollowReply {
    let (text, is_error) = call(
        "perf_follow",
        &FollowArgs {
            file: Some(path.into()),
            cursor,
        },
        env,
    );
    assert!(!is_error, "{text}");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn perf_follow_never_repeats_or_skips_an_event_of_a_growing_file() {
    let dir = TempDir::new("mcp-follow");
    let path = dir.path().join("live.hzperf");
    let path_text = path.to_string_lossy().into_owned();
    let fields = synthetic_fields();
    let written = write_capture(&fields);
    let env = env(None);
    std::fs::write(dir.path().join("whole.hzperf"), &written.bytes).unwrap();
    let whole = dir.path().join("whole.hzperf");
    let expected: Vec<Event> = cli(
        &["follow", whole.to_str().unwrap(), "--every", "0", "--json"],
        &env,
    )
    .lines()
    .map(|l| serde_json::from_str(l).unwrap())
    .collect();
    let count = |f: fn(&Field) -> bool| fields.iter().filter(|x| f(x)).count();
    let healths = count(|f| matches!(f, Field::Health(_)));
    let slows = count(|f| matches!(f, Field::Slow(_)));
    assert!(healths > 20 && slows > 20, "the fixture has live events");
    assert_eq!(
        expected
            .iter()
            .filter(|e| matches!(e, Event::Health(_)))
            .count(),
        healths
    );
    assert_eq!(
        expected
            .iter()
            .filter(|e| matches!(e, Event::Slow(_)))
            .count(),
        slows
    );
    let last_flush = *written.flushed_at.last().unwrap();
    let before_last_stop = written.flushed_at[written.flushed_at.len() - 2];
    let mut cuts: Vec<usize> = written.flushed_at.clone();
    for pair in written.flushed_at.windows(2) {
        cuts.push((pair[0] + pair[1]) / 2);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let (mut got, mut cursor, mut last_cursor) = (Vec::new(), None, 0);
    for (i, &cut) in cuts.iter().enumerate() {
        std::fs::write(&path, &written.bytes[..cut]).unwrap();
        let reply = follow(&path_text, cursor, &env);
        assert!(reply.cursor >= last_cursor, "the cursor never goes back");
        let again = follow(&path_text, cursor, &env);
        assert_eq!(
            again.events, reply.events,
            "a call without new bytes repeats itself"
        );
        got.extend(reply.events);
        assert_eq!(
            got,
            expected[..got.len()],
            "after cut {i}: a prefix, nothing doubled or missing"
        );
        if cut <= before_last_stop {
            assert!(!reply.done, "the last stop has not arrived, after cut {i}");
        }
        if cut >= last_flush {
            assert!(reply.done, "every stop has arrived, after cut {i}");
        }
        (cursor, last_cursor) = (Some(reply.cursor), reply.cursor);
    }
    assert_eq!(got, expected);
    let idle = follow(&path_text, cursor, &env);
    assert!((idle.events.is_empty(), idle.done) == (true, true));
    let from_start = follow(&path_text, None, &env);
    assert_eq!(from_start.events, expected, "no cursor reads everything");
    let (text, is_error) = call(
        "perf_follow",
        &FollowArgs {
            file: Some(path_text),
            cursor: Some(idle.cursor + 1),
        },
        &env,
    );
    assert!(is_error && text.contains("ahead"), "{text}");
}

#[test]
fn the_protocol_answers_requests_ignores_notifications_and_names_its_faults() {
    let env = env(None);
    let init = rpc(
        &request(
            3,
            "initialize",
            r#"{"protocolVersion":"2025-03-26","capabilities":{}}"#,
        ),
        &env,
    );
    assert_eq!(init.id, Some(RpcId::Number(3)));
    let body = init.result.unwrap();
    assert!(body.get().contains(r#""protocolVersion":"2025-03-26""#));
    assert!(body.get().contains(r#""name":"horizon-perf""#));
    assert!(body.get().contains(r#""tools":{}"#));
    let unknown_version = rpc(
        &request(4, "initialize", r#"{"protocolVersion":"1999-01-01"}"#),
        &env,
    );
    assert!(
        unknown_version
            .result
            .unwrap()
            .get()
            .contains(r#""protocolVersion":"2025-06-18""#)
    );
    assert!(
        handle(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &env
        )
        .is_none()
    );
    assert_eq!(
        rpc(&request(5, "ping", "{}"), &env).result.unwrap().get(),
        "{}"
    );
    let string_id = rpc(r#"{"jsonrpc":"2.0","id":"abc","method":"ping"}"#, &env);
    assert_eq!(string_id.id, Some(RpcId::Text("abc".into())));
    let missing = rpc(&request(6, "resources/list", "{}"), &env)
        .error
        .unwrap();
    assert_eq!(missing.code, -32601);
    assert!(missing.message.contains("resources/list"));
    let garbage = rpc("{not json", &env);
    assert_eq!((garbage.id, garbage.error.unwrap().code), (None, -32700));
    let unknown_tool = rpc(
        &request(7, "tools/call", r#"{"name":"perf_nothing","arguments":{}}"#),
        &env,
    );
    assert_eq!(unknown_tool.error.unwrap().code, -32602);
    let bad_arguments = rpc(
        &request(
            8,
            "tools/call",
            r#"{"name":"perf_list","arguments":{"folder":"x"}}"#,
        ),
        &env,
    )
    .error
    .unwrap();
    assert_eq!(bad_arguments.code, -32602);
    assert!(
        bad_arguments.message.contains("folder"),
        "{}",
        bad_arguments.message
    );
    let no_op = rpc(
        &request(9, "tools/call", r#"{"name":"perf_query","arguments":{}}"#),
        &env,
    );
    assert_eq!(no_op.error.unwrap().code, -32602);
    let (text, is_error) = call_json("perf_list", "null", &env);
    assert!(
        is_error || text.contains("captures"),
        "null arguments are no arguments: {text}"
    );
}

#[test]
fn the_mcp_command_serves_the_protocol_on_stdio() {
    let dir = TempDir::new("mcp-stdio");
    let file = save(&dir, "run.hzperf", &capture(&Names::new(&NAMES)));
    let query = serde_json::to_string(&QueryArgs {
        op: "meta".into(),
        files: vec![file.clone()],
        ..Default::default()
    })
    .unwrap();
    let input = format!(
        "{}\n{}\n\n{}\n{}\n",
        request(1, "initialize", r#"{"protocolVersion":"2025-06-18"}"#),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        request(2, "tools/list", "{}"),
        request(
            3,
            "tools/call",
            &format!(r#"{{"name":"perf_query","arguments":{query}}}"#)
        ),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_horizon-perf"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let replies: Vec<Rpc> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let ids: Vec<_> = replies.iter().map(|r| r.id.clone()).collect();
    assert_eq!(
        ids,
        [
            Some(RpcId::Number(1)),
            Some(RpcId::Number(2)),
            Some(RpcId::Number(3))
        ]
    );
    let called: Called = serde_json::from_str(replies[2].result.as_ref().unwrap().get()).unwrap();
    assert_eq!(
        called.content.into_iter().next().unwrap().text + "\n",
        cli(&["query", "meta", &file], &env(None))
    );
}
