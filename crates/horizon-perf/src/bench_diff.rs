//! `diff` on benchmark reports: a report's own `ab` is printed as written, otherwise the repeats of each phase are the blocks.

use std::collections::BTreeMap;

use horizon_bench::compare::{Ab, Paired, Verdict as PairVerdict};
use serde::{Deserialize, Serialize};

use crate::diff::{DiffCapture, DiffOptions, DiffRow, Row, captures, row};
use crate::wire::{BenchKind, DiffOp, Role, Unit, vocab};

pub use horizon_bench::FORMAT;

vocab! {
    /// Frames fields compared when no `ab` is present; the CPU and GPU ones only when every repeat has them.
    BenchMetric {
        P50Ms = "p50_ms",
        P95Ms = "p95_ms",
        CpuP50Ms = "cpu_p50_ms",
        GpuP50Ms = "gpu_p50_ms",
    }
}

/// What `diff` reads of a benchmark report: its frames and its A/B result.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BenchReport {
    #[serde(default)]
    frames: Vec<BenchFrame>,
    #[serde(default)]
    ab: Option<Ab>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct BenchFrame {
    phase: Option<String>,
    p50_ms: Option<f64>,
    p95_ms: Option<f64>,
    cpu_p50_ms: Option<f64>,
    gpu_p50_ms: Option<f64>,
}

impl BenchFrame {
    fn get(&self, metric: BenchMetric) -> Option<f64> {
        match metric {
            BenchMetric::P50Ms => self.p50_ms,
            BenchMetric::P95Ms => self.p95_ms,
            BenchMetric::CpuP50Ms => self.cpu_p50_ms,
            BenchMetric::GpuP50Ms => self.gpu_p50_ms,
        }
    }
}

/// True when the bytes look like a JSON document rather than a gzip capture.
pub fn looks_like_report(bytes: &[u8]) -> bool {
    bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{')
}

pub fn parse(bytes: &[u8]) -> Result<BenchReport, String> {
    #[derive(Deserialize)]
    struct Header {
        format: Option<String>,
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if header.format.as_deref() != Some(FORMAT) {
        return Err(format!(
            "not a benchmark report: format {:?}",
            header.format.as_deref()
        ));
    }
    serde_json::from_slice(bytes).map_err(|e| e.to_string())
}

/// A report's A/B result and which report it is in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbSection {
    pub report: Role,
    pub ab: Ab,
}

#[derive(Clone, Debug)]
pub struct BenchDiff {
    pub ab: Vec<AbSection>,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}

/// The reply of `diff --json` on two benchmark reports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchReply {
    pub ok: bool,
    pub op: DiffOp,
    pub captures: Vec<DiffCapture>,
    pub kind: BenchKind,
    pub unit: Unit,
    pub ab: Vec<AbSection>,
    pub rows: Vec<DiffRow>,
    pub warnings: Vec<String>,
}

fn phases(report: &BenchReport) -> BTreeMap<String, BTreeMap<BenchMetric, Vec<f64>>> {
    let mut out: BTreeMap<String, BTreeMap<BenchMetric, Vec<f64>>> = BTreeMap::new();
    for frame in &report.frames {
        let Some(phase) = &frame.phase else {
            continue;
        };
        let metrics = out.entry(phase.clone()).or_default();
        for &metric in BenchMetric::ALL {
            if let Some(v) = frame.get(metric) {
                metrics.entry(metric).or_default().push(v);
            }
        }
    }
    let repeats = |phase: &str| {
        report
            .frames
            .iter()
            .filter(|f| f.phase.as_deref() == Some(phase))
            .count()
    };
    for (phase, metrics) in &mut out {
        let all = repeats(phase);
        metrics.retain(|_, values| values.len() == all);
    }
    out
}

