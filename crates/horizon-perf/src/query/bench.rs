use horizon_bench::Report;
use horizon_perf_format::v1::BenchResult;

use super::rows::{BenchRow, Row};
use super::{Ctx, QueryError, Reply};
use crate::wire::{SideName, Unit};

/// The benchmark reports a capture embeds: their metrics, frames and `ab`, as the report holds them.
pub(super) fn bench(ctx: &Ctx) -> Result<Reply, QueryError> {
    let sides = ctx.sides(0)?;
    let mut warnings = Vec::new();
    let mut rows = Vec::new();
    let results = ctx.files[0]
        .capture
        .bench
        .iter()
        .filter(|b| sides.contains(&b.side()));
    for (i, result) in results.enumerate() {
        if rows.len() == ctx.q.filters.limit {
            break;
        }
        let id = format!("b:{}", i + 1);
        rows.push(Row::Bench(row(&id, result, &mut warnings)));
    }
    if rows.is_empty() {
        warnings.push("this capture holds no benchmark report".into());
    }
    Ok(Reply {
        warnings,
        ..Reply::new(Unit::Bench, rows)
    })
}

fn row(id: &str, result: &BenchResult, warnings: &mut Vec<String>) -> BenchRow {
    let report = match Report::parse(&result.report_json) {
        Ok(report) => Some(report),
        Err(e) => {
            warnings.push(format!(
                "{id} is a {} document this horizon-perf does not read: {e}",
                result.format
            ));
            None
        }
    };
    let non_empty = |text: &str| (!text.is_empty()).then(|| text.to_string());
    BenchRow {
        id: id.to_string(),
        side: SideName::from(result.side()),
        format: result.format.clone(),
        corpus_id: non_empty(&result.corpus_id),
        scenario: report.as_ref().and_then(|r| r.conditions.scenario.clone()),
        app_version: report.as_ref().map(|r| r.app.version.clone()),
        created: report.as_ref().map(|r| r.created.clone()),
        metrics: report
            .as_ref()
            .map(|r| r.metrics.clone())
            .unwrap_or_default(),
        frames: report
            .as_ref()
            .map(|r| r.frames.clone())
            .unwrap_or_default(),
        ab: report.and_then(|r| r.ab),
    }
}
