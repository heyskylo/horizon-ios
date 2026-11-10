//! The span-based sections of the summary: phases, worst ticks, net, memory, world, workers, cost and warnings.

use std::collections::{BTreeMap, HashMap, HashSet};

use horizon_perf_format::v1::{Side, Stop};

use super::{Line, Section, SummaryOptions, megabytes, thousands};
use crate::diff::{Diff, signed};
use crate::error::Error;
use crate::ids;
use crate::query::traffic::{self, PlayerRate};
use crate::query::warning_text;
use crate::view::{SpanRow, SpanTable, TickSpan, TickUnit, View, Window, is_megabytes};

pub(super) const NOTE: &str = "note    chunk coordinates and entity/block type names are included; no names, UUIDs, addresses, seed or world name";

const PHASE_ROWS: usize = 20;
const CHILDREN_PER_SPAN: usize = 6;
const WORST_TICKS: usize = 5;
const WORST_SPANS: usize = 2;
const MEMORY_ROWS: usize = 3;
const GPU_ROWS: usize = 4;
const NAME_WIDTH_CAP: usize = 30;
const SLOW_MS: f64 = 50.0;
const KIB: f64 = 1024.0;

pub(super) fn all(
    view: &View,
    side: Side,
    options: &SummaryOptions,
    base: Option<&Diff>,
) -> Result<Vec<Section>, Error> {
    let mut parts = vec![phases(view, side, options, base)?];
    parts.extend(super::hot::sections(view, side, options));
    parts.extend([
        worst(view, side),
        net(view, side),
        gpu(view, side),
        memory(view, side),
        super::hot::heap(view, side),
        world(view, side),
        workers(view, side),
        cost(view, side),
        super::bench::lines(view, side),
        warnings(view, side),
    ]);
    Ok(parts.into_iter().filter(|p| !p.is_empty()).collect())
}

fn per_unit(side: Side) -> &'static str {
    if side == Side::Client {
        "frame"
    } else {
        "tick"
    }
}

/// "N of M ticks over 50 ms", from the main thread's histogram when 50 ms is one of its edges.
pub(super) fn slow_share(view: &View, side: Side) -> Option<String> {
    if side != Side::Server {
        return None;
    }
    let thread = view.resolve_thread(side, None).ok()?;
    let histogram = view.summary(side, thread)?.histogram.as_ref()?;
    let at = histogram
        .edges_ms
        .iter()
        .position(|&e| f64::from(e) == SLOW_MS)?;
    let total: u64 = histogram.counts.iter().sum();
    if total == 0 {
        return None;
    }
    let over: u64 = histogram.counts.iter().skip(at + 1).sum();
    Some(format!(
        "{} of {} ticks over {SLOW_MS:.0} ms",
        thousands(over),
        thousands(total)
    ))
}

fn phase_table(
    view: &View,
    side: Side,
    options: &SummaryOptions,
) -> Result<Option<SpanTable>, Error> {
    if options.slow {
        let table = view.slow_table(
            side,
            View::main_unit(side),
            Window {
                from: 0.0,
                to: view.end(side),
            },
        );
        return Ok((table.units > 0).then_some(table));
    }
    match view.resolve_thread(side, options.thread.as_deref()) {
        Ok(thread) => Ok(view.span_table(side, thread, None)),
        Err(detail) if options.thread.is_some() && !view.threads(side).is_empty() => {
            Err(Error::NoSuchThread { detail })
        }
        Err(_) => Ok(None),
    }
}

