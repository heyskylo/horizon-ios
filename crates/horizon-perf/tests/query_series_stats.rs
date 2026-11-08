mod common;

use common::{SERVER, start, stop};
use horizon_perf::query::{Loaded, Row};
use horizon_perf::request::QueryRequest;
use horizon_perf::wire::Op;
use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

fn loaded(values: &[f32]) -> [Loaded; 1] {
    let mut fields = vec![start(SERVER, Kind::Dedicated)];
    for (t, value) in values.iter().enumerate() {
        fields.push(Field::Health(HealthSample {
            side: SERVER,
            t: t as f64,
            mspt_p95: Some(*value),
            ..Default::default()
        }));
    }
    fields.push(stop(SERVER, values.len() as f64));
    let capture = horizon_perf::read_capture(&common::write_capture(&fields).bytes).unwrap();
    [Loaded {
        file: "stats.hzperf".into(),
        capture: std::sync::Arc::new(capture),
    }]
}

fn ask(loaded: &[Loaded], every: f64, stats: bool) -> horizon_perf::query::rows::SeriesRow {
    let request = QueryRequest {
        every: Some(every),
        stats,
        ..QueryRequest::default()
    };
    let query = request
        .into_query(Op::Series, vec!["h:mspt_p95".into()])
        .unwrap();
    let reply = horizon_perf::ops::query_loaded(&query, loaded);
    let Some(Row::Series(row)) = reply.envelope.rows().first().cloned() else {
        panic!("no series row")
    };
    row
}

#[test]
fn stats_state_the_window_mean_and_the_order_statistics_of_the_points() {
    let capture = loaded(&[10.0, 20.0, 30.0, 40.0, 100.0]);
    // Steps [0,2) [2,4) [4,5) average 15, 35 and 100. Each sample stands for one second, so the window's mean is 200 / 5 s = 40, not the points' 50.
    let row = ask(&capture, 2.0, true);
    assert_eq!(row.values, [Some(15.0), Some(35.0), Some(100.0)]);
    let stats = row.stats.unwrap();
    assert_eq!(
        (
            stats.min,
            stats.mean,
            stats.median,
            stats.p95,
            stats.max,
            stats.last,
            stats.points
        ),
        (15.0, 40.0, 35.0, 35.0, 100.0, 100.0, 3)
    );
}

#[test]
fn an_even_number_of_points_has_the_median_between_the_middle_two() {
    let capture = loaded(&[10.0, 20.0, 30.0, 40.0]);
    let stats = ask(&capture, 1.0, true).stats.unwrap();
    assert_eq!((stats.median, stats.p95, stats.mean), (25.0, 30.0, 25.0));
}

#[test]
fn a_request_without_stats_leaves_them_out() {
    let capture = loaded(&[10.0, 20.0]);
    assert_eq!(ask(&capture, 1.0, false).stats, None);
}
