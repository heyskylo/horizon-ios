mod common;

use std::process::Command;

use common::hand::{NAMES, Names, capture};
use common::{TempDir, save, span_rows};
use horizon_perf::query::Row;
use horizon_perf::wire::{Envelope, Op, OpName};

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_horizon-perf"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_query_prints_one_json_line_and_exits_zero() {
    let dir = TempDir::new("qcli");
    let file = save(&dir, "run.hzperf", &capture(&Names::new(&NAMES)));
    let (code, stdout, stderr) = run(&["query", "spans", "--slow", "--limit", "1", &file]);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(stdout.lines().count(), 1);
    let reply: Envelope<Row> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(span_rows(&reply)[0].id, "s:tick_lan");
    assert_eq!(reply.rows().len(), 1);
    assert!(reply.selection.unwrap().slow);
}

#[test]
fn a_failed_query_still_prints_its_error_as_json_and_exits_one() {
    let dir = TempDir::new("qcli-fail");
    let file = save(&dir, "run.hzperf", &capture(&Names::new(&NAMES)));
    let (code, stdout, stderr) = run(&["query", "tick", "k:55", &file]);
    assert_eq!(code, Some(1));
    let reply: Envelope<Row> = serde_json::from_str(&stdout).unwrap();
    assert_eq!((reply.ok, reply.op), (false, OpName::Query(Op::Tick)));
    assert!(stderr.contains("does not keep tick 55"), "{stderr}");
}

#[test]
fn without_a_file_a_query_reads_the_latest_capture_of_the_directory() {
    let dir = TempDir::new("qcli-latest");
    save(
        &dir,
        "profile-20261107-143204Z.hzperf",
        &capture(&Names::new(&NAMES)),
    );
    let (code, stdout, _) = run(&["query", "meta", "--dir", dir.path().to_str().unwrap()]);
    assert_eq!(code, Some(0));
    let reply: Envelope<Row> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(reply.captures[0].file, "profile-20261107-143204Z.hzperf");
}

#[test]
fn bad_filters_are_usage_errors() {
    for args in [
        vec!["query"],
        vec!["query", "spans", "--from", "soon"],
        vec!["query", "spans", "--side", "both"],
        vec!["query", "spans", "--select", "gpu"],
        vec!["query", "spans", "--where", "color=red"],
        vec!["query", "world", "--by", "nope"],
        vec!["query", "spans", "--frobnicate"],
    ] {
        let (code, _, stderr) = run(&args);
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
    }
}

#[test]
fn summary_takes_a_thread_and_the_slow_switch() {
    let dir = TempDir::new("scli");
    let file = save(&dir, "run.hzperf", &capture(&Names::new(&NAMES)));
    let (code, stdout, _) = run(&["summary", "--slow", &file]);
    assert_eq!(code, Some(0));
    assert!(stdout.contains("slow tick phases"), "{stdout}");
    let (code, _, stderr) = run(&["summary", "--thread", "render", &file]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("no thread named render"), "{stderr}");
    let (code, _, _) = run(&["summary", "--slow", "--all", &file]);
    assert_eq!(code, Some(2));
}
