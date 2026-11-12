//! The pprof `profile.proto` (https://github.com/google/pprof/blob/main/proto/profile.proto), encoded by hand and gzipped.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use flate2::Compression;
use flate2::write::GzEncoder;
use horizon_perf_format::v1::{SampleKind, SampleUnit};

use super::{ExportError, ExportOptions, Sampled, sampled, select_of};
use crate::error::side_name;
use crate::samples::NO_NODE;
use crate::view::View;
use crate::wire::{ContextKey, Select, StateName};

const TYPES: [Select; 4] = [Select::Cpu, Select::Wall, Select::Alloc, Select::Live];

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn varint_field(out: &mut Vec<u8>, field: u32, v: u64) {
    varint(out, u64::from(field) << 3);
    varint(out, v);
}

fn bytes_field(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    varint(out, u64::from(field) << 3 | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn packed(out: &mut Vec<u8>, field: u32, values: impl IntoIterator<Item = u64>) {
    let mut body = Vec::new();
    values.into_iter().for_each(|v| varint(&mut body, v));
    bytes_field(out, field, &body);
}

struct Strings {
    list: Vec<String>,
    ix: HashMap<String, u64>,
}

impl Strings {
    fn new() -> Self {
        Self {
            list: vec![String::new()],
            ix: HashMap::from([(String::new(), 0)]),
        }
    }

    fn id(&mut self, s: &str) -> u64 {
        if let Some(&i) = self.ix.get(s) {
            return i;
        }
        self.list.push(s.to_string());
        let at = self.list.len() as u64 - 1;
        self.ix.insert(s.to_string(), at);
        at
    }

    fn value_type(&mut self, kind: &str, unit: &str) -> Vec<u8> {
        let mut body = Vec::new();
        varint_field(&mut body, 1, self.id(kind));
        varint_field(&mut body, 2, self.id(unit));
        body
    }

    fn label(&mut self, out: &mut Vec<u8>, key: &str, value: &str) {
        let mut body = Vec::new();
        varint_field(&mut body, 1, self.id(key));
        varint_field(&mut body, 2, self.id(value));
        bytes_field(out, 3, &body);
    }
}

type FunctionKey = (String, Option<String>, u32);

/// Functions of every side under one id each (from 1), the same function on both sides being one.
#[derive(Default)]
struct Functions {
    ids: HashMap<FunctionKey, u64>,
    list: Vec<FunctionKey>,
}

impl Functions {
    fn id(&mut self, key: FunctionKey) -> u64 {
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        self.list.push(key.clone());
        let id = self.list.len() as u64;
        self.ids.insert(key, id);
        id
    }
}

/// A sample's identity: side, node, thread, context, slow and state.
type SampleKey = (usize, u32, u32, u32, bool, StateName);

pub fn encode(view: &View, options: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    let sides = sampled(view, options)?;
    let types: Vec<Select> = TYPES
        .into_iter()
        .filter(|&t| sides.iter().any(|s| s.samples.has_kind(t)))
        .collect();
    if types.is_empty() {
        return Err(ExportError::NoSamples);
    }
    let mut rows: BTreeMap<SampleKey, Vec<u64>> = BTreeMap::new();
    for (si, side) in sides.iter().enumerate() {
        for (ti, &select) in types.iter().enumerate() {
            for hit in side.samples.hits(&side.pick(select, options.slow)) {
                let key = (si, hit.node, hit.thread, hit.context, hit.slow, hit.state);
                rows.entry(key).or_insert_with(|| vec![0; types.len()])[ti] += hit.count;
            }
        }
    }

    let mut strings = Strings::new();
    let mut functions = Functions::default();
    let mut samples = Vec::new();
    for ((si, node, thread, context, slow, state), values) in rows {
        if values.iter().all(|&v| v == 0) {
            continue;
        }
        let side = &sides[si];
        let mut locations = Vec::new();
        let mut at = node;
        while at != NO_NODE {
            let f = side.samples.function_of(at);
            locations.push(functions.id((f.full.clone(), f.file.clone(), f.line.unwrap_or(0))));
            at = side.samples.parent[at as usize];
        }
        let mut sample = Vec::new();
        packed(&mut sample, 1, locations);
        packed(&mut sample, 2, values);
        for (key, value) in labels(side, thread, context, slow, state) {
            strings.label(&mut sample, key.as_str(), &value.text());
        }
        bytes_field(&mut samples, 2, &sample);
    }

    let mut out = Vec::new();
    for select in &types {
        bytes_field(&mut out, 1, &strings.value_type(select.as_str(), "count"));
    }
    out.extend_from_slice(&samples);
    let mut function_msgs = Vec::new();
    for (i, (name, file, line)) in functions.list.iter().enumerate() {
        let id = i as u64 + 1;
        let (name_id, file_id) = (strings.id(name), strings.id(file.as_deref().unwrap_or("")));
        let mut function = Vec::new();
        varint_field(&mut function, 1, id);
        varint_field(&mut function, 2, name_id);
        varint_field(&mut function, 3, name_id);
        varint_field(&mut function, 4, file_id);
        varint_field(&mut function, 5, u64::from(*line));
        bytes_field(&mut function_msgs, 5, &function);
        let mut line_msg = Vec::new();
        varint_field(&mut line_msg, 1, id);
        varint_field(&mut line_msg, 2, u64::from(*line));
        let mut location = Vec::new();
        varint_field(&mut location, 1, id);
        bytes_field(&mut location, 4, &line_msg);
        bytes_field(&mut out, 4, &location);
    }
    out.extend_from_slice(&function_msgs);

    let started = sides
        .iter()
        .filter_map(|s| view.capture.start.iter().find(|m| m.side() == s.side))
        .map(|m| m.started_unix_ms)
        .min()
        .unwrap_or(0);
    varint_field(&mut out, 9, u64::try_from(started).unwrap_or(0) * 1_000_000);
    let seconds = sides
        .iter()
        .map(|s| view.end(s.side))
        .fold(0.0_f64, f64::max);
    varint_field(&mut out, 10, (seconds * 1e9).round() as u64);
    if let Some((select, unit, period)) = period(&sides) {
        let body = strings.value_type(select.as_str(), unit);
        bytes_field(&mut out, 11, &body);
        varint_field(&mut out, 12, period);
    }
    for s in &strings.list {
        bytes_field(&mut out, 6, s.as_bytes());
    }
    gzip(&out)
}

/// The name of a pprof label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelKey {
    Side,
    Thread,
    Slow,
    State,
    Context(ContextKey),
}

impl LabelKey {
    fn as_str(self) -> &'static str {
        match self {
            LabelKey::Side => "side",
            LabelKey::Thread => "thread",
            LabelKey::Slow => "slow",
            LabelKey::State => "state",
            LabelKey::Context(key) => key.as_str(),
        }
    }
}

