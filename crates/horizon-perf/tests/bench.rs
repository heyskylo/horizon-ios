mod common;

use std::io::{Read, Write};

use common::{CLIENT, TempDir, query, save, start, stop, write_capture};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use horizon_bench::{App, Frames, Hardware, Kind as BenchKind, Report};
use horizon_perf::attach::attach;
use horizon_perf::cli::{CliError, Env, run};
use horizon_perf::query::Row;
use horizon_perf::query::rows::BenchRow;
use horizon_perf::{Error, read_capture};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;
use prost::Message;

fn report() -> Report {
    let app = App {
        name: "horizon-client".into(),
        version: "26.3.1".into(),
        ..Default::default()
    };
    let mut r = Report::with_hardware(BenchKind::Game, app, Hardware::default());
    r.conditions.scenario = Some("village".into());
    r.metric("startup.title_ms", &[100.0, 120.0]);
    r.metric("memory.rss_peak_mb", &[2048.0]);
    // 100 frames at 8 ms, then 100 at 10 ms: p50 is the 100th value, 8.
    let times: Vec<f32> = (0..200).map(|i| if i < 100 { 8.0 } else { 10.0 }).collect();
    r.frames
        .push(Frames::of("village", 0, &times, &[], &[]).unwrap());
    r
}

fn bench_rows(reply: &horizon_perf::wire::Envelope<Row>) -> Vec<&BenchRow> {
    reply
        .rows()
        .iter()
        .map(|row| match row {
            Row::Bench(b) => b,
            other => panic!("not a bench row: {other:?}"),
        })
        .collect()
}

fn inflate(gzip: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(gzip).read_to_end(&mut out).unwrap();
    out
}

fn deflate(raw: &[u8]) -> Vec<u8> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(raw).unwrap();
    gz.finish().unwrap()
}

fn field(capture: Capture) -> Vec<u8> {
    capture.encode_to_vec()
}

/// A finished capture's inflated bytes with a field number no reader knows between the start and the stop.
fn capture_with_unknown_field() -> Vec<u8> {
    let mut raw = field(Capture {
        format: 1,
        ..Default::default()
    });
    raw.extend(field(Capture {
        start: vec![Start {
            side: CLIENT,
            kind: Kind::Client as i32,
            ..Default::default()
        }],
        ..Default::default()
    }));
    // Field 99, length-delimited: the tag 99 << 3 | 2 = 794 is the varint 0x9a 0x06.
    raw.extend([0x9a, 0x06, 0x06, b'f', b'u', b't', b'u', b'r', b'e']);
    raw.extend(field(Capture {
        stop: vec![Stop {
            side: CLIENT,
            t: 5.0,
            reason: StopReason::Normal as i32,
            ..Default::default()
        }],
        ..Default::default()
    }));
    raw
}

fn attachment(name: &str, body: &str) -> Attachment {
    Attachment {
        name: name.into(),
        media_type: "application/json".into(),
        body: body.into(),
    }
}

#[test]
fn a_report_written_by_horizon_bench_reads_through_horizon_perf_with_equal_metrics() {
    let dir = TempDir::new("bench-bare");
    let report = report();
    let written = report.write_to(dir.path()).unwrap();
    let path = written.to_string_lossy().into_owned();

    let reply = query(&["bench", &path]);
    assert!(reply.ok, "{reply:?}");
    let [row] = bench_rows(&reply)[..] else {
        panic!("one report, one row: {reply:?}");
    };
    assert_eq!(row.metrics, report.metrics);
    assert_eq!(row.frames, report.frames);
    assert_eq!(row.scenario.as_deref(), Some("village"));
    assert_eq!(row.format, "horizon-benchmark-report");
    // Hand-computed: two values 100 and 120 have mean 110 and min 100.
    let title = &row.metrics["startup.title_ms"];
    assert_eq!(
        (title.n, title.mean, title.min, title.max),
        (2, 110.0, 100.0, 120.0)
    );
    assert_eq!(row.frames[0].p50_ms, 8.0);
}