/// The spans as a tree, biggest sibling first, with how deep each one sits.
fn tree_order(rows: &[SpanRow]) -> Vec<(&SpanRow, usize)> {
    let known: HashSet<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let mut children: HashMap<Option<&str>, Vec<&SpanRow>> = HashMap::new();
    for row in rows {
        let parent = row
            .parent
            .as_deref()
            .filter(|p| known.contains(p) && *p != row.name);
        children.entry(parent).or_default().push(row);
    }
    for siblings in children.values_mut() {
        siblings.truncate(CHILDREN_PER_SPAN);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    fn walk<'a>(
        parent: Option<&'a str>,
        depth: usize,
        children: &HashMap<Option<&'a str>, Vec<&'a SpanRow>>,
        seen: &mut HashSet<&'a str>,
        out: &mut Vec<(&'a SpanRow, usize)>,
    ) {
        for row in children.get(&parent).into_iter().flatten() {
            if seen.insert(row.name.as_str()) {
                out.push((row, depth));
                walk(Some(row.name.as_str()), depth + 1, children, seen, out);
            }
        }
    }
    walk(None, 0, &children, &mut seen, &mut out);
    out
}

fn phases(
    view: &View,
    side: Side,
    options: &SummaryOptions,
    base: Option<&Diff>,
) -> Result<Vec<Line>, Error> {
    let base = base.filter(|_| !options.slow);
    let Some(table) = phase_table(view, side, options)? else {
        return Ok(Vec::new());
    };
    let mut ordered = tree_order(&table.rows);
    if ordered.len() > PHASE_ROWS {
        let mut walls: Vec<f64> = ordered.iter().map(|(r, _)| r.wall_ms).collect();
        walls.sort_by(|a, b| b.total_cmp(a));
        let floor = walls[PHASE_ROWS - 1];
        let mut room = PHASE_ROWS;
        ordered.retain(|(r, _)| {
            let keep = r.wall_ms >= floor && room > 0;
            room -= usize::from(keep);
            keep
        });
    }
    if ordered.is_empty() {
        return Ok(Vec::new());
    }
    let unit = per_unit(side);
    let mean = table.mean_unit_ms();
    let width = ordered
        .iter()
        .map(|(r, depth)| r.name.len() + 2 * depth)
        .max()
        .unwrap_or(0)
        .clamp(12, NAME_WIDTH_CAP)
        + 2;
    let share = if side == Side::Client {
        "%frame"
    } else {
        "%MSPT"
    };
    let what = if options.slow {
        format!("slow {unit} phases")
    } else {
        format!("{unit} phases")
    };
    let mut lines = vec![Line::heading(match base {
        Some(_) => format!(
            "{:<w$}{:>8}{:>8}  verdict",
            what,
            format!("ms/{unit}"),
            "Δ ms",
            w = width + 2
        ),
        None => format!(
            "{:<w$}{:>8}{:>8}{:>8}{:>8}",
            what,
            format!("ms/{unit}"),
            share,
            "cpu ms",
            "waited",
            w = width + 2
        ),
    })];
    for (row, depth) in ordered {
        let wall = table.per_unit(row.wall_ms);
        let cpu = table.per_unit(row.cpu_ms);
        let id = ids::span(&row.name);
        let label = format!("{}{}", "  ".repeat(depth), row.name);
        let text = if let Some(d) = base {
            let found = d.row(&id);
            let delta = found
                .and_then(|r| r.effect)
                .map_or_else(|| "-".to_string(), signed);
            let verdict = found.map_or("-", |r| r.verdict.mark());
            format!("  {label:<width$}{wall:>8.1}{delta:>8}  {verdict:<8}  [{id}]")
        } else {
            let pct = mean.map_or_else(|| "-".to_string(), |m| format!("{:.1}", wall / m * 100.0));
            format!(
                "  {label:<width$}{wall:>8.1}{pct:>8}{cpu:>8.1}{:>8.1}   [{id}]",
                (wall - cpu).max(0.0)
            )
        };
        lines.push(Line::ranked(text, row.wall_ms));
    }
    if let Some(d) = base {
        for gone in d
            .rows
            .iter()
            .filter(|r| r.id.starts_with("s:") && r.verdict == crate::stats::Verdict::Gone)
        {
            let delta = signed(-gone.a.unwrap_or(0.0));
            lines.push(Line::ranked(
                format!(
                    "  {:<width$}{:>8}{delta:>8}  {:<8}  [{}]",
                    gone.id.trim_start_matches("s:"),
                    "-",
                    gone.verdict.mark(),
                    gone.id
                ),
                0.0,
            ));
        }
    }
    Ok(lines)
}

/// The biggest spans of one unit below its root.
fn culprits(spans: &[TickSpan]) -> Vec<&TickSpan> {
    let roots = spans.iter().filter(|s| s.depth == 0).count();
    let level = if roots == 1 && spans.len() > 1 { 1 } else { 0 };
    let mut shown: Vec<&TickSpan> = spans.iter().filter(|s| s.depth == level).collect();
    shown.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
    shown.truncate(WORST_SPANS);
    shown
}

fn worst(view: &View, side: Side) -> Vec<Line> {
    let mut units: Vec<TickUnit> = view
        .tick_units(side, View::main_unit(side))
        .into_iter()
        .filter(|u| u.t >= 0.0)
        .collect();
    units.sort_by(|a, b| {
        b.wall_ms
            .total_cmp(&a.wall_ms)
            .then(a.counter.cmp(&b.counter))
    });
    units.truncate(WORST_TICKS);
    units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let spans: Vec<String> = culprits(&u.spans)
                .iter()
                .map(|s| format!("{} {:.1}", s.name, s.wall_ms))
                .collect();
            let culprit = if spans.is_empty() {
                String::new()
            } else {
                format!(" ({})", spans.join(", "))
            };
            Line::body(format!(
                "{}{:.1} ms at {:.1} s{culprit} [{}]",
                if i == 0 { "worst   " } else { "        " },
                u.wall_ms,
                u.t,
                ids::tick(u.counter)
            ))
        })
        .collect()
}

