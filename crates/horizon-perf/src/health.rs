use horizon_perf_format::v1::{Capture, HealthSample, Side};

use crate::wire::{HealthUnit, vocab};

pub type Pick = fn(&HealthSample) -> Option<f32>;

vocab! {
    /// A field of a health sample, as `h:NAME` names it.
    HealthField {
        Tps = "tps",
        MsptAvg = "mspt_avg",
        MsptP50 = "mspt_p50",
        MsptP95 = "mspt_p95",
        MsptMax = "mspt_max",
        Fps = "fps",
        FrameMsP50 = "frame_ms_p50",
        FrameMsP95 = "frame_ms_p95",
        FrameMsP99 = "frame_ms_p99",
        FrameMsMax = "frame_ms_max",
        FrameCpuMsP50 = "frame_cpu_ms_p50",
        FrameGpuMsP50 = "frame_gpu_ms_p50",
        CpuProcessCores = "cpu_process_cores",
        CpuSystemCores = "cpu_system_cores",
        RssMb = "rss_mb",
        PhysicalUsedMb = "physical_used_mb",
        SwapUsedMb = "swap_used_mb",
        AllocatorCommitMb = "allocator_commit_mb",
        AllocsPerS = "allocs_per_s",
        AllocMibPerS = "alloc_mib_per_s",
        Players = "players",
        Chunks = "chunks",
        Entities = "entities",
    }
}

impl HealthField {
    pub fn unit(self) -> HealthUnit {
        match self {
            HealthField::Tps => HealthUnit::TicksPerS,
            HealthField::MsptAvg
            | HealthField::MsptP50
            | HealthField::MsptP95
            | HealthField::MsptMax
            | HealthField::FrameMsP50
            | HealthField::FrameMsP95
            | HealthField::FrameMsP99
            | HealthField::FrameMsMax
            | HealthField::FrameCpuMsP50
            | HealthField::FrameGpuMsP50 => HealthUnit::Ms,
            HealthField::Fps => HealthUnit::FramesPerS,
            HealthField::CpuProcessCores | HealthField::CpuSystemCores => HealthUnit::Cores,
            HealthField::RssMb
            | HealthField::PhysicalUsedMb
            | HealthField::SwapUsedMb
            | HealthField::AllocatorCommitMb => HealthUnit::Mb,
            HealthField::AllocsPerS => HealthUnit::PerS,
            HealthField::AllocMibPerS => HealthUnit::MibPerS,
            HealthField::Players | HealthField::Chunks | HealthField::Entities => HealthUnit::Count,
        }
    }

    pub fn pick(self) -> Pick {
        match self {
            HealthField::Tps => |h| h.tps,
            HealthField::MsptAvg => |h| h.mspt_avg,
            HealthField::MsptP50 => |h| h.mspt_p50,
            HealthField::MsptP95 => |h| h.mspt_p95,
            HealthField::MsptMax => |h| h.mspt_max,
            HealthField::Fps => |h| h.fps,
            HealthField::FrameMsP50 => |h| h.frame_ms_p50,
            HealthField::FrameMsP95 => |h| h.frame_ms_p95,
            HealthField::FrameMsP99 => |h| h.frame_ms_p99,
            HealthField::FrameMsMax => |h| h.frame_ms_max,
            HealthField::FrameCpuMsP50 => |h| h.frame_cpu_ms_p50,
            HealthField::FrameGpuMsP50 => |h| h.frame_gpu_ms_p50,
            HealthField::CpuProcessCores => |h| Some(h.cpu_process_cores),
            HealthField::CpuSystemCores => |h| Some(h.cpu_system_cores),
            HealthField::RssMb => |h| Some(h.rss_mb),
            HealthField::PhysicalUsedMb => |h| Some(h.physical_used_mb),
            HealthField::SwapUsedMb => |h| Some(h.swap_used_mb),
            HealthField::AllocatorCommitMb => |h| h.allocator_commit_mb,
            HealthField::AllocsPerS => |h| h.allocs_per_s,
            HealthField::AllocMibPerS => |h| h.alloc_mib_per_s,
            HealthField::Players => |h| Some(h.players as f32),
            HealthField::Chunks => |h| Some(h.chunks as f32),
            HealthField::Entities => |h| Some(h.entities as f32),
        }
    }
}

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
    Series::new(capture, side, pick).mean(lo, hi, MIN_COVERAGE * window_s)
}

/// One health field as spans of time with a value each, in priority order: live samples, then the history tiers, finest first.
pub struct Series {
    sources: Vec<Vec<(Span, f64)>>,
}

impl Series {
    pub fn new(capture: &Capture, side: Side, pick: fn(&HealthSample) -> Option<f32>) -> Self {
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
        Self { sources }
    }

    /// Time-weighted mean over `lo..hi`; None when the recorded data covers less than `min_covered` seconds of it.
    pub fn mean(&self, lo: f64, hi: f64, min_covered: f64) -> Option<f64> {
        let mut covered: Vec<Span> = Vec::new();
        let (mut weighted, mut length) = (0.0, 0.0);
        for source in &self.sources {
            let mut taken: Vec<Span> = Vec::new();
            for &((from, to), value) in source {
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
        (length > 0.0 && length >= min_covered).then(|| weighted / length)
    }
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
