//! Folds sample records into a bounded call-stack trie and counts per context key; ticks decide which samples are slow.

use std::collections::HashMap;

use crate::sampler::{Context, Kind, Record};

/// Call-stack trie nodes kept; a sample needing more is attributed to the deepest node that exists.
pub const MAX_NODES: usize = 1 << 17;
/// Distinct (stack, context) counters kept; a sample needing a new one is counted as lost.
pub const MAX_KEYS: usize = 1 << 18;
const PENDING_MAX: usize = 4096;
const BUCKETS: usize = 96;
const WORST: usize = 10;
const PSEUDO: u64 = 1 << 63;
/// Time blocks a session is cut into; later samples fall in the last one.
pub const MAX_TIME_BLOCKS: usize = 32;

/// What a sample measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Cpu,
    /// A wall sample of a thread that was running.
    Running,
    /// A wall sample of a thread that was not.
    Waiting,
    /// Sampled heap bytes allocated during the session.
    Alloc,
    /// Sampled heap bytes still allocated when the session stopped.
    Live,
}

/// One counter's identity: the stack and the context fields the report groups by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub node: u32,
    pub state: State,
    pub slow: bool,
    pub time_block: u8,
    pub span: u16,
    pub entity: u16,
    pub block: u16,
    pub dim: u8,
    pub chunk: (i32, i32),
}

/// A trie node's frame: a lookup address, or a thread / wall-state marker (high bit set).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frame {
    Addr(u64),
    Thread(u16),
    Wall(State),
}

impl Frame {
    fn code(self) -> u64 {
        match self {
            Frame::Addr(a) => a & !PSEUDO,
            Frame::Thread(i) => PSEUDO | u64::from(i),
            Frame::Wall(s) => PSEUDO | (1 << 32) | s as u64,
        }
    }
}

/// Tick or frame durations of one thread, in log-spaced buckets (a quarter octave from 10 us).
#[derive(Clone, Debug)]
pub struct TickStats {
    pub count: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
    pub buckets: [u32; BUCKETS],
    pub slow: u64,
    /// The longest ticks, longest first: (tick number, nanoseconds).
    pub worst: Vec<(u64, u64)>,
}

impl Default for TickStats {
    fn default() -> Self {
        Self {
            count: 0,
            sum_ns: 0,
            max_ns: 0,
            buckets: [0; BUCKETS],
            slow: 0,
            worst: Vec::new(),
        }
    }
}

/// Lower bound of bucket `i` in nanoseconds.
pub fn bucket_floor_ns(i: usize) -> f64 {
    10_000.0 * 2f64.powf(i as f64 / 4.0)
}

impl TickStats {
    fn add(&mut self, tick: u64, ns: u64) {
        let i = ((ns.max(1) as f64 / 10_000.0).log2() * 4.0)
            .floor()
            .max(0.0) as usize;
        self.buckets[i.min(BUCKETS - 1)] += 1;
        self.count += 1;
        self.sum_ns += ns;
        self.max_ns = self.max_ns.max(ns);
        if self.worst.len() < WORST || ns > self.worst[WORST - 1].1 {
            self.worst.push((tick, ns));
            self.worst.sort_by_key(|&(_, ns)| std::cmp::Reverse(ns));
            self.worst.truncate(WORST);
        }
    }

    /// The `q` quantile (0..1) as the upper edge of its bucket, in nanoseconds.
    pub fn quantile_ns(&self, q: f64) -> f64 {
        let want = (self.count as f64 * q).ceil().max(1.0) as u64;
        let mut seen = 0;
        for (i, &n) in self.buckets.iter().enumerate() {
            seen += u64::from(n);
            if seen >= want {
                return bucket_floor_ns(i + 1).min(self.max_ns as f64);
            }
        }
        self.max_ns as f64
    }
}

#[derive(Clone, Default)]
pub struct Pending {
    samples: Vec<(Key, u64)>,
}

/// Everything a session recorded, bounded: see `MAX_NODES` and `MAX_KEYS`.
#[derive(Clone)]
pub struct Aggregate {
    parents: Vec<u32>,
    frames: Vec<Frame>,
    children: HashMap<(u32, u64), u32>,
    pub counts: HashMap<Key, u64>,
    pub threads: Vec<String>,
    pub ticks: HashMap<u16, TickStats>,
    /// Ticks or frames that ended in each time block, per thread.
    pub block_ticks: HashMap<u16, Vec<u32>>,
    time_block: u8,
    pending: HashMap<u16, Pending>,
    slow_ns: u64,
    /// Samples whose stack was cut short at `MAX_NODES`.
    pub truncated: u64,
    /// Samples dropped at `MAX_KEYS`.
    pub lost: u64,
    pub cpu_samples: u64,
    pub wall_samples: u64,
    pub tick_markers: u64,
}

impl Aggregate {
    /// `slow_ns` is the duration from which a tick's samples count as slow.
    pub fn new(slow_ns: u64) -> Self {
        Self {
            parents: vec![0],
            frames: vec![Frame::Addr(0)],
            children: HashMap::new(),
            counts: HashMap::new(),
            threads: Vec::new(),
            ticks: HashMap::new(),
            block_ticks: HashMap::new(),
            time_block: 0,
            pending: HashMap::new(),
            slow_ns,
            truncated: 0,
            lost: 0,
            cpu_samples: 0,
            wall_samples: 0,
            tick_markers: 0,
        }
    }

    /// Makes `block` (clamped to the last one) the time block of everything added from now on.
    pub fn set_time_block(&mut self, block: usize) {
        self.time_block = block.min(MAX_TIME_BLOCKS - 1) as u8;
    }

