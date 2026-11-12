use horizon_perf_format::v1::Unit as TickUnit;

use super::rows::{Row, SpanRow as SpanOut};
use super::{Ctx, QueryError, Reply, num};
use crate::ids;
use crate::view::{SpanRow, SpanTable, View, Window};
use crate::wire::{Candidate, Unit};

/// Per span: ms per tick (or per slow tick), cpu and waited, one column per capture.
pub(super) fn spans(ctx: &Ctx) -> Result<Reply, QueryError> {
    let slow = ctx.q.filters.slow;
    let mut tables = Vec::new();
    let mut window = None;
    for i in 0..ctx.views.len() {
        let side = ctx.side(i)?;
        let end = ctx.views[i].end(side);
        let table = if slow {
            let w = ctx.window(i, side, 0.0)?;
            window.get_or_insert(w);
            ctx.views[i].slow_table(side, View::main_unit(side), w)
        } else {
            let thread = ctx.thread(i, side)?;
            let w = ctx
                .has_window()
                .then(|| ctx.window(i, side, 0.0))
                .transpose()?;
            window.get_or_insert(w.unwrap_or(Window { from: 0.0, to: end }));
            ctx.views[i].span_table(side, thread, w).ok_or_else(|| {
                QueryError::new(format!(
                    "{} has no spans for the selected side, thread and time",
                    ctx.files[i].file
                ))
            })?
        };
        tables.push(table);
    }
    let unit = match (slow, tables[0].unit) {
        (true, TickUnit::Frame) => Unit::MsPerSlowFrame,
        (true, _) => Unit::MsPerSlowTick,
        (false, TickUnit::Frame) => Unit::MsPerFrame,
        (false, _) => Unit::MsPerTick,
    };

    let mut order: Vec<&str> = Vec::new();
    for table in &tables {
        for row in &table.rows {
            if !order.contains(&row.name.as_str()) {
                order.push(&row.name);
            }
        }
    }
    let wanted = resolve_names(&order, &ctx.q.names)?;
    let rows: Vec<Row> = order
        .iter()
        .filter(|name| {
            wanted
                .as_ref()
                .is_none_or(|w| w.contains(&name.to_string()))
        })
        .take(ctx.q.filters.limit)
        .map(|name| row(name, &tables))
        .collect();
    let mut reply = Reply::new(unit, rows);
    reply.window = window;
    if slow && tables.iter().any(|t| t.units == 0) {
        reply
            .warnings
            .push("a capture has no slow ticks or frames in the selected time".into());
    }
    Ok(reply)
}

fn row(name: &str, tables: &[SpanTable]) -> Row {
    let found: Vec<Option<&SpanRow>> = tables.iter().map(|t| t.row(name)).collect();
    let per = |pick: &dyn Fn(&SpanRow) -> f64| -> Vec<Option<f64>> {
        found
            .iter()
            .zip(tables)
            .map(|(r, t)| r.map(|r| num(t.per_unit(pick(r)))))
            .collect()
    };
    let first = found.iter().flatten().next();
    Row::Span(SpanOut {
        id: ids::span(name),
        name: name.to_string(),
        parent: first.and_then(|r| r.parent.as_deref()).map(ids::span),
        values: per(&|r| r.wall_ms),
        cpu: per(&|r| r.cpu_ms),
        waited: per(&|r| (r.wall_ms - r.cpu_ms).max(0.0)),
        seen: found.iter().map(|r| r.map(|r| r.seen)).collect(),
    })
}

/// The names asked for (plain or `s:` ids): an exact match, else the one name containing it; anything else lists candidates.
fn resolve_names(known: &[&str], wanted: &[String]) -> Result<Option<Vec<String>>, QueryError> {
    if wanted.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for raw in wanted {
        let name = raw.strip_prefix("s:").unwrap_or(raw);
        if known.contains(&name) {
            out.push(name.to_string());
            continue;
        }
        let lower = name.to_lowercase();
        let partial: Vec<&&str> = known
            .iter()
            .filter(|k| k.to_lowercase().contains(&lower))
            .collect();
        let candidates = |names: Vec<&&str>| {
            names
                .into_iter()
                .map(|n| Candidate::new(ids::span(n), n.to_string()))
                .collect()
        };
        match partial.as_slice() {
            [one] => out.push(one.to_string()),
            [] => {
                let list: Vec<&&str> = known.iter().take(10).collect();
                return Err(QueryError::with(
                    format!("no span named {name}"),
                    candidates(list),
                ));
            }
            _ => {
                return Err(QueryError::with(
                    format!("{name} matches {} spans: pick one", partial.len()),
                    candidates(partial),
                ));
            }
        }
    }
    Ok(Some(out))
}
