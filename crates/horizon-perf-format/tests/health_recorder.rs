use horizon_perf_format::v1::{HealthWindow, Side};
use horizon_perf_format::{Gauges, HealthRecorder};

const START: f64 = 1000.0;
/// The MSPT steps from 10 to 30 here, in the middle of a second.
const STEP_AT: f64 = 2730.5;
/// The server runs at 5 ticks per second from the second 7050 to the second 7170.
const DIP: (u64, u64) = (7050, 7170);
const SECONDS: u64 = 7200;

fn rate(second: u64) -> u32 {
    if (DIP.0..DIP.1).contains(&second) {
        5
    } else {
        20
    }
}

fn near(a: f32, b: f64) {
    assert!((a as f64 - b).abs() < 1e-3, "{a} != {b}");
}

/// Two hours of ticks from `START`, each `10` ms until `STEP_AT` and `30` ms after.
fn two_hours() -> HealthRecorder {
    let mut rec = HealthRecorder::new(Side::Server, START);
    for s in 0..SECONDS {
        let n = rate(s);
        for j in 0..n {
            let t = s as f64 + j as f64 / n as f64;
            rec.record(START + t, if t < STEP_AT { 10.0 } else { 30.0 });
        }
    }
    rec.advance(START + SECONDS as f64);
    rec
}

fn window(windows: &[HealthWindow], step_s: f32) -> &HealthWindow {
    windows.iter().find(|w| w.step_s == step_s).unwrap()
}

/// Mean MSPT over seconds [a, b), computed by counting seconds: 10 before 2730, 20 inside it, 30 after.
fn expected_mspt(a: u64, b: u64) -> f64 {
    let low = 2730u64.saturating_sub(a).min(b - a);
    let mixed = u64::from(a <= 2730 && 2730 < b);
    let high = (b - a) - low - mixed;
    (10 * low + 20 * mixed + 30 * high) as f64 / (b - a) as f64
}

fn expected_tps(a: u64, b: u64) -> f64 {
    let dip = b.min(DIP.1).saturating_sub(a.max(DIP.0));
    (5 * dip + 20 * ((b - a) - dip)) as f64 / (b - a) as f64
}

#[test]
fn every_tier_holds_the_independent_means() {
    let rec = two_hours();
    let history = rec.history(START);
    for (step, cap) in [(1u64, 600usize), (10, 360), (60, 1440), (900, 672)] {
        let w = window(&history, step as f32);
        let n = cap.min((SECONDS / step) as usize);
        assert_eq!(w.samples.len(), n, "tier {step}");
        let secs = step;
        let first = SECONDS / secs - n as u64;
        assert_eq!(w.t0, (first * secs) as f64);
        for (k, s) in w.samples.iter().enumerate() {
            let (a, b) = ((first + k as u64) * secs, (first + k as u64 + 1) * secs);
            near(s.mspt_avg.unwrap(), expected_mspt(a, b));
            near(s.tps.unwrap(), expected_tps(a, b));
            let max = if b <= 2730 { 10.0 } else { 30.0 };
            near(s.mspt_max.unwrap(), max);
        }
    }
}

#[test]
fn tps_over_one_five_and_fifteen_minutes() {
    let tps = two_hours().tps();
    // Seconds 7140..7200: 30 at 5 TPS and 30 at 20.
    near(tps.one.unwrap(), (30.0 * 5.0 + 30.0 * 20.0) / 60.0);
    // Seconds 6900..7200: 120 at 5 TPS and 180 at 20.
    near(tps.five.unwrap(), (120.0 * 5.0 + 180.0 * 20.0) / 300.0);
    // Seconds 6300..7200: 120 at 5 TPS and 780 at 20.
    near(tps.fifteen.unwrap(), (120.0 * 5.0 + 780.0 * 20.0) / 900.0);
}

#[test]
fn history_times_are_relative_to_the_capture_start() {
    let rec = two_hours();
    let w = rec.history(START + SECONDS as f64);
    assert_eq!(window(&w, 1.0).t0, -600.0);
    assert_eq!(window(&w, 900.0).t0, -7200.0);
}

#[test]
fn seven_days_leave_the_rings_size_unchanged() {
    let mut rec = HealthRecorder::new(Side::Server, 0.0);
    rec.record(0.5, 1.0);
    let before = rec.memory_bytes();
    for s in 1..7 * 86400u64 {
        rec.record(s as f64 + 0.5, 1.0);
    }
    rec.advance(7.0 * 86400.0);
    assert_eq!(rec.memory_bytes(), before);
    assert!(before < 320_000, "{before}");
    let lens: Vec<usize> = rec.history(0.0).iter().map(|w| w.samples.len()).collect();
    assert_eq!(lens, [600, 360, 1440, 672]);
}

#[test]
fn a_second_summarises_its_ticks_by_nearest_rank() {
    let mut rec = HealthRecorder::new(Side::Server, 0.0);
    for ms in 1..=20 {
        rec.record(ms as f64 / 100.0, ms as f32);
    }
    rec.advance(1.0);
    let (samples, cursor) = rec.samples_since(0, 0.0);
    assert_eq!(cursor, 1);
    let s = &samples[0];
    assert_eq!(s.tps, Some(20.0));
    assert_eq!(s.mspt_avg, Some(10.5));
    assert_eq!(s.mspt_p50, Some(10.0));
    assert_eq!(s.mspt_p95, Some(19.0));
    assert_eq!(s.mspt_max, Some(20.0));
    assert_eq!(s.fps, None);
}

#[test]
fn a_stalled_server_records_zero_tps_for_the_silent_seconds() {
    let mut rec = HealthRecorder::new(Side::Server, 10.0);
    rec.record(10.5, 40.0);
    rec.record(14.5, 40.0);
    rec.advance(15.0);
    let (samples, _) = rec.samples_since(0, 10.0);
    let tps: Vec<_> = samples.iter().map(|s| s.tps).collect();
    assert_eq!(tps, [Some(1.0), Some(0.0), Some(0.0), Some(0.0), Some(1.0)]);
    assert_eq!(samples[2].mspt_avg, None);
    let t: Vec<_> = samples.iter().map(|s| s.t).collect();
    assert_eq!(t, [0.0, 1.0, 2.0, 3.0, 4.0]);
    let (again, _) = rec.samples_since(5, 10.0);
    assert!(again.is_empty());
}

#[test]
fn a_client_fills_the_frame_fields_and_carries_the_gauges() {
    let mut rec = HealthRecorder::new(Side::Client, 0.0);
    rec.set_gauges(Gauges {
        rss_mb: 2048.0,
        players: 3,
        frame_gpu_ms_p50: Some(4.5),
        ..Gauges::default()
    });
    for i in 0..100 {
        rec.record(i as f64 / 100.0, if i < 99 { 5.0 } else { 90.0 });
    }
    rec.advance(1.0);
    let (samples, _) = rec.samples_since(0, 0.0);
    let s = &samples[0];
    assert_eq!(s.fps, Some(100.0));
    assert_eq!(s.frame_ms_p50, Some(5.0));
    assert_eq!(s.frame_ms_p99, Some(5.0));
    assert_eq!(s.frame_ms_max, Some(90.0));
    assert_eq!(s.tps, None);
    assert_eq!(
        (s.rss_mb, s.players, s.frame_gpu_ms_p50),
        (2048.0, 3, Some(4.5))
    );
    assert_eq!(s.allocs_per_s, None);
}
