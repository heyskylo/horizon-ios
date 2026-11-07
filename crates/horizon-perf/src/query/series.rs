use horizon_perf_format::v1::{CounterKind, HealthSample, Side};
use serde_json::json;

use super::{Ctx, QueryError, Reply, num, opt_num};
use crate::health::Series;
use crate::ids::{self, Id, Kind};
use crate::view::{CounterInfo, View};

type Pick = fn(&HealthSample) -> Option<f32>;

const DEFAULT_EVERY_S: f64 = 30.0;
const MAX_POINTS: usize = 2000;

const HEALTH: [(&str, &str, Pick); 23] = [
    ("tps", "ticks/s", |h| h.tps),
    ("mspt_avg", "ms", |h| h.mspt_avg),
    ("mspt_p50", "ms", |h| h.mspt_p50),
    ("mspt_p95", "ms", |h| h.mspt_p95),
    ("mspt_max", "ms", |h| h.mspt_max),
    ("fps", "frames/s", |h| h.fps),
    ("frame_ms_p50", "ms", |h| h.frame_ms_p50),
    ("frame_ms_p95", "ms", |h| h.frame_ms_p95),
    ("frame_ms_p99", "ms", |h| h.frame_ms_p99),
    ("frame_ms_max", "ms", |h| h.frame_ms_max),
    ("frame_cpu_ms_p50", "ms", |h| h.frame_cpu_ms_p50),
    ("frame_gpu_ms_p50", "ms", |h| h.frame_gpu_ms_p50),
    ("cpu_process_cores", "cores", |h| Some(h.cpu_process_cores)),
    ("cpu_system_cores", "cores", |h| Some(h.cpu_system_cores)),
    ("rss_mb", "MB", |h| Some(h.rss_mb)),
    ("physical_used_mb", "MB", |h| Some(h.physical_used_mb)),
    ("swap_used_mb", "MB", |h| Some(h.swap_used_mb)),
    ("allocator_commit_mb", "MB", |h| h.allocator_commit_mb),
    ("allocs_per_s", "per s", |h| h.allocs_per_s),
    ("alloc_mib_per_s", "MiB/s", |h| h.alloc_mib_per_s),
    ("players", "count", |h| Some(h.players as f32)),
    ("chunks", "count", |h| Some(h.chunks as f32)),
    ("entities", "count", |h| Some(h.entities as f32)),
];

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
        return Err(QueryError::new("--every must be positive and --from before --to"));
    }
    let count = ((window.to - window.from) / every).ceil() as usize;
    if count > MAX_POINTS {
        return Err(QueryError::new(format!(
            "{count} points is too many: raise --every above {:.0}",
            (window.to - window.from) / MAX_POINTS as f64
        )));
    }
    let starts: Vec<f64> = (0..count).map(|i| window.from + every * i as f64).collect();
    let buckets: Vec<(f64, f64)> = starts.iter().map(|&s| (s, (s + every).min(window.to))).collect();
    let mut rows = Vec::new();
    for arg in &ctx.q.args {
        for field in resolve(ctx, side, arg)? {
            let (id, unit, values) = field.values(ctx, side, &buckets)?;
            rows.push(json!({
                "field": id,
                "unit": unit,
                "step_s": num(every),
                "t": starts.iter().map(|&t| num(t)).collect::<Vec<_>>(),
                "values": values.into_iter().map(opt_num).collect::<Vec<_>>(),
            }));
        }
    }
    let mut reply = Reply::new("series", rows);
    reply.window = Some(window);
    Ok(reply)
}

enum Field {
    Health(usize),
    Span(String),
    Counter(CounterInfo),
}

fn resolve(ctx: &Ctx, side: Side, arg: &str) -> Result<Vec<Field>, QueryError> {
    let view = &ctx.views[0];
    let health_named = |name: &str| HEALTH.iter().position(|h| h.0 == name);
    let health_candidates = || {
        HEALTH
            .iter()
            .map(|h| (format!("h:{}", h.0), h.0.to_string()))
            .collect::<Vec<_>>()
    };
    if let Some(name) = arg.strip_prefix("h:") {
        return health_named(name).map(|i| vec![Field::Health(i)]).ok_or_else(|| {
            QueryError::with(format!("no health field named {name}"), health_candidates())
        });
    }
    if let Some(name) = arg.strip_prefix("span:").or_else(|| arg.strip_prefix("s:")) {
        return Ok(vec![Field::Span(span_name(ctx, side, name)?)]);
    }
    if arg.starts_with("x:") {
        return counters_named(view, side, arg.strip_prefix("x:").unwrap_or(arg));
    }
    if let Some(i) = health_named(arg) {
        return Ok(vec![Field::Health(i)]);
    }
    if let Ok(found) = counters_named(view, side, arg) {
        return Ok(found);
    }
    if let Ok(name) = span_name(ctx, side, arg) {
        return Ok(vec![Field::Span(name)]);
    }
    Err(QueryError::with(
        format!("{arg} is not a health field, a span or a counter; use h:, span: or x: to say which"),
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
    let candidates = table.rows.iter().take(10).map(|r| (ids::span(&r.name), r.name.clone())).collect();
    Err(QueryError::with(format!("no span named {name}"), candidates))
}

/// `name` or `name{k=v,...}`: every counter with that name (and those labels).
fn counters_named(view: &View, side: Side, text: &str) -> Result<Vec<Field>, QueryError> {
    let (name, labels) = match text.split_once('{') {
        Some((name, rest)) => {
            let pairs = rest.trim_end_matches('}').split(',').filter(|p| !p.is_empty());
            let mut labels: Vec<(String, String)> = pairs
                .filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
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
            .map(|c| (ids::counter(&c.name, &c.labels), c.name.clone()))
            .collect();
        return Err(QueryError::with(format!("no counter named {text}"), candidates));
    }
    Ok(found)
}

impl Field {
    fn values(
        &self,
        ctx: &Ctx,
        side: Side,
        buckets: &[(f64, f64)],
    ) -> Result<(String, String, Vec<Option<f64>>), QueryError> {
        let view = &ctx.views[0];
        match self {
            Field::Health(i) => {
                let (name, unit, pick) = HEALTH[*i];
                let series = Series::new(view.capture, side, pick);
                let values = buckets.iter().map(|&(lo, hi)| series.mean(lo, hi, 0.0)).collect();
                Ok((Id::new(Kind::Health, name).to_string(), unit.to_string(), values))
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
                let unit = if side == Side::Client { "ms_per_frame" } else { "ms_per_tick" };
                Ok((ids::span(name), unit.to_string(), values))
            }
            Field::Counter(info) => {
                let points = counter_points(ctx, side, info.id);
                let rate = info.kind == CounterKind::Sum;
                let values = buckets.iter().map(|&(lo, hi)| counter_value(&points, rate, lo, hi)).collect();
                let unit = match (rate, info.unit.as_str()) {
                    (true, "") => "per_s".to_string(),
                    (true, unit) => format!("{unit}_per_s"),
                    (false, unit) => unit.to_string(),
                };
                Ok((ids::counter(&info.name, &info.labels), unit, values))
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
        .filter_map(|s| s.ids.iter().position(|&i| i == id).and_then(|at| s.values.get(at)).map(|&v| (s.t, v)))
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
        let inside: Vec<f64> = points.iter().filter(|p| p.0 >= lo && p.0 < hi).map(|p| p.1).collect();
        (!inside.is_empty()).then(|| inside.iter().sum::<f64>() / inside.len() as f64)
    }
}