/// The value of a pprof label; it becomes text only when the label is written.
#[derive(Clone, Debug, PartialEq, Eq)]
enum LabelValue {
    Text(String),
    Flag(bool),
    State(StateName),
}

impl LabelValue {
    fn text(&self) -> String {
        match self {
            LabelValue::Text(text) => text.clone(),
            LabelValue::Flag(flag) => flag.to_string(),
            LabelValue::State(state) => state.to_string(),
        }
    }
}

/// The labels of a sample, in a fixed order; a context field that is none has no label.
fn labels(
    side: &Sampled,
    thread: u32,
    context: u32,
    slow: bool,
    state: StateName,
) -> Vec<(LabelKey, LabelValue)> {
    let samples = &side.samples;
    let c = &samples.contexts[context as usize];
    let mut out = vec![
        (
            LabelKey::Side,
            LabelValue::Text(side_name(side.side).to_string()),
        ),
        (
            LabelKey::Thread,
            LabelValue::Text(samples.thread_names[thread as usize].clone()),
        ),
    ];
    let named = [
        (ContextKey::Span, &c.span),
        (ContextKey::Entity, &c.entity),
        (ContextKey::Block, &c.block),
        (ContextKey::Dim, &c.dim),
    ];
    for (key, value) in named {
        if let Some(value) = value {
            out.push((LabelKey::Context(key), LabelValue::Text(value.clone())));
        }
    }
    if let Some((x, z)) = c.chunk {
        out.push((
            LabelKey::Context(ContextKey::Chunk),
            LabelValue::Text(format!("{x},{z}")),
        ));
    }
    if slow {
        out.push((LabelKey::Slow, LabelValue::Flag(true)));
    }
    if state != StateName::Cpu {
        out.push((LabelKey::State, LabelValue::State(state)));
    }
    out
}

/// A kind's period as pprof states it: its type, its unit and whole units per sample.
fn kind_period(kind: &SampleKind) -> Option<(Select, &'static str, u64)> {
    let (unit, scale) = match kind.unit() {
        SampleUnit::Ms => ("nanoseconds", 1e6),
        SampleUnit::Bytes => ("bytes", 1.0),
        SampleUnit::Unspecified => return None,
    };
    Some((
        select_of(kind.source())?,
        unit,
        (kind.period * scale).round() as u64,
    ))
}

/// The first kind's period; none when it rounds to zero or another kind in the same unit has a different one.
fn period(sides: &[Sampled]) -> Option<(Select, &'static str, u64)> {
    let kinds: Vec<_> = sides
        .iter()
        .flat_map(|s| s.samples.profile.kinds.iter())
        .filter_map(kind_period)
        .collect();
    let first = *kinds.first()?;
    let agrees = kinds.iter().all(|k| k.1 != first.1 || k.2 == first.2);
    (first.2 > 0 && agrees).then_some(first)
}

fn gzip(raw: &[u8]) -> Result<Vec<u8>, ExportError> {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(raw)
        .and_then(|()| gz.finish())
        .map_err(|e| ExportError::Write(e.to_string()))
}
