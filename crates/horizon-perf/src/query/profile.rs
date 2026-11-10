//! Sample queries that rank or divide weight: `hot`, `split`, `group` and `frames` (docs/perf-platform.md §6.2).

use std::collections::{BTreeMap, HashMap};

use regex::Regex;

use super::rows::{FrameRow, HotRow, Row, SplitRow};
use super::{Ctx, QueryError, Reply, Sampled, num};
use crate::error::side_name;
use crate::ids::{Id, Kind};
use crate::samples::{Pick, Samples, Tally, Units, out_unit};
use crate::view::Window;
use crate::wire::{Candidate, ContextKey, HotBy, SampleBy, Select};

/// One capture's samples, narrowed by the query's filters.
pub(super) struct Run<'a> {
    pub samples: Samples<'a>,
    pub pick: Pick,
    pub thread: u32,
    pub units: Units,
}

impl Run<'_> {
    /// A weight as a rate over the ticks or frames of the selection.
    pub fn per_unit(&self, value: f64) -> f64 {
        value / self.units.count.max(1) as f64
    }

    /// The window the weights cover, widened to whole time blocks; None when the query covered the whole capture.
    pub fn window(&self) -> Option<Window> {
        (self.pick.window != Window::ALL).then(|| self.samples.snapped(self.pick.window))
    }

    pub fn sampled(&self, samples: u64) -> Sampled {
        Sampled {
            thread: self.samples.thread_names[self.thread as usize].clone(),
            ticks: self.units.count,
            samples,
        }
    }

    pub fn warnings(&self, hits: u64) -> Vec<String> {
        let mut out = Vec::new();
        if hits == 0 {
            out.push("no samples match the selection".to_string());
        }
        if self.units.count == 0 {
            out.push("the selection covers no ticks or frames; values are totals".to_string());
        }
        if self.units.estimated {
            out.push(
                "slow ticks are not counted per time block: the window's share of them is an estimate"
                    .to_string(),
            );
        }
        out
    }
}

/// Opens capture `i`'s samples; `all_threads` keeps every thread's weight (for `group --by thread`).
pub(super) fn prepare<'a>(
    ctx: &'a Ctx<'_>,
    i: usize,
    all_threads: bool,
) -> Result<Run<'a>, QueryError> {
    prepare_on(ctx, i, all_threads, ctx.q.filters.thread.as_deref())
}

/// Like `prepare`, with the thread named by `named` (None: the side's main thread) giving the units.
fn prepare_on<'a>(
    ctx: &'a Ctx<'_>,
    i: usize,
    all_threads: bool,
    named: Option<&str>,
) -> Result<Run<'a>, QueryError> {
    let f = &ctx.q.filters;
    let side = ctx.side(i)?;
    let file = &ctx.files[i].file;
    let samples = Samples::new(&ctx.views[i], side).ok_or_else(|| {
        QueryError::new(format!(
            "{file} has no samples for its {} side: it was captured without them",
            side_name(side)
        ))
    })?;
    if !samples.has_kind(f.select) {
        return Err(QueryError::new(match f.select {
            Select::Live => format!("{file} records no live-allocation samples"),
            other => format!("{file} has no {other} samples"),
        }));
    }
    let thread = samples
        .thread(named)
        .map_err(|e| QueryError::new(format!("{file}: {e}")))?;
    let window = if ctx.has_window() {
        ctx.window(i, side, 0.0)?
    } else {
        Window::ALL
    };
    let pick = Pick {
        thread: (!all_threads).then_some(thread),
        select: f.select,
        slow: f.slow,
        contexts: samples.context_filter(&f.where_),
        window,
    };
    let units = samples.units(thread, f.slow, window);
    Ok(Run {
        samples,
        pick,
        thread,
        units,
    })
}

pub(super) fn unit_of(run: &Run, select: Select, slow: bool) -> crate::wire::Unit {
    out_unit(select, slow, run.samples.tick_unit(run.thread))
}

fn pct(part: f64, whole: f64) -> f64 {
    if whole > 0.0 {
        part / whole * 100.0
    } else {
        0.0
    }
}

