//! Prepare local worlds without blocking the window's event loop.
use super::*;

pub(super) struct WorldRequest {
    pub save_root: PathBuf,
    pub allowlist: PathBuf,
    pub session_lock: Option<horizon_client::save::SessionLock>,
    pub folder: String,
    pub name: Option<String>,
    pub seed: Option<i64>,
    pub creative: bool,
    pub create: Option<horizon_client::title::WorldOptions>,
}

pub(super) struct PreparedWorld {
    pub save_root: PathBuf,
    /// The levels' shapes, from the world's dimension types.
    pub shapes: horizon_client::chunk::LevelShapes,
    pub level: Level,
    pub world: World,
    pub streaming: Streaming,
    pub save: Arc<SaveDir>,
    pub session_lock: horizon_client::save::SessionLock,
    pub rand_sequences: horizon_client::random_sequences::RandomSequences,
    pub locator: Option<Arc<VanillaTerrain>>,
}

impl WorldRequest {
    pub fn new(
        folder: &str,
        name: Option<&str>,
        seed: Option<i64>,
        creative: bool,
        create: Option<&horizon_client::title::WorldOptions>,
    ) -> Result<Self> {
        Ok(Self {
            save_root: std::env::current_dir()?.join("saves").join(folder),
            allowlist: std::env::current_dir()?.join("saves/allowed_symlinks.txt"),
            session_lock: None,
            folder: folder.to_owned(),
            name: name.map(str::to_owned),
            seed,
            creative,
            create: create.cloned(),
        })
    }

    pub fn prepare(
        self,
        res: &Resources,
        pack: &PackResources,
        options: &Options,
        settings: &Settings,
        upgrade: &horizon_world::datafix::progress::UpgradeProgress,
    ) -> Result<PreparedWorld> {
        // Before any chunk is read: a loaded container with a loot table is rolled as its chunk
        // is decoded (`save::roll_loaded_loot`), and the spawn chunks are read while the world
        // opens. Every world opens here: in-frame, on the server thread, on a dedicated server.
        horizon_client::structure_blocks::loot_data(
            &options.resources,
            &horizon_world::loot::PackOverlay::NONE,
        );
        let Self {
            save_root,
            allowlist,
            session_lock,
            folder,
            name,
            seed,
            creative,
            create,
        } = self;
        let name = name.as_deref();
        let create = create.as_ref();
        let session_lock = match session_lock {
            Some(lock) => lock,
            None => horizon_client::save::SessionLock::acquire_validated(&save_root, &allowlist)?,
        };
        let session_lock = prepare_world_storage(
            &save_root,
            &folder,
            options.dedicated.is_some(),
            upgrade,
            session_lock,
        )?;
        // RandomSequences: the world's data/minecraft/random_sequences.dat.
        let rand_sequences =
            horizon_client::save::read_dat(&save_root.join("data/minecraft/random_sequences.dat"))
                .and_then(|t| t.get("data").cloned())
                .map(|d| horizon_client::random_sequences::RandomSequences::from_nbt(&d))
                .unwrap_or_default();
        let existing = Level::read(&save_root);
        let created = existing.is_none();
        let level = existing.unwrap_or_else(|| Level {
            name: name.unwrap_or(&folder).to_owned(),
            // WorldOptions.randomSeed when no seed was given.
            seed: seed.unwrap_or_else(|| {
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as u64);
                (t ^ (t >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15) as i64
            }),
            creative,
            day_time: 0,
            health: 20.0,
            food: 20,
            saturation: 5.0,
            ..Level::default()
        });
        let mut level = level;
        // CreateWorldScreen's choices, before streaming reads the generator.
        if created && let Some(o) = create {
            level.hardcore = o.hardcore;
            level.difficulty = horizon_client::title::DIFFICULTIES[o.difficulty()].into();
            level.allow_commands = Some(o.commands(creative));
            horizon_client::save::write_world_gen_settings_with(
                &save_root,
                level.seed,
                o.world_gen(),
                o.flat_settings(),
            )?;
            // WorldCreationUiState.getGameRules, written with the level.
            level.game_rules = o.rules.by_name();
        }
        // Worlds made without the Create New World screen (--world, the
        // control socket) take its default: commands on for Creative only.
        if created {
            level.allow_commands.get_or_insert(creative);
        }
        if let Some(d) = &options.dimension {
            level.dimension = d.clone();
            level.position = None;
        }
        let dim = Dim::from_id(&level.dimension);
        let shapes = level_shapes(&save_root, res.biomes());
        let shape = shapes.get(dim.id());
        let (mut streaming, save, locator) = make_streaming(
            res, pack, options, settings, &save_root, dim, shape, level.seed,
        )?;
        streaming.radius = settings.render_distance;
        let mut world = World::new(res.facts().air, level.seed);
        world.shape = shape;
        world.fast_lava = dim == Dim::Nether;
        world.game_time = level.time;
        world.day_time = options.time.unwrap_or(level.day_time);
        world.weather = level.weather;
        if let Some(w) = options.weather.as_deref() {
            // /weather: clear, rain or thunder for a long duration.
            let (rain, thunder) = (w != "clear", w == "thunder");
            world.weather = horizon_client::world::Weather {
                clear_time: if w == "clear" { 6000 } else { 0 },
                rain_time: 6000,
                thunder_time: 6000,
                raining: rain,
                thundering: thunder,
                rain_level: if rain { (1.0, 1.0) } else { (0.0, 0.0) },
                thunder_level: if thunder { (1.0, 1.0) } else { (0.0, 0.0) },
            };
        }

        growth::install_live_features(res, pack, options);

        // Persist new metadata on the worker too, before publishing the world.
        if !save_root.join("level.dat").exists() {
            level.write(&save_root)?;
        }
        Ok(PreparedWorld {
            save_root,
            shapes,
            level,
            world,
            streaming,
            save,
            session_lock,
            rand_sequences,
            locator,
        })
    }
}

