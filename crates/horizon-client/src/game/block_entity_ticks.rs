//! `Level.tickBlockEntities` for the block entities ticked together here
//! (hoppers, crafters, jukeboxes, beacons, creaking hearts, spawners, trial
//! spawners, vaults, beehives, furnaces and potent sulfur): only those whose state has a
//! ticker, in vanilla's list order (`horizon_client::block_entities`), in
//! chunks that tick blocks. Sculk, brewing stands and pistons tick in their
//! own passes.
use super::Game;
use horizon_client::{
    Result,
    block_entities::{Pos, Ticking},
    block_entity_anim::Ticker,
    containers::{BlockEntity, FurnaceKind, HOPPER_SLOTS},
    furnace::FurnaceCooking,
};
use horizon_core::position::BlockPos;
use horizon_world::registry::BlockStateId;

impl Game<'_> {
    pub(super) fn tick_block_entities(&mut self) -> Result<()> {
        self.tick_mount_menu();
        self.tick_craft_table();
        self.tick_container_reach();
        self.load_entity_types();
        self.ensure_stateful_entity_table();
        if !self.world.block_entities.has_states() {
            let states = self.ticking_states();
            self.world.block_entities.set_states(states);
            let containers = self.native_containers();
            self.world.block_entities.set_mod_containers(containers);
            let fluids = self.native_fluid_tables();
            self.world.block_entities.set_mod_fluids(fluids);
            let energy = self.native_energy_tables();
            self.world.block_entities.set_mod_energy(energy);
        }
        // Only lingering cooldowns are dropped; a hopper that ticks checks
        // its own block. Once a second is soon enough for the rest.
        if self.world.game_time % 20 == 0 {
            self.forget_gone_hoppers();
        }
        let ticking = self.block_ticking_area();
        self.spawner_tick_positions.clear();
        self.trial_spawner_tick_positions.clear();
        self.item_columns = Some(Default::default());
        self.pass_mobs = Some(Default::default());
        for slot in self.world.block_entity_tickers().iter().copied() {
            let pos = slot.0;
            // Ticks before this one can remove it (`TickingBlockEntity.isRemoved`).
            if self.world.block_entities.is_asleep(&pos)
                || !self.world.block_entities.is_live(slot)
                || !ticking(pos.0, pos.2)
                || !self.border.contains(f64::from(pos.0), f64::from(pos.2))
            {
                continue;
            }
            match self.world.block_entities.get(&pos) {
                Some(BlockEntity::Chest(items)) if items.len() == HOPPER_SLOTS => {
                    self.tick_hopper(pos)
                }
                Some(BlockEntity::Crafter(_)) => self.tick_crafter(pos),
                Some(BlockEntity::Jukebox(_)) => self.tick_jukebox(pos),
                Some(BlockEntity::Beacon(_)) => self.tick_beacon_at(pos),
                Some(BlockEntity::CreakingHeart(_)) => self.tick_creaking_heart(pos)?,
                Some(BlockEntity::Spawner(_)) => {
                    self.tick_spawner(pos)?;
                    self.spawner_tick_positions.push(pos);
                }
                Some(BlockEntity::TrialSpawner(_)) => {
                    self.tick_trial_spawner(pos)?;
                    self.trial_spawner_tick_positions.push(pos);
                }
                Some(BlockEntity::Vault(_)) => self.tick_vault(pos)?,
                Some(BlockEntity::Beehive(_)) => self.tick_beehive(pos),
                Some(BlockEntity::Furnace(_)) => self.tick_furnace(pos)?,
                Some(BlockEntity::Brewing(_)) => self.tick_brewing_stand(pos)?,
                Some(BlockEntity::DaylightDetector) => self.tick_daylight_detector(pos),
                Some(BlockEntity::Campfire(_)) => self.tick_campfire(pos)?,
                Some(BlockEntity::Mod(_)) => self.tick_mod_block_entity(pos),
                Some(BlockEntity::PotentSulfur(_)) => {
                    self.tick_potent_sulfur(pos)?;
                    // A window that simulates the world itself is its own
                    // client; a server's clients tick it on their own.
                    if self.options.execution_role.is_client() {
                        self.client_tick_potent_sulfur(pos);
                    }
                }
                _ => {}
            }
        }
        // Items move between passes: the columns hold for this one only.
        self.item_columns = None;
        self.pass_mobs = None;
        self.update_song_volumes();
        self.scan_for_hearts();
        // The client ticks (particles, sounds, the turning mob) are for a
        // window; a dedicated or integrated server has none.
        if self.options.dedicated.is_none() {
            let spawners = std::mem::take(&mut self.spawner_tick_positions);
            self.client_tick_spawners(&spawners);
            self.spawner_tick_positions = spawners;
            let trial_spawners = std::mem::take(&mut self.trial_spawner_tick_positions);
            for key in trial_spawners.iter().copied() {
                self.client_tick_trial_spawner(key);
            }
            self.trial_spawner_tick_positions = trial_spawners;
        }
        // `ShulkerBoxBlockEntity.tick`: the lids of the boxes this level
        // animates. Their grown collision shape reaches the collision queries
        // through `World::shulkers`, and an opening lid pushes what it runs
        // into (`moveCollidedEntities`), as a moving piston does.
        let pushes = self.tick_shulkers();
        self.sync_shulkers();
        self.push_shulker_entities(pushes);
        Ok(())
    }
    /// `ShulkerBoxBlockEntity.tick` for the boxes in block-ticking chunks, in
    /// position order: the lid's new progress, and the swept box of an opening
    /// lid for `moveCollidedEntities`. A box whose block is gone is dropped.
    /// The boxes are those the level animates: a box opens (`start_open` and
    /// block event 1) before it has one, exactly as vanilla's block entity is
    /// there from the start.
    fn tick_shulkers(&mut self) -> Vec<super::piston_push::ShulkerPush> {
        let ticking = self.block_ticking_area();
        let mut pushes = Vec::new();
        let mut gone = Vec::new();
        let positions: Vec<[i32; 3]> = self.be_anims.shulkers.keys().copied().collect();
        for pos in positions {
            let state = self.world.block(pos[0], pos[1], pos[2]);
            let Some(Ticker::Shulker { facing }) = self.be_anims.ticker(state.0) else {
                gone.push(pos);
                continue;
            };
            if !ticking(pos[0], pos[2])
                || !self.border.contains(f64::from(pos[0]), f64::from(pos[2]))
            {
                continue;
            }
            let tick = self
                .be_anims
                .shulkers
                .get_mut(&pos)
                .map(|shulker| shulker.tick(pos, facing));
            if let Some(push) = tick.and_then(|tick| tick.push) {
                pushes.push((pos, facing, push));
            }
        }
        for pos in gone {
            self.be_anims.shulkers.remove(&pos);
        }
        pushes
    }
    /// Copies the shulker box lids into the world, where the collision queries
    /// read them (`SceneCollisions::shulker_collision_state`), as
    /// `sync_moving_pistons` does for pistons.
    pub(super) fn sync_shulkers(&mut self) {
        self.world.shulkers.clear();
        self.world
            .shulkers
            .extend(self.be_anims.shulkers.iter().map(|(p, s)| (*p, s.clone())));
    }
    /// `BlockState.getTicker` of every state, for the block entities ticked
    /// here. A creaking heart keeps its slot while uprooted (see
    /// `tick_creaking_heart`).
    fn ticking_states(&self) -> Vec<Ticking> {
        let registry = self.pack.assets.registry;
        (0..registry.len())
            .map(|i| {
                let state = BlockStateId(i as u32);
                let name: &str = &self.res.facts().get(state).name;
                match name {
                    "minecraft:hopper"
                    | "minecraft:crafter"
                    | "minecraft:beacon"
                    | "minecraft:creaking_heart"
                    | "minecraft:spawner"
                    | "minecraft:trial_spawner"
                    | "minecraft:vault"
                    | "minecraft:beehive"
                    | "minecraft:bee_nest"
                    | "minecraft:brewing_stand"
                    | "minecraft:daylight_detector"
                    // Lit, cookTick; unlit, cooldownTick.
                    | "minecraft:campfire"
                    | "minecraft:soul_campfire" => Ticking::Yes,
                    // Every state but dry has a ticker.
                    "minecraft:potent_sulfur" => {
                        let dry = registry
                            .state(state)
                            .and_then(|s| s.properties.get("potent_sulfur_state"))
                            .is_some_and(|v| v == "dry");
                        if dry { Ticking::Idle } else { Ticking::Yes }
                    }
                    "minecraft:jukebox" => {
                        let record = registry
                            .state(state)
                            .and_then(|s| s.properties.get("has_record"))
                            .is_some_and(|v| v == "true");
                        if record { Ticking::Yes } else { Ticking::Idle }
                    }
                    n if FurnaceKind::from_block(n).is_some() => Ticking::Yes,
                    n if !n.starts_with("minecraft:") && self.native_ticks(n) => Ticking::Yes,
                    _ => Ticking::No,
                }
            })
            .collect()
    }
    /// `LevelChunk.isTicking` (block coordinates): a block-ticking chunk from
    /// every player's tickets (`update_chunk_tickets`), its entities loaded
    /// (`ServerLevel.areEntitiesLoaded`).
    pub(super) fn block_ticking_area(&self) -> impl Fn(i32, i32) -> bool + use<> {
        let levels = self.tickets.levels();
        let pending = self.world.entities_pending.clone();
        move |x, z| levels.block_ticking_at(x, z) && !pending.contains(&(x >> 4, z >> 4))
    }
    /// `DaylightDetectorBlock.tickEntity` of the detector at `pos`.
    fn tick_daylight_detector(&mut self, pos: Pos) {
        // It updates every 20 game ticks (the behaviour checks this too).
        if self.world.game_time % 20 != 0 {
            return;
        }
        let at = BlockPos::new(pos.0, pos.1, pos.2);
        self.with_level(|b, level, ticks| b.tick_daylight_detector(level, ticks, at));
        let _ = self.process_level_events();
    }
    /// `AbstractFurnaceBlockEntity.serverTick` of the furnace at `pos`, and
    /// its block's `lit` following it.
    fn tick_furnace(&mut self, pos: Pos) -> Result<()> {
        // What the hooks need from before the tick, without cloning stacks: the
        // fuel's id only while the furnace is out (it can light this tick), the
        // input's id (a last item is gone after it smelts) and the result count.
        let before = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Furnace(f)) => (
                f.cooking_timer,
                (f.lit_remaining == 0)
                    .then(|| f.items[1].as_ref().map(|s| s.item.clone()))
                    .flatten(),
                f.items[0].as_ref().map(|s| s.item.clone()),
                f.items[2].as_ref().map_or(0, |s| s.count),
            ),
            _ => return Ok(()),
        };
        let mut drops = Vec::new();
        let (lit, burning, smelted, smelting) = {
            let Some(BlockEntity::Furnace(f)) = self.world.block_entities.get_mut(&pos) else {
                return Ok(());
            };
            let lit = f.tick(
                world_recipes!(self),
                world_items!(self),
                self.res.providers(),
                &mut drops,
            );
            let burning = before.1.filter(|_| f.lit_remaining > 0);
            let smelted = f.items[2]
                .as_ref()
                .filter(|after| after.count > before.3)
                .map(|after| (before.2.clone(), after.item.clone()));
            let started = before.0 == 0 && f.cooking_timer > 0;
            let smelting = started
                .then(|| f.items[0].as_ref().map(|item| item.item.clone()))
                .flatten();
            (lit, burning, smelted, smelting)
        };
        if let Some(item) = smelting {
            self.native_furnace_start_smelt([pos.0, pos.1, pos.2], &item);
        }
        if let Some(fuel) = burning {
            self.native_furnace_burn([pos.0, pos.1, pos.2], &fuel);
        }
        if let Some((Some(item), result)) = smelted {
            self.native_furnace_smelt([pos.0, pos.1, pos.2], &item, &result);
        }
        for stack in drops {
            self.campfire_drop(pos, stack);
        }
        let Some(lit) = lit else {
            if matches!(self.world.block_entities.get(&pos), Some(BlockEntity::Furnace(f)) if f.idle())
            {
                self.world.block_entities.sleep(pos);
            }
            return Ok(());
        };
        let state = self.world.block(pos.0, pos.1, pos.2);
        if let Some(mut s) = self.pack.assets.registry.state(state).cloned()
            && s.properties.contains_key("lit")
        {
            s.properties.insert("lit".into(), lit.to_string());
            if let Ok(id) = self.pack.assets.registry.resolve(&s) {
                self.level_set([pos.0, pos.1, pos.2], id)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod jukebox_stop_tests {
    use super::Game;
    use horizon_client::items::ItemStack;

    /// A windowless game from the prepared resources with a resident chunk and
    /// a jukebox at (0, 64, 0), recording its game events; `None` without the
    /// resources. `f` gets the game and the disc.
    fn with_jukebox(f: impl FnOnce(&mut Game<'_>, &ItemStack)) -> Option<Vec<String>> {
        use crate::game::{Audio, Options, Progress, Settings, load_resources};
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES")?);
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .ok()?;
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
        game.world.chunks.insert(
            (0, 0),
            std::sync::Arc::new(horizon_client::chunk::Chunk::empty(0, 0)),
        );
        let jukebox = game
            .pack
            .assets
            .registry
            .default_state("minecraft:jukebox")?;
        game.world.set(0, 64, 0, jukebox).unwrap();
        let disc = ItemStack::new("minecraft:music_disc_cat", 1);
        crate::game::dispense_fixture::start_recording();
        f(&mut game, &disc);
        Some(crate::game::dispense_fixture::stop_recording())
    }

    fn stops(lines: &[String]) -> Vec<&String> {
        lines
            .iter()
            .filter(|l| l.ends_with("minecraft:jukebox_stop_play"))
            .collect()
    }

    /// `JukeboxSongPlayer.stop` posts `JUKEBOX_STOP_PLAY` at the jukebox, and
    /// `popOutTheItem` (the disc taken out by hand) and the end of a song both
    /// reach it. Horizon stopped the sound and told the parrots but posted the
    /// vibration only when the block itself was broken, so pulling the disc out
    /// was silent to a sculk sensor beside it.
    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn a_jukebox_that_stops_tells_the_listeners() {
        let Some(lines) = with_jukebox(|game, disc| {
            assert!(game.use_jukebox([0, 64, 0], Some(disc)), "the disc goes in");
            assert!(
                game.use_jukebox([0, 64, 0], None),
                "the empty hand pops it out"
            );
        }) else {
            return;
        };
        assert_eq!(
            stops(&lines),
            vec![&"G 0,64,0 minecraft:jukebox_stop_play".to_owned()],
            "{lines:?}"
        );

        // The end of a song is the same stop: tick until the song is over.
        let Some(lines) = with_jukebox(|game, disc| {
            assert!(game.use_jukebox([0, 64, 0], Some(disc)), "the disc goes in");
            for _ in 0..40_000 {
                let playing = matches!(
                    game.world.block_entities.get(&(0, 64, 0)),
                    Some(horizon_client::containers::BlockEntity::Jukebox(j)) if j.playing
                );
                if !playing {
                    break;
                }
                game.tick_jukebox((0, 64, 0));
            }
            // JukeboxSongPlayer.stop posts only while a song plays: taking
            // the finished disc out is silent.
            assert!(
                game.use_jukebox([0, 64, 0], None),
                "the empty hand pops it out"
            );
        }) else {
            return;
        };
        assert_eq!(
            stops(&lines).len(),
            1,
            "the song's own end posts it once, its removal after it nothing: {lines:?}"
        );

        // A broken jukebox posts it from the world's spill instead, and a
        // jukebox that never held a disc posts nothing at all.
        let Some(lines) = with_jukebox(|game, disc| {
            assert!(game.use_jukebox([0, 64, 0], Some(disc)), "the disc goes in");
            game.jukebox_removed([0, 64, 0]);
        }) else {
            return;
        };
        assert!(stops(&lines).is_empty(), "not twice: {lines:?}");
        let Some(lines) = with_jukebox(|game, _| {
            for _ in 0..41 {
                game.tick_jukebox((0, 64, 0));
            }
        }) else {
            return;
        };
        assert!(lines.is_empty(), "{lines:?}");
    }
}
