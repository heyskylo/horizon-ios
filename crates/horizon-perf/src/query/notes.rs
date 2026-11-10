use std::collections::BTreeMap;

use horizon_perf_format::v1::{Warning, WarningCode};

use super::rows::{
    CountChange, CounterRow, MarkRow, MbChange, MemoryDiffRow, MemoryLabelRow, MemoryRow, Row,
    WarningRow,
};
use super::{Ctx, QueryError, Reply, num};
use crate::ids::{Id, Kind};
use crate::view::{Snapshot, is_megabytes};
use crate::wire::{Amount, CounterKindName, MemoryUnit, SideName, Unit, count, round1};

const MIB: f64 = 1024.0 * 1024.0;
const DIFF_COUNTS: usize = 50;
const DEFAULT_MIN_MB: f64 = 1.0;
const RESIDENT: &str = "process.resident_mb";

/// A warning in words: codes carry numbers so each tool words them itself.
pub fn warning_text(w: &Warning) -> String {
    let arg = |i: usize| w.args.get(i).copied().unwrap_or(0.0);
    match w.code() {
        WarningCode::ServerNotHorizon => {
            "the connected server does not run Horizon, so only the client side was captured".into()
        }
        WarningCode::ServerDenied => {
            "the server refused the capture: operator level 4 is required".into()
        }
        WarningCode::ServerAlreadyRunning => "the server already runs a capture".into(),
        WarningCode::ServerPartIncomplete => {
            "the client disconnected while the server's side was being sent".into()
        }
        WarningCode::TimelineCapped => format!(
            "the timeline hit its cap after {:.0} s ({:.1} MiB written)",
            arg(0),
            arg(1) / MIB
        ),
        WarningCode::GpuUnavailable => {
            "GPU pass timing was asked for, but this device or run gave no pass times".into()
        }
        WarningCode::Unspecified => format!("unknown warning {:?}", w.args),
    }
}

/// Marks typed by the person profiling, in time order.
pub(super) fn marks(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let sides = ctx.sides(0)?;
    let first = sides
        .first()
        .copied()
        .ok_or_else(|| QueryError::new("this capture has no data"))?;
    let window = ctx.window(0, first, f64::NEG_INFINITY)?;
    let mut marks: Vec<_> = view
        .capture
        .marks
        .iter()
        .filter(|m| sides.contains(&m.side()) && window.contains(m.t))
        .collect();
    marks.sort_by(|a, b| a.t.total_cmp(&b.t));
    let rows = marks
        .into_iter()
        .take(ctx.q.filters.limit)
        .map(|m| {
            Row::Mark(MarkRow {
                side: SideName::from(m.side()),
                t: num(m.t),
                name: m.name.clone(),
            })
        })
        .collect();
    Ok(Reply::new(Unit::Marks, rows))
}

pub(super) fn warnings(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let sides = ctx.sides(0)?;
    let mut all: Vec<_> = view
        .capture
        .warnings
        .iter()
        .filter(|w| sides.contains(&w.side()))
        .collect();
    all.sort_by(|a, b| a.t.total_cmp(&b.t));
    let rows = all
        .into_iter()
        .take(ctx.q.filters.limit)
        .map(|w| {
            Row::Warning(WarningRow {
                side: SideName::from(w.side()),
                t: num(w.t),
                code: w.code().into(),
                args: w.args.clone(),
                text: warning_text(w),
            })
        })
        .collect();
    Ok(Reply::new(Unit::Warnings, rows))
}

