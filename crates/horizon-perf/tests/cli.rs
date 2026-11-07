mod common;

use std::fs::{self, File};
use std::process::Command;
use std::time::{Duration, SystemTime};

use common::{SERVER, TempDir, health, start, stop, write_capture};
use horizon_perf::cli::{CliError, Env, run};
use horizon_perf::watch::{Event, HealthNote, SlowNote, SlowSpan};
use horizon_perf::wire::{SideName, UnitKind};
use horizon_perf_format::v1::*;
use horizon_perf_format::{CaptureWriter, Field};

fn env(home: Option<&std::path::Path>) -> Env {
    Env {
        horizon_home: home.map(Into::into),
        poll: Duration::from_millis(2),
    }
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn output(parts: &[&str], env: &Env) -> Result<String, CliError> {
    let mut out = Vec::new();
    run(&args(parts), env, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

fn watched_fields() -> Vec<Field> {
    vec![
        start(SERVER, Kind::Dedicated),
        health(SERVER, 1.0, 20.0),
        Field::String(StringEntry {
            side: SERVER,
            id: 5,
            text: "tick_lan".into(),
        }),
        health(SERVER, 2.0, 19.5),
        Field::Slow(SlowEvent {
            side: SERVER,
            t: 2.5,
            unit: Unit::Tick as i32,
            counter: 48812,
            wall_ms: 312.4,
            top: vec![
                SpanCost {
                    name: 5,
                    wall_ms: 180.2,
                    ..Default::default()
                },
                SpanCost {
                    name: 9,
                    wall_ms: 61.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        Field::Mark(Mark {
            side: SERVER,
            t: 2.75,
            name: "after the fix".into(),
        }),
        health(SERVER, 3.0, 10.0),
        stop(SERVER, 3.0),
    ]
}

const WATCHED_LINES: [&str; 7] = [
    "   0.0s  server  capture started (dedicated)",
    "   1.0s  server  TPS 20.0 · RSS 0 MB · 0 players",
    "   2.0s  server  TPS 19.5 · RSS 0 MB · 0 players",
    "   2.5s  server  slow tick 48812: 312.4 ms (tick_lan 180.2, #9 61.0)",
    "   2.8s  server  mark after the fix",
    "   3.0s  server  TPS 10.0 · RSS 0 MB · 0 players",
    "   3.0s  server  stopped (normal)",
];

fn lines(skip: &[usize]) -> String {
    WATCHED_LINES
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip.contains(i))
        .map(|(_, l)| format!("{l}\n"))
        .collect()
}

#[test]
fn follow_prints_health_slow_events_marks_and_the_end_of_a_finished_file() {
    let dir = TempDir::new("follow-done");
    let path = dir.path().join("a.hzperf");
    fs::write(&path, write_capture(&watched_fields()).bytes).unwrap();
    let text = output(&["follow", path.to_str().unwrap()], &env(None)).unwrap();
    assert_eq!(text, lines(&[]));
}

#[test]
fn every_keeps_health_lines_at_least_that_far_apart_but_never_drops_slow_events() {
    let dir = TempDir::new("follow-every");
    let path = dir.path().join("a.hzperf");
    fs::write(&path, write_capture(&watched_fields()).bytes).unwrap();
    let text = output(
        &["follow", path.to_str().unwrap(), "--every", "2"],
        &env(None),
    )
    .unwrap();
    assert_eq!(text, lines(&[2]));
}

#[test]
fn follow_json_prints_one_object_per_line() {
    let dir = TempDir::new("follow-json");
    let path = dir.path().join("a.hzperf");
    fs::write(&path, write_capture(&watched_fields()).bytes).unwrap();
    let text = output(&["follow", path.to_str().unwrap(), "--json"], &env(None)).unwrap();
    let lines: Vec<Event> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 7);
    assert_eq!(
        lines[1],
        Event::Health(HealthNote {
            side: SideName::Server,
            t: 1.0,
            rss_mb: 0.0,
            tps: Some(20.0),
            mspt_avg: None,
            mspt_p95: None,
            mspt_max: None,
            fps: None,
            frame_ms_p95: None,
            players: Some(0),
        })
    );
    assert_eq!(
        lines[3],
        Event::Slow(SlowNote {
            side: SideName::Server,
            t: 2.5,
            unit: UnitKind::Tick,
            counter: 48812,
            wall_ms: f64::from(312.4f32),
            top: vec![
                SlowSpan {
                    name: "tick_lan".into(),
                    wall_ms: f64::from(180.2f32)
                },
                SlowSpan {
                    name: "#9".into(),
                    wall_ms: 61.0
                },
            ],
        })
    );
}

#[test]
fn follow_streams_a_file_that_is_still_growing_and_ends_at_the_stop() {
    let dir = TempDir::new("follow-grow");
    let path = dir.path().join("profile-20261107-143204Z.hzperf");
    let file = File::create(&path).unwrap();
    let writer = std::thread::spawn(move || {
        let mut writer = CaptureWriter::new(file).unwrap();
        for field in watched_fields() {
            std::thread::sleep(Duration::from_millis(15));
            writer.write(field).unwrap();
        }
        writer.finish().unwrap();
    });
    let text = output(
        &["follow", "--dir", dir.path().to_str().unwrap()],
        &env(None),
    )
    .unwrap();
    writer.join().unwrap();
    assert_eq!(text, lines(&[]));
}

#[test]
fn summary_of_latest_reads_the_horizon_home_reports_folder_and_skips_a_file_being_written() {
    let home = TempDir::new("home");
    let reports = home.path().join("reports");
    fs::create_dir(&reports).unwrap();
    let put = |name: &str, fields: &[Field], age_s: u64| {
        let path = reports.join(name);
        fs::write(&path, write_capture(fields).bytes).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_s);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    };
    put(
        "old.hzperf",
        &[
            start(SERVER, Kind::Dedicated),
            health(SERVER, 0.0, 20.0),
            stop(SERVER, 1.0),
        ],
        100,
    );
    put(
        "live.hzperf",
        &[start(SERVER, Kind::Dedicated), health(SERVER, 0.0, 20.0)],
        10,
    );

    let text = output(&["summary", "latest"], &env(Some(home.path()))).unwrap();
    assert!(
        text.starts_with("old.hzperf: dedicated server, 1.0 s\n"),
        "{text}"
    );
    let listing = output(&["list"], &env(Some(home.path()))).unwrap();
    let lines: Vec<&str> = listing.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("live.hzperf  ") && lines[0].ends_with("writing  dedicated"));
    assert!(
        lines[1].starts_with("old.hzperf  ") && lines[1].ends_with("finished  dedicated 1.0 s")
    );
}

#[test]
fn the_binary_refuses_an_unfinished_file_with_the_message_and_exit_code_1() {
    let dir = TempDir::new("bin");
    let path = dir.path().join("live.hzperf");
    fs::write(
        &path,
        write_capture(&[start(SERVER, Kind::Dedicated)]).bytes,
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_horizon-perf"))
        .args(["summary", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "horizon-perf: this capture has no end for its server side: it is still being written or the game stopped\n"
    );
    assert!(result.stdout.is_empty());

    let result = Command::new(env!("CARGO_BIN_EXE_horizon-perf"))
        .arg("frobnicate")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
}
