//! A read-only view of one capture: names resolved through the string tables, spans and ticks gathered per side.

use std::borrow::Cow;
use std::collections::HashMap;

use horizon_perf_format::v1::{Capture, CounterKind, Side, SlowEvent, SpanSummary, Unit};

/// A span of seconds on a side's clock; negative times are before the capture started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub from: f64,
    pub to: f64,
}

impl Window {
    pub const ALL: Window = Window {
        from: f64::NEG_INFINITY,
        to: f64::INFINITY,
    };

    pub fn contains(&self, t: f64) -> bool {
        t >= self.from && t <= self.to
    }
}

pub struct View<'a> {
    pub capture: &'a Capture,
    strings: HashMap<(Side, u32), &'a str>,
}

/// One span's totals, merged by name over every parent it ran under.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanRow {
    pub name: String,
    /// The parent under which most of its time was spent.
    pub parent: Option<String>,
    pub depth: u32,
    pub wall_ms: f64,
    pub cpu_ms: f64,
    pub seen: u64,
}

/// Span totals over `units` ticks or frames.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanTable {
    pub unit: Unit,
    pub units: u64,
    /// The units' own wall time, summed, when the capture recorded it.
    pub wall_total_ms: Option<f64>,
    /// Costliest first.
    pub rows: Vec<SpanRow>,
}

impl SpanTable {
    pub fn per_unit(&self, total: f64) -> f64 {
        total / self.units.max(1) as f64
    }

    pub fn row(&self, name: &str) -> Option<&SpanRow> {
        self.rows.iter().find(|r| r.name == name)
    }

    /// The mean wall time of a unit: recorded, else the roots' time.
    pub fn mean_unit_ms(&self) -> Option<f64> {
        let total = self.wall_total_ms.filter(|t| *t > 0.0).or_else(|| {
            let roots: f64 = self
                .rows
                .iter()
                .filter(|r| r.depth == 0)
                .map(|r| r.wall_ms)
                .sum();
            (roots > 0.0).then_some(roots)
        })?;
        Some(self.per_unit(total))
    }
}

#[derive(Default)]
struct Acc {
    /// Per parent: wall time and the units it ran in.
    parents: HashMap<Option<String>, (f64, u64)>,
    depth: u32,
    wall_ms: f64,
    cpu_ms: f64,
}

#[derive(Default)]
struct Merger {
    by_name: HashMap<String, Acc>,
}

impl Merger {
    /// Adds one span's totals under one parent; `seen` units add up per parent and the most of any parent is the span's.
    fn add(
        &mut self,
        name: &str,
        parent: Option<&str>,
        depth: u32,
        wall: f64,
        cpu: f64,
        seen: u64,
    ) {
        let is_new = !self.by_name.contains_key(name);
        let acc = self.by_name.entry(name.to_string()).or_default();
        acc.depth = if is_new { depth } else { acc.depth.min(depth) };
        acc.wall_ms += wall;
        acc.cpu_ms += cpu;
        let entry = acc.parents.entry(parent.map(str::to_string)).or_default();
        entry.0 += wall;
        entry.1 += seen;
    }

