use horizon_perf_format::v1::Side;

use super::profile;
use super::rows::{Row, TickRow, TickSpanRow};
use super::{Ctx, QueryError, Reply, num};
use crate::ids;
use crate::samples::Samples;
use crate::view::{TickUnit, View};
use crate::wire::{Candidate, Unit, UnitKind};

fn unit_row(u: &TickUnit) -> Row {
    Row::Tick(TickRow {
        id: ids::tick(u.counter),
        t: num(u.t),
        wall_ms: num(u.wall_ms),
        cpu_ms: num(u.cpu_ms),
        before_capture: u.t < 0.0,
        complete: u.complete,
        spans: u
            .spans
            .iter()
            .map(|s| TickSpanRow(s.name.clone(), s.depth, num(s.wall_ms), num(s.cpu_ms)))
            .collect(),
        hot: Vec::new(),
        samples_from: None,
        samples_to: None,
    })
}

const TICK_HOT: usize = 30;

fn unit_kind(side: Side) -> UnitKind {
    if side == Side::Client {
        UnitKind::Frame
    } else {
        UnitKind::Tick
    }
}

fn unit_label(side: Side) -> Unit {
    if side == Side::Client {
        Unit::MsPerFrame
    } else {
        Unit::MsPerTick
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
    let mut reply = Reply::new(unit_label(side), units.iter().map(unit_row).collect());
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
            .map(|u| Candidate::new(ids::tick(u.counter), format!("{:?} ms", num(u.wall_ms))))
            .collect();
        return Err(QueryError::with(
            format!(
                "this capture does not keep {} {counter}: it keeps the worst and the slow ones (the worst are listed)",
                unit_kind(side)
            ),
            kept,
        ));
    };
    let mut row = unit_row(found);
    if let (Row::Tick(tick), true) = (&mut row, found.t >= 0.0)
        && let Some(slow_over_ms) =
            Samples::new(&ctx.views[0], side).map(|s| f64::from(s.profile.slow_over_ms))
        && let Some((hot, covered)) = profile::hot_around(
            &ctx.views[0],
            side,
            (found.t - found.wall_ms / 1000.0).max(0.0),
            found.t,
            found.wall_ms >= slow_over_ms,
            TICK_HOT,
        )
    {
        tick.hot = hot;
        tick.samples_from = Some(num(covered.from));
        tick.samples_to = Some(num(covered.to));
    }
    let mut reply = Reply::new(unit_label(side), vec![row]);
    if !found.complete {
        reply
            .warnings
            .push("only the 8 largest spans of this tick were kept".into());
    }
    Ok(reply)
}
