//! A server's levels, one per dimension (`MinecraftServer.levels`).
//!
//! The game's tick code reads one level: the one in place, held in `Game`'s
//! own fields (the world, its streaming and save, block ticks, entities).
//! A server (the dedicated server and single player's server thread) keeps
//! the other dimensions' levels here and ticks each one with players or forced
//! chunks by swapping it into place (`enter_level`), the way `as_guest` swaps a player
//! in, so the tick code runs unchanged, one level at a time.
//!
//! Shared by every level, as vanilla's derived levels share the
//! Overworld's level data: the game time, the day time and the weather, the
//! game rules, the scoreboard, the maps and the entities waiting to arrive
//! through a portal.

use super::{
    Dim, ExperienceOrb, FallingBlock, Game, ItemEntity, Mob, boats, commands,
    creaking::CreakingState, dragon, fireworks, lan::LanHost, lightning, machines, minecarts,
    ominous_item_spawner, persist, raids::Raiding, sculk::SculkState, tnt, trident,
};
use horizon_client::{
    Result,
    block_entity_anim::BlockEntityAnims,
    random_tick_sections::RandomTickSections,
    save::SaveDir,
    streaming::Streaming,
    tick_chunk_membership::TickChunkMembership,
    ticket_levels::{ChunkTickets, PortalTicket},
    vanillagen::VanillaTerrain,
    world::World,
};
use horizon_world::level::{LevelEvent, Ticks};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// Server ticks a level other than the Overworld stays loaded with no player
/// in it, before it is saved and closed (vanilla unloads a level's chunks
/// once no ticket holds them; the Overworld stays loaded).
const IDLE_TICKS: u32 = 300;

/// A level that is not in place: every per-level field of `Game`.
pub(super) struct Parked {
    world: World,
    streaming: Streaming,
    locator: Option<Arc<VanillaTerrain>>,
    save: Arc<SaveDir>,
    block_ticks: Ticks,
    tick_chunks: TickChunkMembership,
    tickets: ChunkTickets,
    random_tick_sections: RandomTickSections,
    level_events: Vec<LevelEvent>,
    /// Entity portal processes and cooldowns belong to this level's entities.
    portal_states: HashMap<u32, horizon_client::nether_portal::PortalState>,
    falling: horizon_client::join_seq::JoinList<FallingBlock>,
    paintings: Vec<horizon_client::entities::Painting>,
    item_frames: Vec<horizon_client::entities::ItemFrame>,
    armor_stands: horizon_client::join_seq::JoinList<horizon_client::entities::ArmorStand>,
    cushions: Vec<horizon_client::entities::Cushion>,
    others: Vec<commands::OtherEntity>,
    fangs: super::fangs::FangState,
    pub(super) boats: horizon_client::join_seq::JoinList<boats::Boat>,
    pub(super) carts: horizon_client::join_seq::JoinList<minecarts::Minecart>,
    jukebox_sounds: HashMap<(i32, i32, i32), u64>,
    bolts: Vec<lightning::Bolt>,
    crystals: Vec<dragon::Crystal>,
    dragon: Option<dragon::Dragon>,
    breath_clouds: Vec<dragon::Breath>,
    tridents: horizon_client::join_seq::JoinList<trident::ThrownTrident>,
    rockets: horizon_client::join_seq::JoinList<fireworks::Rocket>,
    ominous_spawners: Vec<ominous_item_spawner::OminousItemSpawner>,
    tnts: horizon_client::join_seq::JoinList<tnt::PrimedTnt>,
    hoppers: HashMap<(i32, i32, i32), machines::HopperState>,
    pub(super) mobs: horizon_client::join_seq::JoinList<Mob>,
    /// The entity ids of the level's fence knots (`knot_ids`).
    knot_ids: HashMap<[i32; 3], u32>,
    projectiles: horizon_client::join_seq::JoinList<horizon_client::projectiles::Projectile>,
    dropped: horizon_client::join_seq::JoinList<ItemEntity>,
    orbs: horizon_client::join_seq::JoinList<ExperienceOrb>,
    persist: persist::EntityMemory,
    lectern_pulses: Vec<((i32, i32, i32), i32)>,
    lid_openers: HashMap<(i32, i32, i32), i32>,
    lan_events: Vec<(i32, [i32; 3], i32, Option<u32>)>,
    lan_block_events: Vec<([i32; 3], i32, i32, u32)>,
    lan_entity_events: Vec<(i32, i8, [f64; 3])>,
    lan_critical_hits: Vec<(Option<u32>, i32, bool)>,
    /// The hub's tracked entities of this level (`LanHost::states`), as its last tick described
    /// them: what its players' moves are checked against before this tick describes them again.
    lan_states: horizon_client::lan::entity::States,
    /// Only the per-level parts of these are used (`swap_level`).
    anims: BlockEntityAnims,
    raiding: Raiding,
    sculk: SculkState,
    creaking: CreakingState,
    /// Natural spawning's candidate cache: each level keeps its own players' squares.
    spawn_scratch: super::natural_spawning::SpawnScratch,
    /// Server ticks since a player was last in the level.
    idle: u32,
    /// The level's own world border (`ServerLevel.getWorldBorder`), and what its players were
    /// last told of it (`LanHost::border_sent`).
    border: horizon_client::worldborder::WorldBorder,
    border_sent: Option<horizon_client::lan::border::Sent>,
}

/// Which passes of the server tick the level in place runs: the first
/// reads the network (`Hub::poll`), the last ends the tick for every player
/// (`Hub::end_tick`). A tick with one level is both. `running` is the
/// tick rate manager's answer for this server tick, asked once before the
/// levels (`MinecraftServer.tickServer`); `None` outside `tick_server`.
#[derive(Clone, Copy)]
pub(super) struct Pass {
    pub first: bool,
    pub last: bool,
    pub running: Option<bool>,
    /// Set once this level's player packets were handled before block ticks.
    pub packets_drained: bool,
}

impl Default for Pass {
    fn default() -> Self {
        Self {
            first: true,
            last: true,
            running: None,
            packets_drained: false,
        }
    }
}

/// The levels not in place, by dimension.
#[derive(Default)]
pub(super) struct Levels {
    parked: HashMap<Dim, Parked>,
    /// The levels whose open the mods heard (`native_level_load`), each to
    /// be told closed once.
    heard_open: Vec<Dim>,
}

