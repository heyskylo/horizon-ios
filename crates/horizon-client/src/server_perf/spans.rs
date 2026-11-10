//! The tick thread's spans as the capture's `span_blocks` (live) and `spans` (at the end).
use crate::fast_hash::FxHashMap;
use crate::spans::Span;
use horizon_perf_format::Interner;
use horizon_perf_format::v1::{
    Histogram, Side, SpanBlock, SpanCost, SpanSummary, SpanTotal, Unit, WorstUnit,
};

/// Seconds one `SpanBlock` covers.
pub const BLOCK_S: f32 = 5.0;
/// Ticks kept with their whole span tree.
pub const WORST_KEPT: usize = 16;
/// Upper edges of the tick-time histogram, in ms.
pub const HISTOGRAM_EDGES_MS: [f32; 14] = [
    5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 75.0, 100.0, 150.0, 250.0, 500.0, 1000.0,
];

type Key = (&'static str, &'static str);

/// Wall and CPU milliseconds of one span under one parent, and the units it ran in.
#[derive(Clone, Copy, Default)]
struct Sum {
    wall_ms: f64,
    cpu_ms: f64,
    seen: u64,
    depth: u32,
}

struct Worst {
    t: f64,
    counter: u64,
    wall_ms: f32,
    cpu_ms: f32,
    spans: Vec<Span>,
}

/// The index in `spans` of each span's enclosing span; spans are recorded after the spans inside them, one level deeper.
fn parents(spans: &[Span]) -> Vec<Option<usize>> {
    let mut parent = vec![None; spans.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        while let Some(&j) = open.last()
            && spans[j].depth > span.depth
        {
            open.pop();
            parent[j] = Some(i);
        }
        open.push(i);
    }
    parent
}

/// One tick's spans merged by (name, parent name), with the tree depth of the first of each.
fn merge_by_parent(spans: &[Span]) -> Vec<(Key, Sum)> {
    let parent = parents(spans);
    let mut depth = vec![0u32; spans.len()];
    for i in (0..spans.len()).rev() {
        depth[i] = parent[i].map_or(0, |p| depth[p] + 1);
    }
    let mut merged: Vec<(Key, Sum)> = Vec::new();
    let mut index: FxHashMap<Key, usize> = FxHashMap::default();
    for (i, span) in spans.iter().enumerate() {
        let key = (span.name, parent[i].map_or("", |p| spans[p].name));
        let at = *index.entry(key).or_insert_with(|| {
            merged.push((
                key,
                Sum {
                    depth: depth[i],
                    ..Sum::default()
                },
            ));
            merged.len() - 1
        });
        merged[at].1.wall_ms += f64::from(span.ms);
        merged[at].1.cpu_ms += f64::from(span.cpu_ms);
    }
    merged
}

