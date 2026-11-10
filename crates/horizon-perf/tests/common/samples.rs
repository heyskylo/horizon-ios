//! Captures with a hand-made call tree, so a test can add the weights up itself.

use std::collections::HashMap;

use horizon_perf_format::Field;
use horizon_perf_format::v1::*;

use super::{CLIENT, SERVER, start, stop};

/// One counted-sample row's extras; the defaults are a CPU sample with no context in block 0.
#[derive(Clone, Copy, Default)]
pub struct W {
    pub ctx: u32,
    pub state: u32,
    pub slow: bool,
    pub block: u32,
    pub thread: u32,
}

pub struct Builder {
    strings: Vec<String>,
    funcs: Functions,
    func_ids: HashMap<String, u32>,
    nodes: Nodes,
    contexts: Contexts,
    weights: Weights,
    pub side: i32,
    pub thread: String,
    pub period_ms: f64,
    pub ticks: u64,
    pub slow_ticks: u64,
    pub block_units: Vec<u32>,
    pub wall: bool,
    pub alloc: bool,
    /// Threads after the first: name, ticks and slow ticks.
    pub extra_threads: Vec<(String, u64, u64)>,
}

impl Builder {
    pub fn new(ticks: u64, block_units: &[u32]) -> Self {
        let mut b = Self {
            strings: Vec::new(),
            funcs: Functions::default(),
            func_ids: HashMap::new(),
            nodes: Nodes::default(),
            contexts: Contexts::default(),
            weights: Weights::default(),
            side: SERVER,
            thread: "tick".into(),
            period_ms: 10.0,
            ticks,
            slow_ticks: 5,
            block_units: block_units.to_vec(),
            wall: false,
            alloc: false,
            extra_threads: Vec::new(),
        };
        b.ctx("", "", (0, 0), "");
        b
    }

