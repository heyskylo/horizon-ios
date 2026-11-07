//! The window's first renderer and frames: the renderer built from the prepared textures, the
//! boot screen's frame, and `start_game`, which turns the loaded resources into a `Game`.
use super::*;

/// CPU mip chains for the block sprites and world border.
pub(super) fn prepare_renderer_textures(
    pack: &PackResources,
) -> horizon_client::gfx::PreparedTextures {
    let assets = &pack.render;
    horizon_client::gfx::PreparedTextures::new(
        &assets.pixels.get(),
        assets.sprite_size,
        assets.texture_layers,
        &assets.sprite_mips,
        (&pack.forcefield.2, pack.forcefield.0),
    )
    .with_pbr(
        &assets.pbr_normals,
        &assets.pbr_specular,
        assets.pbr_size,
        assets.texture_layers,
    )
}

/// Upload pack textures and create the renderer on the window's device.
pub(super) fn build_renderer(
    window: Arc<Window>,
    offscreen: bool,
    pack: &PackResources,
    gui: &UiResources,
    previous: Option<&Renderer>,
    prepared: Option<horizon_client::gfx::PreparedTextures>,
    api: horizon_client::graphics::GraphicsApi,
    translucent_batching: bool,
    // HDR with Smooth Gradients (`Graphics::wide_frame`): a 16-bit frame where HDR output is
    // offered (`Renderer::new`).
    wide_frame: bool,
    shader_pack: shader_packs::PickedPack,
    filtering: (i32, i32),
) -> Result<Renderer> {
    let assets = &pack.render;
    let atlas = &gui.atlas;
    // The new renderer's atlas texture starts from `HudAtlas.pixels`, which
    // holds the textures but never a glyph rasterized at run time. The font's
    // cache still answers with the rects it drew into the previous renderer's
    // texture, so forget them: the glyphs that are drawn again are placed into
    // this texture (as `ui_theme` does when its atlas changes). Without this,
    // Chinese and Japanese menu labels go blank when the loading screen hands
    // over to the game.
    atlas.font.reset_cache();
    // Made here when the worker did not: the renderer's own fallback has no LabPBR maps.
    let prepared = Some(prepared.unwrap_or_else(|| prepare_renderer_textures(pack)));
    let atlas_pixels = atlas.pixels();
    let mut renderer = pollster::block_on(Renderer::new(
        window,
        offscreen,
        &assets.pixels.get(),
        assets.sprite_size,
        assets.sprite_minimum_size,
        assets.texture_layers,
        &assets.sprite_mips,
        &assets.animations,
        (&atlas_pixels, atlas.width, atlas.height),
        (&atlas.glint.0, atlas.glint.1, atlas.glint.2),
        (
            &atlas.glint_armor.0,
            atlas.glint_armor.1,
            atlas.glint_armor.2,
        ),
        (&pack.forcefield.2, pack.forcefield.0),
        gui.panorama.clone(),
        prepared,
        previous,
        api,
        translucent_batching,
        wide_frame,
        shader_pack.draws_flat,
        shader_pack.linear,
        shader_pack.hdr_float,
    ))?;
    renderer.hud.sprite_animations = atlas
        .sprite_animations
        .iter()
        .map(|(rect, animation)| (*rect, animation.clone(), (usize::MAX, 0, 0)))
        .collect();
    renderer
        .hud
        .set_text_shader(&renderer.device, atlas.text_shader.as_deref());
    pack.item_renderer
        .prepare_glint_atlas(renderer.device.limits().max_texture_dimension_2d, filtering)?;
    renderer.set_lines_shader(atlas.lines_shader.as_deref());
    renderer.set_custom_sky(&pack.custom_sky);
    let cit_glints = pack.item_renderer.cit.as_ref().map(|c| &c.glint_textures);
    renderer
        .hud
        .set_cit_glints(&renderer.device, &renderer.queue, cit_glints);
    Ok(renderer)
}