/// LevelStorageAccess creates its DirectoryLock before reading/recovering or upgrading level data.
/// Keep that ownership through every mutation and return it with the prepared world.
fn prepare_world_storage(
    save_root: &std::path::Path,
    folder: &str,
    dedicated: bool,
    upgrade: &horizon_world::datafix::progress::UpgradeProgress,
    session_lock: horizon_client::save::SessionLock,
) -> Result<horizon_client::save::SessionLock> {
    // openWorldLoadLevelData: level data that does not read is RecoverWorldDataScreen's to
    // handle (the title screen shows it); a world opened another way must not become a
    // new one with a random seed under the same name.
    // The dedicated server's `Main` takes level.dat_old without asking
    // (`getUnfixedDataTagWithFallback`): there is no world list to recover from.
    if dedicated && horizon_client::save::fall_back_to_level_dat_old(save_root) {
        log::warn!("Horizon: level.dat of \"{folder}\" did not read; level.dat_old took its place");
    }
    if let Some(issue) = horizon_client::save::level_data_needs_recovery(save_root) {
        let why = match issue {
            horizon_client::save::DatIssue::Missing => "level.dat is missing".to_owned(),
            horizon_client::save::DatIssue::Unreadable(why) => why,
        };
        return Err(format!(
            "Failed to load world \"{folder}\": its level data does not read ({why}). \
         Open it from the world list to try restoring level.dat_old."
        )
        .into());
    }
    horizon_client::save::upgrade_older_world_with(save_root, upgrade)?;
    horizon_client::save::migrate_legacy(save_root)?;
    Ok(session_lock)
}

pub(super) struct WorldLoader {
    pub cancelled: bool,
    /// What the world's upgrade has done (`UpgradeProgress`), which
    /// `FileFixerProgressScreen` shows while the file fixers run.
    pub upgrade: Arc<horizon_world::datafix::progress::UpgradeProgress>,
    handle: JoinHandle<std::result::Result<PreparedWorld, String>>,
}

impl WorldLoader {
    pub fn start(
        request: WorldRequest,
        res: Arc<Resources>,
        pack: PackResources,
        options: Options,
        settings: Settings,
    ) -> Result<Self> {
        let upgrade = Arc::new(horizon_world::datafix::progress::UpgradeProgress::new());
        let worker_upgrade = upgrade.clone();
        let handle = std::thread::Builder::new()
            .name("World initialization".into())
            .spawn(move || {
                request
                    .prepare(&res, &pack, &options, &settings, &worker_upgrade)
                    .map_err(|e| e.to_string())
            })?;
        Ok(Self {
            cancelled: false,
            upgrade,
            handle,
        })
    }

