mod common;

use common::samples::*;
use common::{SERVER, TempDir, query, save, start, stop, tree_rows};
use horizon_perf::diff::{Diff, DiffOptions, diff};
use horizon_perf::read_capture;
use horizon_perf::wire::Verdict;
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

/// FNV-1a 64 of each name, low 40 bits in base32, computed outside the code under test.
const ROOT_ID: &str = "f:y2crmj90";
const WORK_ID: &str = "f:399yd9jn";
const OTHER_ID: &str = "f:abj1gf0a";
const VANISHED_ID: &str = "f:5dyx0eqe";
const ARRIVED_ID: &str = "f:v5yjtzzh";

const BLOCKS: u32 = 12;
const UNITS: u32 = 100;

/// Twelve 5 s blocks of 100 ticks, 10 ms per sample, so a count of 10 is 1 ms per tick. `work` counts come from `work(block)`.
fn capture(work: impl Fn(u32) -> u64, extra: &str) -> (String, TempDir) {
    let mut b = Builder::new(u64::from(BLOCKS * UNITS), &[UNITS; BLOCKS as usize]);
    let root = b.node(None, "app::root");
    let worker = b.node(Some(root), "app::work");
    let other = b.node(Some(root), "app::other");
    let nested = b.node(Some(other), "app::work");
    let odd = b.node(Some(root), extra);
    for block in 0..BLOCKS {
        let at = |count: u64, node: u32, b: &mut Builder| {
            b.weight(
                node,
                count,
                W {
                    block,
                    ..W::default()
                },
            )
        };
        at(work(block), worker, &mut b);
        at(10, other, &mut b);
        at(4, nested, &mut b);
        at(5, odd, &mut b);
    }
    let mut fields = vec![start(SERVER, Kind::Dedicated)];
    for block in 0..BLOCKS {
        fields.push(Field::Health(HealthSample {
            side: SERVER,
            t: f64::from(block) * 5.0,
            mspt_avg: Some(10.0),
            ..Default::default()
        }));
    }
    fields.extend(b.sections());
    fields.push(stop(SERVER, 60.0));
    let dir = TempDir::new("diff_calls");
    let path = save(&dir, "c.hzperf", &fields);
    (path, dir)
}

fn compared(calls: bool, min_ms: f64) -> Diff {
    let (a, _da) = capture(|_| 20, "app::vanished");
    let (b, _db) = capture(|block| if block % 2 == 0 { 29 } else { 31 }, "app::arrived");
    let read = |path: &str| read_capture(&std::fs::read(path).unwrap()).unwrap();
    let options = DiffOptions {
        calls,
        min_ms,
        limit: usize::MAX,
        ..DiffOptions::default()
    };
    diff(&read(&a), &read(&b), &options).unwrap()
}

fn close(value: Option<f64>, want: f64) {
    let value = value.unwrap();
    assert!((value - want).abs() < 1e-9, "{value} is not {want}");
}

#[test]
fn a_diff_without_calls_holds_no_function_or_path_rows() {
    let d = compared(false, 0.0);
    assert!(
        d.rows.iter().all(|r| r.id.starts_with("h:")),
        "{:?}",
        d.rows
    );
}

#[test]
fn a_function_that_got_slower_shows_the_hand_computed_change() {
    let d = compared(true, 0.0);
    // work: A 20 + 4 samples = 2.4 ms/tick every block; B (29 or 31) + 4 = 3.3 or 3.5, median 3.4; the 144 differences are 0.9 and 1.1 in equal numbers.
    let work = d.row(WORK_ID).unwrap();
    assert_eq!(work.verdict, Verdict::Slower);
    assert_eq!(work.name.as_deref(), Some("app::work"));
    close(work.a, 2.4);
    close(work.b, 3.4);
    close(work.effect, 1.0);
    let [lo, hi] = work.ci.unwrap();
    assert!(
        (lo - 0.9).abs() < 1e-9 && (hi - 1.1).abs() < 1e-9,
        "[{lo}, {hi}]"
    );
    assert_eq!(work.blocks, [12, 12]);
    // The root adds the other children: 20 + 10 + 4 + 5 = 39 samples before, 48 or 50 after.
    let root = d.row(ROOT_ID).unwrap();
    assert_eq!(root.verdict, Verdict::Slower);
    close(root.a, 3.9);
    close(root.b, 4.9);
    close(root.effect, 1.0);
}

#[test]
fn a_function_that_did_not_change_is_within_noise() {
    let d = compared(true, 0.0);
    let other = d.row(OTHER_ID).unwrap();
    assert_eq!(other.verdict, Verdict::WithinNoise);
    close(other.a, 1.4);
    close(other.b, 1.4);
    close(other.effect, 0.0);
}