/// Functions ranked by their own or their inclusive time.
pub(super) fn hot(ctx: &Ctx) -> Result<Reply, QueryError> {
    let f = &ctx.q.filters;
    let run = prepare(ctx, 0, false)?;
    let s = &run.samples;
    let t = s.function_times(&run.pick);
    let (own, inclusive) = (&t.own, &t.inclusive);
    let n = s.functions.len();
    let by = ctx.q.hot_by.unwrap_or(HotBy::SelfTime);
    let needle = ctx.q.matching.as_ref().map(|m| m.to_lowercase());
    let key = |at: usize| match by {
        HotBy::SelfTime => own[at].0,
        HotBy::Total => inclusive[at].0,
    };
    let mut ranked: Vec<usize> = (0..n)
        .filter(|&at| key(at) > 0.0)
        .filter(|&at| {
            needle.as_ref().is_none_or(|m| {
                s.functions[at].full.to_lowercase().contains(m) || s.functions[at].id == *m
            })
        })
        .collect();
    ranked.sort_by(|&a, &b| {
        key(b)
            .total_cmp(&key(a))
            .then_with(|| s.functions[a].full.cmp(&s.functions[b].full))
    });
    let rows = ranked
        .into_iter()
        .take(f.limit)
        .map(|at| {
            let func = &s.functions[at];
            Row::Hot(HotRow {
                id: func.id.clone(),
                name: func.name.clone(),
                full: func.full.clone(),
                file: func.file.clone(),
                line: func.line,
                self_ms: num(run.per_unit(own[at].0)),
                total: num(run.per_unit(inclusive[at].0)),
                self_pct: num(pct(own[at].0, t.grand_ms)),
                total_pct: num(pct(inclusive[at].0, t.grand_ms)),
                samples: inclusive[at].1,
            })
        })
        .collect();
    let mut reply = Reply::new(unit_of(&run, f.select, f.slow), rows);
    reply.warnings = run.warnings(t.grand_n);
    reply.sampled = Some(run.sampled(t.grand_n));
    reply.window = run.window();
    Ok(reply)
}

/// Where a split looks: all weight, or the weight of one id.
enum Scope {
    All,
    Nodes(Vec<bool>),
}

/// Narrows the pick or the nodes by `id`, which names a span, function, call path or context value.
fn scope(run: &mut Run, id: &str) -> Result<Scope, QueryError> {
    let Some(parsed) = Id::parse(id) else {
        return Err(QueryError::new(format!(
            "{id} is not an id; give s:, f:, p:, e:, b:, d:, c: or t:"
        )));
    };
    let s = &run.samples;
/// What the samples of the time blocks overlapping `[from, to]` say about the main thread: its top functions and the window the blocks cover.
pub(super) fn hot_around(
    view: &View,
    side: Side,
    from: f64,
    to: f64,
    slow: bool,
    limit: usize,
) -> Option<(Vec<HotRow>, Window)> {
    let samples = Samples::new(view, side)?;
    let thread = samples.thread(None).ok()?;
    let window = Window { from, to };
    let pick = Pick {
        thread: Some(thread),
        select: Select::Cpu,
        slow,
        contexts: None,
        window,
    };
    let units = samples.units(thread, slow, window);
    let covered = samples.snapped(window);
    let run = Run {
        samples,
        pick,
        thread,
        units,
    };
    let (rows, _) = hot_rows(&run, HotBy::SelfTime, None, limit);
    Some((rows, covered))
}

    let by_context = |key: ContextKey| {
        let mut wanted = BTreeMap::new();
        wanted.insert(key, vec![parsed.key.clone()]);
        s.context_filter(&wanted)
    };
    let narrowed = match parsed.kind {
        Kind::Span => by_context(ContextKey::Span),
        Kind::Entity => by_context(ContextKey::Entity),
        Kind::Block => by_context(ContextKey::Block),
        Kind::Dimension => by_context(ContextKey::Dim),
        Kind::Chunk => by_context(ContextKey::Chunk),
        Kind::Thread => {
            run.pick.thread = Some(run.thread);
            return Ok(Scope::All);
        }
        Kind::Function => {
            let at = s
                .function_index(id)
                .ok_or_else(|| QueryError::new(format!("no function {id}")))?;
            return Ok(Scope::Nodes(s.under(|n| s.func[n as usize] == at)));
        }
        Kind::Path => {
            let inside = s.under(|n| s.path_id(n) == id);
            if !inside.contains(&true) {
                return Err(QueryError::new(format!("no call path {id}")));
            }
            return Ok(Scope::Nodes(inside));
        }
        _ => {
            return Err(QueryError::new(format!(
                "split cannot narrow to {id}; give s:, f:, p:, e:, b:, d:, c: or t:"
            )));
        }
    };
    let both = match (run.pick.contexts.take(), narrowed) {
        (Some(a), Some(b)) => Some(a.iter().zip(&b).map(|(x, y)| *x && *y).collect()),
        (a, b) => a.or(b),
    };
    run.pick.contexts = both;
    Ok(Scope::All)
}