/// Compares two reports; `ab` results are carried over and never recomputed.
pub fn diff(a: &BenchReport, b: &BenchReport, options: &DiffOptions) -> BenchDiff {
    let mut ab = Vec::new();
    let mut warnings = Vec::new();
    for (report, value) in [(Role::A, a), (Role::B, b)] {
        if let Some(found) = &value.ab {
            ab.push(AbSection {
                report,
                ab: found.clone(),
            });
            if found.first_block_outside_range {
                warnings.push(format!(
                    "{}'s first block lies outside its later B blocks",
                    report.as_str().to_uppercase()
                ));
            }
        }
    }
    let mut rows = Vec::new();
    if ab.is_empty() {
        let (pa, pb) = (phases(a), phases(b));
        let mut names: Vec<&String> = pa.keys().chain(pb.keys()).collect();
        names.sort();
        names.dedup();
        for phase in names {
            let (ma, mb) = (pa.get(phase), pb.get(phase));
            for &metric in BenchMetric::ALL {
                let va = ma.and_then(|m| m.get(&metric));
                let vb = mb.and_then(|m| m.get(&metric));
                if let Some(r) = row(
                    &format!("bench:{phase}.{metric}"),
                    va.map(Vec::as_slice),
                    vb.map(Vec::as_slice),
                ) && r.a.unwrap_or(0.0).max(r.b.unwrap_or(0.0)) >= options.min_ms
                {
                    rows.push(r);
                }
            }
        }
        rows.sort_by(|x, y| {
            let size = |r: &Row| r.effect.or(r.a).or(r.b).unwrap_or(0.0).abs();
            size(y).total_cmp(&size(x)).then_with(|| x.id.cmp(&y.id))
        });
        let limit = options.limit.max(1);
        if rows.len() > limit {
            warnings.push(format!(
                "{} more rows under the limit; raise --limit or --min-ms",
                rows.len() - limit
            ));
            rows.truncate(limit);
        }
    } else if ab.len() == 1 {
        let other = if ab[0].report == Role::A { "B" } else { "A" };
        warnings.push(format!("report {other} has no A/B result"));
    }
    BenchDiff { ab, rows, warnings }
}

impl BenchDiff {
    pub fn text(&self, a: &str, b: &str) -> String {
        let mut out = format!("A {a}  B {b}  (benchmark reports, ms per frame)\n");
        for section in &self.ab {
            let ab = &section.ab;
            out.push_str(&format!(
                "ab in {}: {} {} vs {}, {} pairs of {:?} s blocks\n",
                section.report.as_str().to_uppercase(),
                ab.switch,
                ab.a,
                ab.b,
                ab.pairs,
                ab.block_s,
            ));
            for (metric, r) in &ab.result {
                out.push_str(&format!("  {}\n", ab_line(metric, r)));
            }
        }
        let width = self.rows.iter().map(|r| r.id.len()).max().unwrap_or(0);
        for row in &self.rows {
            out.push_str(&row.text(width));
            out.push('\n');
        }
        for warning in &self.warnings {
            out.push_str(&format!("warning: {warning}\n"));
        }
        out
    }

    /// The JSON report; `a` and `b` are the files' names.
    pub fn reply(&self, a: &str, b: &str) -> BenchReply {
        BenchReply {
            ok: true,
            op: DiffOp::Diff,
            captures: captures(a, b),
            kind: BenchKind::Benchmark,
            unit: Unit::MsPerFrame,
            ab: self.ab.clone(),
            rows: self.rows.iter().map(Row::wire).collect(),
            warnings: self.warnings.clone(),
        }
    }
}

fn verdict_word(verdict: PairVerdict) -> &'static str {
    match verdict {
        PairVerdict::TooFewPairs => "too_few_pairs",
        PairVerdict::WithinNoise => "within_noise",
        PairVerdict::Negligible => "negligible",
        PairVerdict::Slower => "slower",
        PairVerdict::Faster => "faster",
    }
}

pub(crate) fn ab_line(metric: &str, r: &Paired) -> String {
    let ci = match r.ci95 {
        Some([lo, hi]) => format!("CI [{lo:?}, {hi:?}]"),
        None => "no CI".to_string(),
    };
    format!(
        "{metric:<14} {:?} → {:?}  {:?} ms  {ci}  {}",
        r.a,
        r.b,
        r.diff,
        verdict_word(r.verdict),
    )
}
