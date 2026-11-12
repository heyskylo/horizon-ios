//! Folded stacks, `side;thread;frame;...;frame count` per line, for `inferno` and older scripts.

use std::collections::BTreeMap;

use super::{ExportError, ExportOptions, sampled};
use crate::error::side_name;
use crate::samples::{NO_NODE, Samples};
use crate::view::View;
use crate::wire::StateName;

const WAITING: &str = "[waiting]";

fn frame(text: &str) -> String {
    text.replace(';', ",").replace(['\n', '\r'], " ")
}

/// The functions from the root to `node`.
fn stack<'s>(samples: &'s Samples, node: u32) -> Vec<&'s str> {
    let mut frames = Vec::new();
    let mut at = node;
    while at != NO_NODE {
        frames.push(samples.function_of(at).full.as_str());
        at = samples.parent[at as usize];
    }
    frames.reverse();
    frames
}

pub fn encode(view: &View, options: &ExportOptions) -> Result<Vec<u8>, ExportError> {
    let sides = sampled(view, options)?;
    if !sides.iter().any(|s| s.samples.has_kind(options.select)) {
        return Err(ExportError::NoSamplesOfKind(options.select));
    }
    let mut lines: BTreeMap<String, u64> = BTreeMap::new();
    for side in sides {
        let samples = &side.samples;
        let pick = side.pick(options.select, options.slow);
        for hit in samples.hits(&pick) {
            let mut line = vec![
                side_name(side.side).to_string(),
                frame(&samples.thread_names[hit.thread as usize]),
            ];
            line.extend(stack(samples, hit.node).into_iter().map(frame));
            if hit.state == StateName::Waiting {
                line.push(WAITING.to_string());
            }
            *lines.entry(line.join(";")).or_default() += hit.count;
        }
    }
    let mut out = String::new();
    for (stack, count) in lines.into_iter().filter(|(_, n)| *n > 0) {
        out.push_str(&stack);
        out.push(' ');
        out.push_str(&count.to_string());
        out.push('\n');
    }
    Ok(out.into_bytes())
}
