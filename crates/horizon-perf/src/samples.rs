//! A capture's `samples` section as a call tree with normalized function names (docs/perf-platform.md §3.5, §5.2).

use std::collections::{BTreeMap, HashMap};

use horizon_perf_format::v1::{
    Contexts, Functions, Nodes, SampleProfile, SampleSource, Side, Unit as TickUnit,
};

use crate::ids::{self, Fnv, Id, Kind};
use crate::names;
use crate::view::{View, Window};
use crate::wire::{ContextKey, PaceName, SampleBy, Select, StateName, Unit};

pub const NO_NODE: u32 = u32::MAX;

const UNKNOWN: &str = "[unknown]";

/// One function after normalization: every monomorphization and address of it is this row.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub id: String,
    pub name: String,
    pub full: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub crate_name: Option<String>,
}

/// The context of a sample, by name; a chunk of (0, 0) means none was set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub span: Option<String>,
    pub entity: Option<String>,
    pub block: Option<String>,
    pub dim: Option<String>,
    pub chunk: Option<(i32, i32)>,
}

impl Context {
    pub fn chunk_text(&self) -> Option<String> {
        self.chunk
            .map(|(x, z)| format!("{}/{x},{z}", self.dim.as_deref().unwrap_or("unknown")))
    }

    fn matches(&self, key: ContextKey, wanted: &str) -> bool {
        let (prefix, have) = match key {
            ContextKey::Span => ("s:", self.span.clone()),
            ContextKey::Entity => ("e:", self.entity.clone()),
            ContextKey::Block => ("b:", self.block.clone()),
            ContextKey::Dim => ("d:", self.dim.clone()),
            ContextKey::Chunk => ("c:", self.chunk_text()),
        };
        have.is_some_and(|h| h == wanted.strip_prefix(prefix).unwrap_or(wanted))
    }
}

pub struct Samples<'a> {
    pub profile: &'a SampleProfile,
    pub side: Side,
    pub functions: Vec<Function>,
    /// Per node: its function, parent (`NO_NODE` for a root) and thread index.
    pub func: Vec<u32>,
    pub parent: Vec<u32>,
    pub thread_of: Vec<u32>,
    /// Per node: the mod that owns it, as an index into `mod_ids`: its own crate's, else its nearest caller's.
    owner: Vec<Option<u32>>,
    mod_ids: Vec<String>,
    path: Vec<Fnv>,
    pub contexts: Vec<Context>,
    pub thread_names: Vec<String>,
}

/// What to count: the thread, the sample kind, slow units only, the contexts allowed and the time blocks.
#[derive(Clone, Debug)]
pub struct Pick {
    pub thread: Option<u32>,
    pub select: Select,
    pub slow: bool,
    /// Indexed by context; None allows every one.
    pub contexts: Option<Vec<bool>>,
    pub window: Window,
}

/// One row of the weights table that passed a pick.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub node: u32,
    pub thread: u32,
    pub context: u32,
    pub state: StateName,
    pub slow: bool,
    /// Count times the kind's period: milliseconds, or bytes for allocation samples.
    pub value: f64,
    pub count: u64,
}

/// The ticks or frames a pick covers, and whether that figure is an estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Units {
    pub count: u64,
    pub estimated: bool,
}

/// Weight per node: its own, and its own plus everything below it.
pub struct Tally {
    pub self_ms: Vec<f64>,
    pub self_n: Vec<u64>,
    pub total_ms: Vec<f64>,
    pub total_n: Vec<u64>,
    pub grand_ms: f64,
    pub grand_n: u64,
}

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

