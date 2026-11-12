//! The randoms vanilla creates unseeded: `RandomSource.create()` (sounds,
//! screens, the torch flicker, a raid's random) and an entity's own random
//! (`Entity.random`). Each is seeded from one shared counter and the clock, as
//! `RandomSupport.generateUniqueSeed` does. Kept out of `horizon_core::random`,
//! whose streams are only ever seeded explicitly.
use horizon_core::random::LegacyRandom;

/// `RandomSource.create()`.
pub fn create() -> LegacyRandom {
    LegacyRandom::new(unique_seed())
}

/// An entity's own random (`Entity.random`), `RandomSource.create()` as
/// vanilla's is. Draws from an entity's own random never touch the level's.
pub fn entity() -> LegacyRandom {
    create()
}

/// `RandomSupport.SEED_UNIQUIFIER`'s start and step.
const UNIQUIFIER_START: i64 = 8_682_522_807_148_012;
const UNIQUIFIER_STEP: i64 = 1_181_783_497_276_652_981;

struct SeedOrder {
    draws: std::sync::Arc<horizon_core::random::OrderedDraws>,
    lane: usize,
    item: usize,
    lanes: usize,
}

/// Seeds recorded by a diagnostic serial run and replayed by its grouped copy. Client threads
/// continue to allocate normal seeds; verification never rewinds the process-wide counter.
pub struct SeedReplay {
    seeds: Vec<i64>,
    next: std::sync::atomic::AtomicUsize,
}
impl SeedReplay {
    pub fn complete(&self) -> bool {
        self.next.load(std::sync::atomic::Ordering::Relaxed) == self.seeds.len()
    }
}

enum SeedScope {
    Record(Vec<i64>),
    Replay(std::sync::Arc<SeedReplay>),
}

struct RestoreSeeds(Option<SeedScope>);
impl Drop for RestoreSeeds {
    fn drop(&mut self) {
        SEEDS.with(|s| *s.borrow_mut() = self.0.take());
    }
}

thread_local! {
    /// This thread's uniquifier under `HORIZON_TEST_SEEDS`.
    static THREAD_UNIQUIFIER: std::cell::Cell<i64> = const { std::cell::Cell::new(UNIQUIFIER_START) };
    /// New randoms created by a grouped mob turn must follow the same list order as level draws.
    static SEED_ORDER: std::cell::RefCell<Option<SeedOrder>> =
        const { std::cell::RefCell::new(None) };
    static SEEDS: std::cell::RefCell<Option<SeedScope>> = const { std::cell::RefCell::new(None) };
}

/// Run the serial diagnostic copy, retaining the actual seeds it allocated (including clocks).
pub fn record_seeds<R>(f: impl FnOnce() -> R) -> (R, std::sync::Arc<SeedReplay>) {
    let _restore = RestoreSeeds(SEEDS.with(|s| s.replace(Some(SeedScope::Record(Vec::new())))));
    let result = f();
    let seeds = SEEDS.with(|s| match s.take() {
        Some(SeedScope::Record(seeds)) => seeds,
        _ => unreachable!("recording scope restored"),
    });
    (
        result,
        std::sync::Arc::new(SeedReplay {
            seeds,
            next: std::sync::atomic::AtomicUsize::new(0),
        }),
    )
}

/// Replay the serial copy's seeds on this group's thread. The turn's seed ordering scope orders
/// accesses across groups; an extra or missing allocation makes `complete` false.
pub fn replay_seeds<R>(seeds: std::sync::Arc<SeedReplay>, f: impl FnOnce() -> R) -> R {
    let _restore = RestoreSeeds(SEEDS.with(|s| s.replace(Some(SeedScope::Replay(seeds)))));
    f()
}

/// The current diagnostic replay, to inherit on scoped worker threads.
pub fn seed_replay() -> Option<std::sync::Arc<SeedReplay>> {
    SEEDS.with(|s| match &*s.borrow() {
        Some(SeedScope::Replay(seeds)) => Some(std::sync::Arc::clone(seeds)),
        _ => None,
    })
}