/// `split ID --by X` and `group --by X`: the selection's weight divided by one context.
pub(super) fn divide(ctx: &Ctx, group: bool) -> Result<Reply, QueryError> {
    let op = if group { "group" } else { "split" };
    let by = ctx
        .q
        .by_sample
        .filter(|b| !group || b.groups())
        .ok_or_else(|| {
            let ok: Vec<&str> = crate::wire::SampleBy::ALL
                .iter()
                .filter(|b| !group || b.groups())
                .map(|b| b.as_str())
                .collect();
            QueryError::new(format!("{op} needs --by {}", ok.join("|")))
        })?;
    let f = &ctx.q.filters;
    let named = match ctx.q.args.first().and_then(|a| Id::parse(a)) {
        Some(id) if !group && id.kind == Kind::Thread => Some(id.key),
        _ => f.thread.clone(),
    };
    let mut run = prepare_on(ctx, 0, by == SampleBy::Thread, named.as_deref())?;
    let narrow = match (group, ctx.q.args.first()) {
        (false, Some(id)) => scope(&mut run, id)?,
        _ => Scope::All,
    };
    let inside = match &narrow {
        Scope::Nodes(nodes) => Some(nodes.as_slice()),
        Scope::All => None,
    };
    let shares = run.samples.divide(&run.pick, by, inside);
    let whole: f64 = shares.iter().map(|s| s.value).sum();
    let count: u64 = shares.iter().map(|s| s.count).sum();
    let rows = shares
        .into_iter()
        .take(f.limit)
        .map(|share| {
            Row::Split(SplitRow {
                by,
                key: share.key,
                label: share.label,
                value: num(run.per_unit(share.value)),
                pct: num(pct(share.value, whole)),
                samples: share.count,
            })
        })
        .collect();
    let mut reply = Reply::new(unit_of(&run, f.select, f.slow), rows);
    reply.warnings = run.warnings(count);
    if by == SampleBy::Thread {
        reply.warnings.push(format!(
            "every thread's weight is divided by the ticks or frames of {}",
            run.samples.thread_names[run.thread as usize]
        ));
    }
    reply.sampled = Some(run.sampled(count));
    reply.window = run.window();
    Ok(reply)
}

struct Found {
    id: String,
    name: String,
    full: String,
}

/// Per capture, each function's inclusive and own rate and sample count.
type Stats = HashMap<String, (f64, f64, u64)>;

fn stats(run: &Run, t: &Tally) -> (Stats, Vec<Found>) {
    let s = &run.samples;
    let mut stats: Stats = HashMap::new();
    for func in &s.functions {
        stats.insert(func.id.clone(), (0.0, 0.0, 0));
    }
    for node in 0..s.func.len() {
        let id = &s.function_of(node as u32).id;
        let entry = stats.get_mut(id).expect("every function has an entry");
        entry.1 += run.per_unit(t.self_ms[node]);
        if t.total_n[node] > 0 && !s.shadowed(node as u32) {
            entry.0 += run.per_unit(t.total_ms[node]);
            entry.2 += t.total_n[node];
        }
    }
    let found = s
        .functions
        .iter()
        .map(|f| Found {
            id: f.id.clone(),
            name: f.name.clone(),
            full: f.full.clone(),
        })
        .collect();
    (stats, found)
}

enum Wanted {
    Id(String),
    Pattern(Regex),
    Name(String),
}

fn wanted(arg: &str) -> Result<Wanted, QueryError> {
    if let Some(Id {
        kind: Kind::Function,
        ..
    }) = Id::parse(arg)
    {
        return Ok(Wanted::Id(arg.to_string()));
    }
    if arg.len() > 2
        && let Some(pattern) = arg.strip_prefix('/').and_then(|a| a.strip_suffix('/'))
    {
        return Regex::new(pattern)
            .map(Wanted::Pattern)
            .map_err(|e| QueryError::new(format!("bad pattern {arg}: {e}")));
    }
    Ok(Wanted::Name(arg.to_string()))
}

