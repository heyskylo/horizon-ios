//! Resource pack selection bound to the game: the pack screen lists, and a
//! full resource reload after a change, while playing.
use super::{
    Game, PackResources, Resources, UiResources, build_renderer, prepare_renderer_textures,
};
use horizon_client::{
    assets::Assets,
    gfx::PreparedTextures,
    hud::HudAtlas,
    lang,
    pack_screen::{self, Compat, PackInfo, Source as PackSource},
    packs::{self, Pack, Position},
    streaming::WorkerContext,
    text::{Style, Text},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where overlay roots are built (beside options.txt).
pub fn cache_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join(".horizon-cache/resourcepacks")
}
pub fn packs_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join("resourcepacks")
}
/// `Util.getPlatform().openPath`: the system file manager on the folder.
pub fn open_folder(dir: &Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        log::warn!("Horizon: could not open {}: {e}", dir.display());
    }
}
/// `Util.getPlatform().openUri`: the system browser on the link.
pub fn open_uri(uri: &str) {
    // URLs need Windows' registered protocol handler, not Explorer's path
    // handling. ShellExecute also preserves the URL as one Unicode value,
    // without interpolating its query string into a command interpreter.
    #[cfg(windows)]
    if let Err(error) = open::that_detached(uri) {
        log::warn!("Horizon: could not open URI {uri}: {error}");
    }
    #[cfg(not(windows))]
    open_folder(Path::new(uri));
}
/// The resource root to load from, given the vanilla root, the selection and
/// a server's packs, which go on top (`Pack.Position.TOP`, fixed).
pub fn startup_root(
    vanilla: &Path,
    selected: &[String],
    server: &[Pack],
) -> std::io::Result<PathBuf> {
    let all = packs::discover(vanilla, &packs_dir());
    let mut enabled = packs::enabled(&all, selected);
    enabled.extend_from_slice(server);
    packs::resource_root(vanilla, &enabled, &cache_dir())
}
fn info(p: &Pack) -> PackInfo {
    let built_in = !p.id.starts_with("file/");
    PackInfo {
        id: p.id.clone(),
        title: p.title.clone(),
        description: p.raw_description.clone(),
        description_style: p.description_style.clone(),
        source: if built_in {
            PackSource::BuiltIn
        } else {
            PackSource::Default
        },
        compat: p.compatibility,
        icon: p.icon_png().as_deref().and_then(pack_screen::load_icon),
        required: false,
        position: Position::Top,
        fixed: false,
    }
}
/// The vanilla pack: required, at the bottom (`BuiltInPackSource`), with the
/// client JAR's `pack.png` that the prepared resources hold.
fn vanilla_info(vanilla: &Path) -> PackInfo {
    PackInfo {
        id: "vanilla".into(),
        title: Text::translate("resourcePack.vanilla.name", &[]),
        description: Text::translate("resourcePack.vanilla.description", &[]),
        description_style: Style::default(),
        source: PackSource::BuiltIn,
        compat: Compat::Compatible,
        icon: std::fs::read(vanilla.join("pack.png"))
            .ok()
            .as_deref()
            .and_then(pack_screen::load_icon),
        required: true,
        position: Position::Bottom,
        fixed: false,
    }
}
/// Shared by the startup menu, which has no world resources yet.
pub(super) fn pack_infos(vanilla: &Path) -> Vec<PackInfo> {
    let mut infos: Vec<PackInfo> = packs::discover(vanilla, &packs_dir())
        .iter()
        .map(info)
        .collect();
    infos.push(vanilla_info(vanilla));
    infos
}
impl Game<'_> {
    pub(super) fn vanilla_root(&self) -> PathBuf {
        if self.ui.vanilla_root.as_os_str().is_empty() {
            self.options.resources.clone()
        } else {
            self.ui.vanilla_root.clone()
        }
    }
    /// The packs the pack screen offers and the current selection
    /// (`PackSelectionScreen` makes its `PackSelectionModel` from them).
    pub(super) fn refresh_pack_lists(&mut self) {
        let infos = pack_infos(&self.vanilla_root());
        let o = &mut self.ui.options;
        o.pack_infos = infos;
        o.pack_selection = self.settings.resource_packs.clone();
        o.pack_screen = None;
    }
    /// `Minecraft.reloadResourcePacks`: rebuild the overlay and reload every
    /// client resource from it, as vanilla's reload listeners do: block
    /// models and sprites (every section is meshed again,
    /// `LevelRenderer.allChanged`), the shared atlas (GUI, item, entity,
    /// particle and font textures), item models, sounds, languages, GUI
    /// metadata and the cloud and world border textures.
    pub(super) fn reload_packs(&mut self) {
        // Pack reload updates tail-owned mob metadata. Keep menus responsive
        // until those resources exist, then apply the latest selection once.
        if self.rest_loader.is_some() || self.world_loader.is_some() {
            self.pending_pack_reload = true;
            return;
        }
        if self.local_pack_reload.is_some() || self.server_packs.reload.is_some() {
            self.pending_pack_reload = true;
            return;
        }
        let vanilla = self.vanilla_root();
        if self.ui.vanilla_root.as_os_str().is_empty() {
            self.ui.vanilla_root = vanilla.clone();
        }
        let selected = self.settings.resource_packs.clone();
        let server = self.server_packs.active.clone();
        let mods = self.settings.mods.enabled.clone();
        let dev_mods = self.options.dev_mods.clone();
        let assets = self.pack.assets.clone();
        let res = self.res.clone();
        let context = self.pack.context.clone();
        let unicode = self.settings.force_unicode;
        let japanese = self.settings.japanese_glyphs;
        match std::thread::Builder::new()
            .name("local-pack-reload".into())
            .spawn(move || {
                stage_pack(
                    &vanilla,
                    &selected,
                    &server,
                    &mods,
                    &dev_mods,
                    &assets,
                    &res,
                    &context,
                    unicode,
                    japanese,
                    &super::Progress::new(),
                )
                .map_err(|error| error.to_string())
            }) {
            Ok(worker) => self.local_pack_reload = Some(worker),
            Err(error) => self.ui.options.resource_error = Some(error.to_string()),
        }
    }

    pub(super) fn poll_local_pack_reload(&mut self) {
        // World preparation captures the current pack context. Let it install
        // its streaming state before swapping atlases and reloading that state.
        if self.rest_loader.is_some() || self.world_loader.is_some() {
            return;
        }
        if self
            .local_pack_reload
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let worker = self.local_pack_reload.take().unwrap();
            let result = worker
                .join()
                .unwrap_or_else(|_| Err("Resource pack worker panicked".into()));
            // A newer selection supersedes this generation; do not briefly apply obsolete resources.
            if !self.pending_pack_reload
                && let Err(error) =
                    result.and_then(|pack| self.apply_staged_pack(pack).map_err(|e| e.to_string()))
            {
                log::warn!("Horizon: resource reload failed: {error}");
                self.ui.options.resource_error = Some(error);
            }
        }
        if self.pending_pack_reload
            && self.local_pack_reload.is_none()
            && self.server_packs.reload.is_none()
            && self.rest_loader.is_none()
            && self.world_loader.is_none()
        {
            self.pending_pack_reload = false;
            self.reload_packs();
        }
    }
    /// The render-thread half: the renderer on the new pack's textures, then
    /// the swap (`ReloadInstance`'s application listeners).
    pub(super) fn apply_staged_pack(&mut self, staged: StagedPack) -> horizon_client::Result<()> {
        let StagedPack {
            root,
            gui,
            pack,
            textures,
        } = staged;
        // Keep the working GPU resources, and hand its window surface over.
        if let Some(renderer) = &mut self.renderer {
            renderer.release_surface();
        }
        let renderer = match self
            .window
            .clone()
            .map(|window| {
                build_renderer(
                    window,
                    self.options.offscreen,
                    &pack,
                    &gui,
                    self.renderer.as_ref(),
                    Some(textures),
                    self.settings.horizon.api,
                )
            })
            .transpose()
        {
            Ok(renderer) => renderer,
            Err(error) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.recreate_surface().map_err(|restore| {
                        format!(
                            "{error}; could not restore the previous rendering surface: {restore}"
                        )
                    })?;
                }
                return Err(error);
            }
        };
        // Everything loaded: switch over.
        self.waypoint_styles = horizon_client::locator_bar::WaypointStyles::load(&root);
        self.armor_stand_assets.reload_trims(&root);
        self.random_tick_sections.clear();
        self.portals.sections.clear();
        self.options.resources = root.clone();
        let context = pack.context.clone();
        self.pack = pack;
        self.gui = gui;
        // The atlas's sprites moved: the particle engine re-reads every rect
        // and the packs' sprite sets, or every particle draws a stale region.
        self.particles
            .reload(&root, &self.gui.atlas.sprites, &self.gui.atlas.translucent);
        horizon_client::ui_theme::reset_atlas();
        // Villager hats come from the pack's texture metadata (shared data, swapped in place).
        *self
            .res
            .mob_data()
            .villager_hats
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            horizon_client::mobs::villager_hats::VillagerHats::load(&root);
        horizon_client::ui::load_metadata(&root);
        lang::set(lang::Language::load(&[&root], &lang::code()));
        self.refresh_remote_language();
        self.audio.reload(&root);
        self.clear_lod();
        self.streaming.reload_resources(&mut self.world, context);
        self.pending_uploads.clear();
        // New GPU textures: a renderer built from the reloaded resources; the
        // sections are uploaded again as they are meshed.
        if let Some(renderer) = renderer {
            self.renderer = Some(renderer);
            self.apply_ui_settings();
        }
        self.ui.chat.rescale(&self.gui.atlas);
        // The creative screen survives the reload; item names may not.
        self.creative_screen.forget_items();
        Ok(())
    }
}

