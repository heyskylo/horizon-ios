//! The sampled functions and call paths of a capture as per-block series, for `diff --calls` (docs/perf-platform.md §6.6, §7.2).

use std::collections::BTreeMap;

use horizon_perf_format::v1::Side;

use crate::samples::{Pick, Samples};
use crate::stats::MAX_BLOCKS;
use crate::view::{View, Window};
use crate::wire::Select;

/// One function or call path: its inclusive CPU time per tick or frame in each block.
pub(crate) struct CallSeries {
    pub id: String,
    /// The function's short name; a call path has none.
    pub name: Option<String>,
    pub values: Vec<f64>,
}

/// Every function (`f:`) and call path (`p:`) with samples on the side's main thread (or `thread`), cut into the blocks the spans use.
pub(crate) fn call_series(
    view: &View,
    side: Side,
    thread: Option<&str>,
    floor: f64,
) -> Vec<CallSeries> {
    let Some(samples) = Samples::new(view, side) else {
        return Vec::new();
    };
    let Ok(thread) = samples.thread(thread) else {
        return Vec::new();
    };
    let width = f64::from(samples.profile.block_s);
    let last = samples
        .profile
        .weights
        .as_ref()
        .and_then(|w| w.block.iter().max().copied());
    let (Some(last), true) = (last, width > 0.0) else {
        return Vec::new();
    };
    let raw = last as usize + 1;
    let merge = raw.div_ceil(MAX_BLOCKS).max(1);
    let shadowed: Vec<bool> = (0..samples.func.len() as u32)
        .map(|node| samples.shadowed(node))
        .collect();
    let nodes = samples.func.len();

    let mut by_function: Vec<Vec<f64>> = vec![Vec::new(); samples.functions.len()];
    let mut by_node: Vec<Vec<f64>> = Vec::new();
    let mut seen_function = vec![false; samples.functions.len()];
    let mut seen_node = vec![false; nodes];
    let mut kept = 0usize;
    for first in (0..raw).step_by(merge) {
        // Blocks count when they start inside the window, so the edges sit half a block early.
        let window = Window {
            from: first as f64 * width - width / 2.0,
            to: (first + merge) as f64 * width - width / 2.0,
        };
        let units = samples.units(thread, false, window);
        if units.estimated || units.count == 0 {
            continue;
        }
        let pick = Pick {
            thread: Some(thread),
            select: Select::Cpu,
            slow: false,
            contexts: None,
            window,
        };
        let tally = samples.tally(&pick);
        let per_unit = |ms: f64| ms / units.count as f64;
        let mut functions = vec![0.0; samples.functions.len()];
        for node in 0..nodes {
            if tally.total_n[node] == 0 {
                continue;
            }
            seen_node[node] = true;
            if !shadowed[node] {
                let at = samples.func[node] as usize;
                seen_function[at] = true;
                functions[at] += per_unit(tally.total_ms[node]);
            }
        }
        for (values, value) in by_function.iter_mut().zip(functions) {
            values.push(value);
        }
        by_node.push(
            tally
                .total_ms
                .iter()
                .map(|&ms| per_unit(ms))
                .collect::<Vec<f64>>(),
        );
        kept += 1;
    }

    let peak = |values: &[f64]| values.iter().copied().fold(0.0, f64::max);
    let mut out = Vec::new();
    for (at, values) in by_function.into_iter().enumerate() {
        if seen_function[at] && peak(&values) >= floor {
            let function = &samples.functions[at];
            out.push(CallSeries {
                id: function.id.clone(),
                name: Some(function.name.clone()),
                values,
            });
        }
    }
    let mut paths: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for node in 0..nodes {
        if !seen_node[node] {
            continue;
        }
        let values: Vec<f64> = by_node.iter().map(|block| block[node]).collect();
        let sum = paths
            .entry(samples.path_id(node as u32))
            .or_insert_with(|| vec![0.0; kept]);
        for (total, value) in sum.iter_mut().zip(values) {
            *total += value;
        }
    }
    out.extend(
        paths
            .into_iter()
            .filter(|(_, values)| peak(values) >= floor)
            .map(|(id, values)| CallSeries {
                id,
                name: None,
                values,
            }),
    );
    out
}
