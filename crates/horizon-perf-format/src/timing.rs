use crate::v1::TimingSummary;

const MAX_TIMINGS: usize = 1_000_000;

#[derive(Default)]
pub struct TimingRecorder {
    times: Vec<f32>,
    incomplete: bool,
}

impl TimingRecorder {
    pub fn record(&mut self, ms: f32) {
        if self.incomplete {
            return;
        }
        if !ms.is_finite() || ms <= 0.0 || self.times.len() == MAX_TIMINGS {
            self.times = Vec::new();
            self.incomplete = true;
            return;
        }
        self.times.push(ms);
    }

    pub fn finish(mut self) -> Option<TimingSummary> {
        if self.incomplete || self.times.is_empty() {
            return None;
        }
        self.times.sort_unstable_by(f32::total_cmp);
        let count = self.times.len();
        let mean_tail = |divisor: usize| {
            let tail = &self.times[count - count.div_ceil(divisor)..];
            tail.iter().map(|&ms| f64::from(ms)).sum::<f64>() / tail.len() as f64
        };
        Some(TimingSummary {
            count: count as u64,
            slowest_1pct_ms: mean_tail(100),
            slowest_01pct_ms: mean_tail(1000),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_average_the_slowest_units_not_percentile_thresholds() {
        let mut recorder = TimingRecorder::default();
        for _ in 0..990 {
            recorder.record(10.0);
        }
        for _ in 0..9 {
            recorder.record(50.0);
        }
        recorder.record(100.0);
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.count, 1000);
        assert_eq!(summary.slowest_1pct_ms, 55.0);
        assert_eq!(summary.slowest_01pct_ms, 100.0);
    }

    #[test]
    fn short_captures_round_up_to_whole_units() {
        let mut recorder = TimingRecorder::default();
        for _ in 0..100 {
            recorder.record(10.0);
        }
        recorder.record(100.0);
        let summary = recorder.finish().unwrap();
        assert_eq!(summary.slowest_1pct_ms, 55.0);
        assert_eq!(summary.slowest_01pct_ms, 100.0);
    }

    #[test]
    fn missing_invalid_or_capped_data_never_produces_partial_lows() {
        assert!(TimingRecorder::default().finish().is_none());
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut recorder = TimingRecorder::default();
            recorder.record(10.0);
            recorder.record(bad);
            recorder.record(100.0);
            assert!(recorder.finish().is_none());
        }
        let mut recorder = TimingRecorder::default();
        for _ in 0..=MAX_TIMINGS {
            recorder.record(10.0);
        }
        assert!(recorder.finish().is_none());
    }
}