/// Inclusive and own time of the named functions, one column per capture.
pub(super) fn frames(ctx: &Ctx) -> Result<Reply, QueryError> {
    if ctx.q.args.is_empty() {
        return Err(QueryError::new(
            "frames needs at least one NAME: an f: id, a short or full function name, or /regex/",
        ));
    }
    let f = &ctx.q.filters;
    let mut columns: Vec<Stats> = Vec::new();
    let mut known: Vec<Found> = Vec::new();
    let mut first_run = None;
    let mut differs = false;
    for i in 0..ctx.views.len() {
        let run = prepare(ctx, i, false)?;
        let t = run.samples.tally(&run.pick);
        let (stats, found) = stats(&run, &t);
        for func in found {
            if !known.iter().any(|k| k.id == func.id) {
                known.push(func);
            }
        }
        columns.push(stats);
        match &first_run {
            None => {
                let mut reply = Reply::new(unit_of(&run, f.select, f.slow), Vec::new());
                reply.warnings = run.warnings(t.grand_n);
                reply.sampled = Some(run.sampled(t.grand_n));
                reply.window = run.window();
                first_run = Some(reply);
            }
            Some(first) => {
                let thread = first.sampled.as_ref().map(|s| s.thread.as_str());
                differs |= thread != Some(run.sampled(0).thread.as_str())
                    || first.unit != unit_of(&run, f.select, f.slow);
            }
        }
    }
    let first = |id: &str| columns[0].get(id).map_or(f64::MIN, |s| s.0);
    let mut chosen: Vec<&Found> = Vec::new();
    for arg in &ctx.q.args {
        let mut hits: Vec<&Found> = match wanted(arg)? {
            Wanted::Id(id) => known.iter().filter(|k| k.id == id).collect(),
            Wanted::Pattern(re) => known.iter().filter(|k| re.is_match(&k.full)).collect(),
            Wanted::Name(name) => known
                .iter()
                .filter(|k| k.name == name || k.full == name)
                .collect(),
        };
        let candidates = |list: &[&Found]| -> Vec<Candidate> {
            list.iter()
                .take(10)
                .map(|k| Candidate::new(k.id.clone(), k.full.clone()))
                .collect()
        };
        if hits.is_empty() {
            let lower = arg.trim_matches('/').to_lowercase();
            let near: Vec<&Found> = known
                .iter()
                .filter(|k| k.full.to_lowercase().contains(&lower))
                .collect();
            return Err(QueryError::with(
                format!("no function matches {arg}"),
                candidates(&near),
            ));
        }
        if matches!(wanted(arg)?, Wanted::Name(_)) && hits.len() > 1 {
            return Err(QueryError::with(
                format!("{arg} names {} functions: pick one", hits.len()),
                candidates(&hits),
            ));
        }
        hits.sort_by(|a, b| {
            first(&b.id)
                .total_cmp(&first(&a.id))
                .then_with(|| a.full.cmp(&b.full))
        });
        for hit in hits {
            if !chosen.iter().any(|c| c.id == hit.id) {
                chosen.push(hit);
            }
        }
    }
    let column = |id: &str, pick: fn(&(f64, f64, u64)) -> f64| -> Vec<Option<f64>> {
        columns
            .iter()
            .map(|c| c.get(id).map(|s| num(pick(s))))
            .collect()
    };
    let rows = chosen
        .into_iter()
        .take(f.limit)
        .map(|k| {
            Row::Frame(FrameRow {
                id: k.id.clone(),
                name: k.name.clone(),
                full: k.full.clone(),
                values: column(&k.id, |s| s.0),
                self_ms: column(&k.id, |s| s.1),
                samples: columns.iter().map(|c| c.get(&k.id).map(|s| s.2)).collect(),
            })
        })
        .collect();
    let mut reply = first_run.expect("at least one capture");
    reply.rows = rows;
    if differs {
        reply.warnings.push(
            "the captures sampled different threads or units; the selection describes the first"
                .to_string(),
        );
    }
    Ok(reply)
}
