//! What the mob loop (`tick_creature_entities`) prepares for every mob before it runs, on the tick's
//! helper threads while the parallel tick is on (`horizon_client::parallel`): each answer reads only
//! that mob, the blocks and the players, so the helpers give what one thread gives.
use super::super::{Game, natural_spawning::NearestPlayers};
use horizon_client::mobs::{Mob, MobWorld, Seen};
use horizon_client::parallel::par_map;
use horizon_core::geometry::Vec3;

/// From this many mobs a preparation goes to the helper threads.
const MIN_MOBS: usize = 256;

impl Game<'_> {
    /// `LivingEntity.isPushable` reads onClimbable live; a mob's cached one is only refreshed by its
    /// own push step, so one that has not ticked yet (the first tick after a load, a fresh spawn) or
    /// has moved since would be pushed out of a ladder or scaffolding. Refreshed here, from where
    /// each mob stands now; then each mob's entry of the snapshot others push it by goes in
    /// `snapshot`.
    pub(super) fn prepare_mobs(&mut self, player: Option<[i32; 4]>, snapshot: &mut Vec<Seen>) {
        let climbing: Vec<bool> = {
            let w = self.mob_world_base(&[]);
            let rebuild = hookless(&w);
            par_map(&self.mobs, MIN_MOBS, |part, out| {
                let w = rebuild();
                out.extend(part.iter().map(|m| m.on_climbable(&w)));
            })
        };
        for (m, climbing) in self.mobs.iter_mut().zip(climbing) {
            m.on_climbable = climbing;
        }
        snapshot.extend(par_map(&self.mobs, MIN_MOBS, |part, out| {
            out.extend(part.iter().map(|m| super::seen(m, player)));
        }));
    }
}

/// The nearest player each mob's despawn check reads (`Mob.checkDespawn`), found for every mob before
/// the loop. A mob that moved before its turn is asked again where it stands.
pub(super) struct MobsNearest<'p> {
    players: &'p NearestPlayers<'p>,
    /// Each mob's position as the loop started and its nearest player there; empty while the
    /// parallel tick is off (every mob asks at its turn).
    found: Vec<(Vec3, Option<Vec3>)>,
}

impl<'p> MobsNearest<'p> {
    /// `wanted`: whether the loop asks (not with one player, whose position answers).
    pub(super) fn new(players: &'p NearestPlayers<'p>, mobs: &[Mob], wanted: bool) -> Self {
        let found = if wanted && horizon_client::parallel::lanes() > 1 {
            par_map(mobs, MIN_MOBS, |part, out| {
                out.extend(
                    part.iter()
                        .map(|m| (m.position, Self::ask(players, m.position))),
                );
            })
        } else {
            Vec::new()
        };
        Self { players, found }
    }
    fn ask(players: &NearestPlayers, at: Vec3) -> Option<Vec3> {
        players.nearest(at.x, at.y, at.z).map(|p| p.position)
    }
    /// The nearest player to mob `i` of the loop, standing at `at`.
    pub(super) fn at(&self, i: usize, at: Vec3) -> Option<Vec3> {
        match self.found.get(i) {
            Some(&(was, nearest)) if was == at => nearest,
            _ => Self::ask(self.players, at),
        }
    }
}

