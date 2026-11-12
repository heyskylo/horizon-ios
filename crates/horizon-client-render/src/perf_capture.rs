//! A stopped sampler session as the capture's `samples` message (docs/perf-platform.md §3.5).
use std::collections::HashMap;

use horizon_perf_format::Interner;
use horizon_perf_format::v1::{
    BlockUnits, Contexts, Functions, Nodes, Overhead, SampleKind, SampleProfile, SampleSource,
    SampleUnit, SampledThread, Side, Unit, Weights,
};
use horizon_threads::heap_sample::HeapProfile;
use horizon_threads::profile::{Aggregate, Frame, Profile, State, Symbols, symbolize};
use horizon_threads::work::{self, Space};

#[cfg(test)]
mod tests;

const UNKNOWN: &str = "[unknown]";

/// The name `work::set_dim` ids stand for: 1 overworld, 2 the nether, 3 the end.
fn dimension_name(id: u8) -> Option<&'static str> {
    match id {
        1 => Some("overworld"),
        2 => Some("the_nether"),
        3 => Some("the_end"),
        _ => None,
    }
}

/// The crate a demangled function name belongs to, or "" when it names none.
fn crate_of(name: &str) -> &str {
    if let Some(inner) = name.strip_prefix('[') {
        let module = inner.split([' ', ']']).next().unwrap_or("");
        return if module == "unmapped" { "" } else { module };
    }
    let name = name.trim_start_matches(['<', '&', '*']);
    let name = name.strip_prefix("dyn ").unwrap_or(name);
    match name.split_once("::") {
        Some((krate, _)) if krate.chars().all(|c| c.is_alphanumeric() || c == '_') => krate,
        _ => "",
    }
}

#[derive(Default)]
struct FunctionTable {
    rows: Functions,
    index: HashMap<(u32, u32), u32>,
    allocator: Vec<bool>,
}

impl FunctionTable {
    fn id(
        &mut self,
        strings: &mut Interner,
        name: &str,
        file: Option<&str>,
        line: u32,
        addr: u64,
    ) -> u32 {
        let (name_id, file_id) = (
            strings.intern(name),
            file.map_or(0, |file| strings.intern(file)),
        );
        if let Some(&row) = self.index.get(&(name_id, file_id)) {
            return row;
        }
        let row = self.rows.name.len() as u32;
        self.rows.name.push(name_id);
        self.rows.file.push(file_id);
        self.rows.line.push(line);
        self.rows.crate_.push(match crate_of(name) {
            "" => 0,
            krate => strings.intern(krate),
        });
        self.rows.addr.push(addr);
        self.allocator.push(allocator_frame(name));
        self.index.insert((name_id, file_id), row);
        row
    }
}

/// The thread each trie node belongs to; node 0 is the root.
fn thread_of(agg: &Aggregate) -> Vec<u16> {
    let mut of = vec![0u16; agg.nodes()];
    for n in 1..agg.nodes() {
        of[n] = match agg.frame(n as u32) {
            Frame::Thread(i) => i,
            _ => of[agg.parent(n as u32) as usize],
        };
    }
    of
}

/// The trie nodes some counted sample passes through.
fn live_nodes(agg: &Aggregate) -> Vec<bool> {
    let mut live = vec![false; agg.nodes()];
    for key in agg.counts.keys() {
        let mut n = key.node as usize;
        while n != 0 && !live[n] {
            live[n] = true;
            n = agg.parent(n as u32) as usize;
        }
    }
    live
}

#[derive(Default)]
struct NodeTable {
    rows: Nodes,
    index: HashMap<(Option<u32>, u32, u16), u32>,
}

impl NodeTable {
    fn id(&mut self, parent: Option<u32>, function: u32, thread: u16) -> u32 {
        let key = (parent, function, if parent.is_none() { thread } else { 0 });
        if let Some(&row) = self.index.get(&key) {
            return row;
        }
        let nodes = &mut self.rows;
        nodes.parent.push(parent.unwrap_or(0));
        nodes.function.push(function);
        nodes.thread_root.push(match parent {
            None => u32::from(thread) + 1,
            Some(_) => 0,
        });
        let row = nodes.parent.len() as u32 - 1;
        self.index.insert(key, row);
        row
    }
}

fn named(strings: &mut Interner, space: Space, id: u16) -> u32 {
    match id {
        0 => 0,
        id => match work::name(space, id) {
            Some(name) => strings.intern(name),
            None => strings.intern(&format!("#{id}")),
        },
    }
}

