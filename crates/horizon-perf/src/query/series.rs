use std::str::FromStr;

use horizon_perf_format::v1::{CounterKind, Side};

use super::rows::{Row, SeriesRow, SeriesStats};
use super::{Ctx, QueryError, Reply, num, opt_num};
use crate::health::{HealthField, Series};
use crate::ids::{self, Id, Kind};
use crate::stats::median;
use crate::view::{CounterInfo, View};
use crate::wire::{Candidate, SeriesUnit, Unit};

const DEFAULT_EVERY_S: f64 = 30.0;
const MAX_POINTS: usize = 2000;

/// Time series, resampled into `--every` seconds; `--from` before 0 reaches into the history tiers.
pub(super) fn series(ctx: &Ctx) -> Result<Reply, QueryError> {
    let side = ctx.side(0)?;
    if ctx.q.args.is_empty() {
        return Err(QueryError::new(
            "series needs fields: h:NAME for health, span:NAME for a span, x:NAME for a counter",
        ));
    }
    let window = ctx.window(0, side, 0.0)?;
    let every = ctx.q.every.unwrap_or(DEFAULT_EVERY_S);
    if every <= 0.0 || window.to <= window.from {
        return Err(QueryError::new(
            "--every must be positive and --from before --to",
        ));
    }
    let count = ((window.to - window.from) / every).ceil() as usize;
    if count > MAX_POINTS {
        return Err(QueryError::new(format!(
            "{count} points is too many: raise --every above {:.0}",
            (window.to - window.from) / MAX_POINTS as f64
        )));
    }
    let starts: Vec<f64> = (0..count).map(|i| window.from + every * i as f64).collect();
    let buckets: Vec<(f64, f64)> = starts
        .iter()
        .map(|&s| (s, (s + every).min(window.to)))
        .collect();
    let mut rows = Vec::new();
    for arg in &ctx.q.args {
        for field in resolve(ctx, side, arg)? {
            let (id, unit, values) = field.values(ctx, side, &buckets)?;
            let stats = ctx
                .q
                .stats
                .then(|| {
                    series_stats(
                        &values,
                        field.window_mean(ctx, side, window.from, window.to),
                    )
                })
                .flatten();
            rows.push(Row::Series(SeriesRow {
                field: id,
                unit,
                step_s: num(every),
                t: starts.iter().map(|&t| num(t)).collect(),
                values: values.into_iter().map(opt_num).collect(),
                stats,
            }));
        }
    }
    let mut reply = Reply::new(Unit::Series, rows);
    reply.window = Some(window);
    Ok(reply)
}

/// The statistics of a series' finite points; None when it has none.
fn series_stats(values: &[Option<f64>], window_mean: Option<f64>) -> Option<SeriesStats> {
    let points: Vec<f64> = values.iter().flatten().copied().collect();
    let last = *points.last()?;
    let mut sorted = points.clone();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let mean = window_mean.unwrap_or_else(|| points.iter().sum::<f64>() / n as f64);
    Some(SeriesStats {
        min: num(sorted[0]),
        mean: num(mean),
        median: num(median(&points)?),
        p95: num(sorted[((n - 1) as f64 * 0.95).floor() as usize]),
        max: num(sorted[n - 1]),
        last: num(last),
        points: n,
    })
}

enum Field {
    Health(HealthField),
    Span(String),
    Counter(CounterInfo),
}

fn resolve(ctx: &Ctx, side: Side, arg: &str) -> Result<Vec<Field>, QueryError> {
    let view = &ctx.views[0];
    let health_named = |name: &str| HealthField::from_str(name).ok();
    let health_candidates = || {
        HealthField::ALL
            .iter()
            .map(|h| Candidate::new(format!("h:{h}"), h.to_string()))
            .collect::<Vec<_>>()
    };
    if let Some(name) = arg.strip_prefix("h:") {
        return health_named(name)
            .map(|field| vec![Field::Health(field)])
            .ok_or_else(|| {
                QueryError::with(format!("no health field named {name}"), health_candidates())
            });
    }
    if let Some(name) = arg.strip_prefix("span:").or_else(|| arg.strip_prefix("s:")) {
        return Ok(vec![Field::Span(span_name(ctx, side, name)?)]);
    }
    if arg.starts_with("x:") {
        return counters_named(view, side, arg.strip_prefix("x:").unwrap_or(arg));
    }
    if let Some(field) = health_named(arg) {
        return Ok(vec![Field::Health(field)]);
    }
    if let Ok(found) = counters_named(view, side, arg) {
        return Ok(found);
    }
    if let Ok(name) = span_name(ctx, side, arg) {
        return Ok(vec![Field::Span(name)]);
    }
    Err(QueryError::with(
        format!(
            "{arg} is not a health field, a span or a counter; use h:, span: or x: to say which"
        ),
        health_candidates(),
    ))
}

fn span_name(ctx: &Ctx, side: Side, name: &str) -> Result<String, QueryError> {
    let thread = ctx.thread(0, side)?;
    let table = ctx.views[0]
        .span_table(side, thread, None)
        .ok_or_else(|| QueryError::new("this capture has no spans for the selected thread"))?;
    if table.row(name).is_some() {
        return Ok(name.to_string());
    }
    let candidates = table
        .rows
        .iter()
        .take(10)
        .map(|r| Candidate::new(ids::span(&r.name), r.name.clone()))
        .collect();
    Err(QueryError::with(
        format!("no span named {name}"),
        candidates,
    ))
}