impl Levels {
    /// Clear every parked level and dimensions recorded as heard open.
    pub(super) fn clear(&mut self) {
        self.parked.clear();
        self.heard_open.clear();
    }

    /// A parked level, without opening or loading it.
    pub(super) fn parked_world(&self, dim: Dim) -> Option<&World> {
        self.parked.get(&dim).map(|level| &level.world)
    }

    /// A parked level's world border.
    pub(super) fn parked_border(
        &self,
        dim: Dim,
    ) -> Option<&horizon_client::worldborder::WorldBorder> {
        self.parked.get(&dim).map(|level| &level.border)
    }

    /// The worlds of the levels not in place.
    pub(super) fn parked_worlds(&self) -> impl Iterator<Item = &World> {
        self.parked.values().map(|level| &level.world)
    }

    /// The world of the level not in place, in `dim`, to change.
    pub(super) fn parked_world_mut(&mut self, dim: Dim) -> Option<&mut World> {
        self.parked.get_mut(&dim).map(|level| &mut level.world)
    }

    /// The streaming of the level not in place, in `dim`, to change.
    pub(super) fn parked_streaming_mut(&mut self, dim: Dim) -> Option<&mut Streaming> {
        self.parked.get_mut(&dim).map(|level| &mut level.streaming)
    }

    /// The level not in place, in `dim`.
    pub(super) fn parked_mut(&mut self, dim: Dim) -> Option<&mut Parked> {
        self.parked.get_mut(&dim)
    }

    /// The entities of a parked level, counted as `Game::entity_count` counts the one in place.
    pub(super) fn parked_entity_count(&self, dim: Dim) -> Option<usize> {
        let l = self.parked.get(&dim)?;
        Some(
            l.mobs.iter().filter(|m| !m.removed).count()
                + l.dropped.len()
                + l.orbs.len()
                + l.projectiles.len()
                + l.carts.len()
                + l.boats.len()
                + l.falling.len()
                + l.paintings.len()
                + l.item_frames.len()
                + l.armor_stands.len()
                + l.cushions.len()
                + l.tridents.len()
                + l.rockets.len()
                + l.tnts.len(),
        )
    }

    /// The mobs of a parked level.
    pub(super) fn parked_mobs(&self, dim: Dim) -> Option<&[Mob]> {
        self.parked.get(&dim).map(|level| level.mobs.as_slice())
    }

    /// A parked level's loose entities, for `horizon-report`: items, experience orbs,
    /// projectiles, minecarts and boats.
    pub(super) fn parked_entity_counts(&self, dim: Dim) -> Option<[usize; 5]> {
        self.parked.get(&dim).map(|l| {
            [
                l.dropped.as_slice().len(),
                l.orbs.as_slice().len(),
                l.projectiles.as_slice().len(),
                l.carts.as_slice().len(),
                l.boats.as_slice().len(),
            ]
        })
    }

    /// The levels that are not in place, by dimension.
    /// The forced chunks of every level not in place, released.
    pub(super) fn release_parked_forced_chunks(&mut self) {
        for level in self.parked.values_mut() {
            level.streaming.forced.clear();
        }
    }

    pub(super) fn parked_dims(&self) -> Vec<Dim> {
        self.parked.keys().copied().collect()
    }
}