    fn rows(self) -> Vec<SpanRow> {
        let mut rows: Vec<SpanRow> = self
            .by_name
            .into_iter()
            .map(|(name, acc)| {
                let seen = acc.parents.values().map(|p| p.1).max().unwrap_or(0);
                let mut parents: Vec<_> = acc.parents.into_iter().collect();
                parents.sort_by(|a, b| b.1.0.total_cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
                SpanRow {
                    name,
                    parent: parents.into_iter().next().and_then(|(p, _)| p),
                    depth: acc.depth,
                    wall_ms: acc.wall_ms,
                    cpu_ms: acc.cpu_ms,
                    seen,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            b.wall_ms
                .total_cmp(&a.wall_ms)
                .then_with(|| a.name.cmp(&b.name))
        });
        rows
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TickSpan {
    pub name: String,
    pub depth: u32,
    pub wall_ms: f64,
    pub cpu_ms: f64,
}

/// One tick or frame kept with its spans.
#[derive(Clone, Debug, PartialEq)]
pub struct TickUnit {
    pub counter: u64,
    pub t: f64,
    pub wall_ms: f64,
    pub cpu_ms: f64,
    pub spans: Vec<TickSpan>,
    /// The whole span tree, not just the 8 largest spans.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CounterInfo {
    pub id: u32,
    pub name: String,
    pub labels: Vec<(String, String)>,
    pub kind: CounterKind,
    pub unit: String,
}

/// One memory snapshot: its label and every flattened path with its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub label: String,
    pub t: f64,
    pub values: Vec<(String, f64)>,
}

impl Snapshot {
    pub fn get(&self, path: &str) -> Option<f64> {
        self.values.iter().find(|(p, _)| p == path).map(|(_, v)| *v)
    }
}

/// A path holds megabytes when its last key, before any `[name]`, ends in `_mb`; anything else is a count.
pub fn is_megabytes(path: &str) -> bool {
    path.split('[')
        .next()
        .unwrap_or(path)
        .rsplit('.')
        .next()
        .is_some_and(|key| key.ends_with("_mb"))
}

impl<'a> View<'a> {
    pub fn new(capture: &'a Capture) -> Self {
        let strings = capture
            .strings
            .iter()
            .map(|s| ((s.side(), s.id), s.text.as_str()))
            .collect();
        Self { capture, strings }
    }

    /// The text of a string id of `side`; 0 is no string.
    pub fn text(&self, side: Side, id: u32) -> Cow<'a, str> {
        match self.strings.get(&(side, id)) {
            Some(text) => Cow::Borrowed(text),
            None if id == 0 => Cow::Borrowed(""),
            None => Cow::Owned(format!("#{id}")),
        }
    }

    fn opt_text(&self, side: Side, id: u32) -> Option<String> {
        (id != 0).then(|| self.text(side, id).into_owned())
    }

    /// The sides present, server first.
    pub fn sides(&self) -> Vec<Side> {
        let c = self.capture;
        [Side::Server, Side::Client]
            .into_iter()
            .filter(|&side| {
                c.start.iter().any(|m| m.side() == side)
                    || c.stop.iter().any(|m| m.side() == side)
                    || c.health.iter().any(|m| m.side() == side)
                    || c.spans.iter().any(|m| m.side() == side)
                    || c.span_blocks.iter().any(|m| m.side() == side)
            })
            .collect()
    }

    /// The server when the capture has one, else the client.
    pub fn default_side(&self) -> Option<Side> {
        self.sides().into_iter().next()
    }

    /// Seconds the side ran: its stop, else its last sample.
    pub fn end(&self, side: Side) -> f64 {
        self.capture
            .stop
            .iter()
            .find(|s| s.side() == side)
            .map(|s| s.t)
            .or_else(|| {
                self.capture
                    .health
                    .iter()
                    .filter(|h| h.side() == side)
                    .map(|h| h.t)
                    .reduce(f64::max)
            })
            .unwrap_or(0.0)
    }

    pub fn main_unit(side: Side) -> Unit {
        if side == Side::Client {
            Unit::Frame
        } else {
            Unit::Tick
        }
    }

    pub fn main_thread(side: Side) -> &'static str {
        if side == Side::Client {
            "render"
        } else {
            "tick"
        }
    }

    /// The threads that have spans on `side`, as (string id, name).
    pub fn threads(&self, side: Side) -> Vec<(u32, String)> {
        let mut ids: Vec<u32> = self
            .capture
            .spans
            .iter()
            .filter(|s| s.side() == side)
            .map(|s| s.thread)
            .chain(
                self.capture
                    .span_blocks
                    .iter()
                    .filter(|b| b.side() == side)
                    .map(|b| b.thread),
            )
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .map(|id| (id, self.text(side, id).into_owned()))
            .collect()
    }

    /// The requested thread, else the side's main thread (or its only thread).
    pub fn resolve_thread(&self, side: Side, requested: Option<&str>) -> Result<u32, String> {
        let threads = self.threads(side);
        let list = || {
            threads
                .iter()
                .map(|t| t.1.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let wanted = requested.unwrap_or(Self::main_thread(side));
        if let Some((id, _)) = threads.iter().find(|(_, name)| name == wanted) {
            return Ok(*id);
        }
        match (requested, threads.as_slice()) {
            (None, [(id, _)]) => Ok(*id),
            (_, []) => Err(format!(
                "this capture has no spans for its {} side",
                crate::error::side_name(side)
            )),
            _ => Err(format!(
                "no thread named {wanted}; threads with spans: {}",
                list()
            )),
        }
    }

    pub fn summary(&self, side: Side, thread: u32) -> Option<&'a SpanSummary> {
        self.capture
            .spans
            .iter()
            .find(|s| s.side() == side && s.thread == thread)
    }

    /// Span totals of one thread: the end-of-capture summary, or the blocks that start inside `window`.
    pub fn span_table(&self, side: Side, thread: u32, window: Option<Window>) -> Option<SpanTable> {
        match window {
            None => self
                .table_from_summary(side, thread)
                .or_else(|| self.table_from_blocks(side, thread, Window::ALL)),
            Some(w) => self.table_from_blocks(side, thread, w),
        }
    }

    fn table_from_summary(&self, side: Side, thread: u32) -> Option<SpanTable> {
        let summary = self.summary(side, thread)?;
        let mut merger = Merger::default();
        for s in &summary.spans {
            let parent = self.opt_text(side, s.parent);
            merger.add(
                &self.text(side, s.name),
                parent.as_deref(),
                s.depth,
                s.wall_ms,
                s.cpu_ms,
                s.seen,
            );
        }
        Some(SpanTable {
            unit: summary.unit(),
            units: summary.count,
            wall_total_ms: (summary.wall_ms_total > 0.0).then_some(summary.wall_ms_total),
            rows: merger.rows(),
        })
    }

    fn table_from_blocks(&self, side: Side, thread: u32, window: Window) -> Option<SpanTable> {
        let mut merger = Merger::default();
        let mut units = 0u64;
        let mut any = false;
        for block in self.capture.span_blocks.iter().filter(|b| {
            b.side() == side && b.thread == thread && b.t0 >= window.from && b.t0 < window.to
        }) {
            any = true;
            units += u64::from(block.units);
            for (i, &name) in block.span.iter().enumerate() {
                let parent = block.parent.get(i).and_then(|&p| self.opt_text(side, p));
                let depth = Self::block_depth(block, i);
                merger.add(
                    &self.text(side, name),
                    parent.as_deref(),
                    depth,
                    f64::from(block.wall_ms.get(i).copied().unwrap_or(0.0)),
                    f64::from(block.cpu_ms.get(i).copied().unwrap_or(0.0)),
                    u64::from(block.seen.get(i).copied().unwrap_or(0)),
                );
            }
        }
        let unit = self
            .summary(side, thread)
            .map_or(Self::main_unit(side), |s| s.unit());
        any.then(|| SpanTable {
            unit,
            units,
            wall_total_ms: None,
            rows: merger.rows(),
        })
    }

    fn block_depth(block: &horizon_perf_format::v1::SpanBlock, index: usize) -> u32 {
        let mut depth = 0;
        let mut parent = block.parent.get(index).copied().unwrap_or(0);
        while parent != 0 && depth < 64 {
            depth += 1;
            parent = block
                .span
                .iter()
                .position(|&s| s == parent)
                .and_then(|at| block.parent.get(at).copied())
                .unwrap_or(0);
        }
        depth
    }

    /// Per block of one thread: its start, its units and the named span's wall time summed over them.
    pub fn span_points(&self, side: Side, thread: u32, name: &str) -> Vec<(f64, u32, f64)> {
        self.capture
            .span_blocks
            .iter()
            .filter(|b| b.side() == side && b.thread == thread)
            .map(|b| {
                let wall: f64 = b
                    .span
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| self.text(side, **s) == name)
                    .map(|(i, _)| f64::from(b.wall_ms.get(i).copied().unwrap_or(0.0)))
                    .sum();
                (b.t0, b.units, wall)
            })
            .collect()
    }

    /// The slow ticks or frames of `side` inside `window`: the ring from before the capture, then the live ones.
    pub fn slow_events(&self, side: Side, unit: Unit, window: Window) -> Vec<&'a SlowEvent> {
        self.capture
            .slow_history
            .iter()
            .chain(&self.capture.slow)
            .filter(|e| e.side() == side && e.unit() == unit && window.contains(e.t))
            .collect()
    }

