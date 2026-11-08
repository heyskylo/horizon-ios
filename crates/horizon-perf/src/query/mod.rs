//! `horizon-perf query OP` (docs/perf-platform.md §6): every reply is one JSON envelope with `rows`.

mod bench;
mod calls;
mod gpu;
mod meta;
mod notes;
mod profile;
pub mod rows;
mod series;
mod spans;
mod ticks;
pub(crate) mod traffic;
mod world;

use std::collections::BTreeMap;

use horizon_perf_format::v1::{Capture, Side};

use crate::error::side_name;
use crate::view::{View, Window};
use crate::wire::{
    Candidate, CaptureInfo, ContextKey, Envelope, GroupBy, HotBy, KindName, Op, OpName, SampleBy,
    Select, SideName, Unit, bound,
};

pub use crate::wire::{fine, num, opt_num};
pub use notes::warning_text;

/// One side's GPU pass rows, Horizon's own passes first, for the summary.
pub(crate) fn gpu_rows(view: &View, side: Side) -> Vec<rows::GpuPassRow> {
    gpu::rows_of(view, &[side])
}
pub use rows::Row;

/// A loaded capture and the name it is listed under.
pub struct Loaded {
    pub file: String,
    /// Shared, so a caller that keeps a decoded capture (the wasm boundary) hands it out again
    /// without copying it per query.
    pub capture: std::sync::Arc<Capture>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Bound {
    Seconds(f64),
    Mark(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Filters {
    pub side: Option<Side>,
    pub thread: Option<String>,
    pub select: Select,
    pub slow: bool,
    /// The values given for each key, in the order given.
    pub where_: BTreeMap<ContextKey, Vec<String>>,
    pub from: Option<Bound>,
    pub to: Option<Bound>,
    pub limit: usize,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            side: None,
            thread: None,
            select: Select::Cpu,
            slow: false,
            where_: BTreeMap::new(),
            from: None,
            to: None,
            limit: DEFAULT_LIMIT,
        }
    }
}

pub const DEFAULT_LIMIT: usize = 25;

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub op: Op,
    /// What follows the op and is not a file: ids, names, field names, labels.
    pub args: Vec<String>,
    pub filters: Filters,
    pub names: Vec<String>,
    pub worst: Option<usize>,
    pub over: Option<f64>,
    pub every: Option<f64>,
    pub diff: bool,
    pub min_mb: Option<f64>,
    pub by: Option<GroupBy>,
    pub hot_by: Option<HotBy>,
    pub by_sample: Option<SampleBy>,
    /// `hot --match`: functions whose name contains it.
    pub matching: Option<String>,
    pub depth: Option<usize>,
    pub min_pct: Option<f64>,
}

impl Query {
    pub fn new(op: Op) -> Self {
        Self {
            op,
            args: Vec::new(),
            filters: Filters::default(),
            names: Vec::new(),
            worst: None,
            over: None,
            every: None,
            diff: false,
            min_mb: None,
            by: None,
            hot_by: None,
            by_sample: None,
            matching: None,
            depth: None,
            min_pct: None,
        }
    }
}

/// A failed query; `candidates` are (id, name) pairs when a name was ambiguous or unknown.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryError {
    pub message: String,
    pub candidates: Vec<Candidate>,
}

impl QueryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    pub fn with(message: impl Into<String>, candidates: Vec<Candidate>) -> Self {
        Self {
            message: message.into(),
            candidates,
        }
    }

    /// The reply for a query that failed, naming `op` as the caller typed it.
    pub fn envelope(&self, op: OpName) -> Envelope<Row> {
        Envelope::failure(op, &self.message, self.candidates.clone())
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a sample query looked at, for the envelope's selection.
pub(crate) struct Sampled {
    pub thread: String,
    pub ticks: u64,
    pub samples: u64,
}

/// What one operation answers; the envelope around it is common.
pub(crate) struct Reply {
    pub unit: Unit,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
    pub window: Option<Window>,
    pub sampled: Option<Sampled>,
}

impl Reply {
    pub fn new(unit: Unit, rows: Vec<Row>) -> Self {
        Self {
            unit,
            rows,
            warnings: Vec::new(),
            window: None,
            sampled: None,
        }
    }
}

pub(crate) struct Ctx<'a> {
    pub q: &'a Query,
    pub files: &'a [Loaded],
    pub views: Vec<View<'a>>,
}

