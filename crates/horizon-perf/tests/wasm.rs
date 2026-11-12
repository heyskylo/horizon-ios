//! The agreement test of `docs/perf-platform.md` §11 (W1): the C ABI, called natively, gives the
//! bytes the CLI prints for the same capture, query and summary. Run with
//! `cargo test -p horizon-perf --features wasm --test wasm`.

mod common;

use std::time::Duration;

use common::TempDir;
use common::hand::{NAMES, Names, capture};
use common::samples;
use common::save;
use horizon_perf::cli::{self, Env};
use horizon_perf::wasm::{hp_alloc, hp_close, hp_export, hp_free, hp_open, hp_query, hp_summary};
use serde_json::Value;

/// Reads a reply buffer into its kind and payload, and gives its memory back.
fn read(reply: *mut u8) -> (u8, Vec<u8>) {
    assert!(!reply.is_null(), "no reply");
    let head = unsafe { std::slice::from_raw_parts(reply, 5) };
    let kind = head[0];
    let len = u32::from_le_bytes(head[1..5].try_into().unwrap()) as usize;
    let payload = unsafe { std::slice::from_raw_parts(reply.add(5), len) }.to_vec();
    unsafe { hp_free(reply, 5 + len) };
    (kind, payload)
}

/// Copies `bytes` into the module's memory and runs `call` on it.
fn with_bytes<T>(bytes: &[u8], call: impl FnOnce(*const u8, usize) -> T) -> T {
    let ptr = hp_alloc(bytes.len());
    if !bytes.is_empty() {
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
    }
    let out = call(ptr, bytes.len());
    unsafe { hp_free(ptr, bytes.len()) };
    out
}

fn open(bytes: &[u8]) -> (u8, Vec<u8>) {
    read(with_bytes(bytes, |ptr, len| unsafe { hp_open(ptr, len) }))
}

/// Opens a capture and returns its handle.
fn handle(bytes: &[u8]) -> u32 {
    let (kind, payload) = open(bytes);
    assert_eq!(kind, 0, "a JSON reply");
    let reply: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(reply["ok"], true, "{}", String::from_utf8_lossy(&payload));
    reply["handle"].as_u64().unwrap() as u32
}

fn query(handle: u32, request: &str) -> (u8, Vec<u8>) {
    read(with_bytes(request.as_bytes(), |ptr, len| unsafe {
        hp_query(handle, ptr, len)
    }))
}

fn summary(handle: u32, request: &str) -> (u8, Vec<u8>) {
    read(with_bytes(request.as_bytes(), |ptr, len| unsafe {
        hp_summary(handle, ptr, len)
    }))
}

fn cli_output(args: &[&str]) -> Vec<u8> {
    let env = Env {
        horizon_home: None,
        poll: Duration::from_millis(2),
    };
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let _ = cli::run(&args, &env, &mut out);
    out
}

/// The request JSON for a query the CLI spells `parts`.
fn request(file: &str, parts: &[&str]) -> String {
    // Every filter this test uses is a switch or one value after a flag; keep the mapping here
    // explicit so a mismatch shows up as a difference in the compared bytes.
    let mut body = serde_json::Map::new();
    body.insert("op".into(), parts[0].into());
    let mut args = Vec::new();
    let mut filters = serde_json::Map::new();
    let mut i = 1;
    while i < parts.len() {
        match parts[i] {
            "--slow" => {
                filters.insert("slow".into(), true.into());
                i += 1;
            }
            flag @ ("--worst" | "--limit" | "--depth" | "--by" | "--side" | "--thread"
            | "--match") => {
                let key = flag.trim_start_matches("--");
                let value = parts[i + 1];
                filters.insert(
                    key.into(),
                    value
                        .parse::<u64>()
                        .map_or_else(|_| Value::String(value.to_string()), Value::from),
                );
                i += 2;
            }
            arg => {
                args.push(arg.to_string());
                i += 1;
            }
        }
    }
    if !args.is_empty() {
        body.insert("args".into(), Value::from(args));
    }
    body.insert("filters".into(), Value::Object(filters));
    body.insert("file".into(), file.into());
    Value::Object(body).to_string()
}

#[test]
fn query_replies_are_byte_identical_to_the_cli() {
    let dir = TempDir::new("wasm-query");
    let fields = capture(&Names::new(&NAMES));
    let path = save(&dir, "hand.hzperf", &fields);
    let bytes = std::fs::read(&path).unwrap();
    let handle = handle(&bytes);

    let cases: [&[&str]; 10] = [
        &["spans"],
        &["spans", "--slow"],
        &["ticks", "--worst", "3"],
        &["meta"],
        &["series", "h:mspt_p95"],
        &["world"],
        &["net"],
        &["memory"],
        &["marks"],
        &["warnings"],
    ];
    for parts in cases {
        let mut args = vec!["query"];
        args.extend_from_slice(parts);
        args.push(&path);
        let expected = cli_output(&args);
        let expected = String::from_utf8(expected).unwrap();
        let expected = expected.trim_end_matches('\n');
        let answered: Value = serde_json::from_str(expected).unwrap();
        assert_eq!(answered["ok"], true, "{parts:?} did not answer: {expected}");

        let (kind, payload) = query(handle, &request("hand.hzperf", parts));
        assert_eq!(kind, 0, "a JSON reply for {parts:?}");
        let got = String::from_utf8(payload).unwrap();
        assert_eq!(got, expected, "{parts:?} differs from the CLI");
    }
    hp_close(handle);
}

