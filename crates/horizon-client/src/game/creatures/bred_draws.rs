//! The breeding parent's own random in `getBreedOffspring`: the offspring's
//! coat coin and a wolf pup's voice.
use super::super::Game;
use horizon_client::mobs::Species;
use horizon_core::random::RandomSource;

impl Game<'_> {
    /// Cow, Pig, Chicken, Fox, Cat and Wolf.getBreedOffspring: the offspring's
    /// variant coin is the breeding parent's own first draw, not the level's.
    /// (Mooshroom draws its own coin in getOffspringVariant.)
    pub(super) fn bred_coat_coin(&mut self, parent: u32, species: Species) -> bool {
        matches!(
            species,
            Species::Cow
                | Species::Pig
                | Species::Chicken
                | Species::Fox
                | Species::Cat
                | Species::Wolf
        ) && self
            .mobs
            .iter_mut()
            .find(|m| m.id == parent)
            .is_some_and(|m| m.random.next_boolean())
    }
    /// WolfSoundVariants.pickRandomSoundVariant: Registry.getRandom over the
    /// wolf voice registry, from the parent's own random, after the collar
    /// colour's mix.
    pub(super) fn pick_bred_wolf_voice(&mut self, parent: u32, child: u32) {
        let voices = horizon_client::mobs::finalize::sound_variants(Species::Wolf);
        let voice = self.mobs.iter_mut().find(|m| m.id == parent).and_then(|m| {
            let i = m.random.next_int_bound(voices.len() as i32).ok()?;
            Some(i as u8)
        });
        if let Some(c) = self.mobs.iter_mut().find(|m| m.id == child) {
            c.extra.sound_variant = voice;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    /// `Cow`, `Pig`, `Chicken`, `Fox`, `Cat` and `Wolf.getBreedOffspring` draw
    /// the offspring's coat from the breeding parent's own random, and only a
    /// wolf picks a voice there (`WolfSoundVariants.pickRandomSoundVariant`,
    /// that same random, after the collar colour's mix). The level's random
    /// answers neither: two runs that differ only in the level's seed must
    /// agree, and the parent's stream must read as vanilla's own draws in
    /// vanilla's order.
    #[test]
    #[ignore = "needs prepared resources (HORIZON_TEST_RESOURCES)"]
    fn a_pup_takes_its_coat_and_voice_from_the_parents_own_random() {
        use horizon_core::random::{LegacyRandom, RandomSource};
        // The voice registry, not a hardcoded count of wolf voices.
        let voices = horizon_client::mobs::finalize::sound_variants(Species::Wolf).len() as i32;
        assert!(voices > 1, "a wolf has several voices");
        let cases = [
            ("minecraft:wolf", 2u8, 8u8),
            ("minecraft:wolf", 6, 0),
            ("minecraft:cow", 1, 0),
            ("minecraft:pig", 2, 0),
            ("minecraft:chicken", 0, 1),
            ("minecraft:fox", 1, 0),
            ("minecraft:cat", 4, 0),
        ];
        let mut heard = std::collections::BTreeSet::new();
        // One server game: its resources load once, and only the level's seed
        // and the pair change between runs.
        let mut game = super::super::super::native_machines::tests::server_game();
        for (n, &(kind, own, mate)) in cases.iter().enumerate() {
            // Independent entity randoms, kept clear of the ids the spawner
            // hands out: the parent is `part`, the partner `mate_id`.
            let parent_seed = 4_242 + 1_009 * n as i64;
            let mut outcomes = Vec::new();
            for (r, level_seed) in [11i64, 2_000_003].into_iter().enumerate() {
                game.world.random = LegacyRandom::new(level_seed);
                game.mobs.clear();
                let (part, mate_id) = (7u32, 8u32);
                // Ids the spawner hands out stay clear of the pair's.
                game.next_entity = 1_000 + 10 * n as u32 + 2 * r as u32;
                let t = game
                    .res
                    .mob_types()
                    .get(kind)
                    .unwrap_or_else(|| panic!("a {kind}"))
                    .clone();
                for (id, variant) in [(part, own), (mate_id, mate)] {
                    let mut m = Mob::new(id, t.clone(), Vec3::new(8.5, 64.0, 8.5), 0.0);
                    m.random = LegacyRandom::new(parent_seed + i64::from(id));
                    m.extra.variant = variant;
                    m.extra.in_love = 600;
                    game.mobs.push(m);
                }
                game.breed(part, mate_id, None);
                let child = game
                    .mobs
                    .iter()
                    .find(|m| m.id != part && m.id != mate_id)
                    .unwrap_or_else(|| panic!("{kind}: a pup"));
                assert_eq!(child.extra.age, -24000, "{kind}: a baby");
                // What the parent's own stream answers, draw for draw.
                let mut expected = LegacyRandom::new(parent_seed + i64::from(part));
                let coin = expected.next_boolean();
                let coat = if coin { own } else { mate };
                let voice = (kind == "minecraft:wolf").then(|| {
                    let i = expected.next_int_bound(voices).unwrap() as u8;
                    heard.insert(i);
                    i
                });
                assert_eq!(
                    child.extra.variant, coat,
                    "{kind}: the parent's coin picks the coat (level seed {level_seed})"
                );
                assert_eq!(
                    child.extra.sound_variant, voice,
                    "{kind}: the parent's next draw picks the voice (level seed {level_seed})"
                );
                // The stream ends where vanilla's does: the coin, a wolf's
                // voice, then finalizeSpawnChildFromBreeding's 1-to-7 roll.
                if game.ui.rules.bool("mob_drops") {
                    let _ = expected.next_int_bound(7);
                }
                let mut parent = game
                    .mobs
                    .iter()
                    .find(|m| m.id == part)
                    .unwrap()
                    .random
                    .clone();
                for _ in 0..4 {
                    assert_eq!(
                        parent.next_int(),
                        expected.next_int(),
                        "{kind}: the parent's stream goes on where vanilla leaves it"
                    );
                }
                outcomes.push((child.extra.variant, child.extra.sound_variant));
            }
            assert_eq!(
                outcomes[0], outcomes[1],
                "{kind}: the level's seed does not decide the pup"
            );
        }
        assert!(heard.len() > 1, "the wolf's voices vary: {heard:?}");
    }
}
