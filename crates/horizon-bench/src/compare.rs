//! Paired A/B comparison of benchmark blocks (docs/benchmark-report.md, "A/B runs").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{round, Frames};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    TooFewPairs,
    WithinNoise,
    Negligible,
    Slower,
    Faster,
}

/// B against A over paired blocks; lower milliseconds are better.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Paired {
    pub pairs: usize,
    pub a: f64,
    pub b: f64,
    /// Hodges-Lehmann estimate of b - a.
    pub diff: f64,
    /// Absent when `a` is zero.
    pub diff_pct: Option<f64>,
    /// Exact Wilcoxon signed-rank interval; absent below 6 pairs.
    pub ci95: Option<[f64; 2]>,
    pub confidence: Option<f64>,
    /// Median distance between neighbouring blocks of the same arm.
    pub noise: Option<f64>,
    pub verdict: Verdict,
}

type Getter = fn(&Frames) -> Option<f64>;

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// `(a, b)` per pair, `noise` left for the caller.
pub fn paired(pairs: &[(f64, f64)]) -> Paired {
    let n = pairs.len();
    let a = median(&pairs.iter().map(|p| p.0).collect::<Vec<_>>());
    let b = median(&pairs.iter().map(|p| p.1).collect::<Vec<_>>());
    let d: Vec<f64> = pairs.iter().map(|p| p.1 - p.0).collect();
    let mut walsh = Vec::with_capacity(n * (n + 1) / 2);
    for i in 0..n {
        for j in i..n {
            walsh.push((d[i] + d[j]) / 2.0);
        }
    }
    walsh.sort_by(f64::total_cmp);
    let diff = median(&walsh);
    let interval = interval(n, &walsh);
    let verdict = match interval {
        None => Verdict::TooFewPairs,
        Some(([lo, hi], _)) if lo <= 0.0 && 0.0 <= hi => Verdict::WithinNoise,
        Some(_) if diff.abs() < 0.05_f64.max(0.02 * a) => Verdict::Negligible,
        Some(([lo, _], _)) if lo > 0.0 => Verdict::Slower,
        Some(_) => Verdict::Faster,
    };
    Paired {
        pairs: n,
        a: round(a) + 0.0,
        b: round(b) + 0.0,
        diff: round(diff) + 0.0,
        diff_pct: (a != 0.0).then(|| round(100.0 * diff / a) + 0.0),
        ci95: interval.map(|([lo, hi], _)| [round(lo) + 0.0, round(hi) + 0.0]),
        confidence: interval.map(|(_, c)| round4(c)),
        noise: None,
        verdict,
    }
}

/// The widest symmetric interval of the sorted Walsh averages whose tails hold at most 2.5% each, and its confidence.
fn interval(n: usize, walsh: &[f64]) -> Option<([f64; 2], f64)> {
    let m = walsh.len();
    // Null probabilities of T+ (halving is exact in binary, so these are the counts over 2^n).
    let mut p = vec![0.0f64; m + 1];
    p[0] = 1.0;
    for r in 1..=n {
        for t in (0..=m).rev() {
            p[t] = (p[t] + if t >= r { p[t - r] } else { 0.0 }) / 2.0;
        }
    }
    let (mut k, mut tail, mut best) = (0, 0.0, 0.0);
    while k < m {
        tail += p[k];
        if tail > 0.025 {
            break;
        }
        k += 1;
        best = tail;
    }
    (k >= 1).then(|| ([walsh[k - 1], walsh[m - k]], 1.0 - 2.0 * best))
}

/// The result of an A/B run: pairs of blocks in B A A B order, recomputable from the report's `frames`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ab {
    pub switch: String,
    pub a: String,
    pub b: String,
    pub pairs: u32,
    pub block_s: f64,
    pub warmup_s: f64,
    /// Block 0 (measured before any switch) lies outside the later B blocks.
    pub first_block_outside_range: bool,
    /// By `gpu_p50_ms`, `cpu_p50_ms`, `frame_p50_ms`; a metric missing from any block is left out.
    pub result: BTreeMap<String, Paired>,
}