    /// Per-span averages over the slow units: each span's time summed and divided by the number of slow units.
    pub fn slow_table(&self, side: Side, unit: Unit, window: Window) -> SpanTable {
        let events = self.slow_events(side, unit, window);
        let mut merger = Merger::default();
        for event in &events {
            let mut once: HashMap<String, (u32, f64, f64)> = HashMap::new();
            for cost in &event.top {
                let e = once
                    .entry(self.text(side, cost.name).into_owned())
                    .or_default();
                e.0 = e.0.min(cost.depth);
                e.1 += f64::from(cost.wall_ms);
                e.2 += f64::from(cost.cpu_ms);
            }
            for (name, (depth, wall, cpu)) in once {
                merger.add(&name, None, depth, wall, cpu, 1);
            }
        }
        SpanTable {
            unit,
            units: events.len() as u64,
            wall_total_ms: Some(events.iter().map(|e| f64::from(e.wall_ms)).sum()),
            rows: merger.rows(),
        }
    }

    /// Every kept tick or frame: the worst of each summary with whole trees, then the slow events.
    pub fn tick_units(&self, side: Side, unit: Unit) -> Vec<TickUnit> {
        let mut seen: HashMap<u64, ()> = HashMap::new();
        let mut units = Vec::new();
        for summary in self
            .capture
            .spans
            .iter()
            .filter(|s| s.side() == side && s.unit() == unit)
        {
            for w in &summary.worst {
                if seen.insert(w.counter, ()).is_none() {
                    units.push(TickUnit {
                        counter: w.counter,
                        t: w.t,
                        wall_ms: f64::from(w.wall_ms),
                        cpu_ms: f64::from(w.cpu_ms),
                        spans: self.tick_spans(side, &w.spans),
                        complete: true,
                    });
                }
            }
        }
        for e in self.capture.slow.iter().chain(&self.capture.slow_history) {
            if e.side() == side && e.unit() == unit && seen.insert(e.counter, ()).is_none() {
                units.push(TickUnit {
                    counter: e.counter,
                    t: e.t,
                    wall_ms: f64::from(e.wall_ms),
                    cpu_ms: f64::from(e.cpu_ms),
                    spans: self.tick_spans(side, &e.top),
                    complete: false,
                });
            }
        }
        units
    }

