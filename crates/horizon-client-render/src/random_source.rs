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

thread_local! {
    /// This thread's uniquifier under `HORIZON_TEST_SEEDS`.
    static THREAD_UNIQUIFIER: std::cell::Cell<i64> = const { std::cell::Cell::new(UNIQUIFIER_START) };
}

/// `RandomSupport.generateUniqueSeed`. Under `HORIZON_STILL` (screenshots that
/// come out the same every run) the clock is left out, so the seeds repeat run
/// to run, in the order they are taken. Under `HORIZON_TEST_SEEDS` the clock is
/// left out too and each thread steps a uniquifier of its own, as the parity
/// oracles' agent makes vanilla do (docs/parity-workflow.md, "Randoms that
/// repeat"): the client's draws then don't shift the server thread's.
pub fn unique_seed() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static UNIQUIFIER: AtomicI64 = AtomicI64::new(UNIQUIFIER_START);
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
}
