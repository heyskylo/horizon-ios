//! The game's shared read-only tables: `Resources` (blocks, items, recipes, mob types, biomes),
//! `UiResources` and `PackResources`.
use super::*;

impl Resources {
    pub(super) fn rest(&self) -> &WorldRest {
        self.rest
            .get()
            .expect("world resources joined before entry")
    }
    pub(super) fn facts(&self) -> &BlockFacts {
        &self.core.facts
    }
    pub(super) fn items(&self) -> &Items {
        &self.rest().items
    }
    pub(super) fn recipes(&self) -> &Recipes {
        &self.rest().recipes
    }
    pub(super) fn transformers(&self) -> &horizon_client::block_transform::Transformers {
        &self.rest().transformers
    }
    pub(super) fn biomes(&self) -> &Arc<Biomes> {
        &self.core.biomes
    }
    pub(super) fn environment(&self) -> &Environment {
        &self.core.environment
    }
    pub(super) fn mob_types(&self) -> &HashMap<String, Arc<MobType>> {
        &self.rest().mob_types
    }
    pub(super) fn mob_data(&self) -> &horizon_client::mobs::MobData {
        &self.rest().mob_data
    }
    pub(super) fn providers(&self) -> &Providers {
        &self.rest().providers
    }
    pub(super) fn block_entity_render(&self) -> &BlockEntityRenderer {
        &self.rest().block_entity_render
    }
    pub(super) fn animate(&self) -> &horizon_client::animate_tick::AnimateTables {
        &self.rest().animate
    }
    pub(super) fn fog(&self) -> &horizon_client::fog::FogTables {
        &self.rest().fog
    }
    pub(super) fn music(&self) -> &horizon_client::music::MusicTables {
        &self.rest().music
    }
    pub(super) fn ambient(&self) -> &horizon_client::ambience::AmbientTables {
        &self.rest().ambient
    }
    pub(super) fn nether_environment(&self) -> &Environment {
        &self.rest().nether_environment
    }
    pub(super) fn end_environment(&self) -> &Environment {
        &self.rest().end_environment
    }
    pub(super) fn behaviours(&self) -> &Behaviours {
        &self.rest().behaviours
    }
    pub(super) fn collision(&self) -> &BlockCollisionRegistry<'static> {
        &self.core.collision
    }
    pub(super) fn suffocation(&self) -> &horizon_world::suffocation::SuffocationRegistry<'static> {
        &self.core.suffocation
    }
    pub(super) fn motion(&self) -> &MotionResources {
        &self.core.motion
    }
    pub(super) fn threads(&self) -> usize {
        self.core.threads
    }
    pub(super) fn player_arm(&self, slim: bool, left: bool) -> &Option<horizon_client::mobs::Part> {
        let arms = if slim {
            &self.rest().player_arm_slim
        } else {
            &self.rest().player_arm
        };
        &arms[usize::from(left)]
    }
    pub(super) fn player_type(&self) -> &Option<Arc<horizon_client::mobs::MobType>> {
        &self.rest().player_type
    }
    pub(super) fn spin_attack(&self) -> &Option<horizon_client::mobs::Part> {
        &self.rest().spin_attack
    }
    pub(super) fn stations(&self) -> &horizon_client::stations::StationData {
        &self.rest().stations
    }
}

impl UiResources {
    /// From a resource root layering packs over `defaults`, the game's own
    /// resources (`root` itself when no pack is on).
    pub fn load(root: &std::path::Path, defaults: &std::path::Path) -> Result<Self> {
        // The title screen waits for both: the panorama's six faces decode
        // beside the atlas.
        std::thread::scope(|scope| {
            let panorama = named_scope(scope, "panorama", || {
                startup::time("ui.panorama", || {
                    horizon_client::gfx::panorama::Faces::load(root, defaults)
                })
            });
            let mod_image = crate::game::mods::built_with_native_mods();
            let atlas = startup::time("ui.atlas", || {
                HudAtlas::build_over_with_image(root, defaults, mod_image)
            });
            let panorama = panorama
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            Ok(Self {
                atlas: atlas?,
                panorama: Arc::new(panorama),
            })
        })
    }
    /// What a server's game holds, since it draws nothing: the font, which books
    /// and chat measure with, and a one-pixel panorama.
    pub fn server(root: &std::path::Path) -> Result<Self> {
        Ok(Self {
            atlas: HudAtlas::fonts(root)?,
            panorama: Arc::new(horizon_client::gfx::panorama::Faces::colour([0; 4])),
        })
    }
}

