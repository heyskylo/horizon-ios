//! The part of each mob's state for its clients (`Game::lan_entities`) that reads only the mob and
//! the synced tables: its entity data and attribute instances. While the parallel tick is on
//! (`horizon_client::parallel`) it is built after the pass, on the tick's helper threads, and put in
//! place; otherwise in place during the pass, as one thread always did.
use super::super::lan_attributes::{AttrSources, SpeedByType};
use super::{mob_freeze_meta, mob_meta};
use horizon_client::lan::{
    data::SyncData,
    entity::{EntityState, States},
};
use horizon_client::mobs::Mob;
use horizon_protocol::{entity_data::Value, packets::play::AttributeSnapshot};

/// From this many mobs the states go to the helper threads.
const MIN_MOBS: usize = 1024;

/// One mob's entity data (`meta`, and the `defaults` its entries can go back to) and attribute
/// instances; whether its position is sent precisely.
#[allow(clippy::too_many_arguments)]
pub(super) fn mob_state(
    m: &Mob,
    sync: &SyncData,
    sources: AttrSources,
    player_entity: Option<i32>,
    game_time: i64,
    speeds: &mut SpeedByType,
    meta: &mut Vec<(u8, i32, Value)>,
    defaults: &mut Vec<(u8, i32, Value)>,
    attributes: &mut Vec<AttributeSnapshot>,
) -> bool {
    mob_meta(m, sync, player_entity, game_time, meta, defaults);
    mob_freeze_meta(m, meta, defaults);
    sources.mob_attributes(m, speeds, attributes);
    // HappyGhast.aiStep: requiresPrecisePosition while on a still timeout.
    m.species() == horizon_client::mobs::Species::HappyGhast
        && horizon_client::mobs::happy_ghast::on_still_timeout(m)
}

