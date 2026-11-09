//! `NeutralMob`'s persistent anger: the absolute end time vanilla saves
//! (`anger_end_time`) beside Horizon's per-tick countdown (`Extra::anger`).

use super::{Mob, Species};

/// `NeutralMob.readPersistentAngerSaveData`: the anger runs until its end
/// time, so a mob that is loaded again is angry for what is left of it.
pub fn anger_left(anger_end: i64, game_time: i64) -> i32 {
    if anger_end <= 0 {
        return 0;
    }
    (anger_end - game_time).clamp(0, i64::from(i32::MAX)) as i32
}
/// The `NeutralMob`s whose anger is `Extra::anger`: they write and read
/// `anger_end_time` and `angry_at`. The bee keeps its own anger, and a
/// piglin's is a brain memory, not `NeutralMob`'s.
pub fn persistent_anger(s: Species) -> bool {
    matches!(
        s,
        Species::Wolf
            | Species::IronGolem
            | Species::PolarBear
            | Species::Enderman
            | Species::ZombifiedPiglin
    )
}
/// `NeutralMob.readPersistentAngerSaveData`: the gameplay countdown restarts
/// from the end time just read, and the pair is remembered as untouched.
pub fn adopt_anger_end(m: &mut Mob, game_time: i64) {
    m.extra.anger = anger_left(m.extra.anger_end, game_time);
    m.extra.anger_stamp = Some((m.extra.anger, m.age));
}
/// Before a `NeutralMob`'s tick: a countdown nothing changed since the end
/// time was last settled runs from that end time, so ticks the mob missed (its
/// chunk out of the entity-ticking area) still count, as they do for vanilla's
/// `isAngry`. The countdown is what is left after the previous tick, so in
/// a mob ticked every tick nothing changes.
pub fn resume_anger(m: &mut Mob, game_time: i64) {
    if m.extra.anger_stamp == Some((m.extra.anger, m.age)) {
        m.extra.anger = anger_left(m.extra.anger_end, game_time - 1);
        m.extra.anger_stamp = Some((m.extra.anger, m.age));
    }
}
/// `NeutralMob.getPersistentAngerEndTime` at `game_time`, from Horizon's
/// countdown. Vanilla keeps an absolute end time that only
/// `setPersistentAngerEndTime` changes (`startPersistentAngerTimer`,
/// `stopBeingAngry`): it is not rewritten when it passes, and it does not move
/// while the mob is not ticked. So the end stays as it is while the countdown
/// is what the end time says (`anger_left`), or the countdown has not changed
/// since the end was last read or written (`anger_stamp`: the mob has not
/// ticked, or does not count its anger down: NoAI, an enderman). Only a
/// countdown the gameplay set otherwise moves the end to `game_time` plus it,
/// or to -1 for anger the gameplay stopped.
pub fn settle_anger_end(m: &mut Mob, game_time: i64) -> i64 {
    let anger = m.extra.anger;
    let untouched = m.extra.anger_stamp.is_some_and(|(stamped, age)| {
        stamped == anger && (age == m.age || m.no_ai || m.species() == Species::Enderman)
    });
    if anger != anger_left(m.extra.anger_end, game_time) && !untouched {
        m.extra.anger_end = if anger > 0 {
            game_time + i64::from(anger)
        } else {
            -1
        };
    }
    m.extra.anger_stamp = Some((anger, m.age));
    m.extra.anger_end
}
/// `Wolf.defineSynchedData`: DATA_ANGER_END_TIME starts at -1 (the other
/// `NeutralMob`s keep a plain long field, 0 until something sets it).
pub fn init(m: &mut Mob) {
    if m.species() == Species::Wolf {
        m.extra.anger_end = -1;
    }
}
/// `NeutralMob.stopBeingAngry`'s anger: no countdown, end time -1.
pub fn stop(m: &mut Mob) {
    m.extra.anger = 0;
    m.extra.anger_end = -1;
}
/// One tick of a mob: a `NeutralMob`'s countdown resumes from its end time
/// before, and the end time is settled after.
pub fn around_tick(m: &mut Mob, game_time: i64, tick: impl FnOnce(&mut Mob) -> bool) -> bool {
    let neutral = persistent_anger(m.species());
    if neutral {
        resume_anger(m, game_time);
    }
    let kept = tick(m);
    if neutral {
        settle_anger_end(m, game_time);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobs::sensor_scan_tests;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    /// A `NeutralMob` of `species`; no resources are needed, only the species
    /// decides how its anger is kept.
    fn neutral(species: Species, id: &str) -> Mob {
        let mut kind = (*sensor_scan_tests::kind()).clone();
        kind.id = id.into();
        kind.species = species;
        Mob::new(1, Arc::new(kind), Vec3::ZERO, 0.0)
    }
    /// The mob as `readPersistentAngerSaveData` leaves it at `game_time`.
    fn loaded(species: Species, id: &str, end: i64, game_time: i64) -> Mob {
        let mut m = neutral(species, id);
        m.extra.anger_end = end;
        adopt_anger_end(&mut m, game_time);
        m
    }
    /// One tick at `game_time` of the mob's own countdown (`age` goes on with
    /// it), bracketed as `Mob::tick` does.
    fn tick(m: &mut Mob, game_time: i64) {
        resume_anger(m, game_time);
        m.age += 1;
        if m.extra.anger > 0 {
            m.extra.anger -= 1;
        }
        settle_anger_end(m, game_time);
    }

    /// #13076: vanilla writes the end time as it was read, whatever it is,
    /// past, zero, absent (-1) or future, as long as nothing restarted the anger.
    #[test]
    fn a_loaded_end_time_is_written_back_unchanged() {
        for end in [1, 0, -1, -5, 99, 999_999] {
            let mut m = loaded(Species::Wolf, "minecraft:wolf", end, 100);
            assert_eq!(settle_anger_end(&mut m, 100), end, "read at once: {end}");
            assert_eq!(settle_anger_end(&mut m, 100), end, "read again: {end}");
            tick(&mut m, 101);
            assert_eq!(settle_anger_end(&mut m, 101), end, "one tick on: {end}");
        }
    }
    /// #13076: the end time is absolute, so an iron golem that stops ticking
    /// (its chunk left the entity-ticking area) while the clock goes on keeps
    /// 999999, not 999999 plus the ticks it missed.
    #[test]
    fn a_mob_that_misses_ticks_keeps_its_end_time() {
        let mut m = loaded(Species::IronGolem, "minecraft:iron_golem", 999_999, 100);
        assert_eq!(settle_anger_end(&mut m, 100), 999_999);
        assert_eq!(settle_anger_end(&mut m, 105), 999_999);
        // Ticking again, the countdown counts the missed ticks.
        tick(&mut m, 106);
        assert_eq!(m.extra.anger, 999_999 - 106);
        assert_eq!(settle_anger_end(&mut m, 106), 999_999);
        // Also when nothing read the end time while it missed them.
        let mut g = loaded(Species::IronGolem, "minecraft:iron_golem", 2_000, 100);
        tick(&mut g, 101);
        tick(&mut g, 150);
        assert_eq!(g.extra.anger, 2_000 - 150);
        assert_eq!(settle_anger_end(&mut g, 150), 2_000);
    }
    /// A mob whose AI is off (or an enderman, which has no countdown of its
    /// own) ages without counting down: its end time stays.
    #[test]
    fn a_mob_that_does_not_count_down_keeps_its_end_time() {
        let mut m = loaded(Species::Wolf, "minecraft:wolf", 999_999, 100);
        m.no_ai = true;
        m.age += 3;
        assert_eq!(settle_anger_end(&mut m, 103), 999_999);
        let mut e = loaded(Species::Enderman, "minecraft:enderman", 5_000, 100);
        e.age += 3;
        assert_eq!(settle_anger_end(&mut e, 103), 5_000);
    }
    /// `startPersistentAngerTimer` at 150 with 400 ticks ends at 550, and the
    /// countdown going on from there keeps that end.
    #[test]
    fn restarted_anger_ends_from_the_clock() {
        let mut m = loaded(Species::Wolf, "minecraft:wolf", -1, 100);
        m.age += 50;
        m.extra.anger = 400;
        assert_eq!(settle_anger_end(&mut m, 150), 550);
        for t in 151..=160 {
            tick(&mut m, t);
            assert_eq!(settle_anger_end(&mut m, t), 550, "tick {t}");
        }
        // Set within its own tick: the same end.
        let mut w = loaded(Species::Wolf, "minecraft:wolf", -1, 100);
        resume_anger(&mut w, 150);
        w.age += 1;
        w.extra.anger = 400;
        settle_anger_end(&mut w, 150);
        assert_eq!(w.extra.anger_end, 550);
        // A NoAI mob the gameplay angers once (nothing counts it down after).
        let mut n = loaded(Species::Wolf, "minecraft:wolf", -1, 100);
        n.no_ai = true;
        n.extra.anger = 50;
        assert_eq!(settle_anger_end(&mut n, 100), 150);
        assert_eq!(settle_anger_end(&mut n, 101), 150);
    }
    /// Anger the gameplay ends early (`stopBeingAngry`) writes -1.
    #[test]
    fn stopped_anger_writes_minus_one() {
        let mut m = loaded(
            Species::ZombifiedPiglin,
            "minecraft:zombified_piglin",
            900,
            100,
        );
        tick(&mut m, 101);
        m.extra.anger = 0;
        assert_eq!(settle_anger_end(&mut m, 101), -1);
    }
    /// The constructors: a wolf's synched data starts at -1, the plain long
    /// field of the other `NeutralMob`s at 0.
    #[test]
    fn a_new_mob_starts_at_its_classes_default() {
        assert_eq!(neutral(Species::Wolf, "minecraft:wolf").extra.anger_end, -1);
        assert_eq!(
            neutral(Species::IronGolem, "minecraft:iron_golem")
                .extra
                .anger_end,
            0
        );
    }
}