fn rate(kib_s: f64) -> String {
    if kib_s >= KIB * KIB {
        format!("{:.1} GiB/s", kib_s / KIB / KIB)
    } else if kib_s >= KIB {
        format!("{:.1} MiB/s", kib_s / KIB)
    } else {
        format!("{kib_s:.0} KiB/s")
    }
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    let at = ((sorted.len() as f64 * q) as usize).min(sorted.len().saturating_sub(1));
    sorted.get(at).copied().unwrap_or(0.0)
}

fn net(view: &View, side: Side) -> Vec<Line> {
    let Some(net) = view.capture.net.iter().find(|n| n.side() == side) else {
        return Vec::new();
    };
    let end = view.end(side).max(f64::from(net.block_s));
    let players: Vec<PlayerRate> = traffic::rates(net, 0.0, end);
    let (out, inn) = players
        .iter()
        .fold((0u64, 0u64), |a, p| (a.0 + p.bytes_out, a.1 + p.bytes_in));
    let mut text = format!(
        "net     out {}, in {}",
        rate(out as f64 / KIB / end),
        rate(inn as f64 / KIB / end)
    );
    let mut ranked: Vec<(f64, u32)> = players.iter().map(|p| (p.out_kib_s(), p.index)).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    if let Some(&(max, index)) = ranked.last() {
        let sorted: Vec<f64> = ranked.iter().map(|r| r.0).collect();
        text.push_str(&format!(
            " · per player out p50 {} p95 {} max {} [{} max]",
            rate(percentile(&sorted, 0.5)),
            rate(percentile(&sorted, 0.95)),
            rate(max),
            ids::Id::new(ids::Kind::Player, format!("p{index}"))
        ));
    }
    vec![Line::body(text)]
}

fn gpu(view: &View, side: Side) -> Vec<Line> {
    let parts: Vec<String> = crate::query::gpu_rows(view, side)
        .into_iter()
        .take(GPU_ROWS)
        .map(|p| {
            let ms = p.ms.map_or_else(String::new, |ms| format!(" {ms:.2} ms"));
            let overdraw = p
                .counters
                .and_then(|c| c.fragments_per_pixel)
                .map_or_else(String::new, |f| format!(" {f:.2} frag/px"));
            format!("{}{ms}{overdraw} [{}]", p.name, p.id)
        })
        .collect();
    if parts.is_empty() {
        return Vec::new();
    }
    vec![Line::body(format!("gpu     {}", parts.join(" · ")))]
}

