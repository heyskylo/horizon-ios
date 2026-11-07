use horizon_perf_format::v1::Side;
use serde_json::{Value, json};

use super::{Ctx, QueryError, Reply, num};
use crate::ids;
use crate::view::{TickUnit, View};

fn unit_json(u: &TickUnit) -> Value {
    json!({
        "id": ids::tick(u.counter),
        "t": num(u.t),
        "wall_ms": num(u.wall_ms),
        "cpu_ms": num(u.cpu_ms),
        "before_capture": u.t < 0.0,
        "complete": u.complete,
        "spans": u.spans.iter().map(|s| json!([s.name, s.depth, num(s.wall_ms), num(s.cpu_ms)])).collect::<Vec<_>>(),
    })
}

fn unit_name(side: Side) -> &'static str {
    if side == Side::Client {
        "frame"
    } else {
        "tick"
    }
}

fn unit_label(side: Side) -> &'static str {
    if side == Side::Client {
        "ms_per_frame"
    } else {
        "ms_per_tick"
    }
}

/// The worst ticks or frames with their span trees: the summary's worst, the live slow events, then the ring from before the capture.
pub(super) fn ticks(ctx: &Ctx) -> Result<Reply, QueryError> {
    let side = ctx.side(0)?;
    let window = ctx.window(0, side, f64::NEG_INFINITY)?;
    let mut units: Vec<TickUnit> = ctx.views[0]
        .tick_units(side, View::main_unit(side))
        .into_iter()
        .filter(|u| window.contains(u.t) && ctx.q.over.is_none_or(|over| u.wall_ms > over))
        .collect();
    units.sort_by(|a, b| {
        b.wall_ms
            .total_cmp(&a.wall_ms)
            .then(a.counter.cmp(&b.counter))
    });
    units.truncate(ctx.q.worst.unwrap_or(ctx.q.filters.limit));
    let mut reply = Reply::new(unit_label(side), units.iter().map(unit_json).collect());
    reply.window = Some(window);
    Ok(reply)
}

/// One tick or frame by id (`k:48812` or the bare counter) with its span tree.
pub(super) fn tick(ctx: &Ctx) -> Result<Reply, QueryError> {
    let side = ctx.side(0)?;
    let [arg] = ctx.q.args.as_slice() else {
        return Err(QueryError::new("tick takes one id, for example k:48812"));
    };
    let counter: u64 =
        arg.strip_prefix("k:").unwrap_or(arg).parse().map_err(|_| {
            QueryError::new(format!("{arg} is not a tick id; expected k:<counter>"))
        })?;
    let units = ctx.views[0].tick_units(side, View::main_unit(side));
    let Some(found) = units.iter().find(|u| u.counter == counter) else {
        let mut worst: Vec<&TickUnit> = units.iter().collect();
        worst.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
        let kept = worst
            .iter()
            .take(10)
            .map(|u| (ids::tick(u.counter), format!("{} ms", num(u.wall_ms))))
            .collect();
        return Err(QueryError::with(
            format!(
                "this capture does not keep {} {counter}: it keeps the worst and the slow ones (the worst are listed)",
                unit_name(side)
            ),
            kept,
        ));
    };
    let mut reply = Reply::new(unit_label(side), vec![unit_json(found)]);
    if !found.complete {
        reply
            .warnings
            .push("only the 8 largest spans of this tick were kept".into());
    }
    Ok(reply)
}