/// Whether `name` is the allocator's own frame, which a heap sample's stack drops from its leaf end.
fn allocator_frame(name: &str) -> bool {
    let name = name.trim_start_matches('<');
    name.starts_with("horizon_threads::heap_sample::")
        || name.starts_with("__rust")
        || name.starts_with("__rg_")
        || name.starts_with("__rdl_")
        || name.starts_with("alloc::alloc::")
        || name.starts_with("alloc::raw_vec::")
        || name.starts_with("mimalloc::")
}

/// One aggregate's trie nodes as rows of the shared tables: `last[n]` is the output node of trie node `n`.
fn add_tree(
    agg: &Aggregate,
    threads: &[u16],
    symbols: &Symbols,
    functions: &mut FunctionTable,
    nodes: &mut NodeTable,
    strings: &mut Interner,
) -> (Vec<u16>, Vec<Option<u32>>) {
    let (local, live) = (thread_of(agg), live_nodes(agg));
    let thread_of: Vec<u16> = local
        .iter()
        .map(|&t| threads.get(usize::from(t)).copied().unwrap_or(0))
        .collect();
    let mut last: Vec<Option<u32>> = vec![None; agg.nodes()];
    for n in (1..agg.nodes()).filter(|&n| live[n]) {
        let inherited = last[agg.parent(n as u32) as usize];
        last[n] = match agg.frame(n as u32) {
            Frame::Thread(_) => None,
            Frame::Wall(_) => inherited,
            Frame::Addr(addr) => {
                let offset = symbols.module_offset(addr);
                let mut at = inherited;
                for sym in symbols.frames(addr).iter().rev() {
                    let f = functions.id(strings, &sym.name, sym.file.as_deref(), sym.line, offset);
                    at = Some(nodes.id(at, f, thread_of[n]));
                }
                if at == inherited {
                    let f = functions.id(strings, UNKNOWN, None, 0, offset);
                    at = Some(nodes.id(at, f, thread_of[n]));
                }
                at
            }
        };
    }
    (thread_of, last)
}

fn trie_addrs(agg: &Aggregate) -> impl Iterator<Item = u64> + '_ {
    let live = live_nodes(agg);
    (1..agg.nodes())
        .filter(move |&n| live[n])
        .filter_map(|n| match agg.frame(n as u32) {
            Frame::Addr(a) => Some(a),
            _ => None,
        })
}