impl<'a> Samples<'a> {
    pub fn new(view: &View<'a>, side: Side) -> Option<Self> {
        let profile = view.capture.samples.iter().find(|p| p.side() == side)?;
        let text = |id: u32| view.text(side, id);
        let (no_functions, no_nodes, no_contexts) = Default::default();
        let rows: &Functions = profile.functions.as_ref().unwrap_or(&no_functions);
        let mut functions: Vec<Function> = Vec::new();
        let mut by_full: HashMap<String, u32> = HashMap::new();
        let mut taken: HashMap<String, String> = HashMap::new();
        let mut intern =
            |full: String, file: Option<String>, line: u32, crate_name: Option<String>| -> u32 {
                if let Some(&at) = by_full.get(&full) {
                    return at;
                }
                let mut key = ids::hashed(&full);
                let base = key.clone();
                let mut n = 1;
                while taken.get(&key).is_some_and(|other| *other != full) {
                    n += 1;
                    key = format!("{base}~{n}");
                }
                taken.insert(key.clone(), full.clone());
                let at = functions.len() as u32;
                functions.push(Function {
                    id: ids::function(&key),
                    name: names::short(&full),
                    full: full.clone(),
                    file,
                    line: (line > 0).then_some(line),
                    crate_name,
                });
                by_full.insert(full, at);
                at
            };
        let unknown = intern(UNKNOWN.to_string(), None, 0, None);
        let mapped: Vec<u32> = rows
            .name
            .iter()
            .enumerate()
            .map(|(i, &name)| {
                let file = rows
                    .file
                    .get(i)
                    .filter(|&&f| f != 0)
                    .map(|&f| base_name(&text(f)));
                let line = rows.line.get(i).copied().unwrap_or(0);
                let crate_name = rows
                    .crate_
                    .get(i)
                    .filter(|&&c| c != 0)
                    .map(|&c| text(c).into_owned());
                intern(names::normalize(&text(name)), file, line, crate_name)
            })
            .collect();

        let mods = view
            .capture
            .start
            .iter()
            .find(|s| s.side() == side)
            .map_or(&[][..], |s| s.mods.as_slice());
        let native: Vec<_> = mods.iter().filter(|m| !m.crate_name.is_empty()).collect();
        let mod_ids: Vec<String> = native.iter().map(|m| m.id.clone()).collect();
        // Cargo names a crate `my-mod`; its symbols say `my_mod`.
        let owning: HashMap<String, u32> = native
            .iter()
            .enumerate()
            .map(|(at, m)| (m.crate_name.replace('-', "_"), at as u32))
            .collect();

        let thread_names: Vec<String> = profile
            .threads
            .iter()
            .map(|t| text(t.name).into_owned())
            .collect();
        let nodes: &Nodes = profile.nodes.as_ref().unwrap_or(&no_nodes);
        let count = nodes.function.len();
        let (mut func, mut parent, mut thread_of) = (
            Vec::with_capacity(count),
            Vec::with_capacity(count),
            Vec::with_capacity(count),
        );
        let mut path: Vec<Fnv> = Vec::with_capacity(count);
        let mut owner: Vec<Option<u32>> = Vec::with_capacity(count);
        for i in 0..count {
            let f = mapped
                .get(nodes.function[i] as usize)
                .copied()
                .unwrap_or(unknown);
            let root = nodes.thread_root.get(i).copied().unwrap_or(0);
            let up = nodes
                .parent
                .get(i)
                .copied()
                .filter(|&p| root == 0 && (p as usize) < i);
            let (thread, mut hash) = match up {
                Some(p) => (thread_of[p as usize], path[p as usize]),
                None => {
                    let thread = root.saturating_sub(1);
                    let mut hash = Fnv::default();
                    hash.write(thread_names.get(thread as usize).map_or("", String::as_str));
                    hash.write("\n");
                    (thread, hash)
                }
            };
            hash.write(&functions[f as usize].full);
            hash.write("\n");
            func.push(f);
            let own = functions[f as usize]
                .crate_name
                .as_deref()
                .and_then(|c| owning.get(c).copied());
            owner.push(own.or_else(|| up.and_then(|p| owner[p as usize])));
            parent.push(up.unwrap_or(NO_NODE));
            thread_of.push(thread);
            path.push(hash);
        }

        let table: &Contexts = profile.contexts.as_ref().unwrap_or(&no_contexts);
        let name = |ids: &[u32], i: usize| -> Option<String> {
            ids.get(i)
                .filter(|&&id| id != 0)
                .map(|&id| text(id).into_owned())
        };
        let contexts = (0..table.span.len())
            .map(|i| {
                let at = (
                    table.chunk_x.get(i).copied().unwrap_or(0),
                    table.chunk_z.get(i).copied().unwrap_or(0),
                );
                Context {
                    span: name(&table.span, i),
                    entity: name(&table.entity, i),
                    block: name(&table.block, i),
                    dim: name(&table.dim, i),
                    chunk: (at != (0, 0)).then_some(at),
                }
            })
            .collect();
        Some(Self {
            profile,
            side,
            functions,
            func,
            parent,
            thread_of,
            owner,
            mod_ids,
            path,
            contexts,
            thread_names,
        })
    }