    /// Time blocks that received a sample or a tick.
    pub fn time_blocks(&self) -> usize {
        let counted = self.counts.keys().map(|k| k.time_block as usize + 1);
        let ticked = self.block_ticks.values().map(Vec::len);
        counted.chain(ticked).max().unwrap_or(0)
    }

    pub fn nodes(&self) -> usize {
        self.parents.len()
    }

    pub fn frame(&self, node: u32) -> Frame {
        self.frames[node as usize]
    }

    pub fn parent(&self, node: u32) -> u32 {
        self.parents[node as usize]
    }

    /// Nodes from the root's child down to `node`.
    pub fn path(&self, node: u32) -> Vec<u32> {
        let mut path = Vec::new();
        let mut at = node;
        while at != 0 {
            path.push(at);
            at = self.parents[at as usize];
        }
        path.reverse();
        path
    }

    pub fn thread_index(&mut self, name: &str) -> u16 {
        let found = self.threads.iter().position(|t| t == name);
        found.unwrap_or_else(|| {
            self.threads.push(name.to_string());
            self.threads.len() - 1
        }) as u16
    }

    fn child(&mut self, parent: u32, frame: Frame) -> Option<u32> {
        let code = (parent, frame.code());
        if let Some(&node) = self.children.get(&code) {
            return Some(node);
        }
        if self.parents.len() >= MAX_NODES {
            return None;
        }
        let node = self.parents.len() as u32;
        self.parents.push(parent);
        self.frames.push(frame);
        self.children.insert(code, node);
        Some(node)
    }

    /// Adds one drained record of the thread `name`.
    pub fn add(&mut self, name: &str, record: &Record) {
        let thread = self.thread_index(name);
        match record.kind {
            Kind::Tick => {
                self.tick_markers += 1;
                let (tick, ns) = (record.ctx.tick, record.value);
                let stats = self.ticks.entry(thread).or_default();
                stats.add(tick, ns);
                let slow = ns >= self.slow_ns;
                stats.slow += u64::from(slow);
                let ticked = self.block_ticks.entry(thread).or_default();
                let block = self.time_block as usize;
                ticked.resize(ticked.len().max(block + 1), 0);
                ticked[block] += 1;
                for (mut key, at) in self.pending.remove(&thread).unwrap_or_default().samples {
                    key.slow = slow && at == tick;
                    self.count(key);
                }
            }
            Kind::Cpu | Kind::Wall => {
                match record.kind {
                    Kind::Cpu => self.cpu_samples += 1,
                    _ => self.wall_samples += 1,
                }
                let state = match (record.kind, record.value) {
                    (Kind::Cpu, _) => State::Cpu,
                    (_, 1) => State::Running,
                    _ => State::Waiting,
                };
                let key = self.key(thread, state, &record.ctx, record.stack());
                let pending = self.pending.entry(thread).or_default();
                pending.samples.push((key, record.ctx.tick));
                if pending.samples.len() >= PENDING_MAX {
                    self.flush(thread);
                }
            }
        }
    }

    fn key(&mut self, thread: u16, state: State, ctx: &Context, stack: &[usize]) -> Key {
        let mut node = self.child(0, Frame::Thread(thread)).unwrap_or(0);
        if state != State::Cpu {
            node = self.child(node, Frame::Wall(state)).unwrap_or(node);
        }
        let node = self.stack_from(node, stack);
        Key {
            node,
            state,
            slow: false,
            time_block: self.time_block,
            span: ctx.span,
            entity: ctx.entity_type,
            block: ctx.block,
            dim: ctx.dim,
            chunk: ctx.chunk,
        }
    }

    /// The node of `stack` (innermost first) under the thread `thread`'s root, in the current time block.
    pub fn stack_key(&mut self, thread: u16, state: State, ctx: &Context, stack: &[usize]) -> Key {
        let root = self.child(0, Frame::Thread(thread)).unwrap_or(0);
        Key {
            node: self.stack_from(root, stack),
            state,
            slow: false,
            time_block: self.time_block,
            span: ctx.span,
            entity: ctx.entity_type,
            block: ctx.block,
            dim: ctx.dim,
            chunk: ctx.chunk,
        }
    }

    fn stack_from(&mut self, mut node: u32, stack: &[usize]) -> u32 {
        let mut cut = false;
        // Return addresses point after the call; one byte back stays inside the caller's call instruction.
        for (i, &addr) in stack.iter().enumerate().rev() {
            let lookup = if i == 0 { addr } else { addr - 1 } as u64;
            match self.child(node, Frame::Addr(lookup)) {
                Some(next) => node = next,
                None => {
                    cut = true;
                    break;
                }
            }
        }
        self.truncated += u64::from(cut);
        node
    }

    fn count(&mut self, key: Key) {
        self.add_weight(key, 1);
    }

    /// Adds `n` to `key`'s counter; false when the key was lost at `MAX_KEYS`.
    pub fn add_weight(&mut self, key: Key, n: u64) -> bool {
        if self.counts.len() >= MAX_KEYS && !self.counts.contains_key(&key) {
            self.lost += 1;
            false
        } else {
            *self.counts.entry(key).or_default() += n;
            true
        }
    }

    fn flush(&mut self, thread: u16) {
        for (key, _) in self.pending.remove(&thread).unwrap_or_default().samples {
            self.count(key);
        }
    }

    /// Counts the samples still waiting for their tick's marker as not slow.
    pub fn finish(&mut self) {
        let threads: Vec<u16> = self.pending.keys().copied().collect();
        threads.into_iter().for_each(|t| self.flush(t));
    }

    /// Samples of one state (and, for `slow`, of slow ticks only).
    pub fn total(&self, state: State, slow_only: bool) -> u64 {
        self.counts
            .iter()
            .filter(|(k, _)| k.state == state && (!slow_only || k.slow))
            .map(|(_, n)| n)
            .sum()
    }
}