fn memory(view: &View, side: Side) -> Vec<Line> {
    let snaps = view.snapshots(side);
    let Some(last) = snaps.last() else {
        return Vec::new();
    };
    let first = (snaps.len() > 1).then(|| &snaps[0]);
    let paths: Vec<&str> = last.values.iter().map(|(p, _)| p.as_str()).collect();
    let is_leaf = |p: &str| {
        !paths
            .iter()
            .any(|other| other.starts_with(&format!("{p}.")) || other.starts_with(&format!("{p}[")))
    };
    let mut rows: Vec<&(String, f64)> = last
        .values
        .iter()
        .filter(|(p, v)| is_megabytes(p) && is_leaf(p) && !p.starts_with("process.") && *v > 0.0)
        .collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let parts: Vec<String> = rows
        .into_iter()
        .take(MEMORY_ROWS)
        .map(|(path, value)| {
            let name = path.split_once('[').map_or_else(
                || path.rsplit('.').next().unwrap_or(path),
                |(_, rest)| rest.trim_end_matches(']'),
            );
            let change = first
                .map(|f| format!(" ({:+.0})", value - f.get(path).unwrap_or(0.0)))
                .unwrap_or_default();
            format!(
                "{name} {}{change} [{}]",
                megabytes(*value),
                ids::Id::new(ids::Kind::Memory, path.as_str())
            )
        })
        .collect();
    if parts.is_empty() {
        return Vec::new();
    }
    vec![Line::body(format!("memory  {}", parts.join(" · ")))]
}

fn world(view: &View, side: Side) -> Vec<Line> {
    let Some(snap) = view
        .capture
        .world
        .iter()
        .filter(|w| w.side() == side)
        .max_by(|a, b| a.t.total_cmp(&b.t))
    else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let mut dims: Vec<(String, u32, u64)> = snap
        .dims
        .iter()
        .map(|d| {
            let entities = d.entities.iter().map(|e| u64::from(e.count)).sum();
            (
                view.text(side, d.dimension).into_owned(),
                d.chunks,
                entities,
            )
        })
        .collect();
    dims.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !dims.is_empty() {
        let parts: Vec<String> = dims
            .iter()
            .take(3)
            .map(|(name, chunks, entities)| {
                format!(
                    "{name} {} chunks {} entities [d:{name}]",
                    thousands(chunks),
                    thousands(entities)
                )
            })
            .collect();
        lines.push(Line::body(format!("world   {}", parts.join(" · "))));
    }
    let mut costs: BTreeMap<String, f64> = BTreeMap::new();
    for c in &snap.entity_costs {
        *costs
            .entry(view.text(side, c.entity).into_owned())
            .or_default() += f64::from(c.ms_per_tick);
    }
    let mut ranked: Vec<(String, f64)> = costs.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if !ranked.is_empty() {
        let parts: Vec<String> = ranked
            .iter()
            .take(4)
            .map(|(name, ms)| format!("{name} {ms:.1} [e:{name}]"))
            .collect();
        lines.push(Line::body(format!(
            "        by entity type (sampled ms/tick): {}",
            parts.join(" · ")
        )));
    }
    lines
}

fn workers(view: &View, side: Side) -> Vec<Line> {
    let Some(workers) = view.capture.workers.iter().find(|w| w.side() == side) else {
        return Vec::new();
    };
    let mut kinds: Vec<(String, f32)> = workers
        .kinds
        .iter()
        .map(|k| (view.text(side, k.kind).into_owned(), k.cpu_cores))
        .collect();
    kinds.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let parts: Vec<String> = kinds
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, (name, cores))| {
            format!("{name} {cores:.2}{}", if i == 0 { " cores" } else { "" })
        })
        .collect();
    if parts.is_empty() {
        return Vec::new();
    }
    vec![Line::body(format!(
        "workers {} ({})",
        parts.join(" · "),
        crate::error::side_name(side)
    ))]
}

