//! The statistics of docs/perf-platform.md §6.6: block values compared with Hodges-Lehmann and exact Mann-Whitney, drift by Theil-Sen.

/// Fewer blocks than this on a side and no verdict is given.
pub const MIN_BLOCKS: usize = 6;
/// Blocks per side the exact distributions are built for; callers merge neighbouring blocks above it.
pub const MAX_BLOCKS: usize = 32;
/// A drift counts when the fitted line moves the metric by more than this share of its median.
const DRIFT_SHARE: f64 = 0.05;
const NEGLIGIBLE_MS: f64 = 0.05;
const NEGLIGIBLE_SHARE: f64 = 0.02;

pub use crate::wire::Verdict;

impl Verdict {
    /// The summary's one-word form: `~` for within noise or negligible, `?` for too few blocks.
    pub fn mark(self) -> &'static str {
        match self {
            Verdict::WithinNoise | Verdict::Negligible => "~",
            Verdict::TooFewBlocks => "?",
            other => other.as_str(),
        }
    }
}

pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    Some(median_sorted(&v))
}

fn median_sorted(v: &[f64]) -> f64 {
    match v.len() {
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// Counts of the arrangements of `m` and `n` values by Mann-Whitney U: the coefficients of the Gaussian binomial [m+n choose m].
fn mann_whitney_counts(m: usize, n: usize) -> Vec<u128> {
    let len = m * n + 1;
    let mut c = vec![0i128; len];
    c[0] = 1;
    for k in 1..=m {
        for u in k..len {
            c[u] += c[u - k];
        }
        for u in (n + k..len).rev() {
            c[u] -= c[u - (n + k)];
        }
    }
    c.into_iter().map(|v| v as u128).collect()
}

/// Counts of the permutations of `n` values by their number of inversions.
fn inversion_counts(n: usize) -> Vec<u128> {
    let len = n * n.saturating_sub(1) / 2 + 1;
    let mut c = vec![0u128; len];
    c[0] = 1;
    for k in 1..=n {
        let mut next = vec![0u128; len];
        let mut window = 0u128;
        for u in 0..len {
            window += c[u];
            if u >= k {
                window -= c[u - k];
            }
            next[u] = window;
        }
        c = next;
    }
    c
}

/// The most tail positions whose total probability stays within 2.5%, and that probability as (count, total).
fn tail_rank(counts: &[u128]) -> (usize, u128, u128) {
    let total: u128 = counts.iter().sum();
    let (mut k, mut cum) = (0, 0u128);
    while k < counts.len() && (cum + counts[k]) * 40 <= total {
        cum += counts[k];
        k += 1;
    }
    (k, cum, total)
}

/// The largest U with P(U <= u) <= 2.5% for samples of `m` and `n`: the published two-sided 5% critical value.
pub fn critical_u(m: usize, n: usize) -> Option<usize> {
    let (k, _, _) = tail_rank(&mann_whitney_counts(m, n));
    k.checked_sub(1)
}

/// B against A over their block values; lower is better.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    pub a: f64,
    pub b: f64,
    /// Hodges-Lehmann shift: the median of every b - a.
    pub effect: f64,
    /// Exact Mann-Whitney interval of the shift.
    pub ci: Option<[f64; 2]>,
    /// The interval's actual coverage, at least 95%.
    pub confidence: Option<f64>,
    pub verdict: Verdict,
}

fn last_blocks(values: &[f64]) -> &[f64] {
    &values[values.len().saturating_sub(MAX_BLOCKS)..]
}

/// None when a side has no blocks.
pub fn compare(a: &[f64], b: &[f64]) -> Option<Comparison> {
    let (a, b) = (last_blocks(a), last_blocks(b));
    let (median_a, median_b) = (median(a)?, median(b)?);
    let mut diffs: Vec<f64> = a
        .iter()
        .flat_map(|&x| b.iter().map(move |&y| y - x))
        .collect();
    diffs.sort_by(f64::total_cmp);
    let effect = median_sorted(&diffs);
    let (k, cum, total) = tail_rank(&mann_whitney_counts(a.len(), b.len()));
    let interval = (k >= 1).then(|| ([diffs[k - 1], diffs[diffs.len() - k]], cum, total));
    let verdict = match interval {
        _ if a.len() < MIN_BLOCKS || b.len() < MIN_BLOCKS => Verdict::TooFewBlocks,
        None => Verdict::TooFewBlocks,
        Some(([lo, hi], ..)) if lo <= 0.0 && 0.0 <= hi => Verdict::WithinNoise,
        Some(_) if effect.abs() < NEGLIGIBLE_MS.max(NEGLIGIBLE_SHARE * median_a) => {
            Verdict::Negligible
        }
        Some(([lo, _], ..)) if lo > 0.0 => Verdict::Slower,
        Some(_) => Verdict::Faster,
    };
    Some(Comparison {
        a: median_a,
        b: median_b,
        effect,
        ci: interval.map(|(ci, ..)| ci),
        confidence: interval.map(|(_, cum, total)| 1.0 - 2.0 * cum as f64 / total as f64),
        verdict,
    })
}

/// The Theil-Sen line through (x, y) points: the median slope, and its exact 95% interval when the x values differ.
#[derive(Clone, Debug, PartialEq)]
pub struct Trend {
    pub slope: f64,
    pub ci: Option<[f64; 2]>,
}

pub fn theil_sen(points: &[(f64, f64)]) -> Option<Trend> {
    let points = &points[points.len().saturating_sub(MAX_BLOCKS)..];
    let mut slopes = Vec::new();
    let mut distinct = true;
    for (i, p) in points.iter().enumerate() {
        for q in &points[i + 1..] {
            if q.0 == p.0 {
                distinct = false;
            } else {
                slopes.push((q.1 - p.1) / (q.0 - p.0));
            }
        }
    }
    slopes.sort_by(f64::total_cmp);
    let slope = median(&slopes)?;
    let ci = distinct
        .then(|| {
            let (k, ..) = tail_rank(&inversion_counts(points.len()));
            (k >= 1).then(|| [slopes[k - 1], slopes[slopes.len() - k]])
        })
        .flatten();
    Some(Trend { slope, ci })
}

/// A trend that moved the metric over the window: its slope per second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drift {
    pub slope: f64,
}