impl Game<'_> {
    /// A new level for `dim`, opened from the save (`MinecraftServer.createLevels`).
    fn open_level(&self, dim: Dim) -> Result<Parked> {
        let (streaming, save, locator) = super::world_loading::make_streaming(
            &self.res,
            &self.pack,
            &self.options,
            &self.settings,
            &self.world_root,
            dim,
            self.level_shapes.get(dim.id()),
            self.level.seed,
            &self.level.enabled_features,
        )?;
        let mut world = World::new(self.res.facts().air, self.level.seed);
        world.attachments = save.load_attachments();
        world.shape = self.level_shapes.get(dim.id());
        world.fast_lava = self.dimension_type_of(dim).fast_lava;
        let tickets = ChunkTickets::with_saved_portal(save.load_portal_tickets());
        Ok(Parked {
            world,
            streaming,
            locator,
            save,
            block_ticks: Ticks::new(self.level.seed as i32),
            tick_chunks: Default::default(),
            tickets,
            random_tick_sections: Default::default(),
            level_events: Vec::new(),
            portal_states: HashMap::new(),
            falling: Default::default(),
            paintings: Vec::new(),
            item_frames: Vec::new(),
            armor_stands: Default::default(),
            cushions: Vec::new(),
            others: Vec::new(),
            fangs: Default::default(),
            boats: Default::default(),
            carts: Default::default(),
            jukebox_sounds: HashMap::new(),
            bolts: Vec::new(),
            crystals: Vec::new(),
            dragon: None,
            breath_clouds: Vec::new(),
            tridents: Default::default(),
            rockets: Default::default(),
            ominous_spawners: Vec::new(),
            tnts: Default::default(),
            hoppers: HashMap::new(),
            mobs: Default::default(),
            knot_ids: HashMap::new(),
            projectiles: Default::default(),
            dropped: Default::default(),
            orbs: Default::default(),
            persist: Default::default(),
            lectern_pulses: Vec::new(),
            lid_openers: HashMap::new(),
            lan_events: Vec::new(),
            lan_block_events: Vec::new(),
            lan_entity_events: Vec::new(),
            lan_critical_hits: Vec::new(),
            lan_states: Default::default(),
            anims: Default::default(),
            raiding: Default::default(),
            sculk: Default::default(),
            creaking: Default::default(),
            spawn_scratch: Default::default(),
            idle: 0,
            border: self.load_border(dim),
            border_sent: None,
        })
    }

    /// `MinecraftServer.prepareLevels` activates every level's saved tickets:
    /// a level holding saved `PORTAL` tickets opens with the world and ticks
    /// while they hold it (`shouldKeepDimensionActive`).
    pub(super) fn open_saved_portal_levels(&mut self) -> Result<()> {
        if self.options.execution_role.is_client() {
            return Ok(());
        }
        let back = self.dim;
        for dim in [Dim::Overworld, Dim::Nether, Dim::End] {
            if dim == back || self.levels.parked.contains_key(&dim) {
                continue;
            }
            let save = SaveDir::new(
                horizon_client::save::dimension_dir(&self.world_root, dim.id()),
                self.level_shapes.get(dim.id()),
            );
            if !save.load_portal_tickets().is_empty() {
                self.enter_level(None, dim)?;
                self.enter_level(None, back)?;
            }
        }
        Ok(())
    }

    /// `MinecraftServer.prepareLevels`: before the first tick, the chunks the
    /// open levels' restored tickets hold (forced chunks, a native mod's) are
    /// loaded (`ChunkLoadCounter`). The first tick then finds them ticking
    /// and unpacks their saved block and fluid ticks at the game time they
    /// were saved at (`startTickingChunk`), not at whichever tick they finish
    /// streaming in.
    pub(super) fn prepare_levels(&mut self) -> Result<()> {
        if self.options.execution_role.is_client() {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(600);
        self.wait_for_ticket_chunks(deadline);
        let back = self.dim;
        for dim in [Dim::Overworld, Dim::Nether, Dim::End] {
            if dim != back && self.levels.parked.contains_key(&dim) {
                self.enter_level(None, dim)?;
                self.wait_for_ticket_chunks(deadline);
            }
        }
        if self.dim != back {
            self.enter_level(None, back)?;
        }
        Ok(())
    }

    /// Stream the level in place until each chunk its tickets make block
    /// ticking is a ticking chunk (lit, with its neighbours loaded).
    fn wait_for_ticket_chunks(&mut self, deadline: Instant) {
        // Forced chunks are checked two chunks out (their ticket block ticks one).
        // A `PORTAL` ticket's chunks are streamed as an area of their own (the
        // network tick does it, which hasn't run yet), whose chunks out to its
        // radius can become ticking chunks: those are waited for.
        let portal = self.portal_ticket_areas();
        for area in &portal {
            if !self.streaming.extra.contains(area) {
                self.streaming.extra.push(*area);
            }
        }
        loop {
            self.update_chunk_tickets();
            let levels = self.tickets.levels();
            let streaming = &self.streaming;
            let ready = streaming
                .forced
                .iter()
                .chain(&streaming.mod_forced)
                .map(|&c| (c, 2))
                .chain(portal.iter().copied())
                .all(|((cx, cz), reach)| {
                    (cz - reach..=cz + reach).all(|z| {
                        (cx - reach..=cx + reach).all(|x| {
                            !levels.loaded_ticking(x, z) || super::ticking_chunk(&self.world, x, z)
                        })
                    })
                });
            if ready {
                return;
            }
            if Instant::now() >= deadline {
                log::warn!("Horizon: the chunks of the saved tickets did not load");
                return;
            }
            self.update_world();
            self.streaming.wait(Duration::from_millis(5));
        }
    }

    /// Exchange the level in place with `p`.
    fn swap_level(&mut self, p: &mut Parked) {
        use std::mem::swap;
        swap(&mut self.world, &mut p.world);
        swap(&mut self.streaming, &mut p.streaming);
        let mut locator = self.end.locator();
        swap(&mut locator, &mut p.locator);
        self.end.set_locator(locator);
        self.share_spawn_conditions();
        swap(&mut self.save, &mut p.save);
        swap(&mut self.block_ticks, &mut p.block_ticks);
        swap(&mut self.tick_chunks, &mut p.tick_chunks);
        swap(&mut self.tickets, &mut p.tickets);
        swap(&mut self.random_tick_sections, &mut p.random_tick_sections);
        swap(&mut self.level_events, &mut p.level_events);
        swap(&mut self.portals.entities, &mut p.portal_states);
        swap(&mut self.falling, &mut p.falling);
        swap(&mut self.paintings, &mut p.paintings);
        swap(&mut self.item_frames, &mut p.item_frames);
        swap(&mut self.armor_stands, &mut p.armor_stands);
        swap(&mut self.cushions, &mut p.cushions);
        swap(&mut self.others, &mut p.others);
        swap(&mut self.fangs, &mut p.fangs);
        swap(&mut self.boats, &mut p.boats);
        swap(&mut self.carts, &mut p.carts);
        swap(&mut self.jukebox_sounds, &mut p.jukebox_sounds);
        swap(&mut self.bolts, &mut p.bolts);
        swap(&mut self.crystals, &mut p.crystals);
        swap(&mut self.dragon, &mut p.dragon);
        swap(&mut self.breath_clouds, &mut p.breath_clouds);
        swap(&mut self.tridents, &mut p.tridents);
        swap(&mut self.rockets, &mut p.rockets);
        swap(&mut self.ominous_spawners, &mut p.ominous_spawners);
        swap(&mut self.tnts, &mut p.tnts);
        swap(&mut self.hoppers, &mut p.hoppers);
        swap(&mut self.mobs, &mut p.mobs);
        swap(&mut self.knot_ids, &mut p.knot_ids);
        swap(&mut self.creatures.projectiles, &mut p.projectiles);
        swap(&mut self.dropped, &mut p.dropped);
        swap(&mut self.orbs, &mut p.orbs);
        swap(&mut self.persist, &mut p.persist);
        swap(&mut self.lectern_pulses, &mut p.lectern_pulses);
        swap(&mut self.lid_openers, &mut p.lid_openers);
        swap(&mut self.lan_events, &mut p.lan_events);
        swap(&mut self.lan_block_events, &mut p.lan_block_events);
        swap(&mut self.lan_entity_events, &mut p.lan_entity_events);
        swap(&mut self.lan_critical_hits, &mut p.lan_critical_hits);
        let (a, b) = (&mut self.be_anims, &mut p.anims);
        swap(&mut a.chests, &mut b.chests);
        swap(&mut a.books, &mut b.books);
        swap(&mut a.shulkers, &mut b.shulkers);
        swap(&mut a.bells, &mut b.bells);
        swap(&mut a.conduits, &mut b.conduits);
        swap(&mut a.skulls, &mut b.skulls);
        swap(&mut a.pots, &mut b.pots);
        swap(&mut a.gateways, &mut b.gateways);
        swap(&mut a.gateway_servers, &mut b.gateway_servers);
        swap(&mut a.conduit_servers, &mut b.conduit_servers);
        self.raiding.swap_level(&mut p.raiding);
        self.sculk.swap_level(&mut p.sculk);
        self.creaking.swap_level(&mut p.creaking);
        swap(&mut self.spawn_scratch, &mut p.spawn_scratch);
        swap(&mut self.border, &mut p.border);
        // What only the client draws starts over.
        self.particles.clear();
        self.pickups.clear();
    }

    /// Put `target`'s level in place, opening it first if it is not loaded.
    /// The level that was in place is kept with the others; the shared
    /// clock and weather go with the swap. `host` is the hub when the caller
    /// holds it (`self.lan` is taken during the network tick).
    pub(super) fn enter_level(
        &mut self,
        mut host: Option<&mut LanHost>,
        target: Dim,
    ) -> Result<()> {
        // A client holds no levels.
        if self.options.execution_role.is_client() {
            return Ok(());
        }
        if target != self.dim {
            let (mut p, opened) = match self.levels.parked.remove(&target) {
                Some(p) => (p, false),
                None => (self.open_level(target)?, true),
            };
            p.world.game_time = self.world.game_time;
            p.world.day_time = self.world.day_time;
            p.world.end_clock = self.world.end_clock;
            p.world.weather = self.world.weather;
            // The game rules and the difficulty are the server's, not a
            // level's: natural regeneration holds in every dimension.
            p.world.health_rules = self.world.health_rules;
            // The sculk tables' watched blocks are built once per game.
            p.world.watched = std::mem::take(&mut self.world.watched);
            self.swap_level(&mut p);
            // The tracked entities go with their level: a level's moves, read before its tick
            // describes its entities again, would otherwise meet the last level's (the anticheat
            // missed the boat under a player while anyone was in another dimension).
            let states = match host.as_deref_mut() {
                Some(h) => Some(&mut h.states),
                None => self.lan.as_mut().map(|h| &mut h.states),
            };
            if let Some(states) = states {
                std::mem::swap(states, &mut p.lan_states);
            }
            match host.as_deref_mut() {
                Some(h) => std::mem::swap(&mut h.border_sent, &mut p.border_sent),
                None => {
                    if let Some(h) = self.lan.as_mut() {
                        std::mem::swap(&mut h.border_sent, &mut p.border_sent);
                    }
                }
            }
            p.idle = 0;
            let left = std::mem::replace(&mut self.dim, target);
            self.levels.parked.insert(left, p);
            if opened {
                // The new streaming's radius and meshing options.
                self.apply_settings();
                // A mod's tickets held for the level while it was closed.
                self.native_chunk_tickets_opened();
                self.native_level_opened(target);
            }
        }
        let host = match host {
            Some(h) => Some(h),
            None => self.lan.as_mut(),
        };
        if let Some(h) = host {
            h.hub.set_dimension(target.id());
        }
        Ok(())
    }

    /// The entity's memory (its UUID and the compound it was loaded with)
    /// moves with it from the level not in place to the level in place, so
    /// it is saved in the other level as it was.
    pub(super) fn move_entity_memory(&mut self, from: Dim, id: u32) {
        let Some(p) = self.levels.parked.get_mut(&from) else {
            return;
        };
        let uuid = p.persist.uuids.remove(&id);
        let raw = p.persist.raw.remove(&id);
        let slots = p.persist.passenger_slots.remove(&id);
        if let Some(u) = uuid {
            self.persist.uuids.insert(id, u);
        }
        if let Some(r) = raw {
            self.persist.raw.insert(id, r);
        }
        if let Some(s) = slots {
            self.persist.passenger_slots.insert(id, s);
        }
    }

    /// Far terrain for a client in `dim`: the level in place may be another
    /// one, the server ticking its levels in turn.
    pub(super) fn lod_source_in(
        &self,
        dim: Dim,
    ) -> Option<Arc<dyn horizon_client::streaming::LodSource>> {
        if dim == self.dim {
            return Some(self.streaming.lod_source());
        }
        let parked = self.levels.parked.get(&dim)?;
        Some(parked.streaming.lod_source())
    }

    /// Put `dim`'s level in place for a command whose source is there
    /// (`execute in`, `execute at` a player in another level), opening it if
    /// nobody is in it; returns the level to put back afterwards. Only a
    /// server holds other levels.
    pub(super) fn enter_command_level(&mut self, dim: Dim) -> Option<Dim> {
        if self.options.execution_role.is_client() || dim == self.dim {
            return None;
        }
        let back = self.dim;
        match self.enter_level(None, dim) {
            Ok(()) => Some(back),
            Err(e) => {
                log::error!("could not open {}: {e}", dim.id());
                None
            }
        }
    }

    /// One server tick (`MinecraftServer.tickServer`): each level with
    /// players or forced chunks in turn (`tickChildren`), its chunks streamed
    /// and loaded and its world ticked. The first pass reads the network and runs the
    /// tick's shared parts (the clock, the weather, functions, the autosave);
    /// the next ones only their level. While `paused`, only the connections
    /// run. The native server tick hook brackets it (`ServerTickStart` before
    /// the first level, `ServerTickEnd` after the last).
    pub(super) fn tick_server(&mut self, paused: bool) -> Result<()> {
        // `tickRateManager.tick()` once per server tick, before any level
        // (each `ServerLevel.tick` only reads `runsNormally`), and the native
        // `ServerTickStart` before that level work, where NeoForge fires
        // `ServerTickEvent.Pre`. A paused pass and a held server tick nothing.
        let running = (!paused && !self.agent.hold_server).then(|| self.clock.world_runs());
        let server = self.serving.is_some() || self.options.dedicated.is_some();
        // A config edited while the server is paused (empty) still reloads.
        // The parts outside every level get spans of their own, so that a slow
        // tick's line and `horizon-report` say when a mod's tick hook took it.
        if server {
            let _span = horizon_client::spans::guard("mod_configs_poll");
            self.native_configs_poll();
        }
        if let Some(running) = running
            && server
        {
            let _span = horizon_client::spans::guard("mods_server_tick_start");
            self.native_lifecycle_server_tick_start(running);
        }
        let observe_level =
            running.is_some() && (self.serving.is_some() || self.options.dedicated.is_some());
        let dims = self.levels_to_tick();
        let order = dims.clone();
        let n = dims.len();
        let mut result = Ok(());
        for (k, dim) in dims.into_iter().enumerate() {
            // Each level's share of the tick (the first one also reads the network and runs the
            // tick's shared parts): `horizon-report`'s `level_*` spans.
            let _level_span = horizon_client::spans::guard(match dim {
                Dim::Overworld => "level_overworld",
                Dim::Nether => "level_the_nether",
                Dim::End => "level_the_end",
            });
            if let Err(e) = self.enter_level(None, dim) {
                result = Err(e);
                break;
            }
            self.level_pass = Pass {
                first: k == 0,
                last: k + 1 == n,
                running,
                packets_drained: false,
            };
            if observe_level {
                self.native_level_tick_pre(dim.id());
            }
            self.update_dedicated_streaming();
            self.update_world();
            result = horizon_client::block_contact::in_level(
                (self.serving.is_some() || self.options.dedicated.is_some()).then_some(dim.id()),
                || {
                    if paused {
                        self.tick_lan(true);
                        Ok(())
                    } else if k == 0 {
                        self.tick()
                    } else {
                        let running = running.unwrap_or_else(|| self.clock.world_runs());
                        // ClientLevel.tick: the sky flash fades in a frozen level too.
                        self.tick_weather_effects();
                        self.tick_world(running)
                    }
                },
            );
            if result.is_err() {
                break;
            }
            if observe_level {
                self.native_level_tick_post(dim.id());
            }
            self.publish_server_debug(dim);
            // An entity that crossed into a level that already ticked this
            // server tick is in it when the tick ends (`addDuringTeleport`).
            if running == Some(true) {
                let mut moved = false;
                for &earlier in &order[..k] {
                    if !self.arrival_chunks(earlier).is_empty()
                        && self.enter_level(None, earlier).is_ok()
                    {
                        self.land_portal_arrivals();
                        moved = true;
                    }
                }
                if moved && k + 1 < n {
                    self.enter_level(None, dim)?;
                }
            }
        }
        if let Some(&first) = order.first()
            && self.dim != first
        {
            let _ = self.enter_level(None, first);
        }
        self.level_pass = Pass::default();
        if result.is_err() {
            self.native_lifecycle.cancel_server_tick();
        }
        result?;
        let closed = {
            let _span = horizon_client::spans::guard("close_idle_levels");
            self.close_idle_levels()
        };
        if let Err(error) = closed {
            self.native_lifecycle.cancel_server_tick();
            return Err(error);
        }
        // `ServerTickEnd`: an errored tick or an idle-close failure (at
        // the `?` above) never reaches this site.
        {
            let _span = horizon_client::spans::guard("mods_server_tick_end");
            self.native_lifecycle_server_tick_end(paused);
        }
        // The packets counted for `native_server_packets` (only while a mod counts them).
        if server {
            self.diagnostics_packets_tick();
        }
        Ok(())
    }

    /// `tick_server` as a server thread runs it. With `HORIZON_SPIKES=MS`, a
    /// tick longer than MS is logged with its thread CPU time and spans.
    pub(super) fn tick_server_timed(&mut self, paused: bool) -> Result<()> {
        self.tick_server_spans(paused, |_, _, _, _| {})
    }

    /// `tick_server_timed`, handing the tick's wall time, thread CPU time
    /// and spans (milliseconds, summed by name) to `record` while spans are on, with how many
    /// spans the tick itself recorded: a dedicated server's performance report turns them on to
    /// record its ticks.
    pub(super) fn tick_server_spans(
        &mut self,
        paused: bool,
        record: impl FnOnce(f32, Option<f32>, Vec<horizon_client::spans::Span>, usize),
    ) -> Result<()> {
        if self.options.spikes.is_none() && !horizon_client::spans::enabled() {
            return self.tick_server(paused);
        }
        // Spans recorded since the last tick ran between ticks (the chunk pump of the wait): they
        // stay in the tick's list, but only the tick's own count as recorded in it.
        let between = horizon_client::spans::take_raw();
        let (started, cpu) = (Instant::now(), horizon_client::spans::thread_cpu());
        let result = self.tick_server(paused);
        let wall = started.elapsed().as_secs_f32() * 1000.0;
        let cpu = horizon_client::spans::thread_cpu()
            .zip(cpu)
            .map(|(end, start)| (end - start).as_secs_f32() * 1000.0);
        // Taken every tick: nothing else takes this thread's spans.
        let raw = horizon_client::spans::take_raw();
        let in_tick = raw.len();
        let spans = if between.is_empty() {
            horizon_client::spans::merge(&raw)
        } else {
            horizon_client::spans::merge(&[between, raw].concat())
        };
        let notes = horizon_client::spans::take_notes();
        if let Some(threshold) = self.options.spikes
            && wall > threshold
        {
            let cpu = cpu.map_or_else(String::new, |c| format!(", cpu {c:.1} ms"));
            log::info!(
                "Horizon: slow tick {wall:.1} ms{cpu} ({} mobs): {}{}",
                self.mobs.len(),
                horizon_client::spans::describe(&spans),
                slowest_steps(&notes)
            );
        }
        self.native_server_tick_timed(wall, cpu, &spans);
        record(wall, cpu, spans, in_tick);
        result
    }

    /// The end of `MinecraftServer.runServer`'s loop for a server thread: a
    /// finished `/tick sprint` reports to the operators as the server, then
    /// the next tick starts one tick length after this one, or straight away
    /// while the sprint runs (`checkShouldSprintThisTick`). A server more
    /// than two seconds behind skips the ticks it missed: that returns how far
    /// behind it was, for the overload warning.
    pub(super) fn wait_for_next_server_tick(
        &mut self,
        next: &mut Instant,
        paused: bool,
    ) -> Option<Duration> {
        if let Some(report) = self.clock.take_sprint_report() {
            // ServerTickRateManager.finishTickSprint: the server's sendSuccess.
            self.console_broadcast = Some(true);
            self.sprint_report(&report);
            self.console_broadcast = None;
            if let Some(mut host) = self.lan.take() {
                self.send_admin_feedback(&mut host, None);
                self.lan = Some(host);
            }
            self.admin_feedback.clear();
        }
        let now = Instant::now();
        if !paused && self.clock.sprinting() {
            *next = now;
            return None;
        }
        *next += self.clock.tick_duration();
        if *next > now {
            // The chunks loading (the players', force-loaded ones, a
            // `/pregen` task) keep the workers fed until the next tick.
            self.pump_chunks(*next, paused);
            let now = Instant::now();
            if *next > now {
                std::thread::sleep(*next - now);
            }
        } else if now - *next > Duration::from_secs(2) {
            let behind = now - *next;
            *next = now;
            return Some(behind);
        }
        None
    }

    /// The levels this server tick runs, in `MinecraftServer.levels` order
    /// (the Overworld, the Nether, the End): every level with a player in
    /// it, forced chunks or a `/pregen` task, or the Overworld while idle.
    pub(super) fn levels_to_tick(&self) -> Vec<Dim> {
        let guests = self
            .lan
            .as_ref()
            .map_or(&[][..], |h| h.hub.guests.as_slice());
        let mut dims: Vec<Dim> = [Dim::Overworld, Dim::Nether, Dim::End]
            .into_iter()
            .filter(|d| {
                // `/forceload`ed chunks, or a native mod's tickets.
                let held = |s: &Streaming| !s.forced.is_empty() || !s.mod_forced.is_empty();
                let forced = if *d == self.dim {
                    held(&self.streaming)
                } else {
                    self.levels
                        .parked
                        .get(d)
                        .is_some_and(|p| held(&p.streaming))
                };
                guests.iter().any(|g| g.dimension == d.id())
                    // A viewer count not yet told to 0 (`tick_mod_container_viewers`).
                    || self.mod_container_viewers.keys().any(|(k, _)| *k == d.id())
                    || self.player_bed_levels().any(|b| b == *d)
                    || forced
                    // A native mod's tickets for a level not open yet (`native_force_chunk_in`).
                    || self.mod_chunk_tickets.get(d).is_some_and(|l| !l.is_empty())
                    // A game test runs in its own level, players or not.
                    || self.game_tests.has_tests_in(*d)
                    || self.pregen_level() == Some(*d)
                    // A pending portal arrival keeps its destination level
                    // running so it can land and tick (the portal ticket).
                    || !self.arrival_chunks(*d).is_empty()
                    // A portal ticket keeps its dimension active
                    // (`TicketType.shouldKeepDimensionActive`).
                    || !self.portal_tickets_in(*d).is_empty()
            })
            .collect();
        if dims.is_empty() {
            dims.push(Dim::Overworld);
        }
        dims
    }

    /// The portal tickets of `dim`'s level (`placePortalTicket`, or saved).
    pub(super) fn portal_tickets_in(&self, dim: Dim) -> &[PortalTicket] {
        if dim == self.dim {
            self.tickets.portal_tickets()
        } else {
            self.levels
                .parked
                .get(&dim)
                .map_or(&[], |p| p.tickets.portal_tickets())
        }
    }

    /// The areas the level in place's portal tickets keep streamed: around each
    /// exit its block-ticking chunks and their neighbours, the ring vanilla
    /// keeps `FULL` (level 33), so the outer block-ticking ring is a ticking
    /// chunk too; and at least the chunks its expiry waits for
    /// (`portal_ticket_can_expire`).
    pub(super) fn portal_ticket_areas(&self) -> Vec<((i32, i32), i32)> {
        let block_ticking = i32::from(horizon_client::ticket_levels::BLOCK_TICKING);
        self.tickets
            .portal_tickets()
            .iter()
            .map(|t| (t.chunk, (block_ticking - t.level).max(1)))
            .collect()
    }

    /// Count the parked levels' empty ticks, and save and close the ones
    /// (other than the Overworld) nobody has been in for a while.
    pub(super) fn close_idle_levels(&mut self) -> Result<()> {
        let occupied = self.levels_to_tick();
        // A vehicle waiting there for its rider's arrival in another level.
        let waiting: Vec<Dim> = self
            .levels
            .parked
            .keys()
            .copied()
            .filter(|d| self.vehicle_waits_in(*d))
            .collect();
        let mut closing = Vec::new();
        for (dim, p) in &mut self.levels.parked {
            if occupied.contains(dim) || waiting.contains(dim) {
                p.idle = 0;
                continue;
            }
            p.idle += 1;
            if *dim != Dim::Overworld && p.idle >= IDLE_TICKS {
                closing.push(*dim);
            }
        }
        if closing.is_empty() {
            return Ok(());
        }
        let back = self.dim;
        for dim in closing {
            self.enter_level(None, dim)?;
            // Its chunks leave with it: the mods hear before its last save.
            let chunks: Vec<_> = self.world.chunks.values().cloned().collect();
            self.native_chunks_changed(&chunks);
            self.save_level(true)?;
            // Its entities' attachments are saved with them; they leave memory too.
            for uuid in self.persist.uuids.values() {
                self.entity_attachments.remove(uuid);
            }
            self.enter_level(None, back)?;
            self.levels.parked.remove(&dim);
            self.native_level_closed(dim);
        }
        Ok(())
    }

    /// The mods hear `dim`'s level open (`LevelEvent.Load`), on a server.
    pub(super) fn native_level_opened(&mut self, dim: Dim) {
        if self.remote.is_some()
            || self.options.execution_role.is_client()
            || self.levels.heard_open.contains(&dim)
        {
            return;
        }
        self.levels.heard_open.push(dim);
        self.native_level_load(dim.id());
    }

    /// The mods hear `dim`'s level close (`LevelEvent.Unload`), once for
    /// each open they heard.
    fn native_level_closed(&mut self, dim: Dim) {
        let Some(i) = self.levels.heard_open.iter().position(|d| *d == dim) else {
            return;
        };
        self.levels.heard_open.remove(i);
        self.native_level_unload(dim.id());
    }

    /// Every level still open closes with the server, however it ends
    /// (`MinecraftServer.stopServer`'s `level.close()` loop), in vanilla's
    /// order of its levels: the Overworld, the Nether, the End.
    pub(super) fn native_levels_closing(&mut self) {
        for dim in [Dim::Overworld, Dim::Nether, Dim::End] {
            self.native_level_closed(dim);
        }
    }

    /// Save every parked level (`MinecraftServer.saveAllChunks`); the level
    /// in place is the caller's to save.
    pub(super) fn save_parked_levels(&mut self, wait: bool) -> Result<()> {
        if self.levels.parked.is_empty() {
            return Ok(());
        }
        let back = self.dim;
        let dims: Vec<Dim> = self.levels.parked.keys().copied().collect();
        for dim in dims {
            self.enter_level(None, dim)?;
            self.save_level(wait)?;
        }
        self.enter_level(None, back)
    }
}

