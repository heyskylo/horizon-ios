//! Call-tree queries: `tree`, `callers` and `callees`, merged over every occurrence of a function (docs/perf-platform.md §6.2).

use std::collections::BTreeMap;

use super::profile::{Run, prepare, unit_of};
use super::rows::{Folded, Row, TreeRow};
use super::{Ctx, QueryError, Reply, num};
use crate::ids::{Id, Kind};
use crate::samples::{NO_NODE, Samples, Tally};

const TREE_DEPTH: usize = 5;
const TREE_MIN_PCT: f64 = 0.5;
const CALLERS_DEPTH: usize = 3;
const CALLEES_DEPTH: usize = 1;

/// The nodes a query starts from: the thread's roots, a function's outermost occurrences, or one call path.
struct Start {
    nodes: Vec<u32>,
    /// The `f:` id when the nodes are a function's occurrences.
    function: Option<u32>,
}

fn start(run: &Run, id: Option<&String>) -> Result<Start, QueryError> {
    let s = &run.samples;
    let Some(id) = id else {
        let nodes = (0..s.func.len() as u32)
            .filter(|&n| s.parent[n as usize] == NO_NODE && s.thread_of[n as usize] == run.thread)
            .collect();
        return Ok(Start {
            nodes,
            function: None,
        });
    };
    match Id::parse(id) {
        Some(Id {
            kind: Kind::Function,
            ..
        }) => {
            let at = s
                .function_index(id)
                .ok_or_else(|| QueryError::new(format!("no function {id}")))?;
            Ok(Start {
                nodes: s.occurrences(at, Some(run.thread)),
                function: Some(at),
            })
        }
        Some(Id {
            kind: Kind::Path, ..
        }) => {
            let nodes: Vec<u32> = (0..s.func.len() as u32)
                .filter(|&n| s.thread_of[n as usize] == run.thread && s.path_id(n) == *id)
                .collect();
            if nodes.is_empty() {
                return Err(QueryError::new(format!("no call path {id} on this thread")));
            }
            Ok(Start {
                nodes,
                function: None,
            })
        }
        _ => Err(QueryError::new(format!(
            "{id} is not a function or call path id (f: or p:)"
        ))),
    }
}

struct Builder<'a> {
    run: &'a Run<'a>,
    t: &'a Tally,
    children: Vec<Vec<u32>>,
    min_pct: f64,
    limit: usize,
    /// Rows carry their own time as a share too.
    share: bool,
    /// Rows of merged occurrences carry the function's `f:` id even when one call path would do.
    function_ids: bool,
}

impl<'a> Builder<'a> {
    fn new(run: &'a Run<'a>, t: &'a Tally, ctx: &Ctx, min_pct: f64) -> Self {
        let s = &run.samples;
        let mut children = vec![Vec::new(); s.func.len()];
        for n in 0..s.func.len() {
            if s.parent[n] != NO_NODE {
                children[s.parent[n] as usize].push(n as u32);
            }
        }
        Self {
            run,
            t,
            children,
            min_pct,
            limit: ctx.q.filters.limit,
            share: ctx.q.self_share,
            function_ids: false,
        }
    }