/// Some when the slope's interval excludes 0 and slope x window exceeds 5% of the median.
pub fn drift(points: &[(f64, f64)], window_s: f64) -> Option<Drift> {
    if points.len() < MIN_BLOCKS {
        return None;
    }
    let trend = theil_sen(points)?;
    let [lo, hi] = trend.ci?;
    let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    let typical = median(&ys)?;
    let excludes_zero = lo > 0.0 || hi < 0.0;
    (excludes_zero && trend.slope.abs() * window_s > DRIFT_SHARE * typical.abs())
        .then_some(Drift { slope: trend.slope })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values below come from brute-force enumeration and the published two-sided 5% Mann-Whitney table, outside this code.
    #[test]
    fn critical_values_match_the_published_table() {
        let table = [
            (5, 2),
            (6, 5),
            (7, 8),
            (8, 13),
            (9, 17),
            (10, 23),
            (11, 30),
            (12, 37),
            (13, 45),
            (14, 55),
            (15, 64),
            (16, 75),
            (17, 87),
            (18, 99),
            (19, 113),
            (20, 127),
        ];
        for (n, critical) in table {
            assert_eq!(critical_u(n, n), Some(critical), "n = m = {n}");
        }
    }

    #[test]
    fn critical_values_for_unequal_sides() {
        assert_eq!(critical_u(5, 8), Some(6));
        assert_eq!(critical_u(4, 10), Some(5));
        assert_eq!(critical_u(4, 4), Some(0));
        assert_eq!(critical_u(3, 3), None);
    }

    #[test]
    fn six_against_six_by_hand() {
        let a = [10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
        let b = [12.0, 14.0, 15.0, 16.0, 18.0, 19.0];
        // The 36 differences sorted: the 18th and 19th are 3 and 3, the 6th is 0 and the 31st is 6; 19 of 924 arrangements have U <= 5.
        let c = compare(&a, &b).unwrap();
        assert_eq!((c.a, c.b, c.effect), (12.5, 15.5, 3.0));
        assert_eq!(c.ci, Some([0.0, 6.0]));
        assert!((c.confidence.unwrap() - (1.0 - 38.0 / 924.0)).abs() < 1e-12);
        assert_eq!(c.verdict, Verdict::WithinNoise);
    }

    #[test]
    fn a_clear_shift_is_slower_and_the_reverse_is_faster() {
        let a = [10.0, 10.1, 9.9, 10.2, 9.8, 10.0, 10.1, 9.9];
        let b: Vec<f64> = a.iter().map(|v| v + 2.0).collect();
        let up = compare(&a, &b).unwrap();
        assert_eq!(up.verdict, Verdict::Slower);
        let [lo, hi] = up.ci.unwrap();
        assert!(lo > 0.0 && lo <= 2.0 && hi >= 2.0, "{lo} {hi}");
        let down = compare(&b, &a).unwrap();
        assert_eq!(down.verdict, Verdict::Faster);
        assert_eq!(down.effect, -up.effect);
    }

    #[test]
    fn a_shift_below_the_negligible_floor_is_negligible() {
        let a = [100.0; 8];
        let b = [101.0; 8];
        // 1 ms of 100 ms is under 2%, and the interval [1, 1] excludes 0.
        assert_eq!(compare(&a, &b).unwrap().verdict, Verdict::Negligible);
        assert_eq!(compare(&a, &[103.0; 8]).unwrap().verdict, Verdict::Slower);
        assert_eq!(
            compare(&[1.0; 8], &[1.04; 8]).unwrap().verdict,
            Verdict::Negligible
        );
    }

    #[test]
    fn fewer_than_six_blocks_gives_no_verdict() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [9.0, 9.5, 9.7, 9.9, 9.8, 9.6];
        assert_eq!(compare(&a, &b).unwrap().verdict, Verdict::TooFewBlocks);
        assert_eq!(compare(&b, &a).unwrap().verdict, Verdict::TooFewBlocks);
        assert_eq!(compare(&[], &b), None);
    }

    #[test]
    fn a_sample_against_itself_is_within_noise() {
        let a = [3.2, 5.1, 4.4, 4.9, 3.8, 5.5, 4.1];
        let c = compare(&a, &a).unwrap();
        assert_eq!(c.effect, 0.0);
        assert_eq!(c.verdict, Verdict::WithinNoise);
        assert_eq!(
            compare(&[7.0; 6], &[7.0; 6]).unwrap().verdict,
            Verdict::WithinNoise
        );
    }

    #[test]
    fn six_points_by_hand() {
        let points: Vec<(f64, f64)> = [1.0, 3.0, 2.0, 5.0, 4.0, 6.0]
            .iter()
            .enumerate()
            .map(|(i, &y)| (i as f64, y))
            .collect();
        // The 15 pair slopes sorted put the median at 1, the 2nd at -1 and the 14th at 2; 6 of 720 orderings have at most 1 inversion.
        let t = theil_sen(&points).unwrap();
        assert_eq!(t.slope, 1.0);
        assert_eq!(t.ci, Some([-1.0, 2.0]));
    }

    #[test]
    fn inversion_tails_match_enumeration() {
        for (n, k, cum) in [(5, 1, 1), (6, 2, 6), (7, 4, 76), (8, 6, 628)] {
            let (got_k, got_cum, total) = tail_rank(&inversion_counts(n));
            assert_eq!((got_k, got_cum), (k, cum), "n = {n}");
            assert_eq!(total, (1..=n as u128).product::<u128>());
        }
    }

    #[test]
    fn a_steady_series_has_no_drift_and_a_climbing_one_does() {
        let steady: Vec<(f64, f64)> = [50.0, 50.4, 49.8, 50.2, 49.9, 50.1, 50.3, 49.7]
            .iter()
            .enumerate()
            .map(|(i, &y)| (5.0 * i as f64 + 2.5, y))
            .collect();
        assert_eq!(drift(&steady, 40.0), None);
        let climbing: Vec<(f64, f64)> = (0..8)
            .map(|i| {
                (
                    5.0 * f64::from(i) + 2.5,
                    50.0 + 1.05 * f64::from(i) + [0.1, -0.1][i as usize % 2],
                )
            })
            .collect();
        let d = drift(&climbing, 40.0).unwrap();
        assert!((d.slope - 0.21).abs() < 0.02, "{}", d.slope);
    }

    #[test]
    fn a_slope_too_small_for_the_window_is_not_drift() {
        let slow: Vec<(f64, f64)> = (0..8)
            .map(|i| (5.0 * f64::from(i), 100.0 + 0.1 * f64::from(i)))
            .collect();
        // Perfectly monotone, but 0.02 ms/s over 40 s is 0.8% of 100.
        assert_eq!(drift(&slow, 40.0), None);
    }
}