#[cfg(test)]
thread_local! {
    /// A test's choice of the path, over the parallel tick's switch.
    static DEFER: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the states wait for the helper threads: while the parallel tick is on.
fn deferring() -> bool {
    #[cfg(test)]
    if let Some(defer) = DEFER.with(std::cell::Cell::get) {
        return defer;
    }
    horizon_client::parallel::lanes() > 1
}

/// The mobs' states of one pass: built in place, or (while the parallel tick is on) the mobs whose
/// states wait for the helper threads: (their slot in the states, their index in the mobs, the
/// player entity their lead may name).
pub(super) struct Later {
    waiting: Option<Vec<(usize, usize, Option<i32>)>>,
    game_time: i64,
    speeds: SpeedByType,
}

impl Later {
    pub(super) fn new(game_time: i64) -> Self {
        Self {
            waiting: deferring().then(Vec::new),
            game_time,
            speeds: SpeedByType::default(),
        }
    }
    /// Mob `index`'s state `e`, in `slot` of the states: built now, or left for [`Self::fill`].
    pub(super) fn state(
        &mut self,
        (slot, index): (usize, usize),
        m: &Mob,
        player_entity: Option<i32>,
        sync: &SyncData,
        sources: AttrSources,
        e: &mut EntityState,
    ) {
        if let Some(waiting) = &mut self.waiting {
            waiting.push((slot, index, player_entity));
            return;
        }
        let (meta, defaults, attributes) = (&mut e.meta, &mut e.defaults, &mut e.attributes);
        e.precise = mob_state(
            m,
            sync,
            sources,
            player_entity,
            self.game_time,
            &mut self.speeds,
            meta,
            defaults,
            attributes,
        );
    }
    /// The waiting states, built on the helper threads and put in place.
    pub(super) fn fill(
        self,
        mobs: &[Mob],
        sync: &SyncData,
        sources: AttrSources,
        out: &mut States,
    ) {
        let Some(waiting) = self.waiting else {
            return;
        };
        let game_time = self.game_time;
        let built = horizon_client::parallel::par_map(&waiting, MIN_MOBS, |part, done| {
            let mut speeds = SpeedByType::default();
            for &(slot, index, player) in part {
                let (mut meta, mut defaults, mut attributes) = (Vec::new(), Vec::new(), Vec::new());
                let precise = mob_state(
                    &mobs[index],
                    sync,
                    sources,
                    player,
                    game_time,
                    &mut speeds,
                    &mut meta,
                    &mut defaults,
                    &mut attributes,
                );
                done.push((slot, meta, defaults, attributes, precise));
            }
        });
        let states = out.as_mut_slice();
        for (slot, meta, defaults, attributes, precise) in built {
            let e = &mut states[slot];
            (e.meta, e.defaults, e.attributes, e.precise) = (meta, defaults, attributes, precise);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::lan::{NetConfig, hub::HubConfig};

    /// The states put in place after the pass are those built in place during it, for mobs whose
    /// entity data and attributes vary: effects, armor, a horse's rolled jump, variants, a baby,
    /// a happy ghast, and what `/attribute` changed.
    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn deferred_states_match_those_built_in_place() {
        crate::mod_api::in_own_process(
            "game::lan::mob_states::tests::deferred_states_match_those_built_in_place",
            deferred_states_match_those_built_in_place_in_own_process,
        );
    }

    fn deferred_states_match_those_built_in_place_in_own_process() {
        let mut game = super::super::super::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        for command in [
            "summon piglin 8.5 80 8.5 {NoAI:1b,PersistenceRequired:1b}",
            "effect give @e[type=piglin] speed 30 1",
            "item replace entity @e[type=piglin,limit=1] armor.head with iron_helmet",
            "summon horse 10.5 80 8.5 {NoAI:1b}",
            "summon sheep 12.5 80 8.5 {NoAI:1b,Color:3b,Age:-24000}",
            "summon axolotl 14.5 80 8.5 {NoAI:1b,Variant:2}",
            "summon happy_ghast 16.5 84 8.5 {NoAI:1b}",
            "summon pig 18.5 80 8.5 {NoAI:1b}",
            "summon frog 20.5 80 8.5 {NoAI:1b}",
            "attribute @e[type=pig,limit=1] minecraft:scale base set 1.5",
        ] {
            assert!(game.run_command(command), "{command}");
        }
        let examples = game.mobs.clone();
        game.mobs = (0..MIN_MOBS * 2 + 37)
            .map(|i| {
                let mut mob = examples[i % examples.len()].clone();
                mob.id = 10_000 + i as u32;
                mob.removed = i % 11 == 0;
                mob
            })
            .collect::<Vec<_>>()
            .into();
        game.lan_host(
            super::super::LanSettings {
                port: 0,
                guest_commands: true,
                force_game_mode: false,
            },
            NetConfig::loopback(0, String::new(), 2),
            HubConfig {
                default_game_mode: 0,
                guest_commands: true,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 20,
                integrated: false,
                levels: Default::default(),
            },
        )
        .unwrap();
        let mut lan = game.lan.take().unwrap();
        // The shared tables must retain pack-selected effect colors, not just
        // vanilla's registry color. This process owns the global pack palette.
        horizon_client::brewing::set_pack_colors(vec![("speed".into(), 0xAA12_3456)]);
        let players = Default::default();
        let mut runs = Vec::new();
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                DEFER.with(|d| d.set(None));
            }
        }
        let _reset = Reset;
        for after in [0, 10_012] {
            runs.clear();
            for defer in [false, true, false] {
                DEFER.with(|d| d.set(Some(defer)));
                let mut states = States::default();
                game.lan_entities(&players, &mut lan.hub, &mut states, false, after, None);
                runs.push(states.as_slice().to_vec());
            }
            let expected_ids: Vec<_> = game
                .mobs
                .iter()
                .filter(|m| !m.removed && m.id > after)
                .map(|m| m.id as i32)
                .collect();
            assert_eq!(
                runs[0].iter().map(|e| e.id).collect::<Vec<_>>(),
                expected_ids
            );
            assert!(runs[0].iter().filter(|e| !e.meta.is_empty()).count() >= MIN_MOBS);
            assert!(runs[0].iter().any(|e| !e.attributes.is_empty()));
            let colored: Vec<_> = runs[0]
                .iter()
                .filter(|e| e.kind == "minecraft:piglin")
                .collect();
            assert!(!colored.is_empty());
            for e in colored {
                let particles = e.meta.iter().find(|e| e.0 == 10).unwrap();
                let Value::Particles(particles) = &particles.2 else {
                    panic!("effect particle metadata");
                };
                assert_eq!(particles.len(), 1);
                assert_eq!(
                    particles[0].data,
                    Some(horizon_protocol::entity_data::ParticleData::Color(
                        0xFF12_3456_u32 as i32
                    ))
                );
            }
            assert_eq!(runs[0], runs[1], "deferred, after {after}");
            assert_eq!(runs[0], runs[2], "switch off, after {after}");

            // With deferral off the caller can read the metadata immediately,
            // before fill, and fill must not replace the vectors it already owns.
            let m = game
                .mobs
                .iter()
                .find(|m| !m.removed && m.id > after)
                .unwrap();
            let mut states = States::default();
            let e = states.add(m.id as i32, 0, &m.kind.id);
            let mut later = Later::new(game.world.game_time);
            later.state((0, 0), m, None, lan.hub.sync_data(), game.attr_sources(), e);
            assert_eq!(e.meta, runs[0][0].meta);
            assert_eq!(e.defaults, runs[0][0].defaults);
            assert_eq!(e.attributes, runs[0][0].attributes);
            let pointers = (e.meta.as_ptr(), e.defaults.as_ptr(), e.attributes.as_ptr());
            later.fill(
                &game.mobs,
                lan.hub.sync_data(),
                game.attr_sources(),
                &mut states,
            );
            let e = &states.as_slice()[0];
            assert_eq!(
                pointers,
                (e.meta.as_ptr(), e.defaults.as_ptr(), e.attributes.as_ptr())
            );
        }
    }
}
