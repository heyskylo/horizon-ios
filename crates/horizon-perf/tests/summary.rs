mod common;

use common::{CLIENT, SERVER, write_capture};
use horizon_perf::{Error, SummaryOptions, read_capture, summarize};
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

const NAME: &str = "profile-20261107-143204Z.hzperf";

fn server_start() -> Field {
    Field::Start(Start {
        side: SERVER,
        kind: Kind::Dedicated as i32,
        build: Some(Build {
            version: "0.42.1".into(),
            commit: "1d63e0a5a3".into(),
            profile: "release".into(),
            ..Default::default()
        }),
        machine: Some(Machine {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu_threads: 24,
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn tps_only(side: i32, tps: f32) -> HealthSample {
    HealthSample {
        side,
        tps: Some(tps),
        ..Default::default()
    }
}

fn server_samples() -> Vec<Field> {
    (0..10)
        .map(|t| {
            let mut h = tps_only(SERVER, if t < 5 { 20.0 } else { 10.0 });
            h.t = f64::from(t);
            h.mspt_avg = Some(if t < 5 { 40.0 } else { 80.0 });
            h.mspt_p95 = Some(if t == 7 { 120.5 } else { 90.0 });
            h.mspt_max = Some(if t == 3 { 312.4 } else { 200.0 });
            h.cpu_process_cores = if t % 2 == 0 { 2.0 } else { 4.0 };
            h.cpu_system_cores = 8.0;
            h.rss_mb = match t {
                0 => 1000.0,
                9 => 1212.0,
                _ => 1100.0,
            };
            h.players = if t == 9 { 3 } else { 1 };
            h.chunks = if t == 9 { 61_842 } else { 100 };
            h.entities = if t == 9 { 9_310 } else { 100 };
            Field::Health(h)
        })
        .collect()
}

fn tier(step_s: f32, t0: f64, values: &[f32]) -> Field {
    Field::History(HealthWindow {
        side: SERVER,
        step_s,
        t0,
        samples: values.iter().map(|&v| tps_only(SERVER, v)).collect(),
    })
}

fn history() -> Vec<Field> {
    let mut coarse = vec![20.0; 13];
    coarse.push(18.0);
    vec![tier(60.0, -840.0, &coarse), tier(1.0, -10.0, &[10.0; 10])]
}

fn client_fields() -> Vec<Field> {
    let start = Field::Start(Start {
        side: CLIENT,
        kind: Kind::Client as i32,
        ..Default::default()
    });
    let sample = |t: f64, fps, p50, p95, p99, max, cpu, rss, chunks| {
        Field::Health(HealthSample {
            side: CLIENT,
            t,
            fps: Some(fps),
            frame_ms_p50: Some(p50),
            frame_ms_p95: Some(p95),
            frame_ms_p99: Some(p99),
            frame_ms_max: Some(max),
            cpu_process_cores: cpu,
            cpu_system_cores: 6.0,
            rss_mb: rss,
            chunks,
            ..Default::default()
        })
    };
    vec![
        start,
        sample(0.0, 100.0, 5.0, 9.0, 15.0, 30.0, 1.5, 2000.0, 400),
        sample(1.0, 120.0, 7.0, 12.0, 20.0, 45.5, 2.5, 2048.0, 500),
    ]
}

fn read(fields: Vec<Field>) -> Capture {
    read_capture(&write_capture(&fields).bytes).unwrap()
}

fn server_capture() -> Capture {
    let mut fields = vec![server_start()];
    fields.extend(history());
    fields.extend(server_samples());
    fields.push(common::stop(SERVER, 10.0));
    read(fields)
}

const SERVER_TEXT: &str = "\
profile-20261107-143204Z.hzperf: dedicated server, 10.0 s, Horizon 0.42.1 (1d63e0a5a3) release, Linux x86_64, 24 threads
health  TPS 15.0/20 (1m 16.2, 5m 19.2, 15m 19.7) · MSPT avg 60.0 · worst-second p95 120.5 · max 312.4 ms
        CPU 3.0 cores (system 8.0 of 24) · RSS 1.2 GB (+212 MB over the window) · 3 players · 61,842 chunks · 9,310 entities
";

const CLIENT_TEXT: &str = "\
profile-20261107-143204Z.hzperf: client, 2.0 s
health  FPS avg 110.0 · frame p50 6.0 · worst-second p95 12.0 · worst-second p99 20.0 · max 45.5 ms
        CPU 2.0 cores (system 6.0) · RSS 2.0 GB (+48 MB over the window) · 500 chunks
";

#[test]
fn a_server_capture_gets_its_header_and_health_with_trailing_means_from_every_tier() {
    let text = summarize(&server_capture(), NAME, &SummaryOptions::default()).unwrap();
    assert_eq!(text, SERVER_TEXT);
}

#[test]
fn without_history_a_trailing_mean_the_data_does_not_cover_is_left_out() {
    let mut fields = vec![server_start()];
    fields.extend(server_samples());
    fields.push(common::stop(SERVER, 10.0));
    let text = summarize(&read(fields), NAME, &SummaryOptions::default()).unwrap();
    assert!(
        text.contains("health  TPS 15.0/20 · MSPT avg 60.0"),
        "{text}"
    );
}

#[test]
fn a_client_capture_reports_frames_not_ticks() {
    let mut fields = client_fields();
    fields.push(common::stop(CLIENT, 2.0));
    let text = summarize(&read(fields), NAME, &SummaryOptions::default()).unwrap();
    assert_eq!(text, CLIENT_TEXT);
}

fn both_sides() -> Capture {
    let mut fields = vec![server_start()];
    fields.extend(history());
    fields.extend(server_samples());
    fields.extend(client_fields());
    fields.push(common::stop(CLIENT, 2.0));
    fields.push(common::stop(SERVER, 10.0));
    read(fields)
}

#[test]
fn both_sides_each_get_their_own_header_and_health_server_first() {
    let text = summarize(&both_sides(), NAME, &SummaryOptions::default()).unwrap();
    assert_eq!(text, format!("{SERVER_TEXT}{CLIENT_TEXT}"));
}

#[test]
fn the_side_option_picks_one_side_and_a_missing_side_is_an_error() {
    let only = |side| SummaryOptions {
        side: Some(side),
        ..Default::default()
    };
    assert_eq!(
        summarize(&both_sides(), NAME, &only(Side::Client)).unwrap(),
        CLIENT_TEXT
    );
    assert_eq!(
        summarize(&server_capture(), NAME, &only(Side::Client)),
        Err(Error::NoSuchSide { side: Side::Client })
    );
}

#[test]
fn a_budget_cuts_rows_from_the_bottom_and_never_mid_row() {
    let full = format!("{SERVER_TEXT}{CLIENT_TEXT}");
    let options = |budget_tokens| SummaryOptions {
        budget_tokens,
        ..Default::default()
    };
    let one_row_short = summarize(&both_sides(), NAME, &options(full.len() / 4 - 1)).unwrap();
    let expected: Vec<&str> = full.lines().collect();
    assert_eq!(
        one_row_short,
        format!("{}\n", expected[..expected.len() - 1].join("\n"))
    );
    let headers_only = summarize(&both_sides(), NAME, &options(0)).unwrap();
    assert_eq!(headers_only, format!("{}\n{}\n", expected[0], expected[3]));
}

#[test]
fn a_side_with_no_health_samples_says_so() {
    let capture = read(vec![server_start(), common::stop(SERVER, 4.0)]);
    let text = summarize(&capture, NAME, &SummaryOptions::default()).unwrap();
    assert_eq!(
        text,
        "profile-20261107-143204Z.hzperf: dedicated server, 4.0 s, Horizon 0.42.1 (1d63e0a5a3) release, Linux x86_64, 24 threads\nhealth  no health samples in this capture\n"
    );
}