/// A slow tick's line's end: the steps it noted as its slowest (`spans::note`), whose packets
/// or which player's event, or nothing.
fn slowest_steps(notes: &[(f32, String)]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let list: Vec<String> = notes
        .iter()
        .map(|(ms, what)| format!("{what} {ms:.1}"))
        .collect();
    format!("; slowest: {}", list.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_clear_empties_parked_and_heard_open() {
        let mut levels = Levels::default();
        levels.heard_open.push(Dim::Nether);
        assert!(!levels.heard_open.is_empty());
        levels.clear();
        assert!(levels.heard_open.is_empty());
        assert!(levels.parked.is_empty());
    }

    /// `ServerLevel.getWorldBorder`: `/worldborder` in one level leaves the others' alone.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn each_level_keeps_its_own_world_border() {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        assert!(game.run_command("worldborder set 1000"));
        assert!(game.run_command("execute in minecraft:the_nether run worldborder set 100"));
        assert_eq!(game.dim, Dim::Overworld);
        assert_eq!(game.border.size(), 1000.0);
        let nether = game.levels.parked_border(Dim::Nether).unwrap();
        assert_eq!(nether.size(), 100.0);
    }

    /// A game on a scratch world folder, the way `dedicated.rs` opens one
    /// (`load_world` with an absolute folder).
    fn open_game(
        root: &std::path::Path,
        home: &std::path::Path,
        world: &std::path::Path,
    ) -> Game<'static> {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        let settings = Settings::default();
        let loaded = load_resources(
            root.to_path_buf(),
            None,
            root,
            &settings,
            Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        // The game keeps a reference to its resources for the whole test.
        let resources: &'static _ = Box::leak(Box::new(loaded.resources));
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.to_path_buf();
        options.game_dir = Some(home.to_path_buf());
        // A fixed name: no generated horizon-player.txt in the working directory.
        options.player_name = Some("H260".into());
        options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
        let mut game = Game::new(
            resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(root),
            None,
        )
        .unwrap();
        game.build_registries();
        game.set_allow_commands(true);
        let folder = world.to_string_lossy().into_owned();
        game.load_world(
            crate::game::world_loading::WorldRequest::new(
                &folder,
                Some("h260"),
                Some(260),
                true,
                None,
            )
            .unwrap(),
        )
        .unwrap();
        game
    }

    /// Expected values from the official 26.3 dedicated server restarted on the
    /// same world: each level's `world_border` SavedData is read when the level
    /// is made, so the Overworld's saved border is in place for the first
    /// command, and a first `/worldborder set` is not overwritten later.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn a_reopened_world_has_each_levels_saved_border_from_the_start() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let home = std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap()).join("h260");
        let world = home.join("saves/h260");
        {
            let mut game = open_game(&root, &home, &world);
            // A server ticks before it saves.
            game.tick_world(true).unwrap();
            assert!(game.run_command("worldborder set 50"));
            assert!(game.run_command("execute in minecraft:the_nether run worldborder set 100"));
            game.save_game().unwrap();
        }
        let mut game = open_game(&root, &home, &world);
        let mut failures = Vec::new();
        if game.border.size() != 50.0 {
            failures.push(format!("in place after the open: {}", game.border.size()));
        }
        assert!(game.run_command("worldborder set 70"));
        game.tick_world(true).unwrap();
        assert!(game.run_command("execute in minecraft:the_nether run worldborder get"));
        assert!(game.run_command("execute in minecraft:the_end run worldborder get"));
        assert_eq!(game.dim, Dim::Overworld);
        let sizes = (
            game.border.size(),
            game.levels.parked_border(Dim::Nether).map(|b| b.size()),
            game.levels.parked_border(Dim::End).map(|b| b.size()),
        );
        if sizes != (70.0, Some(100.0), Some(59_999_968.0)) {
            failures.push(format!("after the first set and a tick: {sizes:?}"));
        }
        assert!(failures.is_empty(), "reopened borders: {failures:#?}");
    }

    /// Added controls (H260, after the baseline): a new world, and a read and a
    /// set before the first tick that survive a tick, a save and another reopen.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES and a scratch HORIZON_HOME"]
    fn reopened_borders_keep_through_a_tick_and_a_save_and_a_new_world_has_defaults() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let home = std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
            .join("h260-controls");
        let world = home.join("saves/h260");
        let sizes = |game: &mut Game<'_>| {
            assert!(game.run_command("execute in minecraft:overworld run worldborder get"));
            assert!(game.run_command("execute in minecraft:the_nether run worldborder get"));
            assert!(game.run_command("execute in minecraft:the_end run worldborder get"));
            let parked = |d| game.levels.parked_border(d).map(|b| b.size());
            let size = |d: Dim| {
                if game.dim == d {
                    Some(game.border.size())
                } else {
                    parked(d)
                }
            };
            (size(Dim::Overworld), size(Dim::Nether), size(Dim::End))
        };
        const DEFAULT: f64 = 59_999_968.0;
        let mut failures = Vec::new();
        {
            let mut game = open_game(&root, &home, &world);
            // A new world: every level at the default.
            if game.border.size() != DEFAULT {
                failures.push(format!("new world in place: {}", game.border.size()));
            }
            game.tick_world(true).unwrap();
            let got = sizes(&mut game);
            if got != (Some(DEFAULT), Some(DEFAULT), Some(DEFAULT)) {
                failures.push(format!("new world: {got:?}"));
            }
            assert!(game.run_command("worldborder set 50"));
            assert!(game.run_command("execute in minecraft:the_nether run worldborder set 100"));
            game.save_game().unwrap();
        }
        {
            let mut game = open_game(&root, &home, &world);
            // Before the first tick: the read, then the set.
            if game.border.size() != 50.0 {
                failures.push(format!("first read before a tick: {}", game.border.size()));
            }
            assert!(game.run_command("worldborder set 70"));
            if game.border.size() != 70.0 {
                failures.push(format!("first set before a tick: {}", game.border.size()));
            }
            game.tick_world(true).unwrap();
            let got = sizes(&mut game);
            if got != (Some(70.0), Some(100.0), Some(DEFAULT)) {
                failures.push(format!("after a tick: {got:?}"));
            }
            game.save_game().unwrap();
        }
        {
            let mut game = open_game(&root, &home, &world);
            let got = sizes(&mut game);
            if got != (Some(70.0), Some(100.0), Some(DEFAULT)) {
                failures.push(format!("after the save and another reopen: {got:?}"));
            }
        }
        assert!(failures.is_empty(), "reopened borders: {failures:#?}");
    }

    /// `BlendingData.pack` of an old chunk whose border columns a new chunk's
    /// generation calculated (`getOrUpdateBlendingData`), as the official 26.3
    /// server saves it: a constructed 1.20.4 world (full old chunks, their
    /// partial ring removed, `H077_OLD_WORLD`) where chunk (13, 0) is new.
    #[test]
    #[ignore = "needs HORIZON_TEST_RESOURCES, a scratch HORIZON_HOME and the constructed H077_OLD_WORLD"]
    fn an_old_chunk_saves_the_heights_its_new_neighbour_calculated() {
        use horizon_world::nbt::Tag;
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let home = std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap()).join("h077");
        let world = home.join("saves/h077");
        let source = std::path::PathBuf::from(std::env::var_os("H077_OLD_WORLD").unwrap());
        fn copy(from: &std::path::Path, to: &std::path::Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap().flatten() {
                let target = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &target);
                } else if entry.file_name() != "session.lock" {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        copy(&source, &world);
        {
            let mut game = open_game(&root, &home, &world);
            // The world's level.dat has cheats off: the open read it after open_game's setting.
            game.set_allow_commands(true);
            assert!(game.run_command("forceload add 208 0 239 15"));
            let mut loaded = false;
            // As the dedicated server's loop: a tick, then the chunk streaming.
            for _ in 0..2000 {
                game.tick_world(true).unwrap();
                game.update_world();
                std::thread::sleep(std::time::Duration::from_millis(10));
                if game.run_command_quietly("execute if loaded 215 64 8") {
                    loaded = true;
                    break;
                }
            }
            assert!(loaded, "the new chunk (13, 0) was not generated");
            game.save_game().unwrap();
        }
        let save = horizon_client::save::SaveDir::new(
            horizon_client::save::dimension_dir(&world, "minecraft:overworld"),
            horizon_client::chunk::LevelShape::OVERWORLD,
        );
        let blending = |x, z| {
            save.raw_chunk(x, z)
                .and_then(|r| r.tag.get("blending_data").cloned())
        };
        let heights = |tag: &Tag| match tag.get("heights") {
            Some(Tag::List(list)) => Some(
                list.iter()
                    .map(|v| match v {
                        Tag::Float(bits) => *bits,
                        other => panic!("height {other:?}"),
                    })
                    .collect::<Vec<u32>>(),
            ),
            _ => None,
        };
        let sections = |tag: &Tag| {
            (
                tag.get("min_section").cloned(),
                tag.get("max_section").cloned(),
            )
        };
        let mut failures = Vec::new();
        let mut want = vec![f32::MAX.to_bits(); 12];
        want.extend([70.0f32, 70.0, 69.0, 68.0].map(f32::to_bits));
        match blending(12, 0) {
            Some(tag) => {
                if heights(&tag) != Some(want.clone()) {
                    failures.push(format!("(12, 0) heights {:?}", heights(&tag)));
                }
                if sections(&tag) != (Some(Tag::Int(-4)), Some(Tag::Int(20))) {
                    failures.push(format!("(12, 0) sections {:?}", sections(&tag)));
                }
            }
            None => failures.push("(12, 0) has no blending_data".into()),
        }
        // No calculation reached these: the compound stays, without heights.
        for (x, z) in [(11, 0), (0, 0)] {
            match blending(x, z) {
                Some(tag) => {
                    if heights(&tag).is_some()
                        || sections(&tag) != (Some(Tag::Int(-4)), Some(Tag::Int(20)))
                    {
                        failures.push(format!("({x}, {z}) {tag:?}"));
                    }
                }
                None => failures.push(format!("({x}, {z}) has no blending_data")),
            }
        }
        assert!(failures.is_empty(), "saved blending data: {failures:#?}");
    }

    /// Added control (H077, after the baseline): the exact cache state the save
    /// reads. After the generation of (13, 0), the Overworld folder's lookup
    /// holds the calculated data of the edge chunk (12, 0), with the official
    /// server's heights; an old neighbour without new sides holds no height,
    /// and a far chunk no entry.
    #[test]
    #[ignore = "needs HORIZON_TEST_RESOURCES, a scratch HORIZON_HOME and the constructed H077_OLD_WORLD"]
    fn the_overworld_cache_holds_the_edge_chunks_calculated_heights() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let home =
            std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap()).join("h077-cache");
        let world = home.join("saves/h077");
        let source = std::path::PathBuf::from(std::env::var_os("H077_OLD_WORLD").unwrap());
        fn copy(from: &std::path::Path, to: &std::path::Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap().flatten() {
                let target = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &target);
                } else if entry.file_name() != "session.lock" {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        copy(&source, &world);
        let mut game = open_game(&root, &home, &world);
        game.set_allow_commands(true);
        assert!(game.run_command("forceload add 208 0 239 15"));
        let mut loaded = false;
        for _ in 0..2000 {
            game.tick_world(true).unwrap();
            game.update_world();
            std::thread::sleep(std::time::Duration::from_millis(10));
            if game.run_command_quietly("execute if loaded 215 64 8") {
                loaded = true;
                break;
            }
        }
        assert!(loaded, "the new chunk (13, 0) was not generated");
        assert_eq!(game.dim, Dim::Overworld);
        let mut want = vec![f32::MAX.to_bits(); 12];
        want.extend([70.0f32, 70.0, 69.0, 68.0].map(f32::to_bits));
        let edge = game
            .save
            .calculated_blending(12, 0)
            .expect("(12, 0) was calculated");
        let (min, max, heights) = edge.pack();
        assert_eq!((min, max), (-4, 20));
        assert_eq!(heights.map(|h| h.map(f32::to_bits).to_vec()), Some(want));
        // (11, 0) borders no new chunk: the generation's calculation adds no height
        // to it (it may have an entry), so it packs without heights, as vanilla's.
        let inner = game.save.calculated_blending(11, 0);
        assert!(
            inner.as_ref().is_none_or(|data| data.pack().2.is_none()),
            "{inner:?}"
        );
        // (0, 0) is far from the new chunks: nothing calculated it.
        assert!(game.save.calculated_blending(0, 0).is_none());
    }
}