/// `name` or `name{k=v,...}`: every counter with that name (and those labels).
fn counters_named(view: &View, side: Side, text: &str) -> Result<Vec<Field>, QueryError> {
    let (name, labels) = match text.split_once('{') {
        Some((name, rest)) => {
            let pairs = rest
                .trim_end_matches('}')
                .split(',')
                .filter(|p| !p.is_empty());
            let mut labels: Vec<(String, String)> = pairs
                .filter_map(|p| {
                    p.split_once('=')
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                })
                .collect();
            labels.sort();
            (name, Some(labels))
        }
        None => (text, None),
    };
    let found: Vec<Field> = view
        .counters(side)
        .into_iter()
        .filter(|c| c.name == name)
        .filter(|c| {
            labels.as_ref().is_none_or(|want| {
                let mut have = c.labels.clone();
                have.sort();
                &have == want
            })
        })
        .map(Field::Counter)
        .collect();
    if found.is_empty() {
        let candidates = view
            .counters(side)
            .iter()
            .take(10)
            .map(|c| Candidate::new(ids::counter(&c.name, &c.labels), c.name.clone()))
            .collect();
        return Err(QueryError::with(
            format!("no counter named {text}"),
            candidates,
        ));
    }
    Ok(found)
}

impl Field {
    /// The mean over the whole window, where the capture can state one better than the points' average.
    fn window_mean(&self, ctx: &Ctx, side: Side, from: f64, to: f64) -> Option<f64> {
        match self {
            Field::Health(field) => {
                Series::new(ctx.views[0].capture, side, field.pick()).mean(from, to, 0.0)
            }
            Field::Span(_) | Field::Counter(_) => None,
        }
    }

    fn values(
        &self,
        ctx: &Ctx,
        side: Side,
        buckets: &[(f64, f64)],
    ) -> Result<(String, SeriesUnit, Vec<Option<f64>>), QueryError> {
        let view = &ctx.views[0];
        match self {
            Field::Health(field) => {
                let series = Series::new(view.capture, side, field.pick());
                let values = buckets
                    .iter()
                    .map(|&(lo, hi)| series.mean(lo, hi, 0.0))
                    .collect();
                Ok((
                    Id::new(Kind::Health, field.as_str()).to_string(),
                    SeriesUnit::Health(field.unit()),
                    values,
                ))
            }
            Field::Span(name) => {
                let thread = ctx.thread(0, side)?;
                let points = view.span_points(side, thread, name);
                let values = buckets
                    .iter()
                    .map(|&(lo, hi)| {
                        let (units, wall) = points
                            .iter()
                            .filter(|p| p.0 >= lo && p.0 < hi)
                            .fold((0u64, 0.0), |(u, w), p| (u + u64::from(p.1), w + p.2));
                        (units > 0).then(|| wall / units as f64)
                    })
                    .collect();
                let unit = if side == Side::Client {
                    Unit::MsPerFrame
                } else {
                    Unit::MsPerTick
                };
                Ok((ids::span(name), SeriesUnit::Span(unit), values))
            }
            Field::Counter(info) => {
                let points = counter_points(ctx, side, info.id);
                let rate = info.kind == CounterKind::Sum;
                let values = buckets
                    .iter()
                    .map(|&(lo, hi)| counter_value(&points, rate, lo, hi))
                    .collect();
                let unit = match (rate, info.unit.as_str()) {
                    (true, "") => "per_s".to_string(),
                    (true, unit) => format!("{unit}_per_s"),
                    (false, unit) => unit.to_string(),
                };
                Ok((
                    ids::counter(&info.name, &info.labels),
                    SeriesUnit::Counter(unit),
                    values,
                ))
            }
        }
    }
}

fn counter_points(ctx: &Ctx, side: Side, id: u32) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = ctx.views[0]
        .capture
        .counters
        .iter()
        .filter(|s| s.side() == side)
        .filter_map(|s| {
            s.ids
                .iter()
                .position(|&i| i == id)
                .and_then(|at| s.values.get(at))
                .map(|&v| (s.t, v))
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points
}

/// A sum counter's growth per second over the intervals that end in the bucket; a gauge's mean.
fn counter_value(points: &[(f64, f64)], rate: bool, lo: f64, hi: f64) -> Option<f64> {
    if rate {
        let (mut grown, mut seconds) = (0.0, 0.0);
        for pair in points.windows(2) {
            let (t0, v0) = pair[0];
            let (t1, v1) = pair[1];
            if t1 > lo && t1 <= hi && t1 > t0 {
                grown += v1 - v0;
                seconds += t1 - t0;
            }
        }
        (seconds > 0.0).then(|| grown / seconds)
    } else {
        let inside: Vec<f64> = points
            .iter()
            .filter(|p| p.0 >= lo && p.0 < hi)
            .map(|p| p.1)
            .collect();
        (!inside.is_empty()).then(|| inside.iter().sum::<f64>() / inside.len() as f64)
    }
}