    pub fn text(&mut self, text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }
        match self.strings.iter().position(|s| s == text) {
            Some(at) => at as u32 + 1,
            None => {
                self.strings.push(text.to_string());
                self.strings.len() as u32
            }
        }
    }

    /// A function row by its raw name; the same raw name is one row.
    pub fn func(&mut self, name: &str) -> u32 {
        if let Some(&at) = self.func_ids.get(name) {
            return at;
        }
        let (n, f) = (self.text(name), self.text("file.rs"));
        let at = self.funcs.name.len() as u32;
        self.funcs.name.push(n);
        self.funcs.file.push(f);
        self.funcs.line.push(10 + at);
        self.funcs.crate_.push(0);
        self.funcs.addr.push(0x1000 + u64::from(at));
        self.func_ids.insert(name.to_string(), at);
        at
    }

    /// A function that belongs to a crate.
    pub fn func_in(&mut self, name: &str, krate: &str) -> u32 {
        let at = self.func(name);
        self.funcs.crate_[at as usize] = self.text(krate);
        at
    }

    /// A new call node; a root belongs to the sampled thread.
    pub fn node(&mut self, parent: Option<u32>, name: &str) -> u32 {
        let f = self.func(name);
        self.nodes.parent.push(parent.unwrap_or(0));
        self.nodes.function.push(f);
        self.nodes.thread_root.push(u32::from(parent.is_none()));
        self.nodes.parent.len() as u32 - 1
    }

    /// A root call node of sampled thread `thread` (0 is the first thread).
    pub fn root_on(&mut self, thread: u32, name: &str) -> u32 {
        let at = self.node(None, name);
        self.nodes.thread_root[at as usize] = thread + 1;
        at
    }

    /// A context row: entity, dimension, chunk and span (empty text means none).
    pub fn ctx(&mut self, entity: &str, dim: &str, chunk: (i32, i32), span: &str) -> u32 {
        let (span, entity, dim) = (self.text(span), self.text(entity), self.text(dim));
        self.contexts.span.push(span);
        self.contexts.entity.push(entity);
        self.contexts.block.push(0);
        self.contexts.dim.push(dim);
        self.contexts.chunk_x.push(chunk.0);
        self.contexts.chunk_z.push(chunk.1);
        self.contexts.span.len() as u32 - 1
    }

    pub fn weight(&mut self, node: u32, count: u64, w: W) {
        let kind = u32::from(w.state != 0);
        self.wall |= w.state != 0;
        self.weights.node.push(node);
        self.weights.thread.push(w.thread);
        self.weights.kind.push(kind);
        self.weights.state.push(w.state);
        self.weights.slow.push(w.slow);
        self.weights.context.push(w.ctx);
        self.weights.block.push(w.block);
        self.weights.count.push(count);
    }

    /// An allocation sample row (kind 2 after cpu and wall), weighing `count * period` bytes.
    pub fn alloc_weight(&mut self, node: u32, count: u64) {
        self.alloc = true;
        self.weights.node.push(node);
        self.weights.thread.push(0);
        self.weights.kind.push(u32::MAX);
        self.weights.state.push(0);
        self.weights.slow.push(false);
        self.weights.context.push(0);
        self.weights.block.push(0);
        self.weights.count.push(count);
    }

    pub fn profile(mut self) -> (SampleProfile, Vec<String>) {
        let thread = self.thread.clone();
        let name = self.text(&thread);
        let mut threads = vec![(name, self.ticks, self.slow_ticks)];
        for (n, units, slow) in std::mem::take(&mut self.extra_threads) {
            threads.push((self.text(&n), units, slow));
        }
        let kind = |source: SampleSource, unit: SampleUnit, period: f64| SampleKind {
            source: source as i32,
            unit: unit as i32,
            period,
        };
        let mut kinds = vec![kind(SampleSource::Cpu, SampleUnit::Ms, self.period_ms)];
        if self.wall {
            kinds.push(kind(SampleSource::Wall, SampleUnit::Ms, self.period_ms));
        }
        if self.alloc {
            let at = kinds.len() as u32;
            kinds.push(kind(SampleSource::Alloc, SampleUnit::Bytes, 64.0));
            for k in &mut self.weights.kind {
                if *k == u32::MAX {
                    *k = at;
                }
            }
        }
        let profile = SampleProfile {
            side: self.side,
            kinds,
            threads: threads
                .into_iter()
                .map(|(name, units, slow_units)| SampledThread {
                    name,
                    units,
                    slow_units,
                    unit: Unit::Tick as i32,
                })
                .collect(),
            block_s: 5.0,
            block_units: vec![BlockUnits {
                thread: 0,
                units: self.block_units.clone(),
            }],
            functions: Some(self.funcs),
            nodes: Some(self.nodes),
            contexts: Some(self.contexts),
            weights: Some(self.weights),
            slow_over_ms: 50.0,
        };
        (profile, self.strings)
    }

    /// The string table and sample sections of this side.
    pub fn sections(self) -> Vec<Field> {
        let side = self.side;
        let (profile, strings) = self.profile();
        let mut fields: Vec<Field> = strings
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                Field::String(StringEntry {
                    side,
                    id: i as u32 + 1,
                    text,
                })
            })
            .collect();
        fields.push(Field::Samples(profile));
        fields
    }

    /// A finished capture of one side holding these samples.
    pub fn fields(self) -> Vec<Field> {
        let side = self.side;
        let kind = if side == CLIENT {
            Kind::Client
        } else {
            Kind::Dedicated
        };
        let mut fields = vec![start(side, kind)];
        fields.extend(self.sections());
        fields.push(stop(side, 10.0));
        fields
    }

    /// A context row like `ctx` that also names a block entity.
    pub fn block_ctx(&mut self, block: &str, dim: &str, span: &str) -> u32 {
        let at = self.ctx("", dim, (0, 0), span);
        let block = self.text(block);
        self.contexts.block[at as usize] = block;
        at
    }
}

