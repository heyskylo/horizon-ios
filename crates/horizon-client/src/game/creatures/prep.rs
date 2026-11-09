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
        border,
    }
}