/// `base` without its hooks into the game, which can't leave the server thread, as a recipe a
/// helper thread builds its own world from: native mod effects (the phases on helpers don't run
/// them) and the UUID lookup.
fn hookless<'w>(base: &MobWorld<'w>) -> impl Fn() -> MobWorld<'w> + Sync + use<'w> {
    // Every field but the two hooks, named, so that a field added to `MobWorld` is decided here.
    let MobWorld {
        world,
        registry,
        facts,
        fluid_tables,
        movement,
        collision,
        suffocation,
        data,
        player,
        controlling_player_movement,
        nearest_player,
        others,
        dragon,
        leash_boats,
        others_grid,
        others_index,
        others_villagers,
        others_solid,
        daylight,
        monsters_burn,
        game_time,
        day_time,
        raining,
        thundering,
        level,
        block_tags,
        landing,
        biomes,
        mob_griefing,
        universal_anger,
        mob_drops,
        max_entity_cramming,
        difficulty,
        dimension_type,
        world_seed,
        sea_level,
        teams,
        items,
        lid_openers,
        mod_effects: _,
        who_lookup: _,
        foreign: _,
        border,
    } = base.clone();
    move || MobWorld {
        world,
        registry,
        facts,
        fluid_tables,
        movement,
        collision,
        suffocation,
        data,
        player: player.clone(),
        controlling_player_movement,
        nearest_player,
        others,
        dragon,
        leash_boats,
        others_grid,
        others_index,
        others_villagers,
        others_solid,
        daylight,
        monsters_burn,
        game_time,
        day_time,
        raining,
        thundering,
        level,
        block_tags,
        landing,
        biomes,
        mob_griefing,
        universal_anger,
        mob_drops,
        max_entity_cramming,
        difficulty,
        dimension_type,
        world_seed,
        sea_level,
        teams: std::sync::Arc::clone(&teams),
        items,
        lid_openers,
        mod_effects: None,
        who_lookup: None,
        foreign: None,
        border,
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::natural_spawning::LevelPlayer;
    use super::*;
    use horizon_core::geometry::Aabb;

    #[test]
    #[ignore = "needs prepared resources and scratch homes/cwd"]
    fn preparation_refreshes_climbing_and_nearest_after_a_move() {
        let mut game = super::super::super::native_machines::tests::server_game();
        game.build_registries();
        game.set_allow_commands(true);
        for command in [
            "setblock 8 80 9 stone",
            "setblock 8 80 8 ladder[facing=north]",
            "summon pig 8.5 80 8.5 {NoAI:1b}",
            "summon pig 9.5 80 8.5 {NoAI:1b}",
        ] {
            assert!(game.run_command(command), "{command}");
        }
        assert_eq!(game.mobs.len(), 2);
        let examples = game.mobs.clone();
        game.mobs = (0..MIN_MOBS + 37)
            .map(|i| {
                let mut mob = examples[i % 2].clone();
                mob.id = 10_000 + i as u32;
                mob.on_climbable = i % 2 != 0; // deliberately stale in both cases
                mob
            })
            .collect::<Vec<_>>()
            .into();
        let mut snapshot = Vec::new();
        game.prepare_mobs(None, &mut snapshot);
        assert_eq!(snapshot.len(), game.mobs.len());
        for (i, (m, seen)) in game.mobs.iter().zip(&snapshot).enumerate() {
            assert_eq!(m.on_climbable, i % 2 == 0, "mob {i}");
            assert_eq!(seen.id, m.id);
            assert_eq!(seen.position, m.position);
            assert_eq!(seen.pushable, i % 2 != 0, "snapshot {i}");
        }

        let player = |x, spectator| {
            let position = Vec3::new(x, 80.0, 8.5);
            LevelPlayer {
                uuid: 0,
                position,
                bounds: Aabb::new(position, position),
                alive: false, // dead players still count for checkDespawn
                creative: false,
                spectator,
            }
        };
        let players = [player(8.5, true), player(7.5, false), player(9.5, false)];
        let index = NearestPlayers::new(&players);
        let nearest = MobsNearest::new(&index, &game.mobs, true);
        for (i, mob) in game.mobs.iter().enumerate() {
            let expected = if i % 2 == 0 {
                players[1].position
            } else {
                players[2].position
            };
            assert_eq!(nearest.at(i, mob.position), Some(expected));
            // A previous mob's push moved this mob across the nearest-player tie.
            assert_eq!(
                nearest.at(i, Vec3::new(10.0, 80.0, 8.5)),
                Some(players[2].position)
            );
        }
        let no_cache = MobsNearest::new(&index, &game.mobs, false);
        assert_eq!(
            no_cache.at(0, game.mobs[0].position),
            Some(players[1].position)
        );
        let spectators = [player(8.5, true)];
        let index = NearestPlayers::new(&spectators);
        let nearest = MobsNearest::new(&index, &game.mobs, true);
        assert_eq!(nearest.at(0, game.mobs[0].position), None);
    }
}