pub const MAIN: &str = "horizon_client_lib::run::main";
pub const SERVER_TICK: &str = "horizon_client::game::Game::tick_server";
pub const LAN: &str = "horizon_client::game::Game::tick_lan";
pub const BLOCKS: &str = "horizon_client::game::Game::tick_blocks";
pub const LEVEL: &str = "horizon_client_lib::lan::waypoints::Waypoints::tick_level";
pub const UPDATE: &str = "horizon_client_lib::lan::waypoints::Waypoints::update_or_create";
pub const TEAM: &str = "horizon_client_lib::lan::waypoints::waypoint_team";
pub const TRACK: &str = "horizon_client_lib::lan::hub::Hub::track_entities";

/// The ids of the functions above, hashed outside the code under test (FNV-1a 64, low 40 bits, base32).
pub const LEVEL_ID: &str = "f:yg3ymj0d";
pub const UPDATE_ID: &str = "f:57zvmm9s";
pub const TEAM_ID: &str = "f:8m7tt1dc";
pub const TRACK_ID: &str = "f:bvz9skmm";
pub const SERVER_TICK_ID: &str = "f:y7z04p5v";
pub const LAN_ID: &str = "f:ja26a1wk";
pub const BLOCKS_ID: &str = "f:1wcp3vqp";
pub const MAIN_ID: &str = "f:tmkfe9bp";

/// A call tree of 120 samples of 10 ms times `scale`, over `ticks` ticks in two blocks.
pub fn waypoints(scale: u64, ticks: u64) -> Builder {
    waypoints_in(
        Builder::new(ticks, &[(ticks / 2) as u32, (ticks - ticks / 2) as u32]),
        scale,
    )
}

/// The same tree in a builder that may already hold strings and functions.
pub fn waypoints_in(mut b: Builder, scale: u64) -> Builder {
    let zombie = b.ctx("zombie", "overworld", (12, -4), "tick_lan");
    let item = b.ctx("item", "overworld", (13, -4), "tick_lan");
    let nether = b.ctx("zombie", "the_nether", (-3, 7), "tick_blocks");
    let main = b.node(None, MAIN);
    let server = b.node(Some(main), SERVER_TICK);
    let lan = b.node(Some(server), LAN);
    let level = b.node(Some(lan), LEVEL);
    let update = b.node(Some(level), UPDATE);
    let team = b.node(Some(level), TEAM);
    let track = b.node(Some(lan), TRACK);
    let blocks = b.node(Some(server), BLOCKS);
    let level2 = b.node(Some(blocks), LEVEL);
    let update2 = b.node(Some(level2), UPDATE);
    let at = |ctx, block| W {
        ctx,
        block,
        ..W::default()
    };
    b.weight(server, 10 * scale, at(0, 0));
    b.weight(level, 25 * scale, at(zombie, 0));
    b.weight(level, 15 * scale, at(item, 1));
    b.weight(
        update,
        10 * scale,
        W {
            slow: true,
            ..at(zombie, 0)
        },
    );
    b.weight(update, 10 * scale, at(zombie, 0));
    b.weight(update, 10 * scale, at(nether, 1));
    b.weight(team, 10 * scale, at(item, 0));
    b.weight(track, 20 * scale, at(0, 0));
    b.weight(level2, 5 * scale, at(zombie, 1));
    b.weight(update2, 5 * scale, at(nether, 1));
    let wall = |state| W {
        ctx: zombie,
        state,
        ..W::default()
    };
    b.weight(level, 12 * scale, wall(1));
    b.weight(level, 8 * scale, wall(2));
    b
}

/// The standard sampled tree plus two ticks ending at 3 s: k:7 slow (200 ms) and k:8 fast (20 ms).
pub fn waypoints_with_ticks() -> Vec<Field> {
    let mut fields = waypoints(1, 50).fields();
    let stop = fields.pop().unwrap();
    for (counter, wall_ms) in [(7, 200.0), (8, 20.0)] {
        fields.push(Field::Slow(SlowEvent {
            side: SERVER,
            t: 3.0,
            unit: Unit::Tick as i32,
            counter,
            wall_ms,
            cpu_ms: wall_ms,
            top: Vec::new(),
        }));
    }
    fields.push(stop);
    fields
}