/// The loading screen's renderer: one atlas (the boot screen's until the
/// pack's GUI atlas is ready) over one opaque white block-sprite layer, an
/// empty border texture and the title panorama once the menu bundle is ready.
pub(super) fn loading_renderer(
    window: Arc<Window>,
    offscreen: bool,
    atlas: Option<&horizon_client::hud::HudAtlas>,
    panorama: Option<&Arc<horizon_client::gfx::panorama::Faces>>,
    previous: Option<&Renderer>,
    api: horizon_client::graphics::GraphicsApi,
) -> Result<Renderer> {
    static FORCEFIELD: [u8; 4] = [0; 4];
    static EMPTY_HUD: [u8; 4] = [0, 0, 0, 255];
    static EMPTY_GLINT: [u8; 4] = [0; 4];
    let panorama = panorama
        .cloned()
        .unwrap_or_else(|| Arc::new(horizon_client::gfx::panorama::Faces::colour([0; 4])));
    let (pixels, width, height) = atlas.map_or((EMPTY_HUD.as_slice(), 1, 1), |a| {
        (a.pixels.as_slice(), a.width, a.height)
    });
    let (glint_pixels, glint_width, glint_height) = atlas
        .map_or((EMPTY_GLINT.as_slice(), 1, 1), |a| {
            (a.glint.0.as_slice(), a.glint.1, a.glint.2)
        });
    // A glyph rasterized into an earlier texture is not in these pixels; the
    // new texture gets it again when the character is drawn (see
    // `build_renderer`).
    if let Some(atlas) = atlas {
        atlas.font.reset_cache();
    }
    let mut renderer = pollster::block_on(Renderer::new(
        window,
        offscreen,
        &[255, 255, 255, 255],
        1,
        1,
        1,
        &[],
        &[],
        (pixels, width, height),
        (glint_pixels, glint_width, glint_height),
        (glint_pixels, glint_width, glint_height),
        (&FORCEFIELD[..], 1),
        panorama,
        None,
        previous,
        api,
        // It draws no terrain.
        false,
        // An 8-bit frame, which HDR output takes as well.
        false,
        false,
        false,
        false,
    ))?;
    if let Some(atlas) = atlas {
        renderer.hud.sprite_animations = atlas
            .sprite_animations
            .iter()
            .map(|(rect, animation)| (*rect, animation.clone(), (usize::MAX, 0, 0)))
            .collect();
    }
    Ok(renderer)
}

pub(super) fn loading_frame(hud: &HudBatch) -> Frame<'_> {
    Frame {
        weather: &[],
        weather_time: horizon_client::weather::Time::new(0, 0.0),
        camera: glam::DVec3::new(0.5, 100.0, 0.5),
        yaw: 0.0,
        pitch: 10.0,
        fov_degrees: 85.0,
        eye: glam::DVec3::new(0.5, 100.0, 0.5),
        eye_rotation: [0.0, 10.0],
        first_person: true,
        bob: glam::Mat4::IDENTITY,
        spin: glam::Mat4::IDENTITY,
        sky: horizon_client::environment::Sky::default(),
        shadow_scene_revision: (0, 0),
        shadow_cache: false,
        distance: 12.0,
        detail_distance: 12.0,
        depth_far: 1000.0,
        underwater: false,
        water_plane: None,
        cave_culling: false,
        title: true,
        sky_blocked: false,
        skybox: false,
        end_sky: false,
        end_sky_sprite: None,
        boss_world_fog: false,
        environment_fog: None,
        water_fog: [0.0; 3],
        fog: None,
        dark_disc: false,
        outline: Vec::new(),
        outline_after_terrain: false,
        outline_high_contrast: false,
        pvp: Default::default(),
        debug_lines: &[],
        debug_lines_on_top: &[],
        debug_crosshair: &[],
        hud,
        glint_alpha: 0.75,
        glint_ticks: 0.0,
        nether_lighting: false,
        entities: &[],
        static_block_entities: &[],
        visible_static_block_entity_sections: &[],
        static_block_entity_stats: Default::default(),
        entity_motion: &[],
        entity_motion_posed: &[],
        entity_spans: &[],
        skin: None,
        glowing: &[],
        dynamic_lights: &[],
        colored_lights: &[],
        emissive_layers: &[],
        clouds: None,
        lightning: Default::default(),
        additive: &[],
        see_through: &[],
        world_primitives: Default::default(),
        shadow_only: &[],
        celestial: &[],
        stars: None,
        hand: &[],
        hand_items: &[],
        hand_fov_degrees: 70.0,
        screen: &[],
        pack: Default::default(),
        border: None,
        game_time: 0.0,
        shader_game_time: 0.0,
        end_portal_sprites: [[0.0; 4]; 2],
        crumbling_sprite: [0.0; 4],
        ticks: 0,
        darkness: (0.0, 0.0),
        night_vision: 0.0,
        block_flicker: 0.0,
        boss_darken: 0.0,
        chunk_fade: 0.0,
        post_effects: horizon_client::post_chain::Request::none(),
        menu_blur: None,
    }
}