    fn s(&self) -> &Samples<'a> {
        &self.run.samples
    }

    fn sum_ms(&self, group: &[u32], of: &[f64]) -> f64 {
        group.iter().map(|&n| of[n as usize]).sum()
    }

    fn sum_n(&self, group: &[u32]) -> u64 {
        group.iter().map(|&n| self.t.total_n[n as usize]).sum()
    }

    fn pct(&self, ms: f64) -> f64 {
        if self.t.grand_ms > 0.0 {
            ms / self.t.grand_ms * 100.0
        } else {
            0.0
        }
    }

    /// `p:` when every node of the group sits on one call path, else the function's own `f:`.
    fn id_of(&self, group: &[u32]) -> String {
        let s = self.s();
        let first = s.path_id(group[0]);
        if !self.function_ids && group.iter().all(|&n| s.path_id(n) == first) {
            first
        } else {
            s.function_of(group[0]).id.clone()
        }
    }

    fn row(&self, group: &[u32], id: String, depth: usize) -> TreeRow {
        let f = self.s().function_of(group[0]);
        let total = self.sum_ms(group, &self.t.total_ms);
        let own = self.sum_ms(group, &self.t.self_ms);
        let (children, folded) = if depth > 0 {
            self.children_of(group, depth)
        } else {
            (Vec::new(), None)
        };
        TreeRow {
            id,
            function: f.id.clone(),
            name: f.name.clone(),
            crate_name: f.crate_name.clone(),
            total: num(self.run.per_unit(total)),
            self_ms: Some(num(self.run.per_unit(own))),
            pct: num(self.pct(total)),
            self_pct: self.share.then(|| num(self.pct(own))),
            samples: self.sum_n(group),
            children,
            folded,
        }
    }

    /// The group's children merged by function, biggest first; rows under the share floor or past the limit are folded.
    fn children_of(&self, group: &[u32], depth: usize) -> (Vec<TreeRow>, Option<Folded>) {
        let mut by_function: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &n in group {
            for &c in &self.children[n as usize] {
                if self.t.total_n[c as usize] > 0 {
                    by_function
                        .entry(self.s().func[c as usize])
                        .or_default()
                        .push(c);
                }
            }
        }
        let mut merged: Vec<(f64, Vec<u32>)> = by_function
            .into_values()
            .map(|g| (self.sum_ms(&g, &self.t.total_ms), g))
            .collect();
        merged.sort_by(|a, b| {
            b.0.total_cmp(&a.0).then_with(|| {
                self.s()
                    .function_of(a.1[0])
                    .full
                    .cmp(&self.s().function_of(b.1[0]).full)
            })
        });
        self.fold(merged, |g| self.row(&g, self.id_of(&g), depth - 1))
    }

    fn fold(
        &self,
        merged: Vec<(f64, Vec<u32>)>,
        build: impl Fn(Vec<u32>) -> TreeRow,
    ) -> (Vec<TreeRow>, Option<Folded>) {
        let (mut rows, mut folded, mut folded_ms) = (Vec::new(), 0usize, 0.0);
        for (ms, group) in merged {
            if rows.len() < self.limit && self.pct(ms) >= self.min_pct {
                rows.push(build(group));
            } else {
                folded += 1;
                folded_ms += ms;
            }
        }
        let folded = (folded > 0).then(|| Folded {
            rows: folded,
            total: num(self.run.per_unit(folded_ms)),
        });
        (rows, folded)
    }
}

fn finish(ctx: &Ctx, run: &Run, t: &Tally, rows: Vec<TreeRow>, folded: Option<Folded>) -> Reply {
    let f = &ctx.q.filters;
    let mut reply = Reply::new(
        unit_of(run, f.select, f.slow),
        rows.into_iter().map(Row::Tree).collect(),
    );
    reply.warnings = run.warnings(t.grand_n);
    if let Some(folded) = folded {
        reply.warnings.push(format!(
            "{} more rows ({} {}) are not shown: raise --limit or lower --min-pct",
            folded.rows, folded.total, f.select
        ));
    }
    reply.sampled = Some(run.sampled(t.grand_n));
    reply.window = run.window();
    reply
}

/// Top-down subtree of a call path, a function (merged over its occurrences) or the whole thread.
pub(super) fn tree(ctx: &Ctx) -> Result<Reply, QueryError> {
    let run = prepare(ctx, 0, false)?;
    let t = run.samples.tally(&run.pick);
    let begin = start(&run, ctx.q.args.first())?;
    let depth = ctx.q.depth.unwrap_or(TREE_DEPTH);
    let build = Builder::new(&run, &t, ctx, ctx.q.min_pct.unwrap_or(TREE_MIN_PCT));
    let s = &run.samples;
    let (rows, folded) = match begin.function {
        Some(at) if !begin.nodes.is_empty() => (
            vec![build.row(&begin.nodes, s.functions[at as usize].id.clone(), depth)],
            None,
        ),
        _ if begin.nodes.is_empty() => (Vec::new(), None),
        _ if ctx.q.args.is_empty() => {
            let mut by_function: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
            for &n in &begin.nodes {
                by_function.entry(s.func[n as usize]).or_default().push(n);
            }
            let mut merged: Vec<(f64, Vec<u32>)> = by_function
                .into_values()
                .map(|g| (build.sum_ms(&g, &t.total_ms), g))
                .collect();
            merged.sort_by(|a, b| b.0.total_cmp(&a.0));
            build.fold(merged, |g| build.row(&g, build.id_of(&g), depth))
        }
        _ => (
            vec![build.row(&begin.nodes, build.id_of(&begin.nodes), depth)],
            None,
        ),
    };
    Ok(finish(ctx, &run, &t, rows, folded))
}

