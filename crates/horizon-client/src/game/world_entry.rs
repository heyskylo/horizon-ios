//! Entering a world without blocking the window: the deferred entry, world creation, the world
//! resources' background load, and the world's own content tables.
use super::*;

impl Game<'_> {
    pub(super) fn defer_world_entry(&mut self, entry: PendingWorldEntry) -> Result<bool> {
        self.poll_world_rest()?;
        if self.rest_loader.is_none() && !self.content_reload_busy() {
            return Ok(false);
        }
        self.perf.world_opening();
        if self.title.is_none() {
            self.title = Some(
                Title::new(
                    std::env::current_dir()?.join("saves"),
                    &self.options.resources,
                )
                .with_user(|| self.options.player_name()),
            );
        }
        self.pending_entry = Some(entry);
        log::info!("Horizon: world entry waiting for coherent resources");
        Ok(true)
    }

    pub(super) fn resume_world_entry(&mut self) -> Result<()> {
        self.poll_integrated_start()?;
        self.poll_world_rest()?;
        self.poll_world_creation();
        if self.rest_loader.is_some() || self.content_reload_busy() {
            return Ok(());
        }
        let on_world_screens = !horizon_ios_launcher::covered()
            && self.title.as_ref().is_some_and(|title| {
                !matches!(
                    title.mode,
                    horizon_client::title::Mode::Main | horizon_client::title::Mode::Multiplayer(_)
                )
            });
        world_loading::preload_world_data(
            on_world_screens,
            &self.options.resources,
            self.pack.assets.registry,
        );
        if std::mem::take(&mut self.pending_pack_reload) {
            self.reload_packs();
        }
        match self.pending_entry.take() {
            Some(PendingWorldEntry::Local {
                folder,
                name,
                seed,
                creative,
                create,
            }) => {
                self.open_world(&folder, name.as_deref(), seed, creative, create.as_deref())?;
            }
            Some(PendingWorldEntry::Server {
                address,
                name,
                cookies,
                lan,
                realm,
            }) => {
                self.connect_server_with_options(&address, &name, cookies, lan, realm)?;
            }
            Some(PendingWorldEntry::StudioReplay(path)) => {
                let path_str = path.to_string_lossy().to_string();
                if let Err(e) = self.open_studio_replay(&path_str) {
                    log::warn!("Horizon Studio: could not open {path_str}: {e}");
                }
            }
            None => {}
        }
        Ok(())
    }

    /// `CreateWorldScreen.openCreateWorldScreen`: read the world generation
    /// data the create screen describes, on a worker, then open the screen
    /// (`title.preparing`).
    pub(super) fn poll_world_creation(&mut self) {
        let Some(title) = self.title.as_ref() else {
            return;
        };
        if !title.preparing() {
            return;
        }
        if self.creation_job.is_none() {
            let dir = self.options.resources.join("worldgen");
            let registry = self.pack.assets.registry;
            // The chosen packs' worldgen over the game's own:
            // `applyNewPackConfig` reloads with the new pack config.
            let overrides = title
                .creation_packs()
                .map(|(dir, packs)| {
                    world_loading::creation_worldgen_overrides(
                        dir.as_deref(),
                        &self.options.resources,
                        &packs,
                        self.res.biomes(),
                    )
                })
                .unwrap_or_default();
            self.creation_job = std::thread::Builder::new()
                .name("world-creation".into())
                .spawn(move || {
                    horizon_client::vanillagen::shared_data_with(&dir, &overrides, registry)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
                .ok();
            return;
        }
        if !self
            .creation_job
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
        {
            return;
        }
        let outcome = self.creation_job.take().and_then(|job| job.join().ok());
        if let Some(title) = self.title.as_mut() {
            match outcome {
                Some(Ok(())) => title.finish_preparing(),
                // `applyNewPackConfig`'s failure: back out of the read with
                // the message, the Create screen's options intact.
                outcome => {
                    let e = outcome
                        .and_then(|r| r.err())
                        .unwrap_or_else(|| "the read never ran".to_owned());
                    log::warn!("Horizon: preparing world creation failed: {e}");
                    // Clear the flag first: `failed` sets the error screen
                    // with Create as its back target, which
                    // `finish_preparing` would replace with a fresh world.
                    title.finish_preparing();
                    title.failed(format!(
                        "{}: {e}",
                        horizon_client::lang::get_or(
                            "createWorld.preparing",
                            "Preparing for world creation..."
                        )
                    ));
                }
            }
        }
    }

    pub(super) fn poll_world_rest(&mut self) -> Result<()> {
        if let Some(loader) = &self.rest_loader
            && loader.ready()?
        {
            self.rest_loader
                .take()
                .ok_or("missing world loader")?
                .finish()?;
        }
        Ok(())
    }

    // ---- the world's data packs' content tables ---------------------------

    /// The content tables the world plays with: its data packs' over the
    /// game's own when it has packs, a server's tags over the game's own on a
    /// server, `Resources.rest`'s own otherwise.
    pub(super) fn items(&self) -> &Items {
        match &self.world_tables {
            Some(t) => &t.items,
            None => self
                .remote
                .as_ref()
                .and_then(|r| r.tables.items.as_deref())
                .unwrap_or_else(|| self.res.items()),
        }
    }
    pub(super) fn mob_suffocation(
        &self,
    ) -> &horizon_world::suffocation::SuffocationRegistry<'static> {
        self.world_tables
            .as_ref()
            .map_or_else(|| self.res.suffocation(), |t| &t.suffocation)
    }
    pub(super) fn recipes(&self) -> &Recipes {
        match &self.world_tables {
            Some(t) => &t.recipes,
            None => self.synced_recipes().unwrap_or_else(|| self.res.recipes()),
        }
    }
    pub(super) fn stations(&self) -> &horizon_client::stations::StationData {
        match &self.world_tables {
            Some(t) => &t.stations,
            None => self.res.stations(),
        }
    }
    /// The enchantments item names and tooltips read: a server's, with the
    /// entries its data packs add, else the world's.
    pub(super) fn enchantments(&self) -> &Arc<horizon_client::enchant::Enchantments> {
        match self
            .remote
            .as_ref()
            .and_then(|r| r.tables.enchantments.as_ref())
        {
            Some(e) => e,
            None => &self.stations().enchantments,
        }
    }
    /// The world's packs' content files, none without packs.
    /// The biome table the world plays with, `None` before a world opens.
    pub(super) fn packed_biomes(&self) -> Option<Arc<Biomes>> {
        let table = world_loading::packed_biomes(
            &self.world_root,
            &self.options.resources,
            self.res.biomes(),
            &self.level.enabled_features,
        );
        (!Arc::ptr_eq(&table, self.res.biomes())).then_some(table)
    }
    /// What the chunk and far-terrain workers mesh with: the pack's context,
    /// over the world's biomes when it has its own.
    pub(super) fn worker_context(&self) -> Arc<WorkerContext> {
        match &self.world_biomes {
            Some(table) => self.pack.context_with(table.clone()),
            None => self.pack.context.clone(),
        }
    }
    pub(super) fn pack_files(&self) -> horizon_world::loot::PackOverlay<'_> {
        pack_files(&self.world_tables)
    }
    /// Load the world's data packs' content tables (`None`: the world has no
    /// packs, and `Resources.rest` stays in use). This re-reads the tables,
    /// so it runs once per world open and on `/reload`.
    pub(super) fn build_world_tables(
        &self,
        reload: bool,
        enabled_features: &[String],
    ) -> Result<(Option<WorldTables>, horizon_client::health::HealthResources)> {
        self.build_world_tables_at(
            &self.options.resources,
            reload,
            enabled_features,
            self.mods.data_switched,
        )
    }
    /// Reload the content tables with the world's packs, as
    /// `/reload` reloads a server's (`ReloadableServerResources`).
    pub(super) fn reload_world_tables(&mut self) -> Result<()> {
        let prepared = self.prepare_reload_at(self.options.resources.clone(), None)?;
        self.commit_content_reload(prepared)
    }

    pub(super) fn join_world_rest(&mut self) -> Result<()> {
        if self.rest_loader.is_some() {
            let waited = Instant::now();
            log::info!("Horizon: waiting for world resources");
            while self.rest_loader.is_some() {
                self.poll_world_rest()?;
                if self.rest_loader.is_some() {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            log::info!(
                "Horizon: world resources joined after {} ms",
                waited.elapsed().as_millis()
            );
        }
        if self.res.rest.get().is_none() {
            return Err("world resources are unavailable".into());
        }
        Ok(())
    }
}

impl Game<'_> {
    pub(super) fn build_world_tables_at(
        &self,
        root: &std::path::Path,
        reload: bool,
        enabled_features: &[String],
        force_tables: bool,
    ) -> Result<(Option<WorldTables>, horizon_client::health::HealthResources)> {
        // The ask of this load, both here and on `/reload`: a native mod
        // recomputes the pack files it contributes every time.
        let mut contributed = Vec::new();
        self.native_data_pack_files(&mut contributed);
        let files = functions::overlay_files(
            &self.world_root,
            root,
            self.cmds.vanilla_disabled,
            enabled_features,
            contributed,
        );
        let overlay = horizon_world::loot::PackOverlay { files: &files };
        let base = if reload {
            &self.health_resources
        } else {
            &self.motion.health
        };
        let health = base.with_packs(root, &overlay, reload)?;
        // With no pack, `res` holds the tables, unless content mods switched
        // live since it was loaded: their data is in the new root.
        if files.is_empty() && !force_tables {
            return Ok((None, health));
        }
        let mut items = Items::load(root, self.pack.assets.registry, self.res.facts(), &overlay)?;
        // Item damage predicates use the same frozen registry as player hurt.
        Arc::make_mut(&mut items.enchantments).bind_damage_catalog(&health.damage);
        // The recipe tables, the stations and the recipe book read one folder:
        // hold its files so it is read once.
        let _recipe_files = horizon_client::json_folder::read(&root.join("data/minecraft/recipe"));
        let recipes = Recipes::load(root, &items, &overlay)?;
        let stations = horizon_client::stations::StationData::load(root, &items, &overlay);
        let mut data_map_types = Vec::new();
        self.native_data_map_types(&mut data_map_types);
        let data_maps = native_data_maps::DataMaps::load(
            &data_map_types,
            &files,
            &items.item_tags,
            &items.block_tags,
        );
        let causes_suffocation = items
            .block_tags
            .get("minecraft:causes_suffocation")
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let suffocation = horizon_client::mobs::suffocation::bind(
            self.res.core.collision.clone(),
            &causes_suffocation,
        )?;
        // A registry whose pack tags don't load keeps the game's own tags
        // (`in_tag` falls back to them), rather than matching no tag at all.
        let pack_tags = WorldTables::PACK_TAG_REGISTRIES
            .into_iter()
            .filter_map(|registry| {
                match horizon_client::items::load_tags(root, registry, &overlay) {
                    Ok(tags) => Some((registry, tags)),
                    Err(e) => {
                        log::warn!("Horizon: the world's {registry} tags are not applied: {e}");
                        None
                    }
                }
            })
            .collect();
        Ok((
            Some(WorldTables {
                suffocation,
                files,
                items,
                recipes,
                stations,
                data_maps,
                pack_tags,
            }),
            health,
        ))
    }
}