#[test]
fn a_bare_report_answers_meta_and_summary_like_a_capture() {
    let dir = TempDir::new("bench-bare-summary");
    let path = report()
        .write_to(dir.path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let meta = query(&["meta", &path]);
    assert!(meta.ok, "{meta:?}");
    let args: Vec<String> = ["summary", &path].iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let env = Env {
        horizon_home: None,
        poll: std::time::Duration::from_millis(2),
    };
    run(&args, &env, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("bench   village · Horizon 26.3.1"), "{text}");
    assert!(
        text.contains("bench   village: p50 8.00 ms · p95 10.00 ms"),
        "{text}"
    );
}

#[test]
fn a_capture_carries_the_report_verbatim_and_queries_read_it_back() {
    let report = report();
    let fields = vec![
        start(CLIENT, Kind::Client),
        Field::Bench(BenchResult {
            side: CLIENT,
            format: horizon_bench::FORMAT.into(),
            report_json: report.to_json(),
            corpus_id: String::new(),
        }),
        stop(CLIENT, 5.0),
    ];
    let capture = read_capture(&write_capture(&fields).bytes).unwrap();
    assert_eq!(capture.bench[0].report_json, report.to_json());
    assert_eq!(
        Report::parse(&capture.bench[0].report_json).unwrap(),
        report
    );
    let dir = TempDir::new("bench-capture");
    let path = save(&dir, "profile-20261108-120000Z.hzperf", &fields);
    let reply = query(&["bench", &path]);
    assert_eq!(bench_rows(&reply)[0].metrics, report.metrics);
}

#[test]
fn a_capture_without_a_report_says_so() {
    let dir = TempDir::new("bench-none");
    let path = save(
        &dir,
        "a.hzperf",
        &[start(CLIENT, Kind::Client), stop(CLIENT, 1.0)],
    );
    let reply = query(&["bench", &path]);
    assert!(reply.ok);
    assert!(reply.rows().is_empty());
    assert_eq!(
        reply.warnings,
        Some(vec!["this capture holds no benchmark report".to_string()])
    );
}

#[test]
fn attach_keeps_a_field_number_it_does_not_know() {
    let original = capture_with_unknown_field();
    let file = deflate(&original);
    read_capture(&file).expect("the unknown field is skipped by readers");

    let rewritten = attach(&file, &attachment("run", "{}")).unwrap();

    // Attachment { name = 1: "run", media_type = 2: "application/json", body = 3: "{}" }, 27 bytes, as field 22.
    let mut tail = vec![0xb2, 0x01, 0x1b, 0x0a, 0x03];
    tail.extend(b"run");
    tail.extend([0x12, 0x10]);
    tail.extend(b"application/json");
    tail.extend([0x1a, 0x02]);
    tail.extend(b"{}");
    let mut expected = original.clone();
    expected.extend(tail);
    assert_eq!(inflate(&rewritten), expected);

    let capture = read_capture(&rewritten).unwrap();
    assert_eq!(capture.attachments, [attachment("run", "{}")]);
    assert_eq!(capture.stop.len(), 1);
}

#[test]
fn attach_refuses_a_capture_that_is_not_finished_and_a_name_it_already_has() {
    let full = capture_with_unknown_field();
    let stop_len = field(Capture {
        stop: vec![Stop {
            side: CLIENT,
            t: 5.0,
            reason: StopReason::Normal as i32,
            ..Default::default()
        }],
        ..Default::default()
    })
    .len();
    let unfinished = deflate(&full[..full.len() - stop_len]);
    assert!(matches!(
        attach(&unfinished, &attachment("run", "{}")),
        Err(Error::Unfinished { .. })
    ));
    let once = attach(&deflate(&full), &attachment("run", "{}")).unwrap();
    assert!(matches!(
        attach(&once, &attachment("run", "{\"again\":1}")),
        Err(Error::DuplicateAttachment { name }) if name == "run"
    ));
    let twice = attach(&once, &attachment("other", "[1]")).unwrap();
    let names: Vec<_> = read_capture(&twice)
        .unwrap()
        .attachments
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(names, ["run", "other"]);
}

#[test]
fn the_attach_command_rewrites_the_file_in_place() {
    let dir = TempDir::new("attach-cli");
    let path = dir.path().join("a.hzperf");
    std::fs::write(&path, deflate(&capture_with_unknown_field())).unwrap();
    let body = dir.path().join("run.json");
    std::fs::write(&body, r#"{"server":"horizon","bots":25}"#).unwrap();
    let env = Env {
        horizon_home: None,
        poll: std::time::Duration::from_millis(2),
    };
    let cli = |parts: &[&str]| {
        let args: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        run(&args, &env, &mut out).map(|()| String::from_utf8(out).unwrap())
    };
    let file = path.to_str().unwrap();
    let said = cli(&[
        "attach",
        file,
        "--name",
        "server-bench-run",
        "--body",
        body.to_str().unwrap(),
    ])
    .unwrap();
    assert_eq!(said, "Attached server-bench-run (30 bytes) to a.hzperf\n");
    let capture = read_capture(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(capture.attachments[0].name, "server-bench-run");
    assert_eq!(capture.attachments[0].media_type, "application/json");
    assert_eq!(
        capture.attachments[0].body,
        r#"{"server":"horizon","bots":25}"#
    );

    std::fs::write(&body, "not json").unwrap();
    let refused = cli(&[
        "attach",
        file,
        "--name",
        "x",
        "--body",
        body.to_str().unwrap(),
    ]);
    assert!(matches!(refused, Err(CliError::Failed(m)) if m.contains("is not JSON")));
    assert!(matches!(
        cli(&["attach", file, "--name", "x"]),
        Err(CliError::Usage(_))
    ));
    let still = read_capture(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(still.attachments.len(), 1);
}