/// `profile` (and the heap session's `heap`) as a `SampleProfile` of `side`; strings go through `strings`, whose new entries the caller appends first.
pub fn sample_profile(
    profile: &Profile,
    heap: Option<&HeapProfile>,
    side: Side,
    unit: Unit,
    block_name: &dyn Fn(u16) -> Option<String>,
    strings: &mut Interner,
) -> SampleProfile {
    let agg = &profile.agg;
    let addrs: Vec<u64> = trie_addrs(agg)
        .chain(heap.into_iter().flat_map(|h| trie_addrs(&h.agg)))
        .collect();
    let symbols = symbolize(addrs);

    let mut names: Vec<String> = agg.threads.clone();
    let cpu_threads: Vec<u16> = (0..agg.threads.len() as u16).collect();
    let heap_threads: Vec<u16> = heap.map_or_else(Vec::new, |h| {
        h.agg
            .threads
            .iter()
            .map(|name| match names.iter().position(|n| n == name) {
                Some(at) => at as u16,
                None => {
                    names.push(name.clone());
                    names.len() as u16 - 1
                }
            })
            .collect()
    });

    let mut functions = FunctionTable::default();
    let mut nodes = NodeTable::default();
    let mut trees = vec![(
        agg,
        add_tree(
            agg,
            &cpu_threads,
            &symbols,
            &mut functions,
            &mut nodes,
            strings,
        ),
    )];
    if let Some(h) = heap {
        let tree = add_tree(
            &h.agg,
            &heap_threads,
            &symbols,
            &mut functions,
            &mut nodes,
            strings,
        );
        trees.push((&h.agg, tree));
    }

    let wall = agg.counts.keys().any(|k| k.state != State::Cpu);
    let period_ms = 1000.0 / f64::from(profile.config.sampler.hz.max(1));
    let kind = |source: SampleSource, unit: SampleUnit, period: f64| SampleKind {
        source: source as i32,
        unit: unit as i32,
        period,
    };
    let mut kinds = vec![kind(SampleSource::Cpu, SampleUnit::Ms, period_ms)];
    if wall {
        kinds.push(kind(SampleSource::Wall, SampleUnit::Ms, period_ms));
    }
    if heap.is_some() {
        kinds.push(kind(SampleSource::Alloc, SampleUnit::Bytes, 1.0));
        kinds.push(kind(SampleSource::Live, SampleUnit::Bytes, 1.0));
    }
    let kind_of = |state: State| -> u32 {
        let source = match state {
            State::Cpu => SampleSource::Cpu,
            State::Running | State::Waiting => SampleSource::Wall,
            State::Alloc => SampleSource::Alloc,
            State::Live => SampleSource::Live,
        };
        kinds
            .iter()
            .position(|k| k.source == source as i32)
            .unwrap_or(0) as u32
    };

    let mut contexts = Contexts::default();
    let mut context_ids: HashMap<[u32; 6], u32> = HashMap::new();
    let mut weights = Weights::default();
    let mut unknown_roots: HashMap<u16, u32> = HashMap::new();
    for (agg, (thread_of, last)) in &trees {
        let mut counted: Vec<_> = agg.counts.iter().collect();
        counted.sort_by_key(|(k, _)| {
            (
                thread_of[k.node as usize],
                k.node,
                k.state as u8,
                k.slow,
                k.time_block,
                (k.span, k.entity, k.block, k.dim, k.chunk),
            )
        });
        for (key, &count) in counted {
            let thread = thread_of[key.node as usize];
            let mut node = match last[key.node as usize] {
                Some(node) => node,
                None => *unknown_roots.entry(thread).or_insert_with(|| {
                    let f = functions.id(strings, UNKNOWN, None, 0, 0);
                    nodes.id(None, f, thread)
                }),
            };
            if matches!(key.state, State::Alloc | State::Live) {
                while nodes.rows.thread_root[node as usize] == 0
                    && functions.allocator[nodes.rows.function[node as usize] as usize]
                {
                    node = nodes.rows.parent[node as usize];
                }
            }
            let row = [
                named(strings, Space::Span, key.span),
                named(strings, Space::Entity, key.entity),
                match key.block {
                    0 => 0,
                    id => match block_name(id) {
                        Some(name) => strings.intern(&name),
                        None => strings.intern(&format!("#{id}")),
                    },
                },
                dimension_name(key.dim).map_or(0, |name| strings.intern(name)),
                key.chunk.0 as u32,
                key.chunk.1 as u32,
            ];
            let context = *context_ids.entry(row).or_insert_with(|| {
                contexts.span.push(row[0]);
                contexts.entity.push(row[1]);
                contexts.block.push(row[2]);
                contexts.dim.push(row[3]);
                contexts.chunk_x.push(key.chunk.0);
                contexts.chunk_z.push(key.chunk.1);
                contexts.span.len() as u32 - 1
            });
            weights.node.push(node);
            weights.thread.push(u32::from(thread));
            weights.kind.push(kind_of(key.state));
            weights.state.push(match key.state {
                State::Running => 1,
                State::Waiting => 2,
                State::Cpu | State::Alloc | State::Live => 0,
            });
            weights.slow.push(key.slow);
            weights.context.push(context);
            weights.block.push(u32::from(key.time_block));
            weights.count.push(count);
        }
    }

    let blocks = agg.time_blocks();
    let mut threads = Vec::new();
    let mut block_units = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let ticks = agg.ticks.get(&(i as u16)).filter(|_| i < agg.threads.len());
        threads.push(SampledThread {
            name: strings.intern(name),
            units: ticks.map_or(0, |t| t.count),
            slow_units: ticks.map_or(0, |t| t.slow),
            unit: unit as i32,
        });
        if i >= agg.threads.len() {
            continue;
        }
        let mut units = agg
            .block_ticks
            .get(&(i as u16))
            .cloned()
            .unwrap_or_default();
        units.resize(blocks, 0);
        block_units.push(BlockUnits {
            thread: i as u32,
            units,
        });
    }

    SampleProfile {
        side: side as i32,
        kinds,
        threads,
        block_s: profile.config.block_s,
        block_units,
        functions: Some(functions.rows),
        nodes: Some(nodes.rows),
        contexts: Some(contexts),
        weights: Some(weights),
        slow_over_ms: profile.config.ticks_over_ms as f32,
    }
}

/// What the sampler and its writer thread cost during the session.
pub fn overhead(profile: &Profile, heap: Option<&HeapProfile>) -> Overhead {
    let wall_ns = profile.elapsed.as_nanos().max(1) as f64;
    let handler_ns = profile.stats.handler_ns.unwrap_or(0.0) * profile.stats.samples as f64;
    Overhead {
        sampler_share_of_thread: (handler_ns / wall_ns) as f32,
        writer_core_share: (profile.writer_cpu_ns as f64 / wall_ns) as f32,
        dropped_samples: profile.stats.dropped + profile.agg.lost + heap.map_or(0, |h| h.agg.lost),
        heap_samples: heap.map_or(0, |h| h.samples),
        heap_sample_ns: heap.map_or(0, |h| h.sample_ns),
        ..Overhead::default()
    }
}