/// Scope new random creation to one item of the parallel mob loop. Nothing waits until a seed
/// is actually requested. Earlier items finish before it, just as for the level random, and a
/// panic restores the previous scope. This does not rewind or reseed the shared counter.
pub fn in_seed_order<R>(
    draws: std::sync::Arc<horizon_core::random::OrderedDraws>,
    lane: usize,
    item: usize,
    lanes: usize,
    f: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<SeedOrder>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SEED_ORDER.with(|s| *s.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(SEED_ORDER.with(|s| {
        s.replace(Some(SeedOrder {
            draws,
            lane,
            item,
            lanes,
        }))
    }));
    f()
}

/// `RandomSupport.generateUniqueSeed`. Under `HORIZON_STILL` (screenshots that
/// come out the same every run) the clock is left out, so the seeds repeat run
/// to run, in the order they are taken. Under `HORIZON_TEST_SEEDS` the clock is
/// left out too and each thread steps a uniquifier of its own, as the parity
/// oracles' agent makes vanilla do (docs/parity-workflow.md, "Randoms that
/// repeat"): the client's draws then don't shift the server thread's.
static UNIQUIFIER: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(UNIQUIFIER_START);

pub fn unique_seed() -> i64 {
    use std::sync::atomic::Ordering;
    SEED_ORDER.with(|scope| {
        if let Some(order) = &*scope.borrow() {
            for lane in 0..order.lanes {
                if lane != order.lane {
                    order.draws.wait_past(lane, order.item);
                }
            }
        }
    });
    let replay = SEEDS.with(|s| match &*s.borrow() {
        Some(SeedScope::Replay(replay)) => {
            let at = replay.next.fetch_add(1, Ordering::Relaxed);
            replay.seeds.get(at).copied()
        }
        _ => None,
    });
    if let Some(seed) = replay {
        return seed;
    }
    let seed = allocate_seed();
    SEEDS.with(|s| {
        if let Some(SeedScope::Record(seeds)) = &mut *s.borrow_mut() {
            seeds.push(seed);
        }
    });
    seed
}

fn allocate_seed() -> i64 {
    use std::sync::atomic::Ordering;
    if test_seeds() {
        return THREAD_UNIQUIFIER.with(|u| {
            u.set(u.get().wrapping_mul(UNIQUIFIER_STEP));
            u.get()
        });
    }
    let mut current = UNIQUIFIER.load(Ordering::Relaxed);
    let next = loop {
        let next = current.wrapping_mul(UNIQUIFIER_STEP);
        match UNIQUIFIER.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break next,
            Err(v) => current = v,
        }
    };
    if still() {
        return next;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    next ^ nanos
}

/// `HORIZON_STILL=1`, read once, as the clock's `still_requested` reads it.
fn still() -> bool {
    static STILL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *STILL.get_or_init(|| std::env::var("HORIZON_STILL").is_ok_and(|v| v == "1"))
}

/// `HORIZON_TEST_SEEDS=1`, a dev switch for parity harnesses, read once.
fn test_seeds() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("HORIZON_TEST_SEEDS").is_ok_and(|v| v == "1");
        if on {
            log::warn!(
                "HORIZON_TEST_SEEDS=1: unseeded randoms repeat run to run (parity harnesses only)"
            );
        }
        on
    })
}

/// Whether `HORIZON_TEST_SEEDS` is on: unseeded randoms repeat, in the order each thread takes
/// them (the parallel mob loop then stays on one thread, so the order holds).
pub fn repeating_per_thread() -> bool {
    test_seeds()
}