/// A tick's whole span tree, parents before children, siblings costliest first; same-named spans under one parent are one node.
fn tree(spans: &[Span], strings: &mut Interner) -> Vec<SpanCost> {
    let parent = parents(spans);
    struct Node {
        name: &'static str,
        wall_ms: f32,
        cpu_ms: f32,
        children: Vec<usize>,
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut node_of: Vec<usize> = vec![usize::MAX; spans.len()];
    let mut by_parent: FxHashMap<(Option<usize>, &'static str), usize> = FxHashMap::default();
    for i in (0..spans.len()).rev() {
        let up = parent[i].map(|p| node_of[p]);
        let at = *by_parent.entry((up, spans[i].name)).or_insert_with(|| {
            nodes.push(Node {
                name: spans[i].name,
                wall_ms: 0.0,
                cpu_ms: 0.0,
                children: Vec::new(),
            });
            let id = nodes.len() - 1;
            match up {
                Some(p) => nodes[p].children.push(id),
                None => roots.push(id),
            }
            id
        });
        nodes[at].wall_ms += spans[i].ms;
        nodes[at].cpu_ms += spans[i].cpu_ms;
        node_of[i] = at;
    }
    let mut out = Vec::with_capacity(nodes.len());
    let mut stack: Vec<(usize, u32)> = Vec::new();
    let costliest_last = |ids: &mut Vec<usize>, nodes: &[Node]| {
        ids.sort_by(|&a, &b| {
            nodes[a]
                .wall_ms
                .total_cmp(&nodes[b].wall_ms)
                .then_with(|| nodes[b].name.cmp(nodes[a].name))
        });
    };
    costliest_last(&mut roots, &nodes);
    stack.extend(roots.iter().map(|&id| (id, 0)));
    while let Some((id, depth)) = stack.pop() {
        let node = &nodes[id];
        out.push(SpanCost {
            name: strings.intern(node.name),
            wall_ms: node.wall_ms,
            cpu_ms: node.cpu_ms,
            depth,
        });
        let mut children = node.children.clone();
        costliest_last(&mut children, &nodes);
        stack.extend(children.iter().map(|&c| (c, depth + 1)));
    }
    out
}

/// One server tick as the recorder takes it.
pub struct Tick<'a> {
    /// Seconds into the capture when the tick ended.
    pub t: f64,
    /// The server's tick counter.
    pub counter: u64,
    pub wall_ms: f32,
    /// The thread's CPU time; None where the platform does not tell it.
    pub cpu_ms: Option<f32>,
    /// As recorded: each span after the spans inside it.
    pub spans: &'a [Span],
}

/// Sums the server thread's ticks into blocks of [`BLOCK_S`] seconds and, at the end, into the capture's totals.
pub struct SpanRecorder {
    thread: &'static str,
    side: Side,
    unit: Unit,
    block: u32,
    units: u32,
    rows: FxHashMap<Key, Sum>,
    totals: FxHashMap<Key, Sum>,
    count: u64,
    wall_total: f64,
    cpu_total: f64,
    timing: horizon_perf_format::TimingRecorder,
    histogram: [u64; HISTOGRAM_EDGES_MS.len() + 1],
    worst: Vec<Worst>,
    recorded: u64,
}

impl Default for SpanRecorder {
    fn default() -> Self {
        Self::new("", Side::Server, Unit::Tick)
    }
}

impl SpanRecorder {
    pub fn new(thread: &'static str, side: Side, unit: Unit) -> Self {
        Self {
            thread,
            side,
            unit,
            block: 0,
            units: 0,
            rows: FxHashMap::default(),
            totals: FxHashMap::default(),
            count: 0,
            wall_total: 0.0,
            cpu_total: 0.0,
            timing: horizon_perf_format::TimingRecorder::default(),
            histogram: [0; HISTOGRAM_EDGES_MS.len() + 1],
            worst: Vec::new(),
            recorded: 0,
        }
    }

    /// Ticks recorded so far.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Spans the ticks recorded themselves, before they were merged: what timing them cost.
    pub fn spans_recorded(&self) -> u64 {
        self.recorded
    }

    /// The mean tick time in ms.
    pub fn mean_wall_ms(&self) -> f64 {
        self.wall_total / self.count.max(1) as f64
    }

    /// Adds one tick; returns the block it closed, if it began a new one.
    pub fn unit(&mut self, tick: Tick, strings: &mut Interner) -> Option<SpanBlock> {
        let Tick {
            t,
            counter,
            wall_ms,
            cpu_ms,
            spans,
        } = tick;
        let block = (t.max(0.0) / f64::from(BLOCK_S)) as u32;
        let closed = if block != self.block {
            let closed = self.close_block(strings);
            self.block = block;
            closed
        } else {
            None
        };
        let cpu_ms = cpu_ms.unwrap_or(wall_ms);
        self.units += 1;
        self.count += 1;
        self.timing.record(wall_ms);
        self.recorded += spans.len() as u64;
        self.wall_total += f64::from(wall_ms);
        self.cpu_total += f64::from(cpu_ms);
        self.histogram[HISTOGRAM_EDGES_MS.partition_point(|&edge| edge <= wall_ms)] += 1;
        for (key, sum) in merge_by_parent(spans) {
            for rows in [&mut self.rows, &mut self.totals] {
                let row = rows.entry(key).or_insert(Sum {
                    depth: sum.depth,
                    ..Sum::default()
                });
                row.wall_ms += sum.wall_ms;
                row.cpu_ms += sum.cpu_ms;
                row.seen += 1;
            }
        }
        if self.worst.len() < WORST_KEPT || self.worst.iter().any(|w| w.wall_ms < wall_ms) {
            if self.worst.len() >= WORST_KEPT {
                let cheapest = (0..self.worst.len())
                    .min_by(|&a, &b| self.worst[a].wall_ms.total_cmp(&self.worst[b].wall_ms))
                    .unwrap_or(0);
                self.worst.swap_remove(cheapest);
            }
            self.worst.push(Worst {
                t,
                counter,
                wall_ms,
                cpu_ms,
                spans: spans.to_vec(),
            });
        }
        closed
    }

    fn close_block(&mut self, strings: &mut Interner) -> Option<SpanBlock> {
        let units = std::mem::take(&mut self.units);
        let mut rows: Vec<_> = std::mem::take(&mut self.rows).into_iter().collect();
        if units == 0 {
            return None;
        }
        rows.sort_by(|a, b| b.1.wall_ms.total_cmp(&a.1.wall_ms).then(a.0.cmp(&b.0)));
        let mut block = SpanBlock {
            side: self.side as i32,
            thread: strings.intern(self.thread),
            block: self.block,
            t0: f64::from(self.block) * f64::from(BLOCK_S),
            block_s: BLOCK_S,
            units,
            ..SpanBlock::default()
        };
        for ((name, parent), sum) in rows {
            block.span.push(strings.intern(name));
            block.parent.push(if parent.is_empty() {
                0
            } else {
                strings.intern(parent)
            });
            block.wall_ms.push(sum.wall_ms as f32);
            block.cpu_ms.push(sum.cpu_ms as f32);
            block.seen.push(sum.seen as u32);
        }
        Some(block)
    }

    /// The block still open and the capture's totals.
    pub fn finish(mut self, strings: &mut Interner) -> (Option<SpanBlock>, SpanSummary) {
        let last = self.close_block(strings);
        let mut totals: Vec<_> = std::mem::take(&mut self.totals).into_iter().collect();
        totals.sort_by(|a, b| b.1.wall_ms.total_cmp(&a.1.wall_ms).then(a.0.cmp(&b.0)));
        let mut worst = std::mem::take(&mut self.worst);
        worst.sort_by(|a, b| {
            b.wall_ms
                .total_cmp(&a.wall_ms)
                .then(a.counter.cmp(&b.counter))
        });
        let thread = strings.intern(self.thread);
        let summary = SpanSummary {
            side: self.side as i32,
            thread,
            unit: self.unit as i32,
            count: self.count,
            spans: totals
                .into_iter()
                .map(|((name, parent), sum)| SpanTotal {
                    name: strings.intern(name),
                    parent: if parent.is_empty() {
                        0
                    } else {
                        strings.intern(parent)
                    },
                    depth: sum.depth,
                    wall_ms: sum.wall_ms,
                    cpu_ms: sum.cpu_ms,
                    seen: sum.seen,
                })
                .collect(),
            histogram: Some(Histogram {
                edges_ms: HISTOGRAM_EDGES_MS.to_vec(),
                counts: self.histogram.to_vec(),
            }),
            worst: worst
                .into_iter()
                .map(|w| WorstUnit {
                    t: w.t,
                    counter: w.counter,
                    wall_ms: w.wall_ms,
                    cpu_ms: w.cpu_ms,
                    spans: tree(&w.spans, strings),
                })
                .collect(),
            wall_ms_total: self.wall_total,
            cpu_ms_total: self.cpu_total,
            timing: self.timing.finish(),
        };
        (last, summary)
    }
}