    /// The requested thread by name, else the side's main thread, else the only one sampled.
    pub fn thread(&self, requested: Option<&str>) -> Result<u32, String> {
        let wanted = requested.unwrap_or(View::main_thread(self.side));
        if let Some(at) = self.thread_names.iter().position(|n| n == wanted) {
            return Ok(at as u32);
        }
        match (requested, self.thread_names.len()) {
            (None, 1) => Ok(0),
            (_, 0) => Err("the samples hold no threads".to_string()),
            _ => Err(format!(
                "no sampled thread named {wanted}; sampled threads: {}",
                self.thread_names.join(", ")
            )),
        }
    }

    pub fn function_of(&self, node: u32) -> &Function {
        &self.functions[self.func[node as usize] as usize]
    }

    /// The `p:` id of a node: its thread and normalized frames, root to node.
    pub fn path_id(&self, node: u32) -> String {
        ids::path(&self.path[node as usize].key())
    }

    pub fn has_kind(&self, select: Select) -> bool {
        let kinds = &self.profile.kinds;
        match select {
            Select::Cpu => kinds.iter().any(|k| k.source() == SampleSource::Cpu),
            Select::Running | Select::Waiting | Select::Wall => {
                kinds.iter().any(|k| k.source() == SampleSource::Wall)
            }
            Select::Alloc => kinds.iter().any(|k| k.source() == SampleSource::Alloc),
            Select::Live => false,
        }
    }

    fn takes(&self, select: Select, kind: u32, state: StateName) -> bool {
        let Some(k) = self.profile.kinds.get(kind as usize) else {
            return false;
        };
        match (select, k.source()) {
            (Select::Cpu, SampleSource::Cpu) => state == StateName::Cpu,
            (Select::Running, SampleSource::Wall) => state == StateName::Running,
            (Select::Waiting, SampleSource::Wall) => state == StateName::Waiting,
            (Select::Wall, SampleSource::Wall) => state != StateName::Cpu,
            (Select::Alloc, SampleSource::Alloc) => true,
            _ => false,
        }
    }

    /// The contexts that satisfy every key given; a key's values are alternatives.
    pub fn context_filter(&self, wanted: &BTreeMap<ContextKey, Vec<String>>) -> Option<Vec<bool>> {
        if wanted.is_empty() {
            return None;
        }
        Some(
            self.contexts
                .iter()
                .map(|c| {
                    wanted
                        .iter()
                        .all(|(key, values)| values.iter().any(|v| c.matches(*key, v)))
                })
                .collect(),
        )
    }

    fn block_s(&self) -> f64 {
        f64::from(self.profile.block_s)
    }

    /// Blocks that start inside the window count, as in the span tables.
    fn in_window(&self, window: Window, block: u32) -> bool {
        let t0 = f64::from(block) * self.block_s();
        t0 >= window.from && t0 < window.to
    }

    /// The weights rows that pass `pick`.
    pub fn hits<'s>(&'s self, pick: &'s Pick) -> impl Iterator<Item = Hit> + 's {
        let w = self.profile.weights.as_ref();
        let len = w.map_or(0, |w| w.node.len());
        (0..len).filter_map(move |i| {
            let w = w?;
            let get = |v: &[u32]| v.get(i).copied();
            let (node, thread, kind, state) =
                (w.node[i], get(&w.thread)?, get(&w.kind)?, get(&w.state)?);
            let state = StateName::from_weight(state)?;
            let (context, block) = (get(&w.context)?, get(&w.block)?);
            let slow = w.slow.get(i).copied().unwrap_or(false);
            let count = w.count.get(i).copied()?;
            let ok = pick.thread.is_none_or(|t| t == thread)
                && (!pick.slow || slow)
                && self.takes(pick.select, kind, state)
                && self.in_window(pick.window, block)
                && pick
                    .contexts
                    .as_ref()
                    .is_none_or(|c| c.get(context as usize).copied().unwrap_or(false))
                && (node as usize) < self.func.len();
            ok.then(|| Hit {
                node,
                thread,
                context,
                state,
                slow,
                value: count as f64 * self.profile.kinds[kind as usize].period,
                count,
            })
        })
    }