/// The post-load setup once the resources are ready: `Game::new` and the
/// options, packs, mods, control socket, benchmark and world selection the
/// old `run` did after loading. `None` means a harness or dedicated run
/// finished here instead.
#[allow(clippy::too_many_arguments)]
pub(super) fn start_game<'a>(
    res: &'a Arc<Resources>,
    pack: PackResources,
    gui: UiResources,
    options: Options,
    pack_error: Option<String>,
    audio: Audio,
    vanilla_root: PathBuf,
    started: Instant,
    rest_loader: Option<WorldRestLoader>,
    settings: Option<Settings>,
    prepared: Option<GameCpuResources>,
    entity_models: Option<Arc<horizon_client::mobs::EntityModelDocument>>,
) -> Result<Option<Game<'a>>> {
    let handoff = prepared.is_some();
    let mut settings = match settings {
        Some(settings) => settings,
        None => Settings::load(&Game::options_path()),
    };
    if let Some(d) = options.distance_override {
        settings.render_distance = d;
    }
    // A synchronous launch has no handoff worker: build its CPU resources
    // here, from the same entity-model document the rest worker read.
    let prepared = match prepared {
        Some(prepared) => Some(prepared),
        None => Some(GameCpuResources::load(
            &options.resources,
            entity_models.as_deref(),
        )?),
    };
    let mut game = Game::new(res, pack, gui, options, settings, audio, prepared)?;
    game.rest_loader = rest_loader;
    game.ui.vanilla_root = vanilla_root;
    if let Some(error) = pack_error {
        // Minecraft.clearResourcePacksOnError: the selection is cleared and
        // saved, so the next launch starts without the packs.
        game.settings.resource_packs.clear();
        game.settings.save(&Game::options_path());
        game.open_options_at(true, horizon_client::options_ui::Page::Packs);
        game.ui.options.resource_error = Some(error);
    }
    if game.options.benchmark.is_some() {
        // The same conditions on every machine: no VSync, no frame cap, no
        // pause or idle limit without focus. In memory only, never saved.
        game.settings.vsync = false;
        game.settings.max_fps = 260;
        game.settings.inactivity_fps_limit = "minimized".into();
        game.options.no_pause = true;
    }
    game.apply_settings();
    // Benchmarks measure the game alone.
    if game.options.benchmark.is_none() {
        game.mods.dev = game.options.dev_mods.clone();
        game.start_mods();
    }
    // After the mods: their templates are among the ids it lists.
    game.preload_registries();
    game.start_agent_control()?;
    if let Some(scenario) = game.options.benchmark.clone() {
        let control = game
            .control
            .get_or_insert_with(horizon_client::control::Server::local);
        horizon_client::benchmark::start(
            control.sender(),
            horizon_client::benchmark::Setup {
                scenario,
                started,
                app: horizon_bench::App {
                    name: "horizon-client".into(),
                    version: horizon_client::crash::version(),
                    commit: option_env!("HORIZON_COMMIT").map(str::to_owned),
                    mods: Vec::new(),
                },
                simulation_distance: game.settings.simulation_distance as u32,
                vsync: false,
                fps_cap: None,
                offscreen: game.options.offscreen,
                game_dir: std::env::current_dir()?,
            },
        );
    }
    if let Some(directory) = std::env::var_os("HORIZON_NATIVE_CLIENT_PARTICLES_FIXTURE") {
        game.run_native_client_particles_fixture(std::path::Path::new(&directory))?;
        return Ok(None);
    }
    if let Some(directory) = std::env::var_os("HORIZON_BOAT_RIDER_FIXTURE") {
        game.run_boat_rider_fixture(std::path::Path::new(&directory))?;
        return Ok(None);
    }
    if let Some(directory) = std::env::var_os("HORIZON_BOAT_AUTHORITY_FIXTURE") {
        game.run_boat_authority_fixture(std::path::Path::new(&directory))?;
        return Ok(None);
    }
    if let Ok(spec) = std::env::var("HORIZON_DISPENSE_FIXTURE") {
        game.run_component_harness(integrated::ComponentHarness::Dispenser(spec))?;
        return Ok(None);
    }
    if let Ok(spec) = std::env::var("HORIZON_SCULK_FIXTURE") {
        game.run_component_harness(integrated::ComponentHarness::Sculk(spec))?;
        return Ok(None);
    }
    if game.options.execution_role == execution_role::ExecutionRole::DedicatedServer {
        game.run_dedicated()?;
        return Ok(None);
    }
    match game.options.world.clone() {
        _ if game.options.server.is_some() => {
            let (address, name) = (
                game.options.server.clone().unwrap_or_default(),
                game.options.player_name(),
            );
            game.connect_server(&address, &name)?;
            if let Some(report) = std::env::var_os("HORIZON_NET_REPORT") {
                game.run_net_harness(report.as_ref())?;
                return Ok(None);
            }
        }
        Some(folder) => {
            let (seed, creative) = (game.options.seed, game.options.creative);
            if std::env::var_os("HORIZON_COMMAND_SCRIPT").is_some() {
                // The loaded-command fixture pins the server's valid minimum.
                // Client options reject simulationDistance2 (their UI starts at5),
                // so an options.txt value cannot establish this prerequisite.
                if std::env::var_os("HORIZON_HARNESS_LOADED_FIXTURE").is_some() {
                    game.settings.simulation_distance = 2;
                }
                // Headless, but with the same server and memory connection as play.
                game.open_integrated(&folder, None, Some(seed), creative, None)?;
            } else {
                // Start after the window is handed over, so --world uses the
                // same responsive initialization path as the title screen.
                game.title = Some(
                    Title::new(
                        std::env::current_dir()?.join("saves"),
                        &game.options.resources,
                    )
                    .with_user(|| game.options.player_name()),
                );
                game.pending_entry = Some(PendingWorldEntry::Local {
                    folder,
                    name: None,
                    seed: Some(seed),
                    creative,
                    create: None,
                });
            }
        }
        None if handoff => {}
        None => {
            let mut title = Title::new(
                std::env::current_dir()?.join("saves"),
                &game.options.resources,
            )
            .with_user(|| game.options.player_name());
            title.last_server = game.settings.file.string("lastServer");
            title.skip_multiplayer_warning = game.settings.file.bool("skipMultiplayerWarning");
            // Gui.addInitialScreens: the accessibility onboarding comes first.
            if game.settings.file.bool("onboardAccessibility") {
                title.start_onboarding();
            }
            game.title = Some(title);
        }
    }
    if std::env::var_os("HORIZON_COMMAND_SCRIPT").is_some()
        && std::env::var_os("HORIZON_COMMAND_OUTPUT").is_some()
    {
        game.wait_command_harness()?;
        return Ok(None);
    }
    Ok(Some(game))
}