fn cost(view: &View, side: Side) -> Vec<Line> {
    let Some(overhead) = view
        .capture
        .stop
        .iter()
        .find(|s: &&Stop| s.side() == side)
        .and_then(|s| s.overhead.as_ref())
    else {
        return Vec::new();
    };
    let pct = |share: f32| format!("{:.2}%", f64::from(share) * 100.0);
    let unit = per_unit(side);
    let mut parts = Vec::new();
    if overhead.spans_ms_per_tick > 0.0 {
        parts.push(format!(
            "spans {:.2} ms/{unit} ({} of the {unit} thread)",
            overhead.spans_ms_per_tick,
            pct(overhead.spans_share)
        ));
    }
    if overhead.sampler_share_of_thread > 0.0 {
        parts.push(format!(
            "sampler {} of the {unit} thread",
            pct(overhead.sampler_share_of_thread)
        ));
    }
    if overhead.writer_core_share > 0.0 {
        parts.push(format!(
            "writer {} of a core",
            pct(overhead.writer_core_share)
        ));
    }
    if overhead.heap_samples > 0 {
        parts.push(format!(
            "heap sampler {} samples, {:.1} ms",
            thousands(overhead.heap_samples),
            overhead.heap_sample_ns as f64 / 1e6
        ));
    }
    parts.push(format!(
        "{} samples dropped",
        thousands(overhead.dropped_samples)
    ));
    if overhead.dropped_messages > 0 {
        parts.push(format!(
            "{} messages dropped",
            thousands(overhead.dropped_messages)
        ));
    }
    vec![Line::body(format!("cost    {}", parts.join(" · ")))]
}

fn warnings(view: &View, side: Side) -> Vec<Line> {
    view.capture
        .warnings
        .iter()
        .filter(|w| w.side() == side)
        .map(|w| Line::body(format!("warn    {:.1} s: {}", w.t, warning_text(w))))
        .collect()
}

/// The drill-down commands that exist for what this capture holds.
pub(super) fn next(view: &View, sides: &[Side], options: &SummaryOptions) -> String {
    let side = sides.first().copied().unwrap_or(Side::Server);
    let flag = options.side.map_or(String::new(), |s| {
        format!(" --side {}", crate::error::side_name(s))
    });
    let mut commands = Vec::new();
    if !view.tick_units(side, View::main_unit(side)).is_empty() {
        commands.push(format!("horizon-perf query ticks --worst 5{flag}"));
    }
    if !view
        .slow_events(side, View::main_unit(side), Window::ALL)
        .is_empty()
    {
        commands.push(format!("horizon-perf query spans --slow{flag}"));
    }
    let top = view
        .resolve_thread(side, options.thread.as_deref())
        .ok()
        .and_then(|t| view.span_table(side, t, None))
        .and_then(|t| t.rows.first().map(|r| r.name.clone()));
    let health = if side == Side::Client {
        "h:frame_ms_p95"
    } else {
        "h:mspt_p95"
    };
    match top {
        Some(name) => commands.push(format!(
            "horizon-perf query series {health} {} --from -900{flag}",
            ids::span(&name)
        )),
        None => commands.push(format!(
            "horizon-perf query series {health} --from -900{flag}"
        )),
    }
    if let Some(id) = super::hot::top_function(view, side, options) {
        commands.push(format!("horizon-perf query callers {id}{flag}"));
    }
    if view.snapshots(side).len() > 1 {
        commands.push(format!("horizon-perf query memory --diff{flag}"));
    }
    format!("next    {}", commands.join(" · "))
}