impl Ab {
    /// Pairs the `ab.a`/`ab.b` entries of `frames` by `repeat` (0, 1, 2...): pair k is blocks 2k and 2k+1.
    pub fn from_frames(
        switch: &str,
        a: &str,
        b: &str,
        block_s: f64,
        warmup_s: f64,
        frames: &[Frames],
    ) -> Result<Ab, String> {
        let mut blocks: Vec<&Frames> = frames
            .iter()
            .filter(|f| f.phase == "ab.a" || f.phase == "ab.b")
            .collect();
        blocks.sort_by_key(|f| f.repeat);
        if blocks.is_empty()
            || !blocks.len().is_multiple_of(2)
            || blocks
                .iter()
                .enumerate()
                .any(|(i, f)| f.repeat as usize != i)
        {
            return Err("A/B blocks must be numbered 0..N with N even".to_owned());
        }
        if blocks.chunks(2).any(|p| p[0].phase == p[1].phase) {
            return Err("each pair needs one ab.a and one ab.b block".to_owned());
        }
        let pairs = blocks.len() / 2;
        let metrics: [(&str, Getter); 3] = [
            ("gpu_p50_ms", |f| f.gpu_p50_ms),
            ("cpu_p50_ms", |f| f.cpu_p50_ms),
            ("frame_p50_ms", |f| Some(f.p50_ms)),
        ];
        let mut result = BTreeMap::new();
        let mut series: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for (name, get) in metrics {
            let Some(values) = blocks.iter().map(|f| get(f)).collect::<Option<Vec<f64>>>() else {
                continue;
            };
            let by_arm: Vec<(f64, f64)> = blocks
                .chunks(2)
                .zip(values.chunks(2))
                .map(|(p, v)| {
                    if p[0].phase == "ab.a" {
                        (v[0], v[1])
                    } else {
                        (v[1], v[0])
                    }
                })
                .collect();
            let mut stat = paired(&by_arm);
            let gaps: Vec<f64> = (1..values.len().saturating_sub(1))
                .step_by(2)
                .filter(|&i| blocks[i].phase == blocks[i + 1].phase)
                .map(|i| (values[i] - values[i + 1]).abs())
                .collect();
            stat.noise = (!gaps.is_empty()).then(|| round(median(&gaps)) + 0.0);
            result.insert(name.to_owned(), stat);
            series.insert(name, values);
        }
        let first_block_outside_range = series
            .get("gpu_p50_ms")
            .or_else(|| series.get("frame_p50_ms"))
            .is_some_and(|values| {
                let later: Vec<f64> = (1..values.len())
                    .filter(|&i| blocks[i].phase == "ab.b")
                    .map(|i| values[i])
                    .collect();
                blocks[0].phase == "ab.b"
                    && !later.is_empty()
                    && (values[0] < later.iter().copied().fold(f64::INFINITY, f64::min)
                        || values[0] > later.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            });
        Ok(Ab {
            switch: switch.to_owned(),
            a: a.to_owned(),
            b: b.to_owned(),
            pairs: pairs as u32,
            block_s,
            warmup_s,
            first_block_outside_range,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Kind, Report};

    // Expected values: exact-rational Walsh averages and a brute-force sign enumeration, run once outside this code.
    fn check(
        a: &[f64],
        b: &[f64],
        diff: f64,
        ci: Option<[f64; 2]>,
        conf: Option<f64>,
        verdict: Verdict,
    ) {
        let pairs: Vec<(f64, f64)> = a.iter().copied().zip(b.iter().copied()).collect();
        let p = paired(&pairs);
        assert_eq!(
            (p.pairs, p.diff, p.ci95, p.confidence, p.verdict),
            (a.len(), diff, ci, conf, verdict),
            "{b:?}"
        );
    }

    #[test]
    fn the_decided_vectors() {
        use Verdict::*;
        let z = |n: usize| vec![0.0; n];
        let up = |n: usize| (1..=n).map(|v| v as f64).collect::<Vec<_>>();
        check(&z(8), &up(8), 4.5, Some([2.0, 7.0]), Some(0.9609), Slower);
        let wobble = [-0.3, 0.2, -0.1, 0.4, -0.2, 0.1, 0.3, -0.4];
        check(
            &z(8),
            &wobble,
            0.0,
            Some([-0.3, 0.3]),
            Some(0.9609),
            WithinNoise,
        );
        check(&z(6), &up(6), 3.5, Some([1.0, 6.0]), Some(0.9688), Slower);
        check(&z(5), &up(5), 3.0, None, None, TooFewPairs);
        let a = [3.65, 3.70, 3.62, 3.68, 3.66, 3.71, 3.63, 3.69];
        let b = [5.63, 5.60, 5.70, 5.58, 5.65, 5.62, 5.66, 5.61];
        check(&a, &b, 1.96, Some([1.905, 2.03]), Some(0.9609), Slower);
        let tiny: Vec<f64> = (1..=8).map(|v| 10.0 + v as f64 / 100.0).collect();
        check(
            &[10.0; 8],
            &tiny,
            0.045,
            Some([0.02, 0.07]),
            Some(0.9609),
            Negligible,
        );
        let down: Vec<f64> = up(8).iter().map(|_| 0.0).collect();
        check(
            &up(8),
            &down,
            -4.5,
            Some([-7.0, -2.0]),
            Some(0.9609),
            Faster,
        );
        let mixed = [0.5, -0.2, 0.0, 0.3, 0.9, -0.1, 0.4];
        check(
            &z(7),
            &mixed,
            0.225,
            Some([-0.1, 0.65]),
            Some(0.9531),
            WithinNoise,
        );
        let ten = [0.3, -0.1, 0.8, 0.2, -0.5, 0.6, 0.1, 0.4, 0.7, 0.05];
        check(
            &z(10),
            &ten,
            0.25,
            Some([-0.025, 0.55]),
            Some(0.9512),
            WithinNoise,
        );
    }

    #[test]
    fn medians_percent_and_empty_input() {
        let a = [3.65, 3.70, 3.62, 3.68, 3.66, 3.71, 3.63, 3.69];
        let b = [5.63, 5.60, 5.70, 5.58, 5.65, 5.62, 5.66, 5.61];
        let p = paired(&a.iter().copied().zip(b).collect::<Vec<_>>());
        assert_eq!(
            (p.a, p.b, p.diff_pct, p.noise),
            (3.67, 5.625, Some(53.406), None)
        );
        assert_eq!(
            paired(&[(0.0, 1.0); 8]).diff_pct,
            None,
            "no percent of zero"
        );
        assert_eq!(paired(&[]).verdict, Verdict::TooFewPairs);
    }

    fn block(phase: &str, repeat: u32, frame: f64, cpu: Option<f64>, gpu: Option<f64>) -> Frames {
        Frames {
            phase: phase.into(),
            repeat,
            frames: 100,
            seconds: 4.0,
            avg_fps: 250.0,
            low_1pct_fps: 100.0,
            low_01pct_fps: 90.0,
            p50_ms: frame,
            p95_ms: frame + 1.0,
            p99_ms: frame + 2.0,
            p999_ms: frame + 3.0,
            max_ms: frame + 4.0,
            cpu_p50_ms: cpu,
            gpu_p50_ms: gpu,
            histogram: vec![0; crate::HISTOGRAM_EDGES_MS.len()],
        }
    }

    // B A A B B A A B B A A B, as gpu milliseconds; frame time is gpu + 1.
    fn twelve(first_b: f64) -> Vec<Frames> {
        let gpu = [
            first_b, 3.0, 3.2, 5.0, 5.2, 3.1, 3.1, 5.1, 5.4, 3.4, 3.3, 5.3,
        ];
        gpu.iter()
            .enumerate()
            .map(|(i, &g)| {
                block(
                    if matches!(i % 4, 0 | 3) {
                        "ab.b"
                    } else {
                        "ab.a"
                    },
                    i as u32,
                    g + 1.0,
                    Some(g / 2.0),
                    Some(g),
                )
            })
            .collect()
    }

    fn from(frames: &[Frames]) -> Result<Ab, String> {
        Ab::from_frames("gpu-culling", "off", "on", 4.0, 2.0, frames)
    }

    #[test]
    fn noise_and_the_first_block_flag() {
        let ab = from(&twelve(6.4)).unwrap();
        assert!(
            ab.first_block_outside_range,
            "6.4 is 1 ms above the other B blocks"
        );
        let gpu = &ab.result["gpu_p50_ms"];
        // Neighbours (1,2) (3,4) (5,6) (7,8) (9,10): 0.2 0.2 0 0.3 0.1.
        assert_eq!(
            (gpu.pairs, gpu.a, gpu.b, gpu.noise),
            (6, 3.15, 5.25, Some(0.2))
        );
        assert_eq!(
            (gpu.ci95, gpu.verdict),
            (Some([1.8, 3.4]), Verdict::Slower),
            "every d is positive: min and max Walsh averages"
        );
        assert_eq!(ab.result["cpu_p50_ms"].noise, Some(0.1));
        assert_eq!(
            (ab.pairs, ab.switch.as_str(), ab.a.as_str(), ab.b.as_str()),
            (6, "gpu-culling", "off", "on")
        );
        assert!(!from(&twelve(5.2)).unwrap().first_block_outside_range);
    }

    #[test]
    fn a_metric_missing_from_one_block_is_left_out() {
        let mut frames = twelve(5.2);
        frames[7].gpu_p50_ms = None;
        let ab = from(&frames).unwrap();
        assert_eq!(
            ab.result.keys().collect::<Vec<_>>(),
            ["cpu_p50_ms", "frame_p50_ms"]
        );
        let mut frames = twelve(6.4);
        frames[0].gpu_p50_ms = None;
        frames[0].p50_ms = 6.0;
        assert!(
            !from(&frames).unwrap().first_block_outside_range,
            "frame time 6.0 is inside 6.0..6.4 of the later B blocks"
        );
    }

    #[test]
    fn blocks_that_do_not_pair_are_refused() {
        assert!(from(&[]).is_err());
        assert!(from(&twelve(5.2)[..11]).is_err(), "odd number of blocks");
        let mut frames = twelve(5.2);
        frames[1].phase = "ab.b".into();
        assert!(from(&frames).is_err(), "a pair of two B blocks");
        let mut frames = twelve(5.2);
        frames[3].repeat = 9;
        assert!(from(&frames).is_err(), "a gap in the numbering");
    }

    #[test]
    fn a_report_with_ab_round_trips_and_old_ones_still_parse() {
        let mut r = Report::new(Kind::Game, App::default());
        let old = r.to_json();
        assert!(!old.contains("\"ab\""));
        assert_eq!(Report::parse(&old).unwrap().ab, None);
        r.frames = twelve(5.2);
        r.ab = Some(from(&r.frames).unwrap());
        let json = r.to_json();
        assert_eq!(Report::parse(&json).unwrap(), r);
        assert!(
            json.contains("\"first_block_outside_range\": false")
                && json.contains("\"verdict\": \"slower\"")
        );
        assert_eq!(r.for_upload().ab, r.ab);
        let mut few = paired(&[(1.0, 2.0); 3]);
        few.noise = None;
        assert!(serde_json::to_string(&few)
            .unwrap()
            .contains("\"ci95\":null,\"confidence\":null"));
    }
}
