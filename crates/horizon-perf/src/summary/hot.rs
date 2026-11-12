//! The summary's sampled sections: hot functions and the split by entity, block and chunk (docs/perf-platform.md §5.1).

use horizon_perf_format::v1::Side;

use super::{Line, Section, SummaryOptions};
use crate::samples::{Pick, Samples};
use crate::view::{View, Window};
use crate::wire::{SampleBy, Select};

const HOT_ROWS: usize = 8;
const NAME_WIDTH_CAP: usize = 30;
/// Context splits: the rows each shows.
const SPLITS: [(SampleBy, &str, usize); 3] = [
    (SampleBy::Entity, "by entity type", 4),
    (SampleBy::Block, "by block type", 3),
    (SampleBy::Chunk, "by chunk", 3),
];

struct Sampled<'a> {
    samples: Samples<'a>,
    pick: Pick,
    thread: u32,
    units: u64,
}

fn open<'a>(view: &View<'a>, side: Side, options: &SummaryOptions) -> Option<Sampled<'a>> {
    let samples = Samples::new(view, side)?;
    let thread = samples.thread(options.thread.as_deref()).ok()?;
    let pick = Pick {
        thread: Some(thread),
        select: Select::Cpu,
        slow: options.slow,
        contexts: None,
        window: Window::ALL,
    };
    let units = samples.units(thread, options.slow, Window::ALL).count;
    samples.has_kind(Select::Cpu).then_some(Sampled {
        samples,
        pick,
        thread,
        units,
    })
}

impl Sampled<'_> {
    fn per_unit(&self, ms: f64) -> f64 {
        ms / self.units.max(1) as f64
    }
}

/// The function ranking and the context splits; empty when the capture holds no CPU samples of the thread.
pub(super) fn sections(view: &View, side: Side, options: &SummaryOptions) -> Vec<Section> {
    let Some(run) = open(view, side, options) else {
        return Vec::new();
    };
    let mut parts = vec![functions(&run, side, options)];
    for (by, label, rows) in SPLITS {
        parts.push(split(&run, side, by, label, rows));
    }
    parts.retain(|p| !p.is_empty());
    parts
}

fn functions(run: &Sampled, side: Side, options: &SummaryOptions) -> Section {
    let s = &run.samples;
    let times = s.function_times(&run.pick);
    let mut ranked: Vec<usize> = (0..s.functions.len())
        .filter(|&at| times.own[at].0 > 0.0)
        .collect();
    ranked.sort_by(|&a, &b| {
        times.own[b]
            .0
            .total_cmp(&times.own[a].0)
            .then_with(|| s.functions[a].full.cmp(&s.functions[b].full))
    });
    ranked.truncate(HOT_ROWS);
    if ranked.is_empty() {
        return Vec::new();
    }
    let unit = if side == Side::Client {
        "frame"
    } else {
        "tick"
    };
    let which = if options.slow {
        format!("slow {unit}s")
    } else {
        format!("all {unit}s")
    };
    let thread = &s.thread_names[run.thread as usize];
    let title = format!("hot functions, {thread} thread, CPU, {which}");
    let names: Vec<String> = ranked
        .iter()
        .map(|&at| {
            let name: String = s.functions[at].name.chars().take(NAME_WIDTH_CAP).collect();
            format!("  {name}")
        })
        .collect();
    let width = names
        .iter()
        .map(|n| n.chars().count())
        .max()
        .unwrap_or(0)
        .max(title.chars().count())
        + 2;
    let mut lines = vec![Line::heading(format!(
        "{title:<width$}{:>14}{:>14}",
        format!("self ms/{unit}"),
        format!("total ms/{unit}")
    ))];
    for (at, name) in ranked.into_iter().zip(names) {
        lines.push(Line::body(format!(
            "{name:<width$}{:>14.2}{:>14.2}   [{}]",
            run.per_unit(times.own[at].0),
            run.per_unit(times.inclusive[at].0),
            s.functions[at].id
        )));
    }
    lines
}

fn split(run: &Sampled, side: Side, by: SampleBy, label: &str, rows: usize) -> Section {
    let unit = if side == Side::Client {
        "frame"
    } else {
        "tick"
    };
    let parts: Vec<String> = run
        .samples
        .divide(&run.pick, by, None)
        .into_iter()
        .filter(|share| share.value > 0.0)
        .filter_map(|share| {
            let key = share.key?;
            Some(format!(
                "{} {:.1} [{key}]",
                share.label,
                run.per_unit(share.value)
            ))
        })
        .take(rows)
        .collect();
    if parts.is_empty() {
        return Vec::new();
    }
    vec![Line::body(format!(
        "{label} (sampled ms/{unit}): {}",
        parts.join(" · ")
    ))]
}

/// The id of the function with the most own time, for the drill-down hint.
pub(super) fn top_function(view: &View, side: Side, options: &SummaryOptions) -> Option<String> {
    let run = open(view, side, options)?;
    let times = run.samples.function_times(&run.pick);
    let s = &run.samples;
    (0..s.functions.len())
        .filter(|&at| times.own[at].0 > 0.0)
        .max_by(|&a, &b| {
            times.own[a]
                .0
                .total_cmp(&times.own[b].0)
                .then_with(|| s.functions[b].full.cmp(&s.functions[a].full))
        })
        .map(|at| s.functions[at].id.clone())
}

const HEAP_ROWS: usize = 3;
const MIB: f64 = 1024.0 * 1024.0;

/// Sampled heap bytes of every thread: live at the end and allocated during the capture, with the functions that hold the most.
pub(super) fn heap(view: &View, side: Side) -> Vec<Line> {
    let Some(samples) = Samples::new(view, side).filter(|s| s.has_kind(Select::Live)) else {
        return Vec::new();
    };
    let pick = |select| Pick {
        thread: None,
        select,
        slow: false,
        contexts: None,
        window: Window::ALL,
    };
    let live = samples.function_times(&pick(Select::Live));
    let allocated = samples.function_times(&pick(Select::Alloc)).grand_ms;
    let mut ranked: Vec<usize> = (0..samples.functions.len())
        .filter(|&at| live.own[at].0 > 0.0)
        .collect();
    ranked.sort_by(|&a, &b| live.own[b].0.total_cmp(&live.own[a].0));
    let top: Vec<String> = ranked
        .into_iter()
        .take(HEAP_ROWS)
        .map(|at| {
            let name: String = samples.functions[at]
                .name
                .chars()
                .take(NAME_WIDTH_CAP)
                .collect();
            format!("{name} {:.1}", live.own[at].0 / MIB)
        })
        .collect();
    let mut text = format!(
        "heap    live {:.1} MiB, allocated {:.1} MiB (sampled, all threads)",
        live.grand_ms / MIB,
        allocated / MIB
    );
    if !top.is_empty() {
        text.push_str(&format!(" · held by {}", top.join(" · ")));
    }
    vec![Line::body(text)]
}
