//! Exports of a capture for other tools: pprof, folded stacks and a Perfetto trace (docs/perf-platform.md §6.4).

pub mod folded;
pub mod perfetto;
pub mod pprof;

use std::fmt;

use horizon_perf_format::v1::{Capture, SampleSource, Side};

use crate::error::side_name;
use crate::samples::{Pick, Samples};
use crate::view::{View, Window};
use crate::wire::{Select, vocab};

vocab! {
    /// What `export` writes.
    Format {
        Pprof = "pprof",
        Folded = "folded",
        Perfetto = "perfetto",
    }
}

/// What to leave out of an export; a side the capture has and `side` allows is exported.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportOptions {
    pub side: Option<Side>,
    pub thread: Option<String>,
    /// The samples folded stacks count; pprof carries every kind the capture has.
    pub select: Select,
    pub slow: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            side: None,
            thread: None,
            select: Select::Cpu,
            slow: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExportError {
    NoSamples,
    NoTimeline,
    NoSuchSide(Side),
    NoSuchThread {
        wanted: String,
        sampled: Vec<String>,
    },
    NotASampleKind(Select),
    NoSamplesOfKind(Select),
    Write(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoSamples => f.write_str(
                "this capture has no samples to export: profile with the sampler on (samples: true)",
            ),
            ExportError::NoTimeline => f.write_str(
                "this capture has no timeline to export: profile with the timeline on (timeline: true)",
            ),
            ExportError::NoSuchSide(side) => {
                write!(f, "this capture has no {} side to export", side_name(*side))
            }
            ExportError::NoSuchThread { wanted, sampled } => write!(
                f,
                "no thread named {wanted}; threads: {}",
                sampled.join(", ")
            ),
            ExportError::NotASampleKind(select) => {
                write!(f, "--select {select} is not a kind of sample")
            }
            ExportError::NoSamplesOfKind(select) => {
                write!(f, "this capture has no {select} samples to export")
            }
            ExportError::Write(detail) => write!(f, "cannot write the export: {detail}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// One side's samples with the thread the export is limited to.
pub(crate) struct Sampled<'a> {
    pub side: Side,
    pub samples: Samples<'a>,
    pub thread: Option<u32>,
}

impl Sampled<'_> {
    pub fn pick(&self, select: Select, slow: bool) -> Pick {
        Pick {
            thread: self.thread,
            select,
            slow,
            contexts: None,
            window: Window::ALL,
        }
    }
}

pub(crate) fn sides(wanted: Option<Side>) -> impl Iterator<Item = Side> {
    [Side::Server, Side::Client]
        .into_iter()
        .filter(move |&s| wanted.is_none_or(|w| w == s))
}

/// The sides that have samples, server first.
pub(crate) fn sampled<'a>(
    view: &View<'a>,
    options: &ExportOptions,
) -> Result<Vec<Sampled<'a>>, ExportError> {
    let mut found = Vec::new();
    let mut threads: Vec<String> = Vec::new();
    for side in sides(options.side) {
        let Some(samples) = Samples::new(view, side) else {
            continue;
        };
        threads.extend(samples.thread_names.iter().cloned());
        let thread = match &options.thread {
            None => None,
            Some(name) => match samples.thread_names.iter().position(|n| n == name) {
                Some(at) => Some(at as u32),
                None => continue,
            },
        };
        found.push(Sampled {
            side,
            samples,
            thread,
        });
    }
    if let Some(wanted) = &options.thread
        && found.is_empty()
        && !threads.is_empty()
    {
        threads.sort();
        threads.dedup();
        return Err(ExportError::NoSuchThread {
            wanted: wanted.clone(),
            sampled: threads,
        });
    }
    if found.is_empty() {
        return Err(match options.side {
            Some(side) if view.capture.samples.iter().all(|p| p.side() != side) => {
                if view.sides().contains(&side) {
                    ExportError::NoSamples
                } else {
                    ExportError::NoSuchSide(side)
                }
            }
            _ => ExportError::NoSamples,
        });
    }
    Ok(found)
}

/// The `--select` a kind of samples answers to.
pub(crate) fn select_of(source: SampleSource) -> Option<Select> {
    match source {
        SampleSource::Cpu => Some(Select::Cpu),
        SampleSource::Wall => Some(Select::Wall),
        SampleSource::Alloc => Some(Select::Alloc),
        SampleSource::Live => Some(Select::Live),
        SampleSource::Unspecified => None,
    }
}

/// The export as the bytes of its file.
pub fn export(
    capture: &Capture,
    format: Format,
    options: &ExportOptions,
) -> Result<Vec<u8>, ExportError> {
    let view = View::new(capture);
    match format {
        Format::Pprof => pprof::encode(&view, options),
        Format::Folded => folded::encode(&view, options),
        Format::Perfetto => perfetto::encode(&view, options),
    }
}
