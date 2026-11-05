//! What each kind of mob costs the server tick, for `horizon-report` (dedicated/report.rs): while
//! a report records, one tick in [`SAMPLE_EVERY`] times every mob's own tick and adds it up by
//! level and mob type. Off, a mob costs one thread-local read; sampled, two clock reads and a
//! map update. A mob's riders tick inside its own tick, so they count for it.
use horizon_client::{fast_hash::FxHashMap, mobs::MobType};
use std::{
    cell::{Cell, RefCell},
    time::Instant,
};

/// One server tick in this many is sampled while a report records.
pub(super) const SAMPLE_EVERY: usize = 4;

/// The time and the ticks of one mob type in one level.
struct Cost {
    kind: String,
    nanos: u64,
    mob_ticks: u64,
}

thread_local! {
    static SAMPLING: Cell<bool> = const { Cell::new(false) };
    static COSTS: RefCell<FxHashMap<(&'static str, usize), Cost>> = RefCell::default();
}

/// Sample (or stop sampling) the mob ticks this thread runs next.
pub(super) fn set_sampling(on: bool) {
    SAMPLING.with(|s| s.set(on));
}

/// When this tick is sampled, the start of a mob's tick.
#[inline]
pub(super) fn start() -> Option<Instant> {
    SAMPLING.with(Cell::get).then(Instant::now)
}

/// A sampled mob of type `kind` in level `dim` ticked since `started`.
#[inline]
pub(super) fn add(dim: &'static str, kind: &MobType, started: Instant) {
    add_kind(dim, std::ptr::from_ref(kind) as usize, &kind.id, started);
}

/// [`add`] for the type at address `key` named `id`.
fn add_kind(dim: &'static str, key: usize, id: &str, started: Instant) {
    let nanos = started.elapsed().as_nanos() as u64;
    COSTS.with(|c| {
        let mut costs = c.borrow_mut();
        let cost = costs.entry((dim, key)).or_insert_with(|| Cost {
            kind: id.to_owned(),
            nanos: 0,
            mob_ticks: 0,
        });
        cost.nanos += nanos;
        cost.mob_ticks += 1;
    });
}

/// One level's mob type: its sampled time and mob ticks.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MobCost {
    pub dimension: &'static str,
    pub kind: String,
    pub nanos: u64,
    pub mob_ticks: u64,
}

/// The costs sampled since the last call, costliest first; clears them.
pub(super) fn take() -> Vec<MobCost> {
    let costs = COSTS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    let mut out: Vec<MobCost> = costs
        .into_iter()
        .map(|((dimension, _), c)| MobCost {
            dimension,
            kind: c.kind,
            nanos: c.nanos,
            mob_ticks: c.mob_ticks,
        })
        .collect();
    out.sort_by(|a, b| b.nanos.cmp(&a.nanos).then_with(|| a.kind.cmp(&b.kind)));
    out
}

/// What sampling one mob costs on this machine, in nanoseconds: the report's own overhead.
pub(super) fn sample_cost_nanos() -> f64 {
    const N: u32 = 20_000;
    let was = SAMPLING.with(Cell::get);
    let kept = COSTS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    set_sampling(true);
    let timer = Instant::now();
    for _ in 0..N {
        if let Some(started) = start() {
            add_kind("calibration", 1, "calibration", started);
        }
    }
    let per = timer.elapsed().as_nanos() as f64 / f64::from(N);
    set_sampling(was);
    COSTS.with(|c| *c.borrow_mut() = kept);
    per
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mob type at a made-up address.
    fn add(dim: &'static str, (key, id): (usize, &str), started: Instant) {
        add_kind(dim, key, id, started);
    }

    /// Mobs add up by level and type, only on sampled ticks; taking them clears them.
    #[test]
    fn costs_add_up_by_level_and_type_while_sampling() {
        let (zombie, cow) = ((1, "minecraft:zombie"), (2, "minecraft:cow"));
        set_sampling(false);
        assert!(start().is_none(), "nothing is timed while off");
        set_sampling(true);
        for _ in 0..3 {
            let t = start().expect("sampled");
            std::thread::sleep(std::time::Duration::from_millis(1));
            add("minecraft:overworld", zombie, t);
        }
        add("minecraft:overworld", cow, Instant::now());
        add("minecraft:the_nether", zombie, Instant::now());
        set_sampling(false);
        let costs = take();
        assert_eq!(costs.len(), 3, "{costs:?}");
        assert_eq!(costs[0].kind, "minecraft:zombie");
        assert_eq!(costs[0].dimension, "minecraft:overworld");
        assert_eq!(costs[0].mob_ticks, 3);
        assert!(costs[0].nanos >= 3_000_000, "{}", costs[0].nanos);
        let nether = costs
            .iter()
            .find(|c| c.dimension == "minecraft:the_nether")
            .unwrap();
        assert_eq!(
            (nether.kind.as_str(), nether.mob_ticks),
            ("minecraft:zombie", 1)
        );
        assert!(take().is_empty());
    }

    /// Calibrating leaves what was sampled, and the switch, as they were.
    #[test]
    fn calibration_keeps_the_samples() {
        set_sampling(true);
        add("minecraft:overworld", (3, "minecraft:pig"), Instant::now());
        let per = sample_cost_nanos();
        assert!(per > 0.0 && per < 100_000.0, "{per}");
        assert!(start().is_some(), "still sampling");
        set_sampling(false);
        let costs = take();
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].kind, "minecraft:pig");
    }
}