/// Every memory snapshot of the selected sides in time order, so a label can be asked for.
pub(super) fn memory_labels(ctx: &Ctx) -> Result<Reply, QueryError> {
    let mut rows: Vec<MemoryLabelRow> = Vec::new();
    for side in ctx.sides(0)? {
        rows.extend(
            ctx.views[0]
                .snapshots(side)
                .into_iter()
                .map(|s| MemoryLabelRow {
                    label: s.label,
                    t: num(s.t),
                    side: SideName::from(side),
                    rows: s.values.len(),
                }),
        );
    }
    rows.sort_by(|a, b| a.t.total_cmp(&b.t));
    let cut = rows.len().saturating_sub(ctx.q.filters.limit);
    rows.truncate(ctx.q.filters.limit);
    let mut reply = Reply::new(
        Unit::MemoryLabels,
        rows.into_iter().map(Row::MemoryLabel).collect(),
    );
    if cut > 0 {
        reply
            .warnings
            .push(format!("{cut} more snapshots: raise --limit"));
    }
    Ok(reply)
}

/// Every counter the capture defines, with how many samples carry it.
pub(super) fn counters(ctx: &Ctx) -> Result<Reply, QueryError> {
    let view = &ctx.views[0];
    let mut rows = Vec::new();
    for side in ctx.sides(0)? {
        let mut seen: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for sample in view.capture.counters.iter().filter(|s| s.side() == side) {
            for id in &sample.ids {
                *seen.entry(*id).or_default() += 1;
            }
        }
        for c in view.counters(side) {
            rows.push(CounterRow {
                id: crate::ids::counter(&c.name, &c.labels),
                samples: seen.get(&c.id).copied().unwrap_or(0),
                name: c.name,
                unit: c.unit,
                kind: CounterKindName::from(c.kind),
                labels: c.labels.into_iter().collect(),
                side: SideName::from(side),
            });
        }
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id).then(a.side.cmp(&b.side)));
    let cut = rows.len().saturating_sub(ctx.q.filters.limit);
    rows.truncate(ctx.q.filters.limit);
    let mut reply = Reply::new(Unit::Counters, rows.into_iter().map(Row::Counter).collect());
    if cut > 0 {
        reply
            .warnings
            .push(format!("{cut} more counters: raise --limit"));
    }
    Ok(reply)
}

/// A snapshot's biggest rows, or with `--diff` what grew and shrank between two.
pub(super) fn memory(ctx: &Ctx) -> Result<Reply, QueryError> {
    let side = ctx.side(0)?;
    let snaps = ctx.views[0].snapshots(side);
    if snaps.is_empty() {
        return Err(QueryError::new(
            "this capture has no memory snapshots for the selected side",
        ));
    }
    let min_mb = ctx.q.min_mb;
    if ctx.q.diff {
        return diff(ctx, &snaps, min_mb.unwrap_or(DEFAULT_MIN_MB));
    }
    let snap = match ctx.q.args.as_slice() {
        [] => snaps.last(),
        [label] => Some(find(&snaps, label)?),
        _ => {
            return Err(QueryError::new(
                "memory takes one label, or none for the last snapshot",
            ));
        }
    };
    let snap = snap.expect("snapshots are not empty");
    let mut rows: Vec<&(String, f64)> = snap
        .values
        .iter()
        .filter(|(p, v)| !is_megabytes(p) || v.abs() >= min_mb.unwrap_or(0.0))
        .collect();
    rows.sort_by(|a, b| {
        is_megabytes(&b.0)
            .cmp(&is_megabytes(&a.0))
            .then(b.1.total_cmp(&a.1))
            .then_with(|| a.0.cmp(&b.0))
    });
    let rows = rows
        .into_iter()
        .take(ctx.q.filters.limit)
        .map(|(path, v)| {
            let (value, unit) = if is_megabytes(path) {
                (Amount::Float(round1(*v)), MemoryUnit::Mb)
            } else {
                (count(*v), MemoryUnit::Count)
            };
            Row::Memory(MemoryRow {
                id: Id::new(Kind::Memory, path.as_str()).to_string(),
                path: path.clone(),
                value,
                unit,
            })
        })
        .collect();
    Ok(Reply::new(Unit::Memory, rows))
}