/// Who calls a function, bottom-up and merged over its occurrences.
pub(super) fn callers(ctx: &Ctx) -> Result<Reply, QueryError> {
    let run = prepare(ctx, 0, false)?;
    let t = run.samples.tally(&run.pick);
    let id = ctx
        .q
        .args
        .first()
        .ok_or_else(|| QueryError::new("callers needs a function id (f:...)"))?;
    let begin = start(&run, Some(id))?;
    let depth = ctx.q.depth.unwrap_or(CALLERS_DEPTH);
    let build = Builder::new(&run, &t, ctx, ctx.q.min_pct.unwrap_or(0.0));
    let items: Vec<(u32, f64, u64)> = begin
        .nodes
        .iter()
        .filter(|&&n| t.total_n[n as usize] > 0)
        .map(|&n| (n, t.total_ms[n as usize], t.total_n[n as usize]))
        .collect();
    let (rows, folded) = callers_level(&build, items, depth);
    Ok(finish(ctx, &run, &t, rows, folded))
}

fn callers_level(
    build: &Builder,
    items: Vec<(u32, f64, u64)>,
    depth: usize,
) -> (Vec<TreeRow>, Option<Folded>) {
    let s = build.s();
    let mut by_function: BTreeMap<u32, Vec<(u32, f64, u64)>> = BTreeMap::new();
    for (node, ms, n) in items {
        let up = s.parent[node as usize];
        if up != NO_NODE {
            by_function
                .entry(s.func[up as usize])
                .or_default()
                .push((up, ms, n));
        }
    }
    let mut merged: Vec<(f64, Vec<(u32, f64, u64)>)> = by_function
        .into_values()
        .map(|g| (g.iter().map(|i| i.1).sum(), g))
        .collect();
    merged.sort_by(|a, b| {
        b.0.total_cmp(&a.0).then_with(|| {
            s.function_of(a.1[0].0)
                .full
                .cmp(&s.function_of(b.1[0].0).full)
        })
    });
    let (mut rows, mut folded, mut folded_ms) = (Vec::new(), 0usize, 0.0);
    for (ms, group) in merged {
        if rows.len() >= build.limit || build.pct(ms) < build.min_pct {
            folded += 1;
            folded_ms += ms;
            continue;
        }
        let f = s.function_of(group[0].0);
        let samples = group.iter().map(|i| i.2).sum();
        let (children, below) = if depth > 1 {
            callers_level(build, group, depth - 1)
        } else {
            (Vec::new(), None)
        };
        rows.push(TreeRow {
            id: f.id.clone(),
            function: f.id.clone(),
            name: f.name.clone(),
            crate_name: f.crate_name.clone(),
            total: num(build.run.per_unit(ms)),
            self_ms: None,
            pct: num(build.pct(ms)),
            self_pct: None,
            samples,
            children,
            folded: below,
        });
    }
    let folded = (folded > 0).then(|| Folded {
        rows: folded,
        total: num(build.run.per_unit(folded_ms)),
    });
    (rows, folded)
}

/// What a function calls, merged over its occurrences, with its own time last.
pub(super) fn callees(ctx: &Ctx) -> Result<Reply, QueryError> {
    let run = prepare(ctx, 0, false)?;
    let t = run.samples.tally(&run.pick);
    let id = ctx
        .q
        .args
        .first()
        .ok_or_else(|| QueryError::new("callees needs a function id (f:...)"))?;
    let begin = start(&run, Some(id))?;
    let depth = ctx.q.depth.unwrap_or(CALLEES_DEPTH).max(1);
    let mut build = Builder::new(&run, &t, ctx, ctx.q.min_pct.unwrap_or(0.0));
    build.function_ids = true;
    let (mut rows, folded) = if begin.nodes.is_empty() {
        (Vec::new(), None)
    } else {
        build.children_of(&begin.nodes, depth)
    };
    let own = build.sum_ms(&begin.nodes, &t.self_ms);
    if own > 0.0 {
        let f = run.samples.function_of(begin.nodes[0]);
        let samples = begin.nodes.iter().map(|&n| t.self_n[n as usize]).sum();
        rows.push(TreeRow {
            id: f.id.clone(),
            function: f.id.clone(),
            name: "(self)".to_string(),
            crate_name: f.crate_name.clone(),
            total: num(run.per_unit(own)),
            self_ms: Some(num(run.per_unit(own))),
            pct: num(build.pct(own)),
            self_pct: build.share.then(|| num(build.pct(own))),
            samples,
            children: Vec::new(),
            folded: None,
        });
    }
    Ok(finish(ctx, &run, &t, rows, folded))
}
