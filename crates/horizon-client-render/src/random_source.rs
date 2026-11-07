//! `RandomSource.create()`: generators seeded from a counter and the clock,
//! for the client's own randomness that vanilla doesn't seed (sounds, screen
//! animations). Kept out of `horizon_core::random`, whose streams are only
//! ever seeded.
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

/// `RandomSupport.generateUniqueSeed`.
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
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    next ^ nanos
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
