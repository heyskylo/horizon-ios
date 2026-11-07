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

/// An entity's own random (`Entity.random`). Vanilla's is
/// `RandomSource.createThreadLocalInstance`, seeded from Java's
/// `ThreadLocalRandom`; Horizon seeds it as `create` does. Draws from an
/// entity's own random never touch the level's.
pub fn entity() -> LegacyRandom {
    create()
}

/// `RandomSupport.generateUniqueSeed`. Under `HORIZON_STILL` (screenshots that
/// come out the same every run) the clock is left out, so the seeds repeat run
/// to run, in the order they are taken.
pub fn unique_seed() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static UNIQUIFIER: AtomicI64 = AtomicI64::new(8_682_522_807_148_012);
    let mut current = UNIQUIFIER.load(Ordering::Relaxed);
    let next = loop {
        let next = current.wrapping_mul(1_181_783_497_276_652_981);
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