#[test]
fn a_function_in_one_capture_only_is_gone_or_new() {
    let d = compared(true, 0.0);
    let gone = d.row(VANISHED_ID).unwrap();
    assert_eq!(
        (gone.verdict, gone.b, gone.effect),
        (Verdict::Gone, None, None)
    );
    close(gone.a, 0.5);
    let new = d.row(ARRIVED_ID).unwrap();
    assert_eq!((new.verdict, new.a, new.effect), (Verdict::New, None, None));
    close(new.b, 0.5);
}

#[test]
fn call_paths_are_compared_by_their_own_inclusive_time() {
    let d = compared(true, 0.0);
    let paths: Vec<_> = d.rows.iter().filter(|r| r.id.starts_with("p:")).collect();
    // Root, work, other, work under other, and the two that exist on one side: six paths.
    assert_eq!(paths.len(), 6);
    let find = |a: Option<f64>, b: Option<f64>| {
        paths
            .iter()
            .find(|r| {
                let near = |x: Option<f64>, y: Option<f64>| match (x, y) {
                    (Some(x), Some(y)) => (x - y).abs() < 1e-9,
                    (x, y) => x == y,
                };
                near(r.a, a) && near(r.b, b)
            })
            .unwrap_or_else(|| panic!("no path with {a:?} and {b:?}"))
    };
    let direct = find(Some(2.0), Some(3.0));
    assert_eq!(direct.verdict, Verdict::Slower);
    close(direct.effect, 1.0);
    let nested = find(Some(0.4), Some(0.4));
    assert_eq!(nested.verdict, Verdict::WithinNoise);
    // other's own 1.0 and the work below it, 0.4.
    assert_eq!(find(Some(1.4), Some(1.4)).verdict, Verdict::WithinNoise);
    assert_eq!(find(Some(3.9), Some(4.9)).verdict, Verdict::Slower);
    assert_eq!(find(Some(0.5), None).verdict, Verdict::Gone);
    assert_eq!(find(None, Some(0.5)).verdict, Verdict::New);
}

#[test]
fn a_path_row_carries_the_id_the_tree_query_gives_the_same_path() {
    let d = compared(true, 0.0);
    let (a, _dir) = capture(|_| 20, "app::vanished");
    let reply = query(&["tree", &a, "--min-pct", "0"]);
    let rows = tree_rows(&reply);
    let top = rows[0];
    assert!(top.id.starts_with("p:") || top.id.starts_with("f:"));
    let first = &top.children[0];
    let diffed = d
        .row(&first.id)
        .unwrap_or_else(|| panic!("{} is not in the diff", first.id));
    close(diffed.a, first.total);
}

#[test]
fn the_calls_switch_of_the_diff_command_prints_the_function_rows() {
    use horizon_perf::cli::{Env, run};
    use horizon_perf::diff::DiffEnvelope;
    let (a, _da) = capture(|_| 20, "app::vanished");
    let (b, _db) = capture(|block| if block % 2 == 0 { 29 } else { 31 }, "app::arrived");
    let env = Env {
        horizon_home: None,
        poll: std::time::Duration::from_millis(1),
    };
    let ask = |extra: &[&str]| {
        let mut args: Vec<String> = ["diff", &a, &b, "--json", "--limit", "100"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        args.extend(extra.iter().map(|s| s.to_string()));
        let mut out = Vec::new();
        run(&args, &env, &mut out).unwrap();
        serde_json::from_slice::<DiffEnvelope>(&out).unwrap()
    };
    let plain = ask(&[]).rows.unwrap();
    assert!(
        plain
            .iter()
            .all(|r| r.id.starts_with("h:") && r.name.is_none())
    );
    let calls = ask(&["--calls"]).rows.unwrap();
    let work = calls.iter().find(|r| r.id == WORK_ID).unwrap();
    assert_eq!(work.name.as_deref(), Some("app::work"));
    assert_eq!(
        (work.a, work.b, work.diff),
        (Some(2.4), Some(3.4), Some(1.0))
    );
}

#[test]
fn rows_under_the_floor_in_both_captures_are_left_out() {
    let d = compared(true, 1.0);
    let calls: Vec<&str> = d
        .rows
        .iter()
        .filter(|r| !r.id.starts_with("h:"))
        .map(|r| r.id.as_str())
        .filter(|id| id.starts_with("f:"))
        .collect();
    let mut want = vec![ROOT_ID, WORK_ID, OTHER_ID];
    want.sort_unstable();
    let mut got = calls.clone();
    got.sort_unstable();
    assert_eq!(got, want);
    assert_eq!(d.rows.iter().filter(|r| r.id.starts_with("p:")).count(), 3);
}