fn find<'a>(snaps: &'a [Snapshot], label: &str) -> Result<&'a Snapshot, QueryError> {
    snaps.iter().find(|s| s.label == label).ok_or_else(|| {
        let labels: Vec<&str> = snaps.iter().map(|s| s.label.as_str()).collect();
        QueryError::new(format!(
            "no memory snapshot labelled {label}; labels: {}",
            labels.join(", ")
        ))
    })
}

/// Spec D's `memory-diff`: rows are `[path, a, b, b - a]`; megabyte rows keep a change of `min_mb`, counts the 50 biggest changes.
fn diff(ctx: &Ctx, snaps: &[Snapshot], min_mb: f64) -> Result<Reply, QueryError> {
    let (a, b) = match ctx.q.args.as_slice() {
        [] if snaps.len() >= 2 => (&snaps[0], &snaps[snaps.len() - 1]),
        [] => {
            return Err(QueryError::new(
                "a diff needs two snapshots; this capture has one",
            ));
        }
        [x, y] => (find(snaps, x)?, find(snaps, y)?),
        _ => {
            return Err(QueryError::new(
                "memory --diff takes two labels, or none for the first and last",
            ));
        }
    };
    let before_map: BTreeMap<&str, f64> = a.values.iter().map(|(p, v)| (p.as_str(), *v)).collect();
    let after_map: BTreeMap<&str, f64> = b.values.iter().map(|(p, v)| (p.as_str(), *v)).collect();
    let mut paths: Vec<&str> = before_map.keys().chain(after_map.keys()).copied().collect();
    paths.sort_unstable();
    paths.dedup();
    let (mut grew, mut shrank, mut counts) = (Vec::new(), Vec::new(), Vec::new());
    for path in &paths {
        let (before, after) = (
            before_map.get(path).copied().unwrap_or(0.0),
            after_map.get(path).copied().unwrap_or(0.0),
        );
        let delta = after - before;
        if *path == RESIDENT {
            continue;
        }
        if is_megabytes(path) {
            if delta.abs() >= min_mb && delta > 0.0 {
                grew.push((*path, before, after, delta));
            } else if delta.abs() >= min_mb && delta < 0.0 {
                shrank.push((*path, before, after, delta));
            }
        } else if delta != 0.0 {
            counts.push((*path, before, after, delta));
        }
    }
    let rank = |rows: &mut Vec<(&str, f64, f64, f64)>| {
        rows.sort_by(|x, y| y.3.abs().total_cmp(&x.3.abs()).then(x.0.cmp(y.0)))
    };
    rank(&mut grew);
    rank(&mut shrank);
    rank(&mut counts);
    counts.truncate(DIFF_COUNTS);
    let mb_rows = |rows: Vec<(&str, f64, f64, f64)>| -> Vec<MbChange> {
        rows.into_iter()
            .map(|(p, x, y, d)| MbChange(p.to_string(), round1(x), round1(y), round1(d)))
            .collect()
    };
    let resident_mb = match (a.get(RESIDENT), b.get(RESIDENT)) {
        (Some(x), Some(y)) => Some([round1(x), round1(y), round1(y - x)]),
        _ => None,
    };
    fn only(from: &BTreeMap<&str, f64>, not: &BTreeMap<&str, f64>) -> Vec<String> {
        from.keys()
            .filter(|p| !not.contains_key(*p))
            .map(|p| p.to_string())
            .collect()
    }
    let row = MemoryDiffRow {
        from: a.label.clone(),
        to: b.label.clone(),
        seconds: round1(b.t - a.t),
        resident_mb,
        grew_mb: mb_rows(grew),
        shrank_mb: mb_rows(shrank),
        counts: counts
            .into_iter()
            .map(|(p, x, y, d)| CountChange(p.to_string(), count(x), count(y), count(d)))
            .collect(),
        only_in_a: only(&before_map, &after_map),
        only_in_b: only(&after_map, &before_map),
    };
    Ok(Reply::new(Unit::MemoryDiff, vec![Row::MemoryDiff(row)]))
}