    // Wait for construction and disposal off the window thread. Keeping this
    // handle prevents a queued reopen racing the old session lock.
    pub fn cancel(self) -> Self {
        Self {
            cancelled: true,
            upgrade: self.upgrade.clone(),
            handle: std::thread::spawn(move || {
                drop(self.finish());
                Err("world initialization cancelled".into())
            }),
        }
    }

    pub fn ready(&self) -> bool {
        self.handle.is_finished()
    }

    pub fn finish(self) -> Result<PreparedWorld> {
        self.handle
            .join()
            .map_err(|_| "world initialization worker panicked")?
            .map_err(Into::into)
    }
}

/// How long the preloaded world data outlives the world screens: a player
/// who goes back to the title or to Multiplayer and opens no world for this
/// long gets the memory back.
const PRELOAD_KEPT: std::time::Duration = std::time::Duration::from_secs(60);

enum Preload {
    Idle,
    /// Read, or being read, on the "World data preload" thread. `left`: when
    /// the player last left the world screens.
    Started {
        thread: std::thread::JoinHandle<()>,
        left: Option<std::time::Instant>,
    },
    /// A world opened: the data is the world's now, and stays.
    Used(Option<std::thread::JoinHandle<()>>),
}

static PRELOAD: std::sync::Mutex<Preload> = std::sync::Mutex::new(Preload::Idle);

/// Each frame of the title: read what every world opening loads and keeps
/// for the rest of the process (the worldgen bundle, the loot tables and the
/// registries the server sends) on a helper thread while the player is on the
/// world screens (`on_world_screens`: Singleplayer, Create World and their
/// dialogs), so it overlaps the time spent choosing a world. A world opened
/// meanwhile waits for the part being read instead of reading it again. Not
/// at the title screen itself: many players only go to Multiplayer, and
/// small machines need the memory. Dropped after `PRELOAD_KEPT` away from
/// the world screens without a world opening.
pub(super) fn preload_world_data(
    on_world_screens: bool,
    resources: &std::path::Path,
    registry: &'static horizon_world::registry::BlockRegistry,
) {
    let mut preload = PRELOAD.lock().unwrap_or_else(|e| e.into_inner());
    match &mut *preload {
        Preload::Used(_) => {}
        Preload::Idle if on_world_screens => {
            let resources = resources.to_owned();
            let spawned = std::thread::Builder::new()
                .name("World data preload".into())
                .spawn(move || {
                    horizon_threads::behind_server();
                    // Errors are left for the world opening to report.
                    if let Ok(data) = horizon_client::vanillagen::shared_data(
                        &resources.join("worldgen"),
                        registry,
                    ) {
                        // The Overworld's feature decoration, which every
                        // seed shares: about 140 ms off opening the world.
                        let _ = data.prepare_decoration("minecraft:normal", "minecraft:overworld");
                    }
                    // The preload is the game's own data; the world's packs'
                    // tables are built when it opens (finish_open_world).
                    let _ = horizon_client::structure_blocks::loot_data(
                        &resources,
                        &horizon_world::loot::PackOverlay::NONE,
                    );
                    let _ =
                        super::lan::sync_data(&resources, &horizon_world::loot::PackOverlay::NONE);
                });
            match spawned {
                Ok(thread) => *preload = Preload::Started { thread, left: None },
                Err(e) => log::warn!("Horizon: world data preload: {e}"),
            }
        }
        Preload::Idle => {}
        Preload::Started { left, .. } if on_world_screens => *left = None,
        Preload::Started { thread, left } => {
            let left = *left.get_or_insert_with(std::time::Instant::now);
            if left.elapsed() >= PRELOAD_KEPT && thread.is_finished() {
                horizon_client::vanillagen::forget_shared_data();
                horizon_client::structure_blocks::forget_loot_data();
                super::lan::forget_sync_data();
                horizon_threads::release_free_memory();
                *preload = Preload::Idle;
                log::info!("Horizon: preloaded world data dropped, no world opened");
            }
        }
    }
}

/// A world opens: what `preload_world_data` read is the world's from now on.
pub(super) fn world_data_used() {
    let mut preload = PRELOAD.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::mem::replace(&mut *preload, Preload::Idle);
    *preload = Preload::Used(match previous {
        Preload::Started { thread, .. } => Some(thread),
        Preload::Used(thread) => thread,
        Preload::Idle => None,
    });
}