#[test]
fn sampled_queries_are_byte_identical_to_the_cli() {
    let dir = TempDir::new("wasm-samples");
    let fields = samples::waypoints(1, 120).fields();
    let path = save(&dir, "waypoints.hzperf", &fields);
    let bytes = std::fs::read(&path).unwrap();
    let handle = handle(&bytes);

    for parts in [
        &["hot"][..],
        &["hot", "--by", "total"],
        &["hot", "--limit", "3", "--match", "Waypoints"],
        &["group", "--by", "thread"],
        &["frames", "/waypoints::/"],
    ] {
        let mut args = vec!["query"];
        args.extend_from_slice(parts);
        args.push(&path);
        let expected = String::from_utf8(cli_output(&args)).unwrap();
        let expected = expected.trim_end_matches('\n');
        let answered: Value = serde_json::from_str(expected).unwrap();
        assert_eq!(answered["ok"], true, "{parts:?} did not answer: {expected}");

        let (kind, payload) = query(handle, &request("waypoints.hzperf", parts));
        assert_eq!(kind, 0, "a JSON reply for {parts:?}");
        let got = String::from_utf8(payload).unwrap();
        assert_eq!(got, expected, "{parts:?} differs from the CLI");
    }
    hp_close(handle);
}

#[test]
fn summary_is_byte_identical_to_the_cli() {
    let dir = TempDir::new("wasm-summary");
    let fields = capture(&Names::new(&NAMES));
    let path = save(&dir, "hand.hzperf", &fields);
    let bytes = std::fs::read(&path).unwrap();
    let handle = handle(&bytes);

    let expected = String::from_utf8(cli_output(&["summary", &path])).unwrap();
    assert!(expected.contains("TPS"), "{expected}");
    let (kind, payload) = summary(handle, r#"{"file":"hand.hzperf"}"#);
    assert_eq!(kind, 2, "text");
    let got = String::from_utf8(payload).unwrap();
    assert_eq!(
        got.trim_end(),
        expected.trim_end(),
        "the wasm summary differs from the CLI"
    );

    // Options reach the same report: the side and the budget are the CLI's flags.
    let expected = String::from_utf8(cli_output(&[
        "summary", &path, "--side", "server", "--budget", "400",
    ]))
    .unwrap();
    let (_, payload) = summary(
        handle,
        r#"{"file":"hand.hzperf","side":"server","budget":400}"#,
    );
    assert_eq!(
        String::from_utf8(payload).unwrap().trim_end(),
        expected.trim_end()
    );
    hp_close(handle);
}

#[test]
fn export_is_byte_identical_to_the_cli() {
    let dir = TempDir::new("wasm-export");
    let fields = samples::waypoints(1, 120).fields();
    let path = save(&dir, "waypoints.hzperf", &fields);
    let bytes = std::fs::read(&path).unwrap();
    let handle = handle(&bytes);

    for (args, request) in [
        (
            &["export", path.as_str(), "--folded"][..],
            r#"{"kind":"folded"}"#,
        ),
        (
            &["export", path.as_str(), "--pprof"][..],
            r#"{"kind":"pprof"}"#,
        ),
    ] {
        let expected = cli_output(args);
        let reply = read(with_bytes(request.as_bytes(), |ptr, len| unsafe {
            hp_export(handle, ptr, len)
        }));
        assert_eq!(reply.0, 1, "bytes for {request}");
        assert!(!expected.is_empty(), "{request}");
        assert_eq!(reply.1, expected, "{request} differs from the CLI");
    }
    hp_close(handle);
}

#[test]
fn a_malformed_file_is_an_error_reply_not_a_trap() {
    let (kind, payload) = open(b"this is not a capture");
    assert_eq!(kind, 0);
    let reply: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["code"], "invalid_capture");

    // A read of a handle that was never opened is a reply too, and so is a bad request.
    let (kind, payload) = query(9_999, r#"{"op":"meta"}"#);
    assert_eq!(kind, 0);
    let reply: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(reply["error"]["code"], "unknown_handle");

    let dir = TempDir::new("wasm-errors");
    let path = save(&dir, "hand.hzperf", &capture(&Names::new(&NAMES)));
    let handle = handle(&std::fs::read(&path).unwrap());
    let (kind, payload) = query(handle, "not json");
    assert_eq!(kind, 0);
    let reply: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(reply["error"]["code"], "bad_request");
    hp_close(handle);
}

#[test]
fn closing_a_handle_frees_it() {
    let dir = TempDir::new("wasm-close");
    let path = save(&dir, "hand.hzperf", &capture(&Names::new(&NAMES)));
    let handle = handle(&std::fs::read(&path).unwrap());
    let reply = query(handle, r#"{"op":"meta"}"#);
    let parsed: Value = serde_json::from_slice(&reply.1).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(!parsed["rows"].as_array().unwrap().is_empty());

    hp_close(handle);
    let reply = query(handle, r#"{"op":"meta"}"#);
    let parsed: Value = serde_json::from_slice(&reply.1).unwrap();
    assert_eq!(parsed["error"]["code"], "unknown_handle");
}