    fn tick_spans(&self, side: Side, costs: &[horizon_perf_format::v1::SpanCost]) -> Vec<TickSpan> {
        costs
            .iter()
            .map(|c| TickSpan {
                name: self.text(side, c.name).into_owned(),
                depth: c.depth,
                wall_ms: f64::from(c.wall_ms),
                cpu_ms: f64::from(c.cpu_ms),
            })
            .collect()
    }

    pub fn counters(&self, side: Side) -> Vec<CounterInfo> {
        self.capture
            .counter_defs
            .iter()
            .filter(|d| d.side() == side)
            .map(|d| CounterInfo {
                id: d.id,
                name: self.text(side, d.name).into_owned(),
                labels: d
                    .labels
                    .iter()
                    .map(|l| {
                        (
                            self.text(side, l.key).into_owned(),
                            self.text(side, l.value).into_owned(),
                        )
                    })
                    .collect(),
                kind: d.kind(),
                unit: self.text(side, d.unit).into_owned(),
            })
            .collect()
    }

    /// The memory snapshots of `side` in time order.
    pub fn snapshots(&self, side: Side) -> Vec<Snapshot> {
        let mut out: Vec<Snapshot> = self
            .capture
            .memory
            .iter()
            .filter(|m| m.side() == side)
            .map(|m| Snapshot {
                label: self.text(side, m.label).into_owned(),
                t: m.t,
                values: m
                    .path
                    .iter()
                    .zip(&m.value_mb)
                    .map(|(&p, &v)| (self.text(side, p).into_owned(), v))
                    .collect(),
            })
            .collect();
        out.sort_by(|a, b| a.t.total_cmp(&b.t));
        out
    }
}
