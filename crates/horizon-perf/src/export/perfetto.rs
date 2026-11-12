//! The Chrome trace event format (a JSON array) that Perfetto and chrome://tracing open: every side a process, every thread a track.

use std::collections::HashMap;

use horizon_perf_format::v1::Side;
use serde::{Deserialize, Serialize};

use super::{ExportError, ExportOptions};
use crate::error::side_name;
use crate::view::View;
use crate::wire::vocab;

vocab! {
    /// An event's `ph`.
    Phase {
        Complete = "X",
        Instant = "i",
        Metadata = "M",
    }
}

vocab! {
    /// The `name` of a metadata event.
    MetaName {
        ProcessName = "process_name",
        ThreadName = "thread_name",
    }
}

vocab! {
    /// An instant's `s`: the track it is drawn on.
    Scope {
        Process = "p",
    }
}

/// What a metadata event names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceArgs {
    pub name: String,
}

/// One event of the trace; times are microseconds on the side's clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    pub ph: Phase,
    pub name: String,
    pub pid: u32,
    pub tid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dur: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s: Option<Scope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<TraceArgs>,
}

/// The process id of a side's events.
pub fn pid(side: Side) -> u32 {
    match side {
        Side::Server | Side::Unspecified => 1,
        Side::Client => 2,
    }
}

struct Span {
    pid: u32,
    tid: u32,
    start_us: u64,
    depth: u32,
    dur_us: u32,
    name: String,
}

fn metadata(name: MetaName, pid: u32, tid: u32, label: &str) -> TraceEvent {
    TraceEvent {
        ph: Phase::Metadata,
        name: name.as_str().to_string(),
        pid,
        tid,
        ts: None,
        dur: None,
        s: None,
        args: Some(TraceArgs {
            name: label.to_string(),
        }),
    }
}

pub fn encode(view: &View, options: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    let capture = view.capture;
    if let Some(side) = options.side
        && !view.sides().contains(&side)
        && !capture.timeline.iter().any(|c| c.side() == side)
    {
        return Err(ExportError::NoSuchSide(side));
    }
    let mut events = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut tids: HashMap<(Side, String), u32> = HashMap::new();
    let mut next_tid: HashMap<Side, u32> = HashMap::new();
    let mut threads: Vec<(Side, u32, String)> = Vec::new();
    for chunk in capture
        .timeline
        .iter()
        .filter(|c| options.side.is_none_or(|s| s == c.side()))
    {
        let side = chunk.side();
        let thread = view.text(side, chunk.thread).into_owned();
        if options
            .thread
            .as_ref()
            .is_some_and(|wanted| *wanted != thread)
        {
            continue;
        }
        let tid = *tids.entry((side, thread.clone())).or_insert_with(|| {
            let n = next_tid.entry(side).or_insert(0);
            *n += 1;
            threads.push((side, *n, thread.clone()));
            *n
        });
        for i in 0..chunk
            .name
            .len()
            .min(chunk.start_us.len())
            .min(chunk.dur_us.len())
        {
            spans.push(Span {
                pid: pid(side),
                tid,
                start_us: chunk.start_us[i],
                depth: chunk.depth.get(i).copied().unwrap_or(0),
                dur_us: chunk.dur_us[i],
                name: view.text(side, chunk.name[i]).into_owned(),
            });
        }
    }
    if spans.is_empty() {
        return Err(match &options.thread {
            Some(wanted) if !capture.timeline.is_empty() => {
                let mut sampled: Vec<String> = capture
                    .timeline
                    .iter()
                    .map(|c| view.text(c.side(), c.thread).into_owned())
                    .collect();
                sampled.sort();
                sampled.dedup();
                ExportError::NoSuchThread {
                    wanted: wanted.clone(),
                    sampled,
                }
            }
            _ => ExportError::NoTimeline,
        });
    }

    let mut present: Vec<Side> = threads.iter().map(|t| t.0).collect();
    present.sort_by_key(|s| pid(*s));
    present.dedup();
    for side in &present {
        events.push(metadata(
            MetaName::ProcessName,
            pid(*side),
            0,
            side_name(*side),
        ));
    }
    threads.sort_by_key(|(side, tid, _)| (pid(*side), *tid));
    for (side, tid, name) in &threads {
        events.push(metadata(MetaName::ThreadName, pid(*side), *tid, name));
    }
    for mark in capture.marks.iter().filter(|m| present.contains(&m.side())) {
        events.push(TraceEvent {
            ph: Phase::Instant,
            name: mark.name.clone(),
            pid: pid(mark.side()),
            tid: 0,
            ts: Some((mark.t * 1e6).round().max(0.0) as u64),
            dur: None,
            s: Some(Scope::Process),
            args: None,
        });
    }
    spans.sort_by_key(|s| (s.pid, s.tid, s.start_us, s.depth));
    events.extend(spans.into_iter().map(|s| TraceEvent {
        ph: Phase::Complete,
        name: s.name,
        pid: s.pid,
        tid: s.tid,
        ts: Some(s.start_us),
        dur: Some(s.dur_us),
        s: None,
        args: None,
    }));
    write(&events)
}

/// A JSON array with one event per line.
fn write(events: &[TraceEvent]) -> Result<Vec<u8>, ExportError> {
    let mut out = b"[\n".to_vec();
    for (i, event) in events.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",\n");
        }
        serde_json::to_writer(&mut out, event).map_err(|e| ExportError::Write(e.to_string()))?;
    }
    out.extend_from_slice(b"\n]\n");
    Ok(out)
}