impl Ctx<'_> {
    /// The side asked for, else the server when the capture has one, else the client.
    pub fn side(&self, i: usize) -> Result<Side, QueryError> {
        let view = &self.views[i];
        match self.q.filters.side {
            Some(side) if view.sides().contains(&side) => Ok(side),
            Some(side) => Err(QueryError::new(format!(
                "{} has no {} side",
                self.files[i].file,
                side_name(side)
            ))),
            None => view
                .default_side()
                .ok_or_else(|| QueryError::new(format!("{} has no data", self.files[i].file))),
        }
    }

    /// The sides asked for: the one given, else every side present, server first.
    pub fn sides(&self, i: usize) -> Result<Vec<Side>, QueryError> {
        match self.q.filters.side {
            Some(_) => Ok(vec![self.side(i)?]),
            None => Ok(self.views[i].sides()),
        }
    }

    pub fn thread(&self, i: usize, side: Side) -> Result<u32, QueryError> {
        self.views[i]
            .resolve_thread(side, self.q.filters.thread.as_deref())
            .map_err(QueryError::new)
    }

    /// `--from`/`--to` as seconds on the side's clock; negatives count back from its end, `mark:NAME` is a mark's time.
    pub fn window(&self, i: usize, side: Side, default_from: f64) -> Result<Window, QueryError> {
        let end = self.views[i].end(side);
        let f = &self.q.filters;
        let at = |bound: &Option<Bound>, default: f64| -> Result<f64, QueryError> {
            match bound {
                None => Ok(default),
                Some(Bound::Seconds(s)) if *s < 0.0 => Ok(end + s),
                Some(Bound::Seconds(s)) => Ok(*s),
                Some(Bound::Mark(name)) => self.mark_time(i, side, name),
            }
        };
        Ok(Window {
            from: at(&f.from, default_from)?,
            to: at(&f.to, end)?,
        })
    }

    fn mark_time(&self, i: usize, side: Side, name: &str) -> Result<f64, QueryError> {
        let marks = &self.files[i].capture.marks;
        marks
            .iter()
            .find(|m| m.name == name && m.side() == side)
            .or_else(|| marks.iter().find(|m| m.name == name))
            .map(|m| m.t)
            .ok_or_else(|| {
                let names: Vec<&str> = marks.iter().map(|m| m.name.as_str()).collect();
                QueryError::new(format!("no mark named {name}; marks: {}", names.join(", ")))
            })
    }

    pub fn has_window(&self) -> bool {
        self.q.filters.from.is_some() || self.q.filters.to.is_some()
    }
}

/// Runs one query over one or more loaded captures; only `spans` takes several.
pub fn run(q: &Query, files: &[Loaded]) -> Result<Envelope<Row>, QueryError> {
    if q.op.unanswered() {
        return Err(QueryError::new(format!(
            "query {} is not answered by this horizon-perf yet",
            q.op
        )));
    }
    if files.is_empty() {
        return Err(QueryError::new("no capture given"));
    }
    if files.len() > 1 && !matches!(q.op, Op::Spans | Op::Frames) {
        return Err(QueryError::new(format!(
            "query {} takes one capture, got {}",
            q.op,
            files.len()
        )));
    }
    let ctx = Ctx {
        q,
        files,
        views: files.iter().map(|f| View::new(&f.capture)).collect(),
    };
    let reply = match q.op {
        Op::Spans => spans::spans(&ctx)?,
        Op::Ticks => ticks::ticks(&ctx)?,
        Op::Tick => ticks::tick(&ctx)?,
        Op::Series => series::series(&ctx)?,
        Op::World => world::world(&ctx)?,
        Op::Net => traffic::net(&ctx)?,
        Op::Gpu => gpu::gpu(&ctx)?,
        Op::Meta => meta::meta(&ctx)?,
        Op::Memory => notes::memory(&ctx)?,
        Op::Marks => notes::marks(&ctx)?,
        Op::Warnings => notes::warnings(&ctx)?,
        Op::Hot => profile::hot(&ctx)?,
        Op::Tree => calls::tree(&ctx)?,
        Op::Callers => calls::callers(&ctx)?,
        Op::Callees => calls::callees(&ctx)?,
        Op::Split => profile::divide(&ctx, false)?,
        Op::Group => profile::divide(&ctx, true)?,
        Op::Frames => profile::frames(&ctx)?,
        Op::Bench => bench::bench(&ctx)?,
        Op::Samples => unreachable!("answered above"),
    };
    Ok(envelope(&ctx, reply))
}

fn envelope(ctx: &Ctx, reply: Reply) -> Envelope<Row> {
    let captures: Vec<CaptureInfo> = ctx
        .files
        .iter()
        .zip(&ctx.views)
        .map(|(file, view)| {
            let sides = view.sides();
            let kind = view
                .capture
                .start
                .iter()
                .find(|s| Some(s.side()) == sides.first().copied())
                .map_or(KindName::Unspecified, |s| KindName::from(s.kind()));
            CaptureInfo {
                file: file.file.clone(),
                sides: sides.iter().map(|&s| SideName::from(s)).collect(),
                kind,
                seconds: num(sides.first().map_or(0.0, |&s| view.end(s))),
            }
        })
        .collect();
    let f = &ctx.q.filters;
    let side = ctx.side(0).ok();
    let (thread, ticks, samples) = match &reply.sampled {
        Some(s) => (Some(s.thread.clone()), Some(s.ticks), Some(s.samples)),
        None => {
            let thread = side.and_then(|s| ctx.thread(0, s).ok().map(|id| (s, id)));
            let ticks = thread
                .and_then(|(s, id)| ctx.views[0].span_table(s, id, None))
                .map(|t| t.units);
            (
                thread.map(|(s, id)| ctx.views[0].text(s, id).into_owned()),
                ticks,
                None,
            )
        }
    };
    let end = side.map(|s| ctx.views[0].end(s)).unwrap_or(0.0);
    let window = reply.window.unwrap_or(Window { from: 0.0, to: end });
    let selection = crate::wire::Selection {
        side: side.map(SideName::from),
        thread,
        select: f.select,
        slow: f.slow,
        where_: f
            .where_
            .iter()
            .map(|(key, values)| (*key, values.as_slice().into()))
            .collect(),
        from: bound(window.from),
        to: bound(window.to),
        ticks,
        samples,
    };
    Envelope::success(
        OpName::Query(ctx.q.op),
        captures,
        Some(selection),
        reply.unit,
        reply.rows,
        reply.warnings,
    )
}
