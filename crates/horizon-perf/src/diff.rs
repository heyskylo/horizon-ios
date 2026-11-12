//! `horizon-perf diff A B` (docs/perf-platform.md §6.6): two captures cut into time blocks and compared as unpaired samples.

use std::collections::{BTreeMap, HashMap};

use horizon_perf_format::v1::{Capture, Side, Unit as TickUnit};
use serde::{Deserialize, Serialize};

use crate::diff_calls::{CallSeries, call_series};
use crate::error::{Error, side_name};
use crate::health::{HealthField, live, mean};
use crate::stats::{self, MAX_BLOCKS, Verdict, compare, median};
use crate::view::View;
use crate::wire::{DiffOp, Envelope, OpName, Role, SideName, Unit, num, round1, round4};

/// Seconds of a block when the capture holds no span blocks to say.
const DEFAULT_BLOCK_S: f64 = 5.0;

pub const DEFAULT_LIMIT: usize = 25;

type Points = Vec<(f64, f64)>;

const SERVER_HEALTH: [HealthField; 3] = [
    HealthField::MsptAvg,
    HealthField::MsptP50,
    HealthField::MsptP95,
];
const CLIENT_HEALTH: [HealthField; 4] = [
    HealthField::FrameMsP50,
    HealthField::FrameMsP95,
    HealthField::FrameCpuMsP50,
    HealthField::FrameGpuMsP50,
];

/// A compared file, by the role it plays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffCapture {
    pub role: Role,
    pub file: String,
}

/// What a capture diff compared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffSelection {
    pub side: SideName,
    pub thread: Option<String>,
    pub block_s: f64,
    pub blocks: [usize; 2],
}

/// A diff row as it is written to JSON, B against A.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffRow {
    pub id: String,
    /// A function's short name, on `f:` rows of `diff --calls`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub a: Option<f64>,
    pub b: Option<f64>,
    pub diff: Option<f64>,
    pub diff_pct: Option<f64>,
    pub ci95: Option<[f64; 2]>,
    pub confidence: Option<f64>,
    pub blocks: [usize; 2],
    pub verdict: Verdict,
}

/// The reply of `diff --json` on two captures.
pub type DiffEnvelope = Envelope<DiffRow, DiffCapture, DiffSelection>;

pub(crate) fn captures(a: &str, b: &str) -> Vec<DiffCapture> {
    vec![
        DiffCapture {
            role: Role::A,
            file: a.to_string(),
        },
        DiffCapture {
            role: Role::B,
            file: b.to_string(),
        },
    ]
}

#[derive(Clone, Debug)]
pub struct DiffOptions {
    pub side: Option<Side>,
    pub thread: Option<String>,
    /// Rows whose value is under this in both captures are left out.
    pub min_ms: f64,
    pub limit: usize,
    /// Also compare the sampled functions (`f:`) and call paths (`p:`) by their time per tick or frame.
    pub calls: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            side: None,
            thread: None,
            min_ms: 0.0,
            limit: DEFAULT_LIMIT,
            calls: false,
        }
    }
}

/// One metric's block values on each side: B against A.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: Option<String>,
    pub a: Option<f64>,
    pub b: Option<f64>,
    pub effect: Option<f64>,
    pub ci: Option<[f64; 2]>,
    pub confidence: Option<f64>,
    pub blocks: [usize; 2],
    pub verdict: Verdict,
}

impl Row {
    pub fn pct(&self) -> Option<f64> {
        let (a, effect) = (self.a?, self.effect?);
        (a != 0.0).then(|| 100.0 * effect / a)
    }

    pub(crate) fn wire(&self) -> DiffRow {
        DiffRow {
            id: self.id.clone(),
            name: self.name.clone(),
            a: self.a.map(num),
            b: self.b.map(num),
            diff: self.effect.map(num),
            diff_pct: self.pct().map(round1),
            ci95: self.ci.map(|[lo, hi]| [num(lo), num(hi)]),
            confidence: self.confidence.map(round4),
            blocks: self.blocks,
            verdict: self.verdict,
        }
    }

