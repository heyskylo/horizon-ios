use horizon_perf_format::v1::{Capture, HealthSample, Side};

/// Seconds one live health sample stands for.
const LIVE_STEP_S: f64 = 1.0;
/// Share of a trailing window the recorded data must cover for its mean to be reported.
const MIN_COVERAGE: f64 = 0.9;

pub fn live(capture: &Capture, side: Side) -> Vec<&HealthSample> {
    capture.health.iter().filter(|h| h.side() == side).collect()
}

pub fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = values.fold((0.0, 0u32), |(s, n), v| (s + v, n + 1));
    (n > 0).then(|| sum / f64::from(n))
}

pub fn max(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.reduce(f64::max)
}

type Span = (f64, f64);

/// Time-weighted mean over the window ending at `end`; live samples beat finer history tiers, which beat coarser ones; None under 90% coverage.
pub fn trailing_mean(
    capture: &Capture,
    side: Side,
    end: f64,
    window_s: f64,
    pick: fn(&HealthSample) -> Option<f32>,
) -> Option<f64> {
    let (lo, hi) = (end - window_s, end);
    let mut sources: Vec<Vec<(Span, f64)>> = Vec::new();
    sources.push(
        live(capture, side)
            .into_iter()
            .filter_map(|h| pick(h).map(|v| ((h.t, h.t + LIVE_STEP_S), f64::from(v))))
            .collect(),
    );
    let mut tiers: Vec<_> = capture
        .history
        .iter()
        .filter(|w| w.side() == side && w.step_s > 0.0)
        .collect();
    tiers.sort_by(|a, b| a.step_s.total_cmp(&b.step_s));
    for tier in tiers {
        let step = f64::from(tier.step_s);
        sources.push(
            tier.samples
                .iter()
                .enumerate()
                .filter_map(|(i, h)| {
                    let from = tier.t0 + step * i as f64;
                    pick(h).map(|v| ((from, from + step), f64::from(v)))
                })
                .collect(),
        );
    }

    let mut covered: Vec<Span> = Vec::new();
    let (mut weighted, mut length) = (0.0, 0.0);
    for source in sources {
        let mut taken: Vec<Span> = Vec::new();
        for ((from, to), value) in source {
            let clipped = (from.max(lo), to.min(hi));
            if clipped.0 >= clipped.1 {
                continue;
            }
            for (a, b) in uncovered(clipped, &covered) {
                weighted += value * (b - a);
                length += b - a;
                taken.push((a, b));
            }
        }
        covered.extend(taken);
        covered = merge(covered);
    }
    (length >= MIN_COVERAGE * window_s && length > 0.0).then(|| weighted / length)
}

fn uncovered(span: Span, covered: &[Span]) -> Vec<Span> {
    let mut pieces = vec![span];
    for &(from, to) in covered {
        pieces = pieces
            .into_iter()
            .flat_map(|(a, b)| {
                if to <= a || from >= b {
                    vec![(a, b)]
                } else {
                    let mut kept = Vec::new();
                    if from > a {
                        kept.push((a, from));
                    }
                    if to < b {
                        kept.push((to, b));
                    }
                    kept
                }
            })
            .collect();
    }
    pieces
}

fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<Span> = Vec::new();
    for (from, to) in spans {
        match merged.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    merged
}