/// The CPU half of a resource reload (local and server packs run it on a
/// worker; `apply_staged_pack` does the GPU half and the swap): the overlay
/// root, the models and sprites, the menu bundle, the pack's tables and its
/// CPU mip chains.
pub(super) fn stage_pack(
    vanilla_root: &Path,
    selected: &[String],
    server: &[Pack],
    mods_enabled: &BTreeSet<String>,
    dev_mods: &[PathBuf],
    assets: &Arc<Assets>,
    res: &Resources,
    context: &WorkerContext,
    force_unicode: bool,
    japanese: bool,
    progress: &super::Progress,
) -> horizon_client::Result<StagedPack> {
    let root = startup_root(vanilla_root, selected, server)?;
    let root = super::mods::content_root(&root, mods_enabled, dev_mods);
    progress.set(1);
    let render = Arc::new(assets.reload(&root)?);
    let mut atlas = HudAtlas::build_over(&root, vanilla_root)?;
    atlas.font.uniform = force_unicode;
    atlas.font.japanese = japanese;
    let gui = UiResources {
        atlas,
        panorama: Arc::new(horizon_client::gfx::panorama::Faces::load(&root)),
    };
    progress.set(2);
    let pack = PackResources::load(
        &root,
        assets.clone(),
        render,
        res.facts(),
        context.biomes.clone(),
        context.light.clone(),
    )?;
    let textures = prepare_renderer_textures(&pack);
    progress.set(3);
    Ok(StagedPack {
        root,
        gui,
        pack,
        textures,
    })
}

/// Everything a pack reload builds on the CPU (`apply_staged_pack` finishes
/// it on the render thread).
pub(super) struct StagedPack {
    pub root: PathBuf,
    pub gui: UiResources,
    pub pack: PackResources,
    pub textures: PreparedTextures,
}