/// Keep the preload's handle through world entry so engine shutdown can drain it.
pub(super) fn shutdown_preload() -> bool {
    let mut preload = PRELOAD.lock().unwrap_or_else(|e| e.into_inner());
    let running = match &*preload {
        Preload::Started { thread, .. } | Preload::Used(Some(thread)) => !thread.is_finished(),
        _ => false,
    };
    if running {
        return false;
    }
    match std::mem::replace(&mut *preload, Preload::Idle) {
        Preload::Started { thread, .. } | Preload::Used(Some(thread)) => {
            let _ = thread.join();
        }
        _ => {}
    }
    true
}

/// Called after the engine and its streaming pools are gone.
pub(super) fn release_world_data() {
    horizon_client::vanillagen::forget_shared_data();
    horizon_client::structure_blocks::forget_loot_data();
    super::lan::forget_sync_data();
    horizon_threads::release_free_memory();
}

/// A server's world opens: the loot tables its first placed block or broken
/// container needs (`loot_data`) are read on a helper thread while the
/// connection is made, so the game thread never waits on them (~0.6 s on a
/// phone). Kept from then on, as a local world keeps them.
pub(super) fn preload_server_loot(resources: &std::path::Path) {
    world_data_used();
    if horizon_client::structure_blocks::loaded_loot_data().is_some() {
        return;
    }
    let resources = resources.to_owned();
    let spawned = std::thread::Builder::new()
        .name("Loot preload".into())
        .spawn(move || {
            horizon_threads::behind_server();
            let _ = horizon_client::structure_blocks::loot_data(
                &resources,
                &horizon_world::loot::PackOverlay::NONE,
            );
        });
    if let Err(e) = spawned {
        log::warn!("Horizon: loot preload: {e}");
    }
}

/// The worldgen entries of the world's enabled data packs, logging what
/// the generator leaves out of them.
fn world_worldgen_overrides(
    world_root: &std::path::Path,
    biomes: &horizon_client::biome::Biomes,
) -> horizon_world::worldgen::overrides::WorldgenOverrides {
    let packs = horizon_client::datapack::discover(world_root);
    let enabled = horizon_client::datapack::enabled_ids(world_root, &packs);
    let enabled: Vec<_> = enabled
        .iter()
        .filter_map(|id| packs.iter().find(|p| &p.id == id))
        .collect();
    let (overrides, warnings) =
        horizon_client::datapack::worldgen_overrides(&enabled, |b| biomes.contains(b));
    for warning in warnings {
        log::warn!("Horizon: data pack worldgen: {warning}");
    }
    overrides
}

/// The shapes of the world's levels: their dimension types, the world's
/// enabled data packs' over vanilla's. A type the codec refuses keeps the
/// vanilla shape, with a warning.
pub(super) fn level_shapes(
    world_root: &std::path::Path,
    biomes: &horizon_client::biome::Biomes,
) -> horizon_client::chunk::LevelShapes {
    use horizon_client::chunk::{LevelShape, LevelShapes};
    let packs = horizon_client::datapack::discover(world_root);
    let enabled = horizon_client::datapack::enabled_ids(world_root, &packs);
    let enabled: Vec<_> = enabled
        .iter()
        .filter_map(|id| packs.iter().find(|p| &p.id == id))
        .collect();
    let mut shapes = LevelShapes::VANILLA;
    if enabled.is_empty() {
        return shapes;
    }
    // What the packs leave out is logged when the generator reads them.
    let (overrides, _) =
        horizon_client::datapack::worldgen_overrides(&enabled, |b| biomes.contains(b));
    for level in [
        "minecraft:overworld",
        "minecraft:the_nether",
        "minecraft:the_end",
    ] {
        let shape = overrides.level_height(level).and_then(|h| {
            LevelShape::new(h.min_y, h.height, h.logical_height, h.has_skylight).ok()
        });
        match shape {
            Some(shape) => *shapes.get_mut(level) = shape,
            None => log::warn!(
                "Horizon: data pack worldgen: the dimension type of {level} is not one the \
                 game accepts; the level keeps the vanilla build height"
            ),
        }
    }
    shapes
}