    /// Ticks or frames of `thread` inside the pick's window; slow ones are scaled by the window's share when only totals are known.
    pub fn units(&self, thread: u32, slow: bool, window: Window) -> Units {
        let Some(t) = self.profile.threads.get(thread as usize) else {
            return Units {
                count: 0,
                estimated: false,
            };
        };
        let whole = if slow { t.slow_units } else { t.units };
        if window == Window::ALL {
            return Units {
                count: whole,
                estimated: false,
            };
        }
        let blocks = self.profile.block_units.iter().find(|b| b.thread == thread);
        let Some(blocks) = blocks else {
            return Units {
                count: whole,
                estimated: true,
            };
        };
        let inside: u64 = blocks
            .units
            .iter()
            .enumerate()
            .filter(|(b, _)| self.in_window(window, *b as u32))
            .map(|(_, &u)| u64::from(u))
            .sum();
        if !slow {
            return Units {
                count: inside,
                estimated: false,
            };
        }
        let all: u64 = blocks.units.iter().map(|&u| u64::from(u)).sum();
        let share = if all == 0 {
            0.0
        } else {
            inside as f64 / all as f64
        };
        Units {
            count: (whole as f64 * share).round() as u64,
            estimated: true,
        }
    }

    pub fn tick_unit(&self, thread: u32) -> TickUnit {
        self.profile
            .threads
            .get(thread as usize)
            .map_or(TickUnit::Tick, |t| t.unit())
    }

    pub fn tally(&self, pick: &Pick) -> Tally {
        let n = self.func.len();
        let mut t = Tally {
            self_ms: vec![0.0; n],
            self_n: vec![0; n],
            total_ms: vec![0.0; n],
            total_n: vec![0; n],
            grand_ms: 0.0,
            grand_n: 0,
        };
        for hit in self.hits(pick) {
            let at = hit.node as usize;
            t.self_ms[at] += hit.value;
            t.self_n[at] += hit.count;
            t.grand_ms += hit.value;
            t.grand_n += hit.count;
        }
        t.total_ms.clone_from(&t.self_ms);
        t.total_n.clone_from(&t.self_n);
        for at in (0..n).rev() {
            if self.parent[at] != NO_NODE {
                let up = self.parent[at] as usize;
                t.total_ms[up] += t.total_ms[at];
                t.total_n[up] += t.total_n[at];
            }
        }
        t
    }

    /// Whether an ancestor of `node` runs the same function, so counting `node` too would count a recursion twice.
    pub fn shadowed(&self, node: u32) -> bool {
        let f = self.func[node as usize];
        let mut up = self.parent[node as usize];
        while up != NO_NODE {
            if self.func[up as usize] == f {
                return true;
            }
            up = self.parent[up as usize];
        }
        false
    }

    /// The outermost nodes of a function, restricted to `thread` when given.
    pub fn occurrences(&self, function: u32, thread: Option<u32>) -> Vec<u32> {
        (0..self.func.len() as u32)
            .filter(|&n| self.func[n as usize] == function)
            .filter(|&n| thread.is_none_or(|t| self.thread_of[n as usize] == t))
            .filter(|&n| !self.shadowed(n))
            .collect()
    }

    /// Per node: whether the node or an ancestor satisfies `is`.
    pub fn under(&self, is: impl Fn(u32) -> bool) -> Vec<bool> {
        let mut inside = vec![false; self.func.len()];
        for n in 0..self.func.len() {
            let up = self.parent[n];
            inside[n] = is(n as u32) || (up != NO_NODE && inside[up as usize]);
        }
        inside
    }

    pub fn function_index(&self, id: &str) -> Option<u32> {
        self.functions
            .iter()
            .position(|f| f.id == id)
            .map(|i| i as u32)
    }
}

/// Per function: own and inclusive weight with sample counts, and the pick's whole weight.
pub struct FnTimes {
    pub own: Vec<(f64, u64)>,
    pub inclusive: Vec<(f64, u64)>,
    pub grand_ms: f64,
    pub grand_n: u64,
}