    pub(crate) fn text(&self, width: usize) -> String {
        let shown = |v: Option<f64>| v.map_or_else(|| "-".to_string(), ms);
        let mut line = format!(
            "{:<width$} {:>9} → {:<9}",
            self.id,
            shown(self.a),
            shown(self.b)
        );
        if let Some(effect) = self.effect {
            line.push_str(&format!(" {:>9} ms", signed(effect)));
        } else {
            line.push_str(&" ".repeat(13));
        }
        if let Some([lo, hi]) = self.ci {
            line.push_str(&format!("  CI [{}, {}]", signed(lo), signed(hi)));
        }
        line.push_str(&format!("  {}", verdict_text(self.verdict)));
        line
    }
}

pub(crate) fn verdict_text(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::WithinNoise => "~ within noise",
        Verdict::Negligible => "~ negligible",
        Verdict::TooFewBlocks => "? too few blocks",
        other => other.as_str(),
    }
}

/// Milliseconds with enough digits to tell 0.04 from 0.05 and 124.0 from 124.1.
pub(crate) fn ms(v: f64) -> String {
    match v.abs() {
        a if a >= 10.0 => format!("{v:.1}"),
        a if a >= 1.0 => format!("{v:.2}"),
        _ => format!("{v:.3}"),
    }
}

pub(crate) fn signed(v: f64) -> String {
    let text = ms(v);
    if v >= 0.0 && !text.starts_with('-') {
        format!("+{text}")
    } else {
        text
    }
}

/// One side's per-block series: health fields first, then spans.
struct Blocks {
    block_s: f64,
    rows: Vec<(String, Points)>,
    /// The per-block tick or frame time the drift warning watches, and its name.
    trend: Option<(&'static str, Points)>,
    count: usize,
}

impl Blocks {
    fn get(&self, id: &str) -> Option<&Points> {
        self.rows.iter().find(|(i, _)| i == id).map(|(_, p)| p)
    }
}

fn blocks(view: &View, side: Side, thread: Option<u32>) -> Blocks {
    let capture = view.capture;
    let span_blocks: Vec<_> = capture
        .span_blocks
        .iter()
        .filter(|b| b.side() == side && thread == Some(b.thread))
        .collect();
    let base_s = span_blocks
        .first()
        .map(|b| f64::from(b.block_s))
        .filter(|s| *s > 0.0)
        .unwrap_or(DEFAULT_BLOCK_S);
    let samples = live(capture, side);
    let from_spans = span_blocks.iter().map(|b| b.block as usize + 1).max();
    let from_health = samples
        .iter()
        .map(|h| (h.t.max(0.0) / base_s).floor() as usize + 1)
        .max();
    let raw = from_spans.max(from_health).unwrap_or(0);
    let merge = raw.div_ceil(MAX_BLOCKS).max(1);
    let block_s = base_s * merge as f64;
    let at = |index: usize| (index as f64 + 0.5) * block_s;

    let mut rows: Vec<(String, Points)> = Vec::new();
    let fields: &[HealthField] = if side == Side::Client {
        &CLIENT_HEALTH
    } else {
        &SERVER_HEALTH
    };
    let mut health: Vec<(HealthField, Points)> = Vec::new();
    for &name in fields {
        let pick = name.pick();
        let mut by_block: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
        for h in &samples {
            if let Some(v) = pick(h) {
                let index = (h.t.max(0.0) / base_s).floor() as usize / merge;
                by_block.entry(index).or_default().push(f64::from(v));
            }
        }
        let points: Points = by_block
            .into_iter()
            .filter_map(|(i, v)| mean(v.into_iter()).map(|m| (at(i), m)))
            .collect();
        if !points.is_empty() {
            rows.push((format!("h:{name}"), points.clone()));
            health.push((name, points));
        }
    }

    #[derive(Default)]
    struct Acc {
        units: u64,
        roots: f64,
        by_name: HashMap<String, f64>,
    }
    let mut spans: BTreeMap<usize, Acc> = BTreeMap::new();
    for block in &span_blocks {
        let acc = spans.entry(block.block as usize / merge).or_default();
        acc.units += u64::from(block.units);
        for (i, &name) in block.span.iter().enumerate() {
            let wall = f64::from(block.wall_ms.get(i).copied().unwrap_or(0.0));
            *acc.by_name
                .entry(view.text(side, name).into_owned())
                .or_default() += wall;
            if block.parent.get(i).copied().unwrap_or(0) == 0 {
                acc.roots += wall;
            }
        }
    }
    spans.retain(|_, acc| acc.units > 0);
    let mut names: Vec<&String> = spans.values().flat_map(|a| a.by_name.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let points: Points = spans
            .iter()
            .map(|(&i, acc)| {
                (
                    at(i),
                    acc.by_name.get(name).copied().unwrap_or(0.0) / acc.units as f64,
                )
            })
            .collect();
        rows.push((format!("s:{name}"), points));
    }
    let roots: Points = spans
        .iter()
        .map(|(&i, acc)| (at(i), acc.roots / acc.units as f64))
        .collect();

    let preferred = if side == Side::Client {
        HealthField::FrameMsP50
    } else {
        HealthField::MsptAvg
    };
    let trend = health
        .iter()
        .find(|(n, _)| *n == preferred)
        .or_else(|| health.first())
        .map(|(_, points)| points.clone())
        .or_else(|| (!roots.is_empty()).then_some(roots))
        .map(|points| {
            (
                if side == Side::Client {
                    "frame time"
                } else {
                    "MSPT"
                },
                points,
            )
        });
    let count = trend
        .as_ref()
        .map(|(_, p)| p.len())
        .or_else(|| rows.iter().map(|(_, p)| p.len()).max())
        .unwrap_or(0);
    Blocks {
        block_s,
        rows,
        trend,
        count,
    }
}

#[derive(Clone, Debug)]
pub struct Diff {
    pub side: Side,
    pub unit: TickUnit,
    /// The span thread compared, when both captures have it.
    pub thread: Option<String>,
    pub block_s: f64,
    /// Blocks of the tick or frame time on each side.
    pub blocks: [usize; 2],
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}

impl Diff {
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn reply_unit(&self) -> Unit {
        if self.unit == TickUnit::Frame {
            Unit::MsPerFrame
        } else {
            Unit::MsPerTick
        }
    }

