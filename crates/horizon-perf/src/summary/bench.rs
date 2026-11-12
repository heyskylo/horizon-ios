//! The summary's benchmark lines: each phase's frame times, and an A/B run's verdicts as the report wrote them.

use horizon_bench::{Frames, Report};
use horizon_perf_format::v1::Side;

use super::{Line, Section};
use crate::bench_diff::ab_line;
use crate::view::View;

pub(super) fn lines(view: &View, side: Side) -> Section {
    let mut out = Vec::new();
    for result in view.capture.bench.iter().filter(|b| b.side() == side) {
        let Ok(report) = Report::parse(&result.report_json) else {
            out.push(Line::body(format!(
                "bench   {}: a document this horizon-perf does not read",
                result.format
            )));
            continue;
        };
        let scenario = report.conditions.scenario.as_deref().unwrap_or("run");
        out.push(Line::body(format!(
            "bench   {scenario} · Horizon {}",
            report.app.version
        )));
        let mut phases: Vec<&str> = Vec::new();
        for frames in &report.frames {
            if !phases.contains(&frames.phase.as_str()) {
                phases.push(&frames.phase);
            }
        }
        for phase in phases {
            let repeats: Vec<&Frames> = report.frames.iter().filter(|f| f.phase == phase).collect();
            let mean = |get: fn(&Frames) -> f64| {
                repeats.iter().map(|f| get(f)).sum::<f64>() / repeats.len() as f64
            };
            let plural = if repeats.len() == 1 { "" } else { "s" };
            out.push(Line::body(format!(
                "bench   {phase}: p50 {:.2} ms · p95 {:.2} ms · {:.0} fps ({} repeat{plural})",
                mean(|f| f.p50_ms),
                mean(|f| f.p95_ms),
                mean(|f| f.avg_fps),
                repeats.len()
            )));
        }
        if let Some(ab) = &report.ab {
            out.push(Line::body(format!(
                "bench   ab {}: {} vs {}, {} pairs",
                ab.switch, ab.a, ab.b, ab.pairs
            )));
            for (metric, paired) in &ab.result {
                out.push(Line::body(format!("bench     {}", ab_line(metric, paired))));
            }
        }
    }
    out
}