/// One value of a context and the weight that carried it; a None key is the samples that had none.
pub struct Share {
    pub key: Option<String>,
    pub label: String,
    pub value: f64,
    pub count: u64,
}

impl Samples<'_> {
    /// Each function's own time, and its inclusive time with a recursion counted once.
    pub fn function_times(&self, pick: &Pick) -> FnTimes {
        let t = self.tally(pick);
        let mut times = FnTimes {
            own: vec![(0.0, 0); self.functions.len()],
            inclusive: vec![(0.0, 0); self.functions.len()],
            grand_ms: t.grand_ms,
            grand_n: t.grand_n,
        };
        for node in 0..self.func.len() {
            let at = self.func[node] as usize;
            times.own[at].0 += t.self_ms[node];
            times.own[at].1 += t.self_n[node];
            if t.total_n[node] > 0 && !self.shadowed(node as u32) {
                times.inclusive[at].0 += t.total_ms[node];
                times.inclusive[at].1 += t.total_n[node];
            }
        }
        times
    }

    /// The pick's weight divided by one context, biggest first; `inside` limits it to the weight of some nodes.
    pub fn divide(&self, pick: &Pick, by: SampleBy, inside: Option<&[bool]>) -> Vec<Share> {
        let mut buckets: BTreeMap<Option<String>, Share> = BTreeMap::new();
        for hit in self.hits(pick) {
            if inside.is_some_and(|nodes| !nodes[hit.node as usize]) {
                continue;
            }
            let (key, label) = self.bucket(&hit, by);
            let share = buckets.entry(key.clone()).or_insert(Share {
                key,
                label,
                value: 0.0,
                count: 0,
            });
            share.value += hit.value;
            share.count += hit.count;
        }
        let mut shares: Vec<Share> = buckets.into_values().collect();
        shares.sort_by(|a, b| b.value.total_cmp(&a.value).then_with(|| a.key.cmp(&b.key)));
        shares
    }

    fn bucket(&self, hit: &Hit, by: SampleBy) -> (Option<String>, String) {
        let ctx = &self.contexts[hit.context as usize];
        let named = |kind: Kind, text: &Option<String>| {
            (
                text.as_ref().map(|t| Id::new(kind, t.as_str()).to_string()),
                text.clone().unwrap_or_else(|| "(none)".to_string()),
            )
        };
        let word = |text: String| (Some(text.clone()), text);
        match by {
            SampleBy::Span => named(Kind::Span, &ctx.span),
            SampleBy::Entity => named(Kind::Entity, &ctx.entity),
            SampleBy::Block => named(Kind::Block, &ctx.block),
            SampleBy::Dim => named(Kind::Dimension, &ctx.dim),
            SampleBy::Chunk => {
                let (key, label) = named(Kind::Chunk, &ctx.chunk_text());
                (key, label)
            }
            SampleBy::Thread => named(
                Kind::Thread,
                &Some(self.thread_names[hit.thread as usize].clone()),
            ),
            SampleBy::Mod => match self.owner[hit.node as usize] {
                Some(at) => word(self.mod_ids[at as usize].clone()),
                None => (None, "(none)".to_string()),
            },
            SampleBy::State => word(hit.state.to_string()),
            SampleBy::Slow => word(
                if hit.slow {
                    PaceName::Slow
                } else {
                    PaceName::Normal
                }
                .to_string(),
            ),
        }
    }
}

/// The unit a reply over `select` is measured in.
pub fn out_unit(select: Select, slow: bool, ticks: TickUnit) -> Unit {
    let frame = ticks == TickUnit::Frame;
    match (select == Select::Alloc, slow, frame) {
        (false, false, false) => Unit::MsPerTick,
        (false, false, true) => Unit::MsPerFrame,
        (false, true, false) => Unit::MsPerSlowTick,
        (false, true, true) => Unit::MsPerSlowFrame,
        (true, false, false) => Unit::BytesPerTick,
        (true, false, true) => Unit::BytesPerFrame,
        (true, true, false) => Unit::BytesPerSlowTick,
        (true, true, true) => Unit::BytesPerSlowFrame,
    }
}