    /// "24 vs 24 blocks of 5 s".
    pub fn blocks_text(&self) -> String {
        format!(
            "{} vs {} blocks of {} s",
            self.blocks[0],
            self.blocks[1],
            trim(self.block_s)
        )
    }

    /// The text report; `a` and `b` are the files' names.
    pub fn text(&self, a: &str, b: &str) -> String {
        let what = match &self.thread {
            Some(t) => format!("{} {t} thread", side_name(self.side)),
            None => side_name(self.side).to_string(),
        };
        let mut out = format!(
            "A {a}  B {b}  ({}, {what}, {})\n",
            self.blocks_text(),
            self.reply_unit().as_str().replace("_per_", "/")
        );
        let width = self.rows.iter().map(|r| r.id.len()).max().unwrap_or(0);
        for row in &self.rows {
            out.push_str(&row.text(width));
            out.push('\n');
        }
        for warning in &self.warnings {
            out.push_str(&format!("warning: {warning}\n"));
        }
        out
    }

    /// The JSON report; `a` and `b` are the files' names.
    pub fn envelope(&self, a: &str, b: &str) -> DiffEnvelope {
        Envelope::success(
            OpName::Diff(DiffOp::Diff),
            captures(a, b),
            Some(DiffSelection {
                side: SideName::from(self.side),
                thread: self.thread.clone(),
                block_s: num(self.block_s),
                blocks: self.blocks,
            }),
            self.reply_unit(),
            self.rows.iter().map(Row::wire).collect(),
            self.warnings.clone(),
        )
    }
}

fn trim(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

fn pick_side(a: &View, b: &View, requested: Option<Side>) -> Result<Side, Error> {
    let (in_a, in_b) = (a.sides(), b.sides());
    match requested {
        Some(side) => {
            for (name, sides) in [("A", &in_a), ("B", &in_b)] {
                if !sides.contains(&side) {
                    return Err(Error::Incomparable(format!(
                        "capture {name} has no {} side",
                        side_name(side)
                    )));
                }
            }
            Ok(side)
        }
        None => in_a
            .iter()
            .copied()
            .find(|s| in_b.contains(s))
            .ok_or_else(|| Error::Incomparable("the captures have no side in common".into())),
    }
}

fn thread_of(
    view: &View,
    side: Side,
    requested: Option<&str>,
) -> Result<Option<(u32, String)>, Error> {
    match view.resolve_thread(side, requested) {
        Ok(id) => Ok(Some((id, view.text(side, id).into_owned()))),
        Err(detail) if requested.is_some() && !view.threads(side).is_empty() => {
            Err(Error::NoSuchThread { detail })
        }
        Err(_) => Ok(None),
    }
}

fn build_profile(capture: &Capture, side: Side) -> Option<&str> {
    capture
        .start
        .iter()
        .find(|s| s.side() == side)
        .and_then(|s| s.build.as_ref())
        .map(|b| b.profile.as_str())
        .filter(|p| !p.is_empty())
}

/// Whether both captures were built the same way: Some(profiles) when they were not.
pub(crate) fn build_mismatch(a: &Capture, b: &Capture, side: Side) -> Option<(String, String)> {
    match (build_profile(a, side), build_profile(b, side)) {
        (Some(x), Some(y)) if x != y => Some((x.to_string(), y.to_string())),
        _ => None,
    }
}

/// Compares `b` against `a` on one side (the common one unless asked): health fields and the main thread's spans.
pub fn diff(a: &Capture, b: &Capture, options: &DiffOptions) -> Result<Diff, Error> {
    let (va, vb) = (View::new(a), View::new(b));
    let side = pick_side(&va, &vb, options.side)?;
    let requested = options.thread.as_deref();
    let (ta, tb) = (
        thread_of(&va, side, requested)?,
        thread_of(&vb, side, requested)?,
    );
    let blocks_a = blocks(&va, side, ta.as_ref().map(|t| t.0));
    let blocks_b = blocks(&vb, side, tb.as_ref().map(|t| t.0));
    if blocks_a.rows.is_empty() && blocks_b.rows.is_empty() {
        return Err(Error::Incomparable(
            "neither capture has health samples or span blocks for that side".into(),
        ));
    }

    let mut ids: Vec<&str> = Vec::new();
    for set in [&blocks_a, &blocks_b] {
        for (id, _) in &set.rows {
            if !ids.contains(&id.as_str()) {
                ids.push(id);
            }
        }
    }
    let mut rows: Vec<Row> = ids
        .into_iter()
        .filter_map(|id| {
            let values = |p: &Points| p.iter().map(|x| x.1).collect::<Vec<f64>>();
            let (va, vb) = (blocks_a.get(id).map(values), blocks_b.get(id).map(values));
            row(id, va.as_deref(), vb.as_deref())
        })
        .filter(|r| r.a.unwrap_or(0.0).max(r.b.unwrap_or(0.0)) >= options.min_ms)
        .collect();
    if options.calls {
        rows.extend(call_rows(&va, &vb, side, requested, options.min_ms));
    }
    let (health, mut spans): (Vec<Row>, Vec<Row>) =
        rows.drain(..).partition(|r| r.id.starts_with("h:"));
    spans.sort_by(|x, y| {
        magnitude(y)
            .total_cmp(&magnitude(x))
            .then_with(|| x.id.cmp(&y.id))
    });
    let mut warnings = Vec::new();
    let room = options.limit.saturating_sub(health.len());
    if spans.len() > room {
        warnings.push(format!(
            "{} more rows under the limit; raise --limit or --min-ms",
            spans.len() - room
        ));
        spans.truncate(room);
    }
    let rows: Vec<Row> = health.into_iter().chain(spans).collect();

    for (label, set) in [("A", &blocks_a), ("B", &blocks_b)] {
        if let Some((what, points)) = &set.trend {
            let window = set.block_s * points.len() as f64;
            if let Some(d) = stats::drift(points, window) {
                warnings.push(format!(
                    "{label}'s {what} drifts {} over its window (Theil–Sen {} ms/s): the window was not steady",
                    if d.slope > 0.0 { "upward" } else { "downward" },
                    signed(d.slope)
                ));
            }
        }
    }
    if (blocks_a.block_s - blocks_b.block_s).abs() > 1e-9
        && blocks_a.count > 0
        && blocks_b.count > 0
    {
        warnings.push(format!(
            "the captures cut their blocks differently ({} s and {} s)",
            trim(blocks_a.block_s),
            trim(blocks_b.block_s)
        ));
    }
    if let Some((x, y)) = build_mismatch(a, b, side) {
        warnings.push(format!("A is a {x} build and B a {y} build"));
    }
    Ok(Diff {
        side,
        unit: View::main_unit(side),
        thread: match (&ta, &tb) {
            (Some(x), Some(y)) if x.1 == y.1 => Some(x.1.clone()),
            _ => None,
        },
        block_s: blocks_a.block_s,
        blocks: [blocks_a.count, blocks_b.count],
        rows,
        warnings,
    })
}

/// The functions and call paths of both captures, matched by id; one a capture lacks is new or gone.
fn call_rows(a: &View, b: &View, side: Side, thread: Option<&str>, min_ms: f64) -> Vec<Row> {
    let (calls_a, calls_b) = (
        call_series(a, side, thread, min_ms),
        call_series(b, side, thread, min_ms),
    );
    let mut names: BTreeMap<&str, &Option<String>> = BTreeMap::new();
    for set in [&calls_a, &calls_b] {
        for call in set {
            names.entry(call.id.as_str()).or_insert(&call.name);
        }
    }
    let by_id = |set: &[CallSeries]| -> HashMap<String, usize> {
        set.iter()
            .enumerate()
            .map(|(i, c)| (c.id.clone(), i))
            .collect()
    };
    let (index_a, index_b) = (by_id(&calls_a), by_id(&calls_b));
    names
        .into_iter()
        .filter_map(|(id, name)| {
            let mut row = row(
                id,
                values_of(&calls_a, &index_a, id),
                values_of(&calls_b, &index_b, id),
            )?;
            row.name = name.clone();
            (row.a.unwrap_or(0.0).max(row.b.unwrap_or(0.0)) >= min_ms).then_some(row)
        })
        .collect()
}

fn values_of<'a>(
    set: &'a [CallSeries],
    index: &HashMap<String, usize>,
    id: &str,
) -> Option<&'a [f64]> {
    index.get(id).map(|&i| set[i].values.as_slice())
}

fn magnitude(row: &Row) -> f64 {
    row.effect.or(row.a).or(row.b).unwrap_or(0.0).abs()
}

pub(crate) fn row(id: &str, a: Option<&[f64]>, b: Option<&[f64]>) -> Option<Row> {
    let blocks = [a.map_or(0, <[f64]>::len), b.map_or(0, <[f64]>::len)];
    let lone = |id: &str, a: Option<f64>, b: Option<f64>, verdict| Row {
        id: id.to_string(),
        name: None,
        a,
        b,
        effect: None,
        ci: None,
        confidence: None,
        blocks,
        verdict,
    };
    Some(match (a, b) {
        (Some(x), Some(y)) => {
            let c = compare(x, y)?;
            Row {
                id: id.to_string(),
                name: None,
                a: Some(c.a),
                b: Some(c.b),
                effect: Some(c.effect),
                ci: c.ci,
                confidence: c.confidence,
                blocks,
                verdict: c.verdict,
            }
        }
        (Some(x), None) => lone(id, median(x), None, Verdict::Gone),
        (None, Some(y)) => lone(id, None, median(y), Verdict::New),
        (None, None) => return None,
    })
}