pub(super) fn make_streaming(
    res: &Resources,
    pack: &PackResources,
    options: &Options,
    settings: &Settings,
    world_root: &std::path::Path,
    dim: Dim,
    shape: horizon_client::chunk::LevelShape,
    seed: i64,
) -> Result<(Streaming, Arc<SaveDir>, Option<Arc<VanillaTerrain>>)> {
    let save = Arc::new(SaveDir::new(
        horizon_client::save::dimension_dir(world_root, dim.id()),
        shape,
    ));
    let mut locator = None;
    let flat = (dim == Dim::Overworld)
        .then(|| horizon_client::save::world_gen_settings(world_root))
        .flatten()
        .and_then(|s| {
            horizon_client::flatgen::FlatTerrain::from_settings(
                &s,
                pack.assets.registry,
                res.biomes(),
                shape,
            )
        });
    let mut streaming = if let Some(inner) = flat {
        Streaming::new(
            Arc::new(Persistent {
                inner: Arc::new(inner),
                save: save.clone(),
                registry: pack.assets.clone(),
                biomes: res.biomes().clone(),
            }),
            pack.context.clone(),
            res.threads(),
        )
    } else if options.test_terrain && dim == Dim::Overworld {
        let inner = Arc::new(TestTerrain::new(pack.assets.registry, res.biomes())?);
        Streaming::new(
            Arc::new(Persistent {
                inner,
                save: save.clone(),
                registry: pack.assets.clone(),
                biomes: res.biomes().clone(),
            }),
            pack.context.clone(),
            res.threads(),
        )
    } else {
        // WorldOptions.generateStructures, from the world's settings.
        let structures =
            horizon_client::save::WorldGen::read(world_root).is_none_or(|w| w.structures);
        let preset = horizon_client::save::WorldGen::preset(world_root);
        let overrides = world_worldgen_overrides(world_root, res.biomes());
        let inner = Arc::new(VanillaTerrain::for_world_with(&options.resources.join("worldgen"), &overrides, pack.assets.registry, res.biomes(), seed, preset, dim.id(), structures).map_err(|e| {
                if overrides.is_empty() {
                    format!("{e}\nPrepare the worldgen bundle with the launcher, or: horizon-prepare --prepare")
                } else {
                    format!("{e}\nThe world's data packs change its world generation: one of them may not be made for this version")
                }
            })?);
        if dim == Dim::Overworld && structures {
            inner.search_rings_behind();
        }
        locator = Some(inner.clone());
        Streaming::new(
            Arc::new(Persistent {
                inner,
                save: save.clone(),
                registry: pack.assets.clone(),
                biomes: res.biomes().clone(),
            }),
            pack.context.clone(),
            res.threads(),
        )
    };
    streaming.forced = save.load_forced_chunks();
    streaming.radius = settings.render_distance;
    streaming.biome_blend = settings.biome_blend;
    streaming.smooth_lighting = settings.smooth_lighting;
    streaming.cutout_leaves = settings.cutout_leaves;
    streaming.biome_zoom = BiomeZoom::new(obfuscate_seed(seed));
    streaming.sky_light = dim != Dim::Nether;
    streaming.nether_shading = dim == Dim::Nether;
    // A server draws nothing: neither the integrated server nor the
    // dedicated one meshes sections.
    streaming.mesh = options.execution_role.is_client();
    Ok((streaming, save, locator))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage_fixture(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "horizon-storage-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn locked_world_cannot_be_recovered_or_migrated() {
        use horizon_world::nbt::Tag;
        let root = storage_fixture("locked");
        std::fs::write(root.join("level.dat"), b"broken current data").unwrap();
        horizon_client::save::write_dat(
            &root.join("level.dat_old"),
            Tag::compound([(
                "Data",
                Tag::compound([
                    ("DataVersion", Tag::Int(horizon_world::DATA_VERSION)),
                    ("LevelName", Tag::String("recovery fixture".into())),
                ]),
            )]),
        )
        .unwrap();
        std::fs::create_dir(root.join("region")).unwrap();
        std::fs::write(root.join("region/r.0.0.mca"), b"legacy region").unwrap();
        let old = std::fs::read(root.join("level.dat_old")).unwrap();
        let held = horizon_client::save::SessionLock::acquire(&root).unwrap();
        let progress = horizon_world::datafix::progress::UpgradeProgress::new();
        let error = prepare_storage_fixture(&root, true, &progress)
            .err()
            .unwrap();
        assert!(error.to_string().contains("already open"));
        assert_eq!(
            std::fs::read(root.join("level.dat")).unwrap(),
            b"broken current data"
        );
        assert_eq!(std::fs::read(root.join("level.dat_old")).unwrap(), old);
        assert!(root.join("region/r.0.0.mca").is_file());
        assert!(!root.join("dimensions").exists());
        drop(held);
        let opened = prepare_storage_fixture(&root, true, &progress).unwrap();
        assert!(horizon_client::save::level_data_needs_recovery(&root).is_none());
        assert!(
            root.join("dimensions/minecraft/overworld/region/r.0.0.mca")
                .is_file()
        );
        drop(opened);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn prepare_storage_fixture(
        root: &std::path::Path,
        dedicated: bool,
        progress: &horizon_world::datafix::progress::UpgradeProgress,
    ) -> Result<horizon_client::save::SessionLock> {
        let lock = horizon_client::save::SessionLock::acquire_validated(
            root,
            &root.join("allowed_symlinks.txt"),
        )?;
        prepare_world_storage(root, "fixture", dedicated, progress, lock)
    }

    #[cfg(unix)]
    #[test]
    fn forbidden_session_link_is_rejected_before_target_is_written() {
        let root = storage_fixture("symlink");
        let target = root.with_extension("sentinel");
        std::fs::write(&target, b"must survive opening").unwrap();
        std::os::unix::fs::symlink(&target, root.join("session.lock")).unwrap();
        let progress = horizon_world::datafix::progress::UpgradeProgress::new();
        let error = prepare_storage_fixture(&root, false, &progress)
            .err()
            .unwrap();
        assert!(error.to_string().contains("forbidden symbolic links"));
        assert_eq!(std::fs::read(&target).unwrap(), b"must survive opening");
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_file(target).unwrap();
    }

    #[test]
    fn failed_preparation_releases_world_lock() {
        let root = storage_fixture("failed");
        std::fs::write(root.join("level.dat"), b"broken current data").unwrap();
        let progress = horizon_world::datafix::progress::UpgradeProgress::new();
        assert!(prepare_storage_fixture(&root, false, &progress).is_err());
        assert!(!horizon_client::save::SessionLock::is_locked(&root));
        let retry = horizon_client::save::SessionLock::acquire(&root).unwrap();
        drop(retry);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn used_preload_is_still_drained_before_shutdown() {
        let (release, wait) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            wait.recv().unwrap();
        });
        *PRELOAD.lock().unwrap() = Preload::Started { thread, left: None };
        world_data_used();
        assert!(!shutdown_preload());
        release.send(()).unwrap();
        loop {
            let ready = match &*PRELOAD.lock().unwrap() {
                Preload::Used(Some(thread)) => thread.is_finished(),
                _ => panic!("preload handle lost"),
            };
            if ready {
                break;
            }
            std::thread::yield_now();
        }
        assert!(shutdown_preload());
        assert!(matches!(*PRELOAD.lock().unwrap(), Preload::Idle));
        assert!(shutdown_preload());
    }
    #[test]
    fn polling_does_not_wait_for_world_initialization() {
        let (release, wait) = std::sync::mpsc::channel();
        let loader = WorldLoader {
            cancelled: false,
            upgrade: Default::default(),
            handle: std::thread::spawn(move || {
                wait.recv().unwrap();
                Err("test world failure".into())
            }),
        };
        assert!(!loader.ready());
        release.send(()).unwrap();
        assert_eq!(
            loader.finish().err().unwrap().to_string(),
            "test world failure"
        );
    }

    #[test]
    fn cancellation_keeps_initialization_tracked_until_it_has_stopped() {
        let (release, wait) = std::sync::mpsc::channel();
        let loader = WorldLoader {
            cancelled: false,
            upgrade: Default::default(),
            handle: std::thread::spawn(move || {
                wait.recv().unwrap();
                Err("cancelled test world".into())
            }),
        }
        .cancel();
        assert!(loader.cancelled);
        assert!(!loader.ready());
        release.send(()).unwrap();
        assert_eq!(
            loader.finish().err().unwrap().to_string(),
            "world initialization cancelled"
        );
    }

    #[test]
    fn worker_panics_reach_the_client() {
        let loader = WorldLoader {
            cancelled: false,
            upgrade: Default::default(),
            handle: std::thread::spawn(|| panic!("broken world initialization")),
        };
        assert_eq!(
            loader.finish().err().unwrap().to_string(),
            "world initialization worker panicked"
        );
    }

    #[test]
    fn prepared_world_can_cross_threads() {
        fn send<T: Send>() {}
        send::<PreparedWorld>();
    }
}