/// A harness's sync step: the calling thread's uniquifier from now on. Only
/// under `HORIZON_TEST_SEEDS`, where the clock is left out.
pub fn set_thread_uniquifier(value: i64) -> Result<(), &'static str> {
    if !test_seeds() {
        return Err("setting the seed uniquifier needs HORIZON_TEST_SEEDS=1");
    }
    THREAD_UNIQUIFIER.with(|u| u.set(value));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::random::RandomSource;

    /// One shared counter: seeds taken back to back never repeat, even when
    /// the clock hasn't moved between them.
    #[test]
    fn each_seed_and_each_random_is_new() {
        let seeds: std::collections::HashSet<i64> = (0..1000).map(|_| unique_seed()).collect();
        assert_eq!(seeds.len(), 1000);
        assert_ne!(create().next_long(), create().next_long());
    }

    #[test]
    fn replay_is_scoped_and_detects_missing_or_extra_seeds() {
        let (expected, tape) = record_seeds(|| [unique_seed(), unique_seed()]);
        replay_seeds(std::sync::Arc::clone(&tape), || {
            assert_eq!(unique_seed(), expected[0]);
            assert!(!tape.complete(), "one allocation is missing");
            assert_eq!(unique_seed(), expected[1]);
            assert!(tape.complete());
            unique_seed();
            assert!(!tape.complete(), "an extra allocation is detected");
        });
        assert!(seed_replay().is_none());
        let caught = std::panic::catch_unwind(|| {
            record_seeds(|| panic!("recording panics"));
        });
        assert!(caught.is_err());
        let (fresh, fresh_tape) = record_seeds(unique_seed);
        replay_seeds(fresh_tape, || assert_eq!(unique_seed(), fresh));
    }

    /// New RNGs and level draws share the same dependency direction, even when the later lane
    /// gets to its allocation first. A panic releases both its lane and its TLS scopes.
    #[test]
    fn grouped_new_randoms_follow_serial_order_and_release_on_unwind() {
        use horizon_core::random::OrderedDraws;
        use std::sync::{Arc, mpsc};
        let (sent, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let (expected, tape) =
                record_seeds(|| (0..60).map(|_| unique_seed()).collect::<Vec<_>>());
            let draws = OrderedDraws::new(&LegacyRandom::new(7), 3);
            let mut actual = std::thread::scope(|s| {
                let handles: Vec<_> = (0..3)
                    .rev()
                    .map(|lane| {
                        let (draws, tape) = (Arc::clone(&draws), Arc::clone(&tape));
                        s.spawn(move || {
                            replay_seeds(tape, || {
                                let mut random = draws.lane(lane);
                                let _done = random.lane_done();
                                let mut out = Vec::new();
                                for item in (lane..30).step_by(3) {
                                    random.begin_item(item);
                                    let values =
                                        in_seed_order(Arc::clone(&draws), lane, item, 3, || {
                                            [unique_seed(), unique_seed()]
                                        });
                                    out.extend([(item * 2, values[0]), (item * 2 + 1, values[1])]);
                                    random.end_item();
                                }
                                out
                            })
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap())
                    .collect::<Vec<_>>()
            });
            actual.sort_by_key(|v| v.0);
            assert_eq!(
                actual.into_iter().map(|v| v.1).collect::<Vec<_>>(),
                expected
            );
            assert!(tape.complete());

            let draws = OrderedDraws::new(&LegacyRandom::new(7), 2);
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::thread::scope(|s| {
                    let earlier = Arc::clone(&draws);
                    s.spawn(move || {
                        let mut random = earlier.lane(0);
                        let _done = random.lane_done();
                        random.begin_item(0);
                        in_seed_order(earlier, 0, 0, 2, || panic!("earlier turn"));
                    });
                    let mut random = draws.lane(1);
                    let _done = random.lane_done();
                    random.begin_item(1);
                    in_seed_order(Arc::clone(&draws), 1, 1, 2, unique_seed);
                });
            }));
            assert!(caught.is_err());
            assert!(SEED_ORDER.with(|s| s.borrow().is_none()));
            let _ = sent.send(());
        });
        finished
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("seed wait released");
    }
}
