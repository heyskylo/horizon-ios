//! `diff` on benchmark reports: a report's own `ab` is printed as written, otherwise the repeats of each phase are the blocks.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::diff::{DiffOptions, Row, row};

pub const FORMAT: &str = "horizon-benchmark-report";

/// Frames fields compared when no `ab` is present; the CPU and GPU ones only when every repeat has them.
const METRICS: [&str; 4] = ["p50_ms", "p95_ms", "cpu_p50_ms", "gpu_p50_ms"];

/// True when the bytes look like a JSON document rather than a gzip capture.
pub fn looks_like_report(bytes: &[u8]) -> bool {
    bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{')
}

pub fn parse(bytes: &[u8]) -> Result<Value, String> {
    let report: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    match report.get("format").and_then(Value::as_str) {
        Some(FORMAT) => Ok(report),
        other => Err(format!("not a benchmark report: format {other:?}")),
    }
}

/// A report's A/B result: which report it is in, and the `ab` object untouched.
#[derive(Clone, Debug, PartialEq)]
pub struct Ab {
    pub report: &'static str,
    pub ab: Value,
}

#[derive(Clone, Debug)]
pub struct BenchDiff {
    pub ab: Vec<Ab>,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}

fn phases(report: &Value) -> BTreeMap<String, BTreeMap<&'static str, Vec<f64>>> {
    let mut out: BTreeMap<String, BTreeMap<&'static str, Vec<f64>>> = BTreeMap::new();
    let frames = report.get("frames").and_then(Value::as_array);
    for frame in frames.into_iter().flatten() {
        let Some(phase) = frame.get("phase").and_then(Value::as_str) else {
            continue;
        };
        let metrics = out.entry(phase.to_string()).or_default();
        for name in METRICS {
            if let Some(v) = frame.get(name).and_then(Value::as_f64) {
                metrics.entry(name).or_default().push(v);
            }
        }
    }
    let repeats = |phase: &str| {
        frames
            .into_iter()
            .flatten()
            .filter(|f| f.get("phase").and_then(Value::as_str) == Some(phase))
            .count()
    };
    for (phase, metrics) in &mut out {
        let all = repeats(phase);
        metrics.retain(|_, values| values.len() == all);
    }
    out
}

/// Compares two reports; `ab` results are carried over and never recomputed.
pub fn diff(a: &Value, b: &Value, options: &DiffOptions) -> BenchDiff {
    let mut ab = Vec::new();
    let mut warnings = Vec::new();
    for (report, value) in [("a", a), ("b", b)] {
        if let Some(found) = value.get("ab").filter(|v| !v.is_null()) {
            ab.push(Ab {
                report,
                ab: found.clone(),
            });
            if found.get("first_block_outside_range") == Some(&Value::Bool(true)) {
                warnings.push(format!(
                    "{}'s first block lies outside its later B blocks",
                    report.to_uppercase()
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
            for metric in METRICS {
                let va = ma.and_then(|m| m.get(metric));
                let vb = mb.and_then(|m| m.get(metric));
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
        let other = if ab[0].report == "a" { "B" } else { "A" };
        warnings.push(format!("report {other} has no A/B result"));
    }
    BenchDiff { ab, rows, warnings }
}

impl BenchDiff {
    pub fn text(&self, a: &str, b: &str) -> String {
        let mut out = format!("A {a}  B {b}  (benchmark reports, ms per frame)\n");
        for section in &self.ab {
            let ab = &section.ab;
            let text = |key: &str| ab.get(key).and_then(Value::as_str).unwrap_or("?");
            out.push_str(&format!(
                "ab in {}: {} {} vs {}, {} pairs of {} s blocks\n",
                section.report.to_uppercase(),
                text("switch"),
                text("a"),
                text("b"),
                ab.get("pairs").map_or("?".to_string(), Value::to_string),
                ab.get("block_s").map_or("?".to_string(), Value::to_string),
            ));
            let results = ab.get("result").and_then(Value::as_object);
            for (metric, r) in results.into_iter().flatten() {
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

    pub fn json(&self, a: &str, b: &str) -> Value {
        json!({
            "ok": true,
            "op": "diff",
            "captures": [{"role": "a", "file": a}, {"role": "b", "file": b}],
            "kind": "benchmark",
            "unit": "ms_per_frame",
            "ab": self.ab.iter().map(|s| json!({"report": s.report, "ab": s.ab})).collect::<Vec<_>>(),
            "rows": self.rows.iter().map(Row::json).collect::<Vec<_>>(),
            "warnings": self.warnings,
        })
    }
}

fn ab_line(metric: &str, r: &Value) -> String {
    let field = |key: &str| r.get(key).map_or("?".to_string(), Value::to_string);
    let ci = match r.get("ci95").and_then(Value::as_array).map(Vec::as_slice) {
        Some([lo, hi]) => format!("CI [{lo}, {hi}]"),
        _ => "no CI".to_string(),
    };
    let verdict = r.get("verdict").and_then(Value::as_str).unwrap_or("?");
    format!(
        "{metric:<14} {} → {}  {} ms  {ci}  {verdict}",
        field("a"),
        field("b"),
        field("diff"),
    )
}
