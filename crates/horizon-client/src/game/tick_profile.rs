//! What each kind of mob costs the server tick, for `horizon-report` (dedicated/report.rs): while
//! a report records, one tick in [`SAMPLE_EVERY`] times every mob's own tick and adds it up by
//! level and mob type. A native mod samples apart from it (`native_mob_cost_sampling`), with
//! sums of its own, so neither takes the other's. Off, a mob costs one thread-local read;
//! sampled, two clock reads and a map update for each who samples. A mob's riders tick inside
//! its own tick, so they count for it. The server tick's spans are shared the same way
//! ([`want_spans`]).
use horizon_client::{fast_hash::FxHashMap, mobs::MobType};
use std::{
    cell::{Cell, RefCell},
    sync::atomic::{AtomicU8, Ordering},
    time::Instant,
};

/// One server tick in this many is sampled while a report records.
pub(super) const SAMPLE_EVERY: usize = 4;

/// Who samples the mob ticks: a report's sampled tick, a mod.
const REPORT: u8 = 1;
const MOD: u8 = 2;

/// The time and the ticks of one mob type in one level.
struct Cost {
    kind: String,
    nanos: u64,
    mob_ticks: u64,
}

thread_local! {
    /// [`REPORT`] and [`MOD`], each set while it samples.
    static SAMPLING: Cell<u8> = const { Cell::new(0) };
    /// The report's sums, then the mod's.
    static COSTS: RefCell<[FxHashMap<(&'static str, usize), Cost>; 2]> = RefCell::default();
}

/// Who wants the tick's spans recorded besides nobody: `HORIZON_SPIKES` (the slow ring's steps),
/// a report's recording, a mod (`native_spans`).
pub(super) const SPANS_SPIKES: u8 = 1;
pub(super) const SPANS_REPORT: u8 = 2;
pub(super) const SPANS_MOD: u8 = 4;
static SPAN_USERS: AtomicU8 = AtomicU8::new(0);

/// `user` wants the spans recorded, or no longer does: they stay on while anyone does.
pub(super) fn want_spans(user: u8, on: bool) {
    let users = if on {
        SPAN_USERS.fetch_or(user, Ordering::Relaxed) | user
    } else {
        SPAN_USERS.fetch_and(!user, Ordering::Relaxed) & !user
    };
    horizon_client::spans::enable(users != 0);
}

#[cfg(test)]
pub(super) fn wanted(user: u8) -> bool {
    SPAN_USERS.load(Ordering::Relaxed) & user != 0
}

/// Sample (or stop sampling) the mob ticks this thread runs next, for the report.
pub(super) fn set_sampling(on: bool) {
    set(REPORT, on);
}

/// Sample (or stop sampling) the mob ticks this thread runs next, for a mod ([`take_mod`]).
pub(super) fn set_mod_sampling(on: bool) {
    set(MOD, on);
}

fn set(who: u8, on: bool) {
    SAMPLING.with(|s| s.set(if on { s.get() | who } else { s.get() & !who }));
}

/// When this tick is sampled, the start of a mob's tick.
#[inline]
pub(super) fn start() -> Option<Instant> {
    (SAMPLING.with(Cell::get) != 0).then(Instant::now)
}

/// A sampled mob of type `kind` in level `dim` ticked since `started`.
#[inline]
pub(super) fn add(dim: &'static str, kind: &MobType, started: Instant) {
    add_kind(dim, std::ptr::from_ref(kind) as usize, &kind.id, started);
}

/// A mob of type `kind` starts its tick: the CPU sampler's work, and `horizon-report`'s cost by mob type when sampled.
pub(super) fn start_mob(kind: &MobType) -> Option<(Instant, MobType)> {
    horizon_threads::work::set_entity(horizon_threads::work::entity_id(&kind.id));
    start().map(|t| (t, kind.clone()))
}

/// The mob [`start_mob`] started ended its tick in level `dim`.
pub(super) fn end_mob(dim: &'static str, sampled: Option<(Instant, MobType)>) {
    if let Some((started, kind)) = sampled {
        add(dim, &kind, started);
    }
}

/// [`add`] for the type at address `key` named `id`.
fn add_kind(dim: &'static str, key: usize, id: &str, started: Instant) {
    let nanos = started.elapsed().as_nanos() as u64;
    let who = SAMPLING.with(Cell::get);
    COSTS.with(|c| {
        let mut costs = c.borrow_mut();
        for (sums, bit) in costs.iter_mut().zip([REPORT, MOD]) {
            if who & bit == 0 {
                continue;
            }
            let cost = sums.entry((dim, key)).or_insert_with(|| Cost {
                kind: id.to_owned(),
                nanos: 0,
                mob_ticks: 0,
            });
            cost.nanos += nanos;
            cost.mob_ticks += 1;
        }
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

/// The report's costs sampled since the last call, costliest first; clears them.
pub(super) fn take() -> Vec<MobCost> {
    take_of(0)
}

/// The mod's costs sampled since the last call ([`set_mod_sampling`]), as [`take`].
pub(super) fn take_mod() -> Vec<MobCost> {
    take_of(1)
}

fn take_of(sums: usize) -> Vec<MobCost> {
    let costs = COSTS.with(|c| std::mem::take(&mut c.borrow_mut()[sums]));
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
    let kept = COSTS.with(|c| std::mem::take(&mut c.borrow_mut()[0]));
    SAMPLING.with(|s| s.set(REPORT));
    let timer = Instant::now();
    for _ in 0..N {
        if let Some(started) = start() {
            add_kind("calibration", 1, "calibration", started);
        }
    }
    let per = timer.elapsed().as_nanos() as f64 / f64::from(N);
    SAMPLING.with(|s| s.set(was));
    COSTS.with(|c| c.borrow_mut()[0] = kept);
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

    /// The mod's samples and the report's are kept apart: each takes its own, and one
    /// stopping leaves the other sampling.
    #[test]
    fn a_mod_samples_apart_from_the_report() {
        set_mod_sampling(true);
        add(
            "minecraft:overworld",
            (4, "minecraft:cow"),
            start().expect("mod samples"),
        );
        set_sampling(true);
        add(
            "minecraft:overworld",
            (5, "minecraft:pig"),
            start().unwrap(),
        );
        set_sampling(false);
        assert!(start().is_some(), "the mod still samples");
        let report = take();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].kind, "minecraft:pig");
        let mut kinds: Vec<_> = take_mod().into_iter().map(|c| c.kind).collect();
        kinds.sort();
        assert_eq!(kinds, ["minecraft:cow", "minecraft:pig"].map(String::from));
        let _ = sample_cost_nanos();
        assert!(take_mod().is_empty(), "calibration is the report's");
        set_mod_sampling(false);
        assert!(start().is_none());
    }

    /// Spans stay on while anyone wants them.
    #[test]
    fn spans_stay_on_while_anyone_wants_them() {
        // Only the mod's bit: other tests may want them as the report meanwhile.
        want_spans(SPANS_MOD, true);
        assert!(horizon_client::spans::enabled());
        assert_ne!(SPAN_USERS.load(Ordering::Relaxed) & SPANS_MOD, 0);
        want_spans(SPANS_MOD, false);
        assert_eq!(SPAN_USERS.load(Ordering::Relaxed) & SPANS_MOD, 0);
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
