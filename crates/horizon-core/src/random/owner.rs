//! The two gameplay randoms vanilla keeps apart by owner: an entity's own
//! (`Entity.random`) and the level's (`Level.random`). Both are plain
//! `LegacyRandom`s, both made by `RandomSource.create()`. The types change no
//! draw. They make a function that names one owner refuse the other.
//!
//! Vanilla has more owners than these two, and none of them is ever wrapped as
//! `EntityRandom` or `LevelRandom`: the fishing hook's synchronized random
//! (`FishingHook.syncronizedRandom`, reseeded every tick), `ShufflingList`'s,
//! `Raid`'s, `EnchantmentMenu`'s, `EnderDragonFight`'s, `WanderingTraderSpawner`'s
//! and `MinecraftServer`'s. Typing the hook's as `EntityRandom` would make its
//! two randoms interchangeable again.
//!
//! Both types implement `RandomSource`, so generic helpers take either. Each
//! is `Clone`: a clone is a copy of the owner's state, not a second owner, so
//! code that clones one to get around a borrow must write the copy back. Their
//! `fork` and `fork_positional` return randoms typed as the owner that are not
//! it. Nothing in vanilla's entity or level code forks these owners; don't
//! store a fork as one.
//!
//! Until every parameter that wants an owner names it, both also deref to
//! `LegacyRandom`, so code that takes a bare `&mut LegacyRandom` still
//! compiles. That guard is off until the `Deref` goes.
use super::{InvalidBound, LegacyPositionalRandom, LegacyRandom, PositionalRandom, RandomSource};
use crate::position::BlockPos;

macro_rules! owner_random {
    ($(#[$doc:meta])* $name:ident, $positional:ident, $make:ident) => {
        $(#[$doc])*
        #[repr(transparent)]
        #[derive(Clone, Debug)]
        pub struct $name(LegacyRandom);

        impl $name {
            /// The one way to make this owner's random, so every site that
            /// wraps one can be found.
            #[inline]
            pub const fn $make(random: LegacyRandom) -> Self { Self(random) }
        }

        impl std::ops::Deref for $name {
            type Target = LegacyRandom;
            #[inline]
            fn deref(&self) -> &LegacyRandom { &self.0 }
        }
        impl std::ops::DerefMut for $name {
            #[inline]
            fn deref_mut(&mut self) -> &mut LegacyRandom { &mut self.0 }
        }

        impl RandomSource for $name {
            type Positional = $positional;
            #[inline]
            fn set_seed(&mut self, seed: i64) { self.0.set_seed(seed) }
            #[inline]
            fn next_int(&mut self) -> i32 { self.0.next_int() }
            #[inline]
            fn next_int_bound(&mut self, bound: i32) -> Result<i32, InvalidBound> { self.0.next_int_bound(bound) }
            #[inline]
            fn next_long(&mut self) -> i64 { self.0.next_long() }
            #[inline]
            fn next_boolean(&mut self) -> bool { self.0.next_boolean() }
            #[inline]
            fn next_float(&mut self) -> f32 { self.0.next_float() }
            #[inline]
            fn next_double(&mut self) -> f64 { self.0.next_double() }
            #[inline]
            fn next_gaussian(&mut self) -> f64 { self.0.next_gaussian() }
            /// Typed as this owner's, but not it (see the module docs).
            #[inline]
            fn fork_positional(&mut self) -> $positional { $positional(self.0.fork_positional()) }
            /// Typed as this owner's, but not it (see the module docs).
            #[inline]
            fn fork(&mut self) -> Self { Self(self.0.fork()) }
        }

        /// `fork_positional`'s factory: its randoms are typed as the owner's
        /// without being it.
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $positional(LegacyPositionalRandom);

        impl PositionalRandom for $positional {
            type Source = $name;
            #[inline]
            fn at(&self, position: BlockPos) -> $name { $name(self.0.at(position)) }
            #[inline]
            fn from_seed(&self, seed: i64) -> $name { $name(self.0.from_seed(seed)) }
            #[inline]
            fn from_hash_of(&self, name: &str) -> $name { $name(self.0.from_hash_of(name)) }
            #[inline]
            fn from_hash_of_utf16(&self, name: &[u16]) -> $name { $name(self.0.from_hash_of_utf16(name)) }
        }
    };
}

owner_random!(
    /// An entity's own random, vanilla's `Entity.random`. The other owners
    /// (the module docs list them) are never wrapped as one.
    EntityRandom,
    EntityPositional,
    own
);
owner_random!(
    /// The level's random, vanilla's `Level.random`. The other owners
    /// (the module docs list them) are never wrapped as one.
    LevelRandom,
    LevelPositional,
    for_level
);

#[cfg(test)]
mod tests {
    use super::*;

    /// A wrapper draws exactly what the random it wraps would.
    #[test]
    fn owners_draw_as_the_wrapped_random() {
        fn draws(r: &mut impl RandomSource) -> [f64; 5] {
            let int = f64::from(r.next_int_bound(7).unwrap());
            [
                int,
                r.next_long() as f64,
                r.next_float().into(),
                r.next_double(),
                r.next_gaussian(),
            ]
        }
        let expected = draws(&mut LegacyRandom::new(42));
        assert_eq!(
            draws(&mut EntityRandom::own(LegacyRandom::new(42))),
            expected
        );
        assert_eq!(
            draws(&mut LevelRandom::for_level(LegacyRandom::new(42))),
            expected
        );
        let fork = LevelRandom::for_level(LegacyRandom::new(42)).fork();
        assert_eq!(fork.state(), LegacyRandom::new(42).fork().state());
    }
}