impl PackResources {
    /// Everything a pack overlay at `root` provides, over `assets` and `render`
    /// loaded from it.
    pub(super) fn load(
        root: &std::path::Path,
        assets: Arc<Assets>,
        render: Arc<RenderAssets>,
        facts: &BlockFacts,
        biomes: Arc<Biomes>,
        light: Arc<LightTables>,
    ) -> Result<Self> {
        let visibility = models::load_visibility(root, assets.registry)?;
        let tables = Arc::new(
            MeshTables::new(&assets, &render, facts, visibility)?
                .with_polytone_colormaps(root, assets.registry),
        );
        let item_renderer = horizon_client::item_render::ItemRenderer::load_with_library(
            root,
            render.model_library.clone(),
        )?;
        Ok(Self::assemble(
            root,
            assets,
            render,
            tables,
            item_renderer,
            biomes,
            light,
        ))
    }
    /// The workers' context over `biomes`: this pack's own when `biomes` is
    /// the table it was built with, else the same parts with that table.
    pub(super) fn context_with(&self, biomes: Arc<Biomes>) -> Arc<WorkerContext> {
        let old = &self.context;
        if Arc::ptr_eq(&old.biomes, &biomes) {
            return old.clone();
        }
        Arc::new(WorkerContext {
            assets: old.assets.clone(),
            render: old.render.clone(),
            tables: old.tables.clone(),
            biomes,
            light: old.light.clone(),
        })
    }
    /// The pack tables over their parts, which `load_resources` builds on
    /// separate workers.
    pub(super) fn assemble(
        root: &std::path::Path,
        assets: Arc<Assets>,
        render: Arc<RenderAssets>,
        tables: Arc<MeshTables>,
        item_renderer: horizon_client::item_render::ItemRenderer,
        biomes: Arc<Biomes>,
        light: Arc<LightTables>,
    ) -> Self {
        // These pack-local resources do not depend on one another. Decode the
        // images and language map together so a custom-sky pack cannot put all
        // of the pack assembly on the critical path behind its own files.
        let (names, clouds, forcefield, custom_sky, environments, polytone_particles) =
            std::thread::scope(|scope| {
                let names = named_scope(scope, "pack.names", || Names::load(root));
                let clouds = named_scope(scope, "pack.clouds", || load_clouds(root));
                let forcefield = named_scope(scope, "pack.forcefield", || load_forcefield(root));
                let custom_sky = named_scope(scope, "pack.custom_sky", || {
                    horizon_client::custom_sky::load(root)
                });
                let environments =
                    named_scope(scope, "pack.environments", || pack_environments(root));
                let polytone_particles = named_scope(scope, "pack.particles", || {
                    pack_particles(root, assets.registry)
                });
                (
                    names
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                    clouds
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                    forcefield
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                    custom_sky
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                    environments
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                    polytone_particles
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                )
            });
        let polytone_post = horizon_client::polytone_post::PolytonePost::read(
            root,
            horizon_client::packs::polytone_configs(&horizon_client::packs::Source::Dir(
                root.to_path_buf(),
            )),
        )
        .map(Arc::new);
        Self {
            content: None,
            layer_names: Arc::new(
                render
                    .sprite_layers
                    .iter()
                    .map(|(name, layer)| (*layer, name.trim_start_matches("minecraft:").to_owned()))
                    .collect(),
            ),
            context: Arc::new(WorkerContext {
                assets: assets.clone(),
                render: render.clone(),
                tables,
                biomes,
                light,
            }),
            assets,
            render,
            item_renderer: Arc::new(item_renderer),
            names: Arc::new(names),
            clouds: Arc::new(clouds),
            forcefield: Arc::new(forcefield),
            custom_sky,
            environments,
            polytone_particles,
            polytone_post,
            polytone_colors: Arc::new(read_colors(root)),
        }
    }
}

/// The overworld's, Nether's and End's attribute stacks with the Polytone biome and dimension
/// modifiers of the packs merged at `root` (#11486); `None` when no pack has any. The overlay
/// root links the vanilla data the stacks start from.
fn pack_environments(root: &std::path::Path) -> Option<Arc<[Environment; 3]>> {
    let modifiers = horizon_client::environment::PackModifiers::read(root);
    if modifiers.is_empty() {
        return None;
    }
    let modifiers = modifiers.with_configs(horizon_client::packs::polytone_configs(
        &horizon_client::packs::Source::Dir(root.to_path_buf()),
    ));
    let load = |d: &str| Environment::load_dimension_with(root, d, &modifiers);
    match (load("overworld"), load("the_nether"), load("the_end")) {
        (Ok(o), Ok(n), Ok(e)) => Some(Arc::new([o, n, e])),
        (o, n, e) => {
            for error in [o.err(), n.err(), e.err()].into_iter().flatten() {
                log::warn!("pack environment: {error}");
            }
            None
        }
    }
}

/// The custom particles and block emitters of the packs merged at `root` (#11486); `None` when
/// no pack has any.
fn pack_particles(
    root: &std::path::Path,
    blocks: &'static horizon_world::registry::BlockRegistry,
) -> Option<Arc<horizon_client::particles::polytone::Registry>> {
    let configs = horizon_client::packs::polytone_configs(&horizon_client::packs::Source::Dir(
        root.to_path_buf(),
    ));
    let registry = horizon_client::particles::polytone::Registry::read(root, configs, blocks);
    (!registry.is_empty()).then(|| Arc::new(registry))
}

/// The packs' custom colours; their dye and sheep colours go to the draw paths' table.
fn read_colors(root: &std::path::Path) -> horizon_client::pack_palette::PackColors {
    let colors = horizon_client::pack_palette::read(root);
    horizon_client::brewing::set_pack_colors(colors.effects.clone());
    colors
}
