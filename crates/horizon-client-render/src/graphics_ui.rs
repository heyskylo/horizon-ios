//! Horizon's graphics settings screen: every video setting, vanilla's and
//! Horizon's, on one list of pages (General, Quality, Performance, Shaders,
//! Advanced, HDR), on the settings shell (`settings_shell`), which draws it, with
//! a page list, a search field and a description of the hovered setting with its
//! cost. Which settings it shows, in what pages, with what description and cost,
//! and what they do live here.
use crate::{
    gfx::EffectiveOutput,
    graphics::{
        EntityCullingMode, FarDetail, FrameGeneration, GpuCullingMode, GraphicsApi, HdrMode,
        LeafCullingMode, Overlay, Pacing, ParticleCullingMode, PresentMode, Preset, Upscaling,
    },
    graphics_style::Cost,
    lang,
    options_ui::{Event, Page as OptionsPage},
    settings::{Opt, Settings},
    settings_shell::{self as shell, Group, Output, Row, Screen, Sheet, Value},
    text::Text,
};

/// The language key of the screen's name.
const TITLE: &str = "Graphics";

/// The screen's pages, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Quality,
    Performance,
    /// Shader packs, either kind (`shader_pack_list`), and what they cost.
    Shaders,
    Advanced,
    /// HDR output's own settings.
    Hdr,
}
impl Page {
    pub const ALL: [Page; 6] = [
        Page::General,
        Page::Quality,
        Page::Performance,
        Page::Shaders,
        Page::Advanced,
        Page::Hdr,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Quality => "Quality",
            Page::Performance => "Performance",
            Page::Shaders => "Shaders",
            Page::Advanced => "Advanced",
            Page::Hdr => "HDR",
        }
    }
    /// What the page holds, in the strip while no setting is pointed at.
    pub fn about(self) -> &'static str {
        match self {
            Page::General => "How far you see, the window and the HUD.",
            Page::Quality => "The sky and weather, lighting, textures and the image.",
            Page::Performance => "Chunk loading, culling and power use.",
            Page::Shaders => "Shader packs, Horizon's and shaders/ folder ones, and their options.",
            Page::Advanced => "How frames reach the screen, and the performance overlay.",
            Page::Hdr => "Brighter-than-white output on an HDR display.",
        }
    }
}

/// Horizon's own settings (`graphics::Graphics`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H {
    Preset,
    LodDistance,
    FarDetail,
    BlueMapAutoDetect,
    BlueMapRadius,
    CaveCulling,
    GpuCulling,
    EntityCulling,
    EntityCullingPrecheck,
    ParticleCulling,
    LeafCulling,
    OpaqueIce,
    CacheShulkerBoxes,
    CacheSigns,
    CacheDecoratedPots,
    CacheBells,
    CacheChests,
    CacheBanners,
    CacheCopperGolemStatues,
    ChunkUpdates,
    UploadBudget,
    Sky,
    Fog,
    Weather,
    AnimatedTextures,
    DitherCutoutMips,
    PresentMode,
    Pacing,
    Workers,
    Overlay,
    ResolutionScale,
    Antialiasing,
    Upscaling,
    UpscalingSharpness,
    ScreenshotQuality,
    FrameGeneration,
    Hdr,
    HdrBlackLevel,
    /// Starts HDR calibration (`crate::hdr_calibration`).
    HdrCalibrate,
    HdrPeak,
    HdrPaperWhite,
    HdrUiBrightness,
    HdrLighting,
    BatteryMode,
    ShadowCache,
    /// Opens the shader pack list (`options_ui::Page::ShaderPacks`).
    ShaderPacks,
    GraphicsApi,
    TranslucentBatching,
    HdrSmoothGradients,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Vanilla(Opt),
    Horizon(H),
    /// A Horizon extra that changes the picture (`extras::Item`, kept in `horizon-extras.txt`).
    Extra(crate::extras::Item),
}

#[derive(Clone, Copy)]
pub struct Setting {
    pub item: Item,
    pub cost: Cost,
    pub text: &'static str,
}
fn v(opt: Opt, cost: Cost, text: &'static str) -> Setting {
    Setting {
        item: Item::Vanilla(opt),
        cost,
        text,
    }
}
fn h(opt: H, cost: Cost, text: &'static str) -> Setting {
    Setting {
        item: Item::Horizon(opt),
        cost,
        text,
    }
}
/// An extra's row: its description is its own language key.
fn e(item: crate::extras::Item, cost: Cost) -> Setting {
    Setting {
        item: Item::Extra(item),
        cost,
        text: item.description_key(),
    }
}

/// Every page's settings, in named groups of related rows.
pub fn rows() -> Vec<(Page, Vec<(&'static str, Vec<Setting>)>)> {
    use Cost::*;
    // Suggested, never switched on, where far terrain costs the GPU most.
    let far = if crate::graphics::reduced_far_detail_suggested() {
        "Reduced draws far terrain coarser: faster on this integrated GPU."
    } else {
        "Reduced draws far terrain coarser, for GPUs it slows down."
    };
    let mut pages = vec![
        (
            Page::General,
            vec![
                (
                    "Preset",
                    vec![
                        h(
                            H::Preset,
                            None,
                            "Sets the options below together. Changing one makes it Custom.",
                        ),
                        v(
                            Opt::Preset,
                            None,
                            "Vanilla's own preset: sets only vanilla's options, as Minecraft does.",
                        ),
                    ],
                ),
                (
                    "Distance",
                    vec![
                        v(
                            Opt::RenderDistance,
                            High,
                            "Full-detail terrain range. LOD Distance can extend local worlds farther.",
                        ),
                        h(
                            H::LodDistance,
                            Medium,
                            "Coarse distant terrain in local worlds. Integrated GPUs stop at 128 chunks.",
                        ),
                        h(H::FarDetail, Medium, far),
                        h(
                            H::BlueMapAutoDetect,
                            None,
                            "On servers, finds their BlueMap web map for far terrain. Next join.",
                        ),
                        h(
                            H::BlueMapRadius,
                            None,
                            "How far around you BlueMap tiles are downloaded. Next join.",
                        ),
                        v(
                            Opt::SimulationDistance,
                            Medium,
                            "How far mobs, crops and machines keep working. CPU time.",
                        ),
                        v(Opt::Gamma, None, "Brightens dark places."),
                    ],
                ),
                (
                    "Display",
                    vec![
                        v(Opt::GuiScale, None, "Size of menus and the HUD."),
                        v(Opt::Fullscreen, None, "Fills the screen (borderless)."),
                        v(
                            Opt::ExclusiveFullscreen,
                            None,
                            "Fullscreen takes the display over, at the mode below.",
                        ),
                        v(
                            Opt::ExclusiveMode,
                            None,
                            "The display mode exclusive fullscreen uses. Applies when you leave.",
                        ),
                        v(
                            Opt::Vsync,
                            None,
                            "Waits for the display's refresh: no tearing, a little latency.",
                        ),
                        v(
                            Opt::MaxFps,
                            None,
                            "Caps frames per second; a cap saves power and heat.",
                        ),
                        v(
                            Opt::InactivityFps,
                            None,
                            "Lowers the frame rate when minimised, or idle too.",
                        ),
                    ],
                ),
                (
                    "Interface",
                    vec![
                        v(Opt::MenuBlur, Low, "Blurs the world behind menus."),
                        v(Opt::AttackIndicator, None, "Shows attack cooldown."),
                        v(Opt::Autosave, None, "Shows when the world saves."),
                    ],
                ),
            ],
        ),
        (
            Page::Quality,
            vec![
                (
                    "Sky and weather",
                    vec![
                        v(
                            Opt::Clouds,
                            Low,
                            "Cloud layer: off, flat (fast) or 3D (fancy).",
                        ),
                        v(Opt::CloudRange, Low, "How far clouds reach, in chunks."),
                        h(H::Sky, Low, "Draws the sky, sun, moon and stars."),
                        h(
                            H::Fog,
                            None,
                            "Distance fog. Off shows the edge of the loaded world.",
                        ),
                        h(H::Weather, High, "Draws rain and snow."),
                        v(
                            Opt::WeatherRadius,
                            Medium,
                            "How far rain and snow fall around you.",
                        ),
                        v(Opt::Particles, Medium, "How many particles effects spawn."),
                        e(crate::extras::Item::HideBlockParticles, None),
                    ],
                ),
                (
                    "Lighting and entities",
                    vec![
                        v(
                            Opt::SmoothLighting,
                            Low,
                            "Soft shadows in corners. Costs chunk building time only.",
                        ),
                        v(
                            Opt::BiomeBlend,
                            Low,
                            "Blends grass and water colours across biomes when building chunks.",
                        ),
                        v(
                            Opt::EntityDistance,
                            Medium,
                            "How far away mobs and items are drawn, as a share of normal.",
                        ),
                        v(
                            Opt::EntityShadows,
                            Low,
                            "Round shadows under mobs and items.",
                        ),
                        v(Opt::Vignette, None, "Darkens the screen's edges."),
                        v(Opt::ChunkFade, None, "Fades newly loaded chunks in."),
                        e(crate::extras::Item::DynamicLights, Low),
                        e(crate::extras::Item::Fullbright, None),
                    ],
                ),
                (
                    "Textures",
                    vec![
                        v(
                            Opt::Mipmaps,
                            None,
                            "Smaller copies of block textures for distant terrain: less shimmer.",
                        ),
                        v(
                            Opt::TextureFiltering,
                            Low,
                            "How textures are sampled: sharper at a slant with Anisotropic.",
                        ),
                        v(
                            Opt::MaxAnisotropy,
                            Low,
                            "How far Anisotropic filtering goes. Higher is sharper at a slant.",
                        ),
                        v(
                            Opt::CutoutLeaves,
                            Medium,
                            "Leaves with see-through gaps. Off draws them solid.",
                        ),
                        v(
                            Opt::ImprovedTransparency,
                            Medium,
                            "Sorts water and glass more exactly behind each other.",
                        ),
                        h(
                            H::AnimatedTextures,
                            Low,
                            "Animates water, lava, fire and other moving textures.",
                        ),
                        h(
                            H::DitherCutoutMips,
                            Low,
                            "Keeps fine plants in high-res packs from fading far away.",
                        ),
                    ],
                ),
                (
                    "Image",
                    vec![
                        h(
                            H::ResolutionScale,
                            High,
                            "World pixels per screen pixel. Below 100% is faster on the GPU.",
                        ),
                        h(
                            H::Antialiasing,
                            Medium,
                            "Smooths jagged edges. FXAA: softer. TAA: smoothest, moving things may trail.",
                        ),
                        h(
                            H::Upscaling,
                            High,
                            "Renders the world smaller and rebuilds it with TAA. Faster, softer.",
                        ),
                        h(
                            H::UpscalingSharpness,
                            Low,
                            "Sharpens the upscaled picture. Higher: crisper edges, more shimmer.",
                        ),
                        h(
                            H::ScreenshotQuality,
                            None,
                            "F2 screenshots at this many times the game's resolution. Higher takes longer.",
                        ),
                        h(
                            H::FrameGeneration,
                            Medium,
                            "Shows a made-up frame between each two real ones: smoother, a little more lag.",
                        ),
                    ],
                ),
            ],
        ),
        (
            Page::Performance,
            vec![
                (
                    "Chunk loading",
                    vec![
                        v(
                            Opt::ChunkBuilder,
                            Low,
                            "Which chunk rebuilds wait for the frame: none, nearby or yours.",
                        ),
                        h(
                            H::ChunkUpdates,
                            Low,
                            "Meshed sections uploaded per frame. Lower is smoother while loading.",
                        ),
                        h(
                            H::UploadBudget,
                            Low,
                            "Mesh data sent to the GPU per frame. Lower is smoother while loading.",
                        ),
                        h(
                            H::Workers,
                            None,
                            "Threads that generate, light and mesh chunks. Next start.",
                        ),
                    ],
                ),
                (
                    "Culling",
                    vec![
                        h(
                            H::CaveCulling,
                            Saves,
                            "Skips terrain hidden behind closed sections. Off costs a lot.",
                        ),
                        h(
                            H::GpuCulling,
                            Saves,
                            "Culls nearby terrain on the GPU: less CPU at high render distance, more GPU.",
                        ),
                        h(
                            H::EntityCulling,
                            None,
                            "Hides entities behind opaque blocks.",
                        ),
                        h(
                            H::ParticleCulling,
                            Saves,
                            "Skips particles outside the view, below one pixel or behind terrain.",
                        ),
                        h(
                            H::EntityCullingPrecheck,
                            Saves,
                            "Runs the mob AABB precheck after this many loaded mobs. Zero disables it.",
                        ),
                        h(
                            H::LeafCulling,
                            Saves,
                            "Draws or drops leaf faces inside canopies. Off keeps vanilla's fancy leaves.",
                        ),
                        h(
                            H::OpaqueIce,
                            Saves,
                            "Draws ice in the solid pass, without its see-through texture.",
                        ),
                    ],
                ),
                (
                    "Block Entities",
                    vec![
                        h(
                            H::CacheShulkerBoxes,
                            Saves,
                            "Reuses settled boxes; lid movement stays animated.",
                        ),
                        h(
                            H::CacheSigns,
                            Saves,
                            "Reuses sign text until text, light, or font changes.",
                        ),
                        h(
                            H::CacheDecoratedPots,
                            Saves,
                            "Reuses still pots; break wobble and sherd changes redraw.",
                        ),
                        h(
                            H::CacheBells,
                            Saves,
                            "Reuses still bells; ringing stays animated.",
                        ),
                        h(
                            H::CacheChests,
                            Saves,
                            "Reuses closed chests; moving lids stay animated.",
                        ),
                        h(
                            H::CacheBanners,
                            Saves,
                            "Reuses poles; flags and pattern layers still update.",
                        ),
                        h(
                            H::CacheCopperGolemStatues,
                            Saves,
                            "Reuses statue geometry until its block state changes.",
                        ),
                    ],
                ),
                (
                    "Power",
                    vec![h(
                        H::BatteryMode,
                        None,
                        "60 FPS cap (30 in menus or unfocused), half the workers: less power.",
                    )],
                ),
            ],
        ),
        (
            Page::Shaders,
            vec![(
                "Shader packs",
                vec![
                    h(
                        H::ShaderPacks,
                        High,
                        "Picks a shader pack, Horizon's or a shaders/ folder one, and sets its options.",
                    ),
                    h(
                        H::ShadowCache,
                        Saves,
                        "Reuses the pack shadow map when sun, view and world match. Off by default.",
                    ),
                ],
            )],
        ),
        (
            Page::Advanced,
            vec![
                (
                    "Frames",
                    vec![
                        h(
                            H::PresentMode,
                            None,
                            "How frames reach the screen while VSync is off.",
                        ),
                        h(
                            H::Pacing,
                            None,
                            "Efficient sleeps until the next frame; Precise spins, like vanilla.",
                        ),
                        h(
                            H::GraphicsApi,
                            None,
                            "Vulkan, then DirectX 12 (Windows) or OpenGL (Linux) where it fails. Next start.",
                        ),
                        h(
                            H::TranslucentBatching,
                            Saves,
                            "Water and glass in one buffer: fewer draw calls, a bit more VRAM. Next start.",
                        ),
                    ],
                ),
                (
                    "Overlay",
                    vec![h(
                        H::Overlay,
                        None,
                        "Frame rate and frame times over the game, with a graph.",
                    )],
                ),
            ],
        ),
        (
            Page::Hdr,
            vec![
                (
                    "Output",
                    vec![
                        h(
                            H::Hdr,
                            None,
                            "Lava, fire and sunlight shine brighter than white. Needs an HDR display.",
                        ),
                        h(
                            H::HdrSmoothGradients,
                            Low,
                            "16-bit frame: dark skies and shadows don't band. A bit more VRAM. Next start.",
                        ),
                        h(
                            H::HdrCalibrate,
                            None,
                            "Sets the three below full screen, one test picture at a time.",
                        ),
                    ],
                ),
                (
                    "Brightness",
                    vec![
                        h(
                            H::HdrBlackLevel,
                            None,
                            "Lifts shadows onto your display's darkest black. 0: true black, no lift.",
                        ),
                        h(
                            H::HdrPeak,
                            None,
                            "How bright highlights get. Auto: the peak your system reports.",
                        ),
                        h(
                            H::HdrPaperWhite,
                            None,
                            "How bright white looks in the world. Auto: your desktop's white.",
                        ),
                        h(
                            H::HdrUiBrightness,
                            None,
                            "How bright the HUD and menus are, against paper white.",
                        ),
                        h(
                            H::HdrLighting,
                            None,
                            "Light sources, the sun and sunlight shine past white. Off: just as SDR.",
                        ),
                    ],
                ),
            ],
        ),
    ];
    // Only Windows and Linux have more than one graphics API, translucent batching (Metal
    // draws flat) and GPU culling (Metal draws no count the GPU wrote).
    if cfg!(target_os = "macos") {
        for (_, groups) in &mut pages {
            for (_, rows) in groups {
                rows.retain(|r| {
                    !matches!(
                        r.item,
                        Item::Horizon(H::GraphicsApi | H::TranslucentBatching | H::GpuCulling)
                    )
                });
            }
        }
    }
    pages
}

/// BlueMap download radii, in chunks.
const BLUE_MAP_RADII: [u32; 5] = [16, 32, 64, 128, 256];
const UPLOAD_STEPS: [u32; 8] = [0, 256, 512, 1024, 2048, 4096, 8192, 16384];
/// What the LOD Distance slider stops at, in chunks: off, fine steps up to 256
/// for a short ring of far terrain past a small render distance (a phone or an
/// integrated GPU at 8 chunks wants LOD to 12 or 16, not 64), then each doubling
/// up to the whole world.
const LOD_STEPS: [u32; 24] = [
    0,
    16,
    24,
    32,
    48,
    64,
    96,
    128,
    192,
    256,
    512,
    1024,
    2048,
    4096,
    8192,
    16384,
    32768,
    65536,
    131072,
    262144,
    524288,
    1048576,
    2097152,
    crate::lod::MAX_CHUNKS,
];

/// Whether the system set HDR up for the display, and whether the player kept it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setup {
    /// The system says nothing: the player calibrates.
    Unknown,
    /// Peak Brightness and Paper White follow the system.
    Automatic,
    /// The system said, and the player changed them anyway.
    Changed,
}
/// `out`: the output of the renderer the settings are shown for, with what its display reports.
fn setup(s: &Settings, out: &EffectiveOutput) -> Setup {
    setup_of(&s.horizon, out.reported.is_some())
}
fn setup_of(g: &crate::graphics::Graphics, reported: bool) -> Setup {
    if !reported {
        Setup::Unknown
    } else if g.hdr_peak == 0 && g.hdr_paper_white == 0 {
        Setup::Automatic
    } else {
        Setup::Changed
    }
}
/// The description beside `item` where it depends on [`setup`]; `text` otherwise.
fn description(
    item: Item,
    s: &Settings,
    out: &EffectiveOutput,
    text: &'static str,
) -> &'static str {
    match (item, setup(s, out)) {
        (Item::Horizon(H::HdrCalibrate), Setup::Unknown) => {
            "Your display didn't say how bright it gets: calibrate to set it up."
        }
        (Item::Horizon(H::HdrCalibrate), Setup::Automatic) => {
            "Already set up for your display. Only calibrate if HDR looks wrong."
        }
        (Item::Horizon(H::HdrCalibrate), Setup::Changed) => {
            "Puts Peak Brightness and Paper White back to your display's setup."
        }
        (Item::Horizon(H::HdrPeak | H::HdrPaperWhite), Setup::Automatic | Setup::Changed) => {
            "Set up for your display. Leave on Auto unless it looks wrong."
        }
        (Item::Horizon(H::GpuCulling), _) if s.horizon.gpu_culling == GpuCullingMode::Auto => {
            "Auto: tries it where your CPU limits the frame, keeps it if frames get faster."
        }
        (Item::Horizon(H::EntityCulling), _) => match s.horizon.entity_culling {
            EntityCullingMode::Off => "No entity occlusion culling.",
            EntityCullingMode::Synchronous => "CPU occlusion test runs this frame.",
            EntityCullingMode::Asynchronous => "Occlusion checks run on a worker thread.",
        },
        (Item::Horizon(H::LeafCulling), _) => match s.horizon.leaf_culling {
            LeafCullingMode::Off => "Every leaf face. Vanilla's fancy leaves.",
            LeafCullingMode::Hidden => {
                "Drops leaf faces behind two leaf layers: faster, same crowns."
            }
            LeafCullingMode::Solid => {
                "Leaves fully inside a canopy draw solid: fewer see-through layers."
            }
            LeafCullingMode::SolidAggressive => {
                "Leaves with one open side draw solid too: faster, chunkier edges."
            }
            LeafCullingMode::Hollow => "Drops faces between leaves: faster, but gaps look through.",
        },
        (Item::Horizon(H::ParticleCulling), _) => match s.horizon.particle_culling {
            ParticleCullingMode::Off => "Draws every particle.",
            ParticleCullingMode::Synchronous => {
                "Skips offscreen, subpixel and terrain-hidden particles."
            }
            ParticleCullingMode::Asynchronous => {
                "Skips offscreen, subpixel and terrain-hidden particles with less render-thread work."
            }
        },
        _ => text,
    }
}
fn on_off(on: bool) -> String {
    lang::get(if on { "On" } else { "Off" })
}
/// A control's caption and its value, both through the language table: Horizon's captions are
/// English sentences, and some of the values ("Off", "12 chunks") are words too.
/// "Caption: value" (`options.generic_value`), both looked up in the language table.
fn labelled(caption: &str, value: impl std::fmt::Display) -> Text {
    let value = value.to_string();
    Text::option_name_value(
        Text::literal(lang::get(caption)),
        Text::literal(lang::get(&value)),
    )
}
/// "Caption: 50%" (`options.percent_value`, Options.percentValueLabel): French and German put a
/// space before the `%`.
fn labelled_percent(caption: &str, percent: u32) -> Text {
    Text::translate_with_fallback(
        "options.percent_value",
        "%s: %s%%",
        &[
            Text::literal(lang::get(caption)),
            Text::literal(percent.to_string()),
        ],
    )
}

/// Peak Brightness's slider: Auto, then each step of the peak's scale.
const PEAK_POSITIONS: u32 = crate::graphics::PEAK_STEPS + 1;

impl H {
    fn slider(self, s: &Settings) -> Option<f32> {
        let g = &s.horizon;
        match self {
            H::ChunkUpdates => Some(g.chunk_updates as f32 / 64.0),
            H::EntityCullingPrecheck => Some(g.entity_culling_precheck as f32 / 512.0),
            H::UploadBudget => {
                let i = UPLOAD_STEPS
                    .iter()
                    .position(|&b| b >= g.upload_budget_kib)
                    .unwrap_or(UPLOAD_STEPS.len() - 1);
                Some(i as f32 / (UPLOAD_STEPS.len() - 1) as f32)
            }
            H::LodDistance => {
                let i = LOD_STEPS
                    .iter()
                    .position(|&d| d >= g.lod_distance)
                    .unwrap_or(LOD_STEPS.len() - 1);
                Some(i as f32 / (LOD_STEPS.len() - 1) as f32)
            }
            H::Workers => Some(g.workers as f32 / max_workers() as f32),
            H::ResolutionScale => Some((g.resolution_scale as f32 - 50.0) / 150.0),
            H::UpscalingSharpness => Some(g.upscaling_sharpness as f32 / 100.0),
            // Auto, then 10..480 by 10.
            H::HdrPaperWhite => Some(g.hdr_paper_white as f32 / 480.0),
            // Auto, then the peak's scale.
            H::HdrPeak => Some(match g.hdr_peak {
                0 => 0.0,
                n => (1.0 + crate::graphics::peak_step_of(n as f32)) / PEAK_POSITIONS as f32,
            }),
            H::HdrBlackLevel => Some(g.hdr_black_level as f32 / 100.0),
            H::HdrUiBrightness => Some((g.hdr_ui_brightness as f32 - 25.0) / 75.0),
            _ => None,
        }
    }
    pub fn slider_steps(self) -> Option<u32> {
        match self {
            H::ChunkUpdates => Some(64),
            H::EntityCullingPrecheck => Some(32),
            H::UploadBudget => Some((UPLOAD_STEPS.len() - 1) as u32),
            H::Workers => Some(max_workers()),
            H::ResolutionScale => Some(30),
            H::UpscalingSharpness => Some(20),
            H::HdrPaperWhite => Some(48),
            H::HdrPeak => Some(PEAK_POSITIONS),
            H::HdrBlackLevel => Some(100),
            H::HdrUiBrightness => Some(15),
            _ => None,
        }
    }
    fn label(self, s: &Settings, out: &EffectiveOutput) -> Text {
        let g = &s.horizon;
        match self {
            H::Preset => labelled("Preset", g.preset.label()),
            H::LodDistance => match g.lod_distance {
                0 => labelled("LOD Distance", "Off"),
                n => labelled("LOD Distance", lang::tr("%s chunks", &[&n])),
            },
            H::FarDetail => labelled("Far Detail", g.far_detail.label()),
            H::BlueMapAutoDetect => labelled("Find BlueMap", on_off(g.blue_map.auto_detect)),
            H::BlueMapRadius => labelled(
                "BlueMap Radius",
                lang::tr("%s chunks", &[&g.blue_map.radius_chunks()]),
            ),
            H::CaveCulling => labelled("Cave Culling", on_off(g.cave_culling)),
            // Greyed out where it isn't offered (`Item::blocked`): Off, as it is there, while
            // the saved value waits.
            H::GpuCulling => labelled(
                "GPU Culling",
                if crate::graphics::gpu_culling_offer().reason().is_none() {
                    g.gpu_culling.label()
                } else {
                    GpuCullingMode::Off.label()
                },
            ),
            H::EntityCulling => labelled("Entity Culling", g.entity_culling.label()),
            H::EntityCullingPrecheck => labelled(
                "Entity Culling Precheck",
                if g.entity_culling_precheck == 0 {
                    "Off".to_string()
                } else {
                    lang::tr("%s mobs", &[&g.entity_culling_precheck])
                },
            ),
            H::ParticleCulling => labelled("Particle Culling", g.particle_culling.label()),
            H::LeafCulling => labelled("Leaf Culling", g.leaf_culling.label()),
            H::OpaqueIce => labelled("Opaque Ice", on_off(g.opaque_ice)),
            H::CacheShulkerBoxes => labelled("Shulker Boxes", on_off(g.cache_shulker_boxes)),
            H::CacheSigns => labelled("Signs", on_off(g.cache_signs)),
            H::CacheDecoratedPots => labelled("Decorated Pots", on_off(g.cache_decorated_pots)),
            H::CacheBells => labelled("Bells", on_off(g.cache_bells)),
            H::CacheChests => labelled("Chests", on_off(g.cache_chests)),
            H::CacheBanners => labelled("Banners", on_off(g.cache_banners)),
            H::CacheCopperGolemStatues => {
                labelled("Copper Golem Statues", on_off(g.cache_copper_golem_statues))
            }
            H::ChunkUpdates => match g.chunk_updates {
                0 => labelled("Chunk Updates", "Unlimited"),
                n => labelled("Chunk Updates", lang::tr("%s per frame", &[&n])),
            },
            H::UploadBudget => match g.upload_budget_kib {
                0 => labelled("Upload Budget", "Unlimited"),
                k if k >= 1024 => {
                    labelled("Upload Budget", lang::tr("%s MiB/frame", &[&(k / 1024)]))
                }
                k => labelled("Upload Budget", lang::tr("%s KiB/frame", &[&k])),
            },
            H::Sky => labelled("Sky", on_off(g.sky)),
            H::Fog => labelled("Fog", on_off(g.fog)),
            H::Weather => labelled("Weather", on_off(g.weather)),
            H::AnimatedTextures => labelled("Animated Textures", on_off(g.animated_textures)),
            H::DitherCutoutMips => labelled("Distant Plant Dither", on_off(g.dither_cutout_mips)),
            H::PresentMode => labelled("Present Mode", g.present_mode.label()),
            H::Pacing => labelled(
                "Frame Pacing",
                match g.pacing {
                    Pacing::Efficient => "Efficient",
                    Pacing::Precise => "Precise",
                },
            ),
            H::Workers => match g.workers {
                0 => labelled(
                    "Worker Threads",
                    lang::tr("Auto (%s)", &[&g.worker_threads()]),
                ),
                n => labelled("Worker Threads", n),
            },
            H::ResolutionScale => labelled_percent("Resolution Scale", g.resolution_scale),
            H::Antialiasing => labelled(
                "Anti-aliasing",
                match (g.fxaa, g.taa) {
                    (_, true) => "TAA",
                    (true, false) => "FXAA",
                    (false, false) => "Off",
                },
            ),
            H::Upscaling => labelled("Upscaling", g.upscaling.label()),
            H::UpscalingSharpness => {
                labelled("Upscaling Sharpness", format!("{}%", g.upscaling_sharpness))
            }
            H::ScreenshotQuality => {
                labelled("Screenshot Quality", format!("{}x", g.screenshot_quality))
            }
            H::FrameGeneration => labelled("Frame Generation", g.frame_generation.label()),
            H::Hdr => labelled("HDR", g.hdr.label()),
            H::HdrCalibrate => Text::literal(lang::get(match setup(s, out) {
                Setup::Unknown => "Calibrate...",
                Setup::Automatic => "Calibrate Anyway...",
                Setup::Changed => "Use Automatic Setup",
            })),
            H::HdrPaperWhite => match g.hdr_paper_white {
                0 => labelled(
                    "Paper White",
                    lang::tr("Auto (%s)", &[&g.paper_white_nits(out.reported)]),
                ),
                n => labelled("Paper White", n.to_string()),
            },
            H::HdrPeak => match g.hdr_peak {
                0 => labelled(
                    "Peak Brightness",
                    lang::tr("Auto (%s)", &[&g.peak_nits(out.reported)]),
                ),
                n => labelled("Peak Brightness", n.to_string()),
            },
            H::HdrBlackLevel => labelled("Black Level", format!("{:.2}", g.black_level_nits())),
            H::HdrUiBrightness => labelled_percent("UI Brightness", g.hdr_ui_brightness),
            H::HdrLighting => labelled("HDR Lighting", on_off(g.hdr_lighting)),
            H::BatteryMode => labelled("Battery Mode", on_off(g.battery)),
            H::ShadowCache => labelled("Shadow Cache", on_off(g.shadow_cache)),
            // A press across the row, as Calibrate...: the list it opens shows the pick.
            H::ShaderPacks => Text::literal(lang::get("Shader Packs...")),
            H::GraphicsApi => labelled("Graphics API", g.api.label()),
            // These two are greyed out where the device can't (`Item::blocked`): Off, as
            // they are there, while the saved value waits for a device that can.
            H::TranslucentBatching => labelled(
                "Translucent Batching",
                on_off(g.translucent_batching && crate::graphics::translucent_batching_offered()),
            ),
            H::HdrSmoothGradients => labelled(
                "Smooth Gradients",
                on_off(g.hdr_smooth_gradients && out.wide_frame_supported),
            ),
            H::Overlay => labelled(
                "Performance Overlay",
                match g.overlay {
                    Overlay::Off => "Off",
                    Overlay::Fps => "FPS",
                    Overlay::Graph => "FPS and Graph",
                },
            ),
        }
    }
    fn set_slider(self, s: &mut Settings, v: f32) {
        let g = &mut s.horizon;
        match self {
            H::ChunkUpdates => g.chunk_updates = (v * 64.0).round() as u32,
            H::EntityCullingPrecheck => g.entity_culling_precheck = (v * 512.0).round() as u32,
            H::UploadBudget => {
                g.upload_budget_kib =
                    UPLOAD_STEPS[(v * (UPLOAD_STEPS.len() - 1) as f32).round() as usize]
            }
            H::LodDistance => {
                g.lod_distance = LOD_STEPS[(v * (LOD_STEPS.len() - 1) as f32).round() as usize]
            }
            H::Workers => g.workers = (v * max_workers() as f32).round() as u32,
            H::ResolutionScale => g.resolution_scale = (50.0 + (v * 30.0).round() * 5.0) as u32,
            H::UpscalingSharpness => g.upscaling_sharpness = (v * 20.0).round() as u32 * 5,
            H::HdrPaperWhite => g.hdr_paper_white = (v * 48.0).round() as u32 * 10,
            H::HdrPeak => {
                g.hdr_peak = match (v * PEAK_POSITIONS as f32).round() as u32 {
                    0 => 0,
                    step => crate::graphics::peak_nits_at(step - 1),
                }
            }
            H::HdrBlackLevel => g.hdr_black_level = (v * 100.0).round() as u32,
            H::HdrUiBrightness => g.hdr_ui_brightness = (25.0 + (v * 15.0).round() * 5.0) as u32,
            _ => {}
        }
    }
    fn cycle(self, s: &mut Settings, back: bool, out: &EffectiveOutput) {
        fn step<T: Copy + PartialEq>(all: &[T], now: T, back: bool) -> T {
            let i = all.iter().position(|x| *x == now).unwrap_or(0);
            let n = all.len();
            all[if back { (i + n - 1) % n } else { (i + 1) % n }]
        }
        if self == H::Preset {
            let preset = step(&Preset::ALL, s.horizon.preset, back);
            crate::graphics::apply_preset(preset, s);
            s.horizon.preset = preset;
            return;
        }
        let g = &mut s.horizon;
        match self {
            H::FarDetail => {
                g.far_detail = step(&[FarDetail::Full, FarDetail::Reduced], g.far_detail, back)
            }
            H::BlueMapAutoDetect => g.blue_map.auto_detect = !g.blue_map.auto_detect,
            H::BlueMapRadius => {
                g.blue_map.radius = step(&BLUE_MAP_RADII, g.blue_map.radius_chunks(), back)
            }
            H::CaveCulling => g.cave_culling = !g.cave_culling,
            H::GpuCulling => g.gpu_culling = step(&GpuCullingMode::ALL, g.gpu_culling, back),
            H::EntityCulling => {
                g.entity_culling = step(&EntityCullingMode::ALL, g.entity_culling, back)
            }
            H::ParticleCulling => {
                g.particle_culling = step(&ParticleCullingMode::ALL, g.particle_culling, back)
            }
            H::LeafCulling => {
                g.leaf_culling = step(&LeafCullingMode::ALL, g.leaf_culling, back);
            }
            H::OpaqueIce => g.opaque_ice = !g.opaque_ice,
            H::CacheShulkerBoxes => g.cache_shulker_boxes = !g.cache_shulker_boxes,
            H::CacheSigns => g.cache_signs = !g.cache_signs,
            H::CacheDecoratedPots => g.cache_decorated_pots = !g.cache_decorated_pots,
            H::CacheBells => g.cache_bells = !g.cache_bells,
            H::CacheChests => g.cache_chests = !g.cache_chests,
            H::CacheBanners => g.cache_banners = !g.cache_banners,
            H::CacheCopperGolemStatues => {
                g.cache_copper_golem_statues = !g.cache_copper_golem_statues
            }
            H::Sky => g.sky = !g.sky,
            H::Fog => g.fog = !g.fog,
            H::Weather => g.weather = !g.weather,
            H::AnimatedTextures => g.animated_textures = !g.animated_textures,
            H::DitherCutoutMips => g.dither_cutout_mips = !g.dither_cutout_mips,
            H::PresentMode => g.present_mode = step(&PresentMode::ALL, g.present_mode, back),
            // Off, FXAA, TAA.
            H::Antialiasing => {
                let i = if g.taa { 2 } else { usize::from(g.fxaa) };
                let i = (i + if back { 2 } else { 1 }) % 3;
                (g.fxaa, g.taa) = (i == 1, i == 2);
                if !g.taa {
                    g.upscaling = Upscaling::Native;
                }
            }
            // Upscaling is TAA's: picking a ratio turns it on.
            H::Upscaling => {
                g.upscaling = step(&Upscaling::ALL, g.upscaling, back);
                if g.upscaling != Upscaling::Native {
                    (g.fxaa, g.taa) = (false, true);
                }
            }
            H::ScreenshotQuality => {
                g.screenshot_quality = step(
                    &crate::graphics::SCREENSHOT_QUALITIES,
                    g.screenshot_quality,
                    back,
                )
            }
            H::FrameGeneration => {
                g.frame_generation = step(&FrameGeneration::ALL, g.frame_generation, back)
            }
            H::Hdr => {
                g.hdr = step(&HdrMode::ALL, g.hdr, back);
                if g.hdr == HdrMode::On && !out.hdr_offered() {
                    g.hdr = step(&HdrMode::ALL, g.hdr, back);
                }
            }
            H::HdrLighting => g.hdr_lighting = !g.hdr_lighting,
            H::BatteryMode => g.battery = !g.battery,
            H::ShadowCache => g.shadow_cache = !g.shadow_cache,
            H::GraphicsApi => g.api = step(GraphicsApi::choices(), g.api, back),
            H::TranslucentBatching => g.translucent_batching = !g.translucent_batching,
            H::HdrSmoothGradients => g.hdr_smooth_gradients = !g.hdr_smooth_gradients,
            H::Pacing => {
                g.pacing = match g.pacing {
                    Pacing::Efficient => Pacing::Precise,
                    Pacing::Precise => Pacing::Efficient,
                }
            }
            H::Overlay => {
                g.overlay = step(
                    &[Overlay::Off, Overlay::Fps, Overlay::Graph],
                    g.overlay,
                    back,
                )
            }
            _ => {}
        }
    }
}
fn max_workers() -> u32 {
    std::thread::available_parallelism().map_or(8, |n| n.get() as u32)
}

impl Item {
    /// False for a setting whose system is not in Horizon yet (`Opt::available`).
    fn available(self) -> bool {
        !matches!(self, Item::Vanilla(o) if !o.available())
    }
    /// Why a control can't be used now: HDR's On requires an offered output, while Auto
    /// can also be chosen on a reporting SDR display. Brightness controls need HDR output
    /// active; saved values wait for it. Translucent Batching switches where the device can batch
    /// (`graphics::translucent_batching_offered`), its saved value waiting for one that can,
    /// Smooth Gradients only where it can draw the 16-bit frame, the same way, and GPU Culling
    /// only where it is offered (`graphics::gpu_culling_offer`), with the reason it isn't.
    fn blocked(self, s: &Settings, out: &EffectiveOutput) -> Option<&'static str> {
        if !self.available() {
            return Some("This feature is not yet available in Horizon.");
        }
        match self {
            Item::Horizon(
                H::HdrCalibrate
                | H::HdrBlackLevel
                | H::HdrPeak
                | H::HdrPaperWhite
                | H::HdrUiBrightness
                | H::HdrLighting
                | H::HdrSmoothGradients,
            ) if !out.hdr_offered() => Some("Your display or system doesn't offer HDR output."),
            Item::Horizon(H::HdrSmoothGradients) if !out.wide_frame_supported => {
                Some("Your GPU or graphics API doesn't support it.")
            }
            Item::Horizon(
                H::HdrCalibrate
                | H::HdrBlackLevel
                | H::HdrPeak
                | H::HdrPaperWhite
                | H::HdrUiBrightness
                | H::HdrLighting
                | H::HdrSmoothGradients,
            ) if !out.requests_hdr(s.horizon.hdr) => Some("Turn HDR on to use it."),
            Item::Horizon(H::TranslucentBatching)
                if !crate::graphics::translucent_batching_offered() =>
            {
                Some("Your GPU or graphics API doesn't support it.")
            }
            Item::Horizon(H::EntityCullingPrecheck)
                if s.horizon.entity_culling != EntityCullingMode::Asynchronous =>
            {
                Some("Only applies to asynchronous entity culling.")
            }
            Item::Horizon(H::GpuCulling) => crate::graphics::gpu_culling_offer().reason(),
            // VideoSettingsScreen.tick: Max Anisotropy only works with Anisotropic filtering.
            Item::Vanilla(Opt::MaxAnisotropy) if s.file.int("textureFiltering") != 2 => {
                Some("Only with Anisotropic texture filtering.")
            }
            _ => None,
        }
    }
    /// A value stepped between `<` `>`: not a tick box, a slider or a press.
    fn steps(self, s: &Settings, out: &EffectiveOutput) -> bool {
        self.switch(s, out).is_none()
            && self.slider(s).is_none()
            && !matches!(self, Item::Horizon(H::HdrCalibrate | H::ShaderPacks))
    }
    /// On/off settings and their value.
    pub fn switch(self, s: &Settings, out: &EffectiveOutput) -> Option<bool> {
        let g = &s.horizon;
        Some(match self {
            Item::Horizon(H::BlueMapAutoDetect) => g.blue_map.auto_detect,
            Item::Horizon(H::CaveCulling) => g.cave_culling,
            Item::Horizon(H::OpaqueIce) => g.opaque_ice,
            Item::Horizon(H::CacheShulkerBoxes) => g.cache_shulker_boxes,
            Item::Horizon(H::CacheSigns) => g.cache_signs,
            Item::Horizon(H::CacheDecoratedPots) => g.cache_decorated_pots,
            Item::Horizon(H::CacheBells) => g.cache_bells,
            Item::Horizon(H::CacheChests) => g.cache_chests,
            Item::Horizon(H::CacheBanners) => g.cache_banners,
            Item::Horizon(H::CacheCopperGolemStatues) => g.cache_copper_golem_statues,
            Item::Horizon(H::Sky) => g.sky,
            Item::Horizon(H::Fog) => g.fog,
            Item::Horizon(H::Weather) => g.weather,
            Item::Horizon(H::AnimatedTextures) => g.animated_textures,
            Item::Horizon(H::DitherCutoutMips) => g.dither_cutout_mips,
            Item::Horizon(H::HdrLighting) => g.hdr_lighting,
            Item::Horizon(H::BatteryMode) => g.battery,
            Item::Horizon(H::ShadowCache) => g.shadow_cache,
            Item::Horizon(H::TranslucentBatching) => {
                g.translucent_batching && crate::graphics::translucent_batching_offered()
            }
            Item::Horizon(H::HdrSmoothGradients) => {
                g.hdr_smooth_gradients && out.wide_frame_supported
            }
            Item::Vanilla(Opt::Vsync) => s.vsync,
            Item::Extra(e) => return e.switch(&s.extras),
            Item::Vanilla(Opt::Fullscreen) => s.fullscreen,
            Item::Vanilla(Opt::ExclusiveFullscreen) => s.file.bool("exclusiveFullscreen"),
            Item::Vanilla(Opt::SmoothLighting) => s.smooth_lighting,
            Item::Vanilla(Opt::EntityShadows) => s.entity_shadows,
            Item::Vanilla(Opt::Vignette) => s.vignette,
            Item::Vanilla(Opt::Autosave) => s.autosave_indicator,
            Item::Vanilla(Opt::CutoutLeaves) => s.cutout_leaves,
            Item::Vanilla(Opt::ImprovedTransparency) => s.improved_transparency,
            _ => return None,
        })
    }
    pub fn slider(self, s: &Settings) -> Option<f32> {
        match self {
            Item::Vanilla(o) => o.slider_position(s),
            Item::Horizon(h) => h.slider(s),
            Item::Extra(_) => None,
        }
    }
    pub fn label(self, s: &Settings, out: &EffectiveOutput) -> Text {
        match self {
            Item::Vanilla(Opt::RenderDistance) if s.horizon.lod_distance > 0 => labelled(
                "Full Detail Distance",
                lang::tr("%s chunks", &[&s.render_distance]),
            ),
            Item::Vanilla(o) => o.label(s),
            Item::Horizon(h) => h.label(s, out),
            Item::Extra(e) => Text::option_name_value(
                Text::literal(e.caption_text()),
                Text::literal(e.value(&s.extras)),
            ),
        }
    }
    pub fn set_slider(self, s: &mut Settings, v: f32, out: &EffectiveOutput) {
        if !self.available() || self.blocked(s, out).is_some() {
            return;
        }
        match self {
            // Options.applyGraphicsPreset, as Video Settings' slider does it.
            Item::Vanilla(o) => crate::options_ui::set_slider(o, s, v),
            Item::Horizon(h) => h.set_slider(s, v),
            Item::Extra(_) => {}
        }
        self.touched(s);
    }
    pub fn cycle(self, s: &mut Settings, back: bool, framebuffer: [i32; 2], out: &EffectiveOutput) {
        if !self.available() {
            return;
        }
        match self {
            Item::Vanilla(o) => o.cycle(s, back, framebuffer),
            Item::Horizon(h) => h.cycle(s, back, out),
            Item::Extra(e) => e.cycle(&mut s.extras, back),
        }
        self.touched(s);
    }
    /// A preset no longer describes the settings once one of them changes.
    fn touched(self, s: &mut Settings) {
        let presetless = matches!(
            self,
            Item::Extra(_)
                | Item::Horizon(
                    H::Preset
                        | H::BlueMapAutoDetect
                        | H::BlueMapRadius
                        | H::Overlay
                        | H::ScreenshotQuality
                        | H::Pacing
                        | H::PresentMode
                        | H::Workers
                        | H::FrameGeneration
                        | H::BatteryMode
                        | H::ShadowCache
                        | H::GraphicsApi
                        | H::TranslucentBatching
                        | H::HdrSmoothGradients
                        | H::GpuCulling
                )
                | Item::Vanilla(
                    Opt::GuiScale
                        | Opt::Fullscreen
                        | Opt::ExclusiveFullscreen
                        | Opt::ExclusiveMode
                        | Opt::Vsync
                        | Opt::MaxFps
                        | Opt::Gamma
                        | Opt::InactivityFps
                        | Opt::MenuBlur
                        | Opt::ChunkFade
                        | Opt::Vignette
                        | Opt::Autosave
                        | Opt::AttackIndicator
                )
        );
        if !presetless {
            s.horizon.preset = Preset::Custom;
        }
    }
    pub fn slider_steps(self) -> Option<u32> {
        match self {
            Item::Vanilla(o) => o.slider_steps(),
            Item::Horizon(h) => h.slider_steps(),
            Item::Extra(_) => None,
        }
    }
    pub fn step_slider(self, s: &mut Settings, forward: bool, width: f32, out: &EffectiveOutput) {
        if !self.available() || self.blocked(s, out).is_some() {
            return;
        }
        let Some(pos) = self.slider(s) else {
            return;
        };
        let next_pos = shell::slider_step(pos, self.slider_steps(), width, forward);
        self.set_slider(s, next_pos, out);
        self.touched(s);
    }
}

/// Whether a setting is on the screen now: Upscaling only while TAA is, which does it.
fn shown(item: Item, s: &Settings) -> bool {
    item != Item::Horizon(H::Upscaling) || s.horizon.taa
}
/// A control's label as its name and its value ("Render Distance" and "12 chunks").
fn parts(item: Item, s: &Settings, out: &EffectiveOutput) -> (String, Option<String>) {
    shell::name_and_value(&item.label(s, out).plain())
}

/// A setting's row: its control, what it does (and what its chosen value does), what it costs,
/// and why it can't be used now.
fn row(setting: Setting, s: &Settings, out: &EffectiveOutput) -> Row<Item> {
    let item = setting.item;
    let (name, value) = parts(item, s, out);
    let value = match (item.switch(s, out), item.slider(s)) {
        (Some(on), _) => Value::Switch(on),
        (None, Some(position)) => Value::Slider {
            position,
            steps: item.slider_steps(),
            text: value.unwrap_or_default(),
        },
        _ => value.map_or(Value::Press, Value::Choice),
    };
    let about = lang::get(setting.text);
    let chosen = lang::get(description(item, s, out, setting.text));
    Row {
        chosen: (chosen != about).then_some(chosen),
        ..Row::new(item, name, value)
            .about(about)
            .cost(setting.cost)
            .blocked(item.blocked(s, out).map(lang::get))
    }
}

/// The vanilla rows `options.txt` keeps as it read them, not in `Settings`' own fields.
const FILE_KEYS: [&str; 3] = [
    "exclusiveFullscreen",
    "textureFiltering",
    "maxAnisotropyBit",
];

/// What the screen's rows read from the settings: what Undo puts back (`GraphicsScreen::undo`),
/// and what changes beside the screen (fullscreen, the language, the display's modes). A test
/// changes every row and checks this reads the change.
fn read(s: &Settings) -> impl std::fmt::Debug + '_ {
    let distances = (
        s.render_distance,
        s.simulation_distance,
        s.cloud_range,
        s.weather_radius,
        s.entity_distance,
        s.biome_blend,
    );
    let frames = (
        s.gamma,
        s.gui_scale,
        s.vsync,
        s.max_fps,
        &s.inactivity_fps_limit,
        s.menu_blur,
        s.chunk_fade,
        s.prioritize_chunk_updates,
    );
    let picture = (
        s.attack_indicator,
        s.autosave_indicator,
        &s.clouds,
        s.particles,
        s.smooth_lighting,
        s.entity_shadows,
        s.vignette,
        s.mipmap_levels,
        s.cutout_leaves,
        s.improved_transparency,
        &s.graphics_preset,
    );
    let window = (
        s.fullscreen,
        &s.video_modes,
        &s.file.fullscreen_resolution,
        FILE_KEYS.map(|key| s.file.get(key)),
        &s.language,
        s.force_unicode,
    );
    (&s.horizon, &s.extras, distances, frames, picture, window)
}

/// Horizon's graphics screen on the settings shell (`settings_shell`): its pages and rows from
/// [`rows`], what pressing them does, and Undo.
#[derive(Default)]
pub struct GraphicsScreen {
    /// The window's size in pixels, which a vanilla row's step can need (the fullscreen
    /// resolution): the options screen sets it before each input.
    pub(crate) framebuffer: [i32; 2],
    /// The output of the renderer the settings are drawn for (`OptionsUi::draw`), handed in
    /// each time the screen is reached.
    pub(crate) output: EffectiveOutput,
}

impl Screen for GraphicsScreen {
    type Page = Page;
    type Row = Item;
    fn title(&self) -> &'static str {
        TITLE
    }
    /// The rows read the settings, and what the device and display offer.
    fn stamp(&self, s: &Settings) -> u64 {
        use crate::graphics as g;
        let offers = (
            self.output,
            g::translucent_batching_offered(),
            g::gpu_culling_offer(),
            g::reduced_far_detail_suggested(),
        );
        shell::stamp_of(&(read(s), offers))
    }
    fn sheets(&self, s: &Settings) -> Vec<Sheet<Page, Item>> {
        let sheet = |(page, groups): (Page, Vec<(&'static str, Vec<Setting>)>)| Sheet {
            id: page,
            name: lang::get(page.name()),
            about: lang::get(page.about()),
            groups: groups
                .into_iter()
                .map(|(name, settings)| Group {
                    name,
                    rows: settings
                        .into_iter()
                        .filter(|r| shown(r.item, s))
                        .map(|r| row(r, s, &self.output))
                        .collect(),
                })
                .collect(),
            listed: true,
            heading: None,
        };
        rows().into_iter().map(sheet).collect()
    }
    fn press(&mut self, s: &mut Settings, item: Item, back: bool) -> Vec<Output> {
        match item {
            Item::Horizon(H::HdrCalibrate) if setup(s, &self.output) == Setup::Changed => {
                s.horizon.hdr_peak = 0;
                s.horizon.hdr_paper_white = 0;
                vec![Output::Event(Event::Changed)]
            }
            Item::Horizon(H::HdrCalibrate) => vec![Output::Push(OptionsPage::HdrCalibration)],
            // The list of either kind, as the Shaders page opens it from anywhere.
            Item::Horizon(H::ShaderPacks) => {
                s.shader_options = None;
                vec![
                    Output::Event(Event::ListShaderPacks),
                    Output::Event(Event::OpenShaderOptions),
                    Output::Push(OptionsPage::ShaderPacks),
                ]
            }
            _ => {
                item.cycle(s, back, self.framebuffer, &self.output);
                vec![Output::Event(Event::Changed)]
            }
        }
    }
    fn slide(&mut self, s: &mut Settings, item: Item, position: f32) {
        item.set_slider(s, position, &self.output);
    }
    /// With the "Scroll wheel changes settings" extra, the wheel steps a value between `<` `>`
    /// (up goes back); a slider and a tick box leave it to scroll the list.
    fn wheel_step(&mut self, s: &mut Settings, item: Item, back: bool) -> Option<Vec<Output>> {
        item.steps(s, &self.output).then(|| {
            item.cycle(s, back, self.framebuffer, &self.output);
            vec![
                Output::Event(Event::ButtonClick),
                Output::Event(Event::Changed),
            ]
        })
    }
    fn undoes(&self) -> bool {
        true
    }
    /// What this screen edits back to what it was when the screen opened: Horizon's graphics
    /// settings and the vanilla options on its rows. Not the other options (the narrator, or
    /// one `horizon-ctl options-set` changed meanwhile), nor fullscreen, which F11 and the
    /// window manager change.
    fn undo(&mut self, s: &mut Settings, b: &Settings) -> Vec<Output> {
        s.horizon = b.horizon.clone();
        s.render_distance = b.render_distance;
        s.simulation_distance = b.simulation_distance;
        s.gamma = b.gamma;
        s.gui_scale = b.gui_scale;
        s.vsync = b.vsync;
        s.max_fps = b.max_fps;
        s.inactivity_fps_limit = b.inactivity_fps_limit.clone();
        s.menu_blur = b.menu_blur;
        s.extras.fullbright = b.extras.fullbright;
        s.extras.dynamic_lights = b.extras.dynamic_lights;
        s.extras.hide_block_particles = b.extras.hide_block_particles;
        s.attack_indicator = b.attack_indicator;
        s.autosave_indicator = b.autosave_indicator;
        s.clouds = b.clouds.clone();
        s.cloud_range = b.cloud_range;
        s.weather_radius = b.weather_radius;
        s.particles = b.particles;
        s.smooth_lighting = b.smooth_lighting;
        s.biome_blend = b.biome_blend;
        s.entity_distance = b.entity_distance;
        s.entity_shadows = b.entity_shadows;
        s.vignette = b.vignette;
        s.chunk_fade = b.chunk_fade;
        s.mipmap_levels = b.mipmap_levels;
        s.cutout_leaves = b.cutout_leaves;
        s.improved_transparency = b.improved_transparency;
        s.prioritize_chunk_updates = b.prioritize_chunk_updates;
        // Presets set it beside the options above.
        s.graphics_preset = b.graphics_preset.clone();
        for key in FILE_KEYS {
            if let Some(value) = b.file.get(key) {
                s.file.set(key, value.clone());
            }
        }
        s.file.fullscreen_resolution = b.file.fullscreen_resolution.clone();
        Vec::new()
    }
    /// Calibration can change what Undo puts back, as a row does.
    fn changes(&self, outputs: &[Output]) -> bool {
        let calibrates = outputs.contains(&Output::Push(OptionsPage::HdrCalibration));
        calibrates || shell::changes_settings(outputs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graphics_style as style,
        settings_shell::{Columns, Hosted, Shell, Target},
    };
    use winit::keyboard::KeyCode as K;

    pub(super) const SIZE: (f32, f32) = (640.0, 360.0);
    pub(super) const FB: [i32; 2] = [1920, 1080];
    pub(super) const CLICK: Output = Output::Event(Event::ButtonClick);
    pub(super) const CHANGED: Output = Output::Event(Event::Changed);
    pub(super) const DRAGGED: Output = Output::Event(Event::SliderDrag);
    /// A renderer whose destination offers nothing and whose display reports nothing.
    pub(super) const NONE: EffectiveOutput = EffectiveOutput::NONE;

    pub(super) type Ui = Shell<GraphicsScreen>;
    pub(super) fn ui() -> Ui {
        let mut ui = Ui::default();
        ui.screen.framebuffer = FB;
        ui
    }
    /// What a window's renderer shows the screen when its display offers HDR output (`hdr`)
    /// and reports `system` HDR, at 400/1000 nits.
    pub(super) fn offering(hdr: bool, system: Option<bool>) -> EffectiveOutput {
        use crate::gfx::{HdrOutput, output::tests};
        let (caps, reading) = tests::window(hdr.then_some(HdrOutput::Pq), system);
        tests::snapshot(
            &crate::graphics::Graphics::default(),
            Default::default(),
            caps,
            reading,
        )
    }
    fn items() -> Vec<(Page, Item)> {
        rows()
            .into_iter()
            .flat_map(|(page, groups)| {
                groups
                    .into_iter()
                    .flat_map(|(_, rows)| rows)
                    .map(move |r| (page, r.item))
            })
            .collect()
    }
    /// Where `item`'s row is on screen now (its page open and the list scrolled to it first).
    pub(super) fn row(ui: &mut Ui, s: &Settings, item: Item) -> [f32; 4] {
        ui.show(items().into_iter().find(|(_, i)| *i == item).unwrap().0);
        let (_, y) = ui.point_at(s, SIZE, item);
        let cols = Columns::of(SIZE.0, SIZE.1);
        [cols.list_x, y - style::ROW / 2.0, cols.list_w, style::ROW]
    }
    pub(super) fn press(ui: &mut Ui, s: &mut Settings, at: (f32, f32)) -> Vec<Output> {
        ui.click(s, at, SIZE, false, false, false)
    }
    fn press_shift(ui: &mut Ui, s: &mut Settings, at: (f32, f32), shift: bool) -> Vec<Output> {
        ui.click(s, at, SIZE, false, false, shift)
    }
    fn key(ui: &mut Ui, s: &mut Settings, key: K) -> Vec<Output> {
        ui.key(s, key, None, false, SIZE)
    }
    fn focus_on(ui: &mut Ui, item: Item) {
        ui.set_focus(Some(Target::Row(item)));
    }

    #[test]
    fn one_screen_holds_every_video_setting_once_with_a_short_description() {
        let all = items();
        for (i, (_, item)) in all.iter().enumerate() {
            assert!(!all[..i].iter().any(|(_, x)| x == item), "{item:?} twice");
        }
        // Vanilla's Video Settings options are here beside Horizon's.
        for opt in [
            Opt::RenderDistance,
            Opt::Vsync,
            Opt::GuiScale,
            Opt::Clouds,
            Opt::Mipmaps,
        ] {
            assert!(all.iter().any(|(_, i)| *i == Item::Vanilla(opt)), "{opt:?}");
        }
        for page in Page::ALL {
            assert!(all.iter().any(|(p, _)| *p == page), "{page:?} is empty");
        }
        for (_, groups) in rows() {
            for r in groups.into_iter().flat_map(|(_, rows)| rows) {
                assert!(!r.text.is_empty() && r.text.len() < 80, "{:?}", r.item);
            }
        }
    }

    #[test]
    fn a_picked_page_shows_alone_in_its_groups_from_the_top() {
        let mut s = Settings::default();
        let mut ui = ui();
        let settings = |ui: &mut Ui, s: &Settings| {
            let (lines, _) = ui.lines_at(s, SIZE);
            assert!(
                lines[0].group.is_some(),
                "the list starts at a group's name"
            );
            ui.listed(s, SIZE)
        };
        // Upscaling shows only with TAA, off by default.
        let on = |page| -> Vec<(Page, Item)> {
            items()
                .into_iter()
                .filter(|(p, i)| *p == page && *i != Item::Horizon(H::Upscaling))
                .collect()
        };
        assert_eq!(settings(&mut ui, &s), on(Page::General));
        ui.scroll(&s, SIZE, -3.0);
        assert!(ui.scroll_amount() > 0.0);
        let [x, y, w, h] = Columns::of(SIZE.0, SIZE.1).page(1);
        assert_eq!(press(&mut ui, &mut s, (x + w / 2.0, y + h / 2.0)), [CLICK]);
        assert_eq!(settings(&mut ui, &s), on(Page::Quality));
        assert_eq!(ui.scroll_amount(), 0.0);
        assert_eq!(ui.current_page(&s, SIZE), Some(Page::Quality));
        // Groups sit apart: each name follows the gap under the panel before it.
        let (lines, _) = ui.lines_at(&s, SIZE);
        let names: Vec<_> = lines.iter().filter(|l| l.group.is_some()).collect();
        assert!(names.len() > 1);
        for pair in names.windows(2) {
            let (_, panel) = pair[0].group.unwrap();
            assert_eq!(
                pair[1].y,
                pair[0].y + style::GROUP_ROW + panel + style::GROUP_GAP
            );
        }
        // Opened at a page (Advanced..., the HDR key), it shows that page.
        let mut ui = Ui::default();
        ui.show(Page::Hdr);
        assert_eq!(settings(&mut ui, &s), on(Page::Hdr));
    }

    #[test]
    fn a_720p_window_keeps_the_page_list_the_buttons_and_five_rows() {
        // 1280x720 at GUI scale 3.
        let size = (427.0, 240.0);
        let mut s = Settings::default();
        let mut ui = ui();
        let cols = Columns::of(size.0, size.1);
        assert!((cols.bottom - cols.top) / style::ROW >= 5.0);
        let [x, y, w, h] = cols.page(Page::ALL.len() - 1);
        assert!(y >= 0.0 && x + w <= size.0);
        // Undo, over Done, stays clear of the page list.
        assert!(cols.button(1)[1] >= y + h);
        ui.click(
            &mut s,
            (x + w / 2.0, y + h / 2.0),
            size,
            false,
            false,
            false,
        );
        assert_eq!(ui.current_page(&s, size), Some(Page::Hdr));
    }

    #[test]
    fn the_search_filters_as_you_type_and_drops_pages_left_empty() {
        let mut s = Settings::default();
        let mut ui = ui();
        let cols = Columns::of(SIZE.0, SIZE.1);
        let all = ui.listed(&s, SIZE);
        assert!(all.iter().all(|(page, _)| *page == Page::General));
        let [x, y, ..] = cols.search();
        press(&mut ui, &mut s, (x + 4.0, y + 4.0));
        assert!(ui.typing());
        for c in ["c", "l", "o", "u", "d"] {
            ui.key(&mut s, K::KeyA, Some(c), false, SIZE);
        }
        let found = ui.listed(&s, SIZE);
        assert_eq!(
            found,
            [
                (Page::Quality, Item::Vanilla(Opt::Clouds)),
                (Page::Quality, Item::Vanilla(Opt::CloudRange))
            ]
        );
        // Words of a description find it too.
        ui.set_search("tearing");
        let found = ui.listed(&s, SIZE);
        assert!(found.iter().any(|(_, i)| *i == Item::Vanilla(Opt::Vsync)));
        ui.set_search("hdr");
        let [x, y, w, h] = cols.page(Page::ALL.len() - 1);
        press(&mut ui, &mut s, (x + w / 2.0, y + h / 2.0));
        assert_eq!(ui.current_page(&s, SIZE), Some(Page::Hdr));
        let [x, y, ..] = cols.clear();
        assert_eq!(press(&mut ui, &mut s, (x + 2.0, y + 2.0)), [CLICK]);
        // The clear button leaves the page picked meanwhile open, alone, from the top.
        let listed = ui.listed(&s, SIZE);
        assert!(listed.len() < items().len() && listed.iter().all(|(p, _)| *p == Page::Hdr));
        assert_eq!(ui.scroll_amount(), 0.0);
        assert_eq!(ui.current_page(&s, SIZE), Some(Page::Hdr));
    }

    #[test]
    fn undo_appears_after_changes_and_puts_back_what_the_screen_opened_with() {
        let mut s = Settings::default();
        let mut ui = ui();
        let undo = Columns::of(SIZE.0, SIZE.1).button(1);
        let at = |[x, y, ..]: [f32; 4]| (x + 2.0, y + 2.0);
        // Nothing changed: Undo does nothing.
        assert!(press(&mut ui, &mut s, at(undo)).is_empty());
        let fog = row(&mut ui, &s, Item::Horizon(H::Fog));
        assert_eq!(press(&mut ui, &mut s, at(fog)), [CLICK, CHANGED]);
        let sky = row(&mut ui, &s, Item::Horizon(H::Sky));
        press(&mut ui, &mut s, at(sky));
        assert!(!s.horizon.fog && !s.horizon.sky && ui.changed());
        // F11 and another option meanwhile: Undo leaves them as they are.
        s.fullscreen = true;
        s.chat_opacity = 0.25;
        assert_eq!(press(&mut ui, &mut s, at(undo)), [CLICK, CHANGED]);
        assert!(s.horizon.fog && s.horizon.sky && !ui.changed());
        assert!(s.fullscreen);
        assert_eq!(s.chat_opacity, 0.25);
        // Calibration counts as a change too.
        ui.screen.output = offering(true, None);
        s.horizon.hdr = HdrMode::On;
        let calibrate = row(&mut ui, &s, Item::Horizon(H::HdrCalibrate));
        press(&mut ui, &mut s, at(calibrate));
        assert!(ui.changed());
        // From the keyboard: Undo puts it back and goes away.
        ui.set_focus(Some(Target::Undo));
        assert_eq!(key(&mut ui, &mut s, K::Enter), [CLICK, CHANGED]);
        assert!(!ui.screen.output.requests_hdr(s.horizon.hdr) && !ui.changed());
        assert_eq!(ui.focused(), Some(Target::Done));
    }

    #[test]
    fn a_slider_takes_a_press_on_its_track_and_its_click_plays_on_release() {
        let mut s = Settings::default();
        let mut ui = ui();
        s.horizon.chunk_updates = 16;
        let r = row(&mut ui, &s, Item::Horizon(H::ChunkUpdates));
        let (tx, tw) = Columns::of(SIZE.0, SIZE.1).track();
        // On its name: focused, unchanged.
        assert!(press(&mut ui, &mut s, (r[0] + 4.0, r[1] + 4.0)).is_empty());
        assert_eq!(s.horizon.chunk_updates, 16);
        assert!(ui.changes_value());
        assert!(ui.release().is_none());
        // At the track's end: the most, then dragged to its start.
        assert_eq!(press(&mut ui, &mut s, (tx + tw, r[1] + 4.0)), [DRAGGED]);
        assert_eq!(H::ChunkUpdates.slider(&s), Some(1.0));
        assert_eq!(
            ui.click(&mut s, (-100.0, -100.0), SIZE, true, false, false),
            [DRAGGED]
        );
        assert_eq!(H::ChunkUpdates.slider(&s), Some(0.0));
        assert!(ui.release().is_some());
        assert!(ui.release().is_none());
    }

    #[test]
    fn the_scrollbar_can_be_held_and_dragged() {
        // #3552: AbstractScrollArea.updateScrolling holds the scrollbar on a press over its
        // column, mouseDragged moves the list with the pointer, onRelease lets go.
        let mut s = Settings::default();
        let mut ui = ui();
        let cols = Columns::of(SIZE.0, SIZE.1);
        let (_, content) = ui.lines_at(&s, SIZE);
        let x = cols.bar_x() + 2.0;
        let drag =
            |ui: &mut Ui, s: &mut Settings, y, drag| ui.click(s, (x, y), SIZE, drag, false, false);
        assert!(drag(&mut ui, &mut s, cols.top + 5.0, false).is_empty());
        drag(&mut ui, &mut s, cols.top + 15.0, true);
        assert!(ui.scroll_amount() > 0.0);
        drag(&mut ui, &mut s, cols.bottom + 50.0, true);
        assert_eq!(ui.scroll_amount(), content - (cols.bottom - cols.top));
        drag(&mut ui, &mut s, cols.top - 50.0, true);
        assert_eq!(ui.scroll_amount(), 0.0);
        assert!(ui.release().is_none());
        drag(&mut ui, &mut s, cols.bottom + 50.0, true);
        assert_eq!(ui.scroll_amount(), 0.0);
    }

    #[test]
    fn a_held_scroller_lets_go_when_the_window_gets_too_short_for_it() {
        let mut s = Settings::default();
        let mut ui = ui();
        let cols = Columns::of(SIZE.0, SIZE.1);
        let x = cols.bar_x() + 2.0;
        ui.click(&mut s, (x, cols.top + 5.0), SIZE, false, false, false);
        assert!(ui.scroller_held());
        // A 130-high GUI leaves the list under the 40 px the scroller's thumb needs.
        let short = (SIZE.0, 130.0);
        assert!(!Columns::of(short.0, short.1).scrolls());
        assert!(
            ui.click(&mut s, (x, 80.0), short, true, false, false)
                .is_empty()
        );
        assert!(!ui.scroller_held());
    }

    /// The rows only vanilla's Video Settings had work here as they do there: its preset
    /// applies its distances (`Options.applyGraphicsPreset`), Max Anisotropy waits for
    /// Anisotropic filtering, Exclusive Fullscreen is a tick box of `options.txt`.
    #[test]
    fn vanilla_video_rows_work_on_the_graphics_screen() {
        let mut s = Settings::default();
        let preset = Item::Vanilla(Opt::Preset);
        preset.set_slider(&mut s, 0.0, &NONE);
        assert_eq!(
            (s.graphics_preset.as_str(), s.render_distance, s.cloud_range),
            ("fast", 8, 32)
        );
        preset.set_slider(&mut s, 2.0 / 3.0, &NONE);
        assert_eq!(
            (s.graphics_preset.as_str(), s.render_distance, s.cloud_range),
            ("fabulous", 32, 128)
        );
        let anisotropy = Item::Vanilla(Opt::MaxAnisotropy);
        let filtering = Item::Vanilla(Opt::TextureFiltering);
        while s.file.int("textureFiltering") == 2 {
            filtering.cycle(&mut s, false, FB, &NONE);
        }
        assert!(anisotropy.blocked(&s, &NONE).is_some());
        while s.file.int("textureFiltering") != 2 {
            filtering.cycle(&mut s, false, FB, &NONE);
        }
        assert!(anisotropy.blocked(&s, &NONE).is_none());
        let exclusive = Item::Vanilla(Opt::ExclusiveFullscreen);
        assert_eq!(exclusive.switch(&s, &NONE), Some(false));
        exclusive.cycle(&mut s, false, FB, &NONE);
        assert_eq!(exclusive.switch(&s, &NONE), Some(true));
        assert!(s.file.bool("exclusiveFullscreen"));
        // The graphics extras switch their own setting.
        let lights = Item::Extra(crate::extras::Item::DynamicLights);
        lights.cycle(&mut s, false, FB, &NONE);
        assert!(s.extras.dynamic_lights);
        assert_eq!(lights.switch(&s, &NONE), Some(true));
    }

    #[test]
    fn the_wheel_scrolls_and_stops_at_both_ends_without_changing_values() {
        let mut s = Settings::default();
        let mut ui = ui();
        s.gamma = 0.25;
        let gamma = row(&mut ui, &s, Item::Vanilla(Opt::Gamma));
        // A slider being changed doesn't take the wheel either: a controller scrolls with it.
        press(&mut ui, &mut s, (gamma[0] + 4.0, gamma[1] + 4.0));
        assert!(ui.changes_value());
        let before = ui.scroll_amount();
        ui.scroll(&s, SIZE, -1.0);
        assert_eq!(ui.scroll_amount(), before + 18.0);
        assert_eq!(s.gamma, 0.25);
        let cols = Columns::of(SIZE.0, SIZE.1);
        let (_, content) = ui.lines_at(&s, SIZE);
        ui.scroll(&s, SIZE, -1000.0);
        assert_eq!(ui.scroll_amount(), content - (cols.bottom - cols.top));
        ui.scroll(&s, SIZE, 1000.0);
        assert_eq!(ui.scroll_amount(), 0.0);
    }

    #[test]
    fn a_hovered_value_takes_the_wheel_only_with_the_extra_on() {
        let mut s = Settings::default();
        let mut ui = ui();
        let item = Item::Vanilla(Opt::AttackIndicator);
        let r = row(&mut ui, &s, item);
        let before = format!("{s:?}");
        let outputs = ui.wheel(&mut s, SIZE, (r[0] + 4.0, r[1] + 4.0), -1.0);
        assert_eq!(format!("{s:?}"), before, "the hovered value stays put");
        assert!(outputs.is_empty());

        s.extras.wheel_cycles_settings = true;
        let r = row(&mut ui, &s, item);
        let before = format!("{s:?}");
        let outputs = ui.wheel(&mut s, SIZE, (r[0] + 4.0, r[1] + 4.0), -1.0);
        assert_ne!(format!("{s:?}"), before, "the hovered value steps");
        assert_eq!(outputs, [CLICK, CHANGED]);
        // A tick box leaves it to scroll the list.
        let vsync = row(&mut ui, &s, Item::Vanilla(Opt::Vsync));
        let before = s.vsync;
        assert!(
            ui.wheel(&mut s, SIZE, (vsync[0] + 4.0, vsync[1] + 4.0), -1.0)
                .is_empty()
        );
        assert_eq!(s.vsync, before);
    }

    #[test]
    fn the_arrows_move_between_rows_in_view_and_over_to_the_page_list() {
        let mut s = Settings::default();
        let mut ui = ui();
        // General's last row, against the bottom of the view, then Up: the row over it.
        let last = Item::Vanilla(Opt::Autosave);
        row(&mut ui, &s, last);
        focus_on(&mut ui, last);
        key(&mut ui, &mut s, K::ArrowUp);
        let indicator = Item::Vanilla(Opt::AttackIndicator);
        assert_eq!(ui.focused_row(), Some(indicator));
        assert!(ui.in_view(&s, SIZE, indicator));
        // Left leaves the plate.
        key(&mut ui, &mut s, K::ArrowLeft);
        assert_eq!(ui.focused_row(), None, "{:?}", ui.focused());
        // Up from Done: the page list's last page.
        ui.set_focus(Some(Target::Done));
        key(&mut ui, &mut s, K::ArrowUp);
        assert_eq!(ui.focused(), Some(Target::Page(Page::Hdr)));
        // Its row wheeled far out of view: Down takes the first row in view, as it is.
        focus_on(&mut ui, Item::Horizon(H::Preset));
        ui.scroll(&s, SIZE, -1000.0);
        let before = ui.scroll_amount();
        assert!(before > 0.0);
        key(&mut ui, &mut s, K::ArrowDown);
        let item = ui.focused_row().unwrap();
        assert!(ui.in_view(&s, SIZE, item), "{item:?}");
        assert_eq!(ui.scroll_amount(), before);
    }

    #[test]
    fn the_keyboard_tabs_through_settings_and_steps_sliders() {
        let mut s = Settings::default();
        let mut ui = ui();
        s.horizon.chunk_updates = 10;
        let tab_to = |ui: &mut Ui, s: &mut Settings, target| {
            (0..40).any(|_| {
                key(ui, s, K::Tab);
                ui.focused() == Some(target)
            })
        };
        // Tab reaches the page list; Enter opens Performance and Tab goes on into it.
        assert!(tab_to(&mut ui, &mut s, Target::Page(Page::Performance)));
        assert_eq!(key(&mut ui, &mut s, K::Enter), [CLICK]);
        let chunk_updates = Item::Horizon(H::ChunkUpdates);
        assert!(tab_to(&mut ui, &mut s, Target::Row(chunk_updates)) && ui.changes_value());
        // Tab scrolled its row into view.
        assert!(ui.in_view(&s, SIZE, chunk_updates));
        assert_eq!(key(&mut ui, &mut s, K::ArrowRight), [DRAGGED]);
        assert_eq!(s.horizon.chunk_updates, 11);
        key(&mut ui, &mut s, K::ArrowLeft);
        assert_eq!(s.horizon.chunk_updates, 10);
        // Space lets go of the value: the arrows move the focus again.
        key(&mut ui, &mut s, K::Space);
        assert!(!ui.changes_value());
        key(&mut ui, &mut s, K::ArrowDown);
        assert_eq!(ui.focused_row(), Some(Item::Horizon(H::UploadBudget)));
        // Enter presses a switch; Escape leaves.
        row(&mut ui, &s, Item::Horizon(H::Fog));
        focus_on(&mut ui, Item::Horizon(H::Fog));
        assert_eq!(key(&mut ui, &mut s, K::Enter), [CLICK, CHANGED]);
        assert!(!s.horizon.fog);
        assert_eq!(key(&mut ui, &mut s, K::Escape), [Output::Done]);
    }

    /// The strip says what a row does, what its chosen value does where that differs, and why a
    /// greyed row can't be used; the search finds a row by what it does.
    #[test]
    fn rows_describe_their_value_their_cost_and_why_they_are_greyed() {
        let mut s = Settings::default();
        let rows_of = |s: &Settings| -> Vec<Row<Item>> {
            let sheets = GraphicsScreen::default().sheets(s);
            let groups = sheets.into_iter().flat_map(|sheet| sheet.groups);
            groups.flat_map(|group| group.rows).collect()
        };
        let find = |s: &Settings, item| rows_of(s).into_iter().find(|r| r.id == item).unwrap();
        let culling = find(&s, Item::Horizon(H::EntityCulling));
        assert_eq!(
            culling.chosen.as_deref(),
            Some("No entity occlusion culling.")
        );
        assert!(culling.cost.is_some() && culling.live);
        s.horizon.entity_culling = EntityCullingMode::Off;
        let precheck = find(&s, Item::Horizon(H::EntityCullingPrecheck));
        assert!(!precheck.live);
        assert_eq!(
            precheck.why.as_deref(),
            Some("Only applies to asynchronous entity culling.")
        );
        let calibrate = find(&s, Item::Horizon(H::HdrCalibrate));
        assert_eq!(calibrate.value, Value::Press);
    }

    /// Every change a press or a slide makes, the stamp reads (`read`: the rows are built again),
    /// and Undo puts back, but for fullscreen, which F11 changes too.
    #[test]
    fn every_change_a_row_makes_is_stamped_and_undone() {
        let mut screen = GraphicsScreen {
            framebuffer: FB,
            output: NONE,
        };
        let default = Settings::default();
        for (_, item) in items() {
            let mut s = default.clone();
            if item.blocked(&s, &NONE).is_some() {
                continue;
            }
            match item.slider(&s) {
                Some(position) => {
                    item.set_slider(&mut s, if position < 0.5 { 1.0 } else { 0.0 }, &NONE)
                }
                None => {
                    screen.press(&mut s, item, false);
                }
            }
            if format!("{s:?}") == format!("{default:?}") {
                continue;
            }
            // `read` and not the stamp, which also reads what other tests offer meanwhile.
            let reads = |s: &Settings| format!("{:?}", read(s));
            assert_ne!(reads(&s), reads(&default), "{item:?}");
            if item != Item::Vanilla(Opt::Fullscreen) {
                screen.undo(&mut s, &default);
                assert_eq!(reads(&s), reads(&default), "{item:?}");
            }
        }
    }

    #[test]
    fn rows_split_vanilla_labels_into_name_and_value() {
        let s = Settings::default();
        let (name, value) = parts(Item::Horizon(H::LodDistance), &s, &NONE);
        assert_eq!(
            (name.as_str(), value.as_deref()),
            ("LOD Distance", Some("Off"))
        );
        let (name, value) = parts(Item::Horizon(H::HdrCalibrate), &s, &NONE);
        assert!(!name.is_empty() && value.is_none());
    }

    #[test]
    fn lod_distance_slides_after_render_distance_without_changing_it() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::LodDistance);
        let near = s.render_distance;
        assert_eq!(item.label(&s, &NONE).plain(), "LOD Distance: Off");
        assert_eq!(item.slider(&s), Some(0.0));
        let items: Vec<_> = items().into_iter().map(|(_, i)| i).collect();
        let render = items
            .iter()
            .position(|i| *i == Item::Vanilla(Opt::RenderDistance))
            .unwrap();
        assert_eq!(items[render + 1], item);
        // Every stop is reachable and reads back as the same position.
        for (i, &distance) in LOD_STEPS.iter().enumerate() {
            let at = i as f32 / (LOD_STEPS.len() - 1) as f32;
            item.set_slider(&mut s, at, &NONE);
            assert_eq!(s.horizon.lod_distance, distance);
            assert_eq!(item.slider(&s), Some(at));
            assert_eq!(s.render_distance, near);
        }
        // The ends are off and the whole world, which the far terrain plans.
        item.set_slider(&mut s, 0.0, &NONE);
        assert_eq!(s.horizon.lod_distance, 0);
        item.set_slider(&mut s, 1.0, &NONE);
        assert_eq!(s.horizon.lod_distance, crate::lod::MAX_CHUNKS);
        assert_eq!(
            item.label(&s, &NONE).plain(),
            format!("LOD Distance: {} chunks", crate::lod::MAX_CHUNKS)
        );
        item.set_slider(&mut s, 11.0 / 23.0, &NONE);
        assert_eq!(s.horizon.lod_distance, 1024);
        assert_eq!(item.label(&s, &NONE).plain(), "LOD Distance: 1024 chunks");
        // The fine stops: a ring of far terrain just past a small render
        // distance, which the doubling steps skipped (#9694).
        item.set_slider(&mut s, 1.0 / 23.0, &NONE);
        assert_eq!(s.horizon.lod_distance, 16);
        assert_eq!(item.label(&s, &NONE).plain(), "LOD Distance: 16 chunks");
        item.set_slider(&mut s, 2.0 / 23.0, &NONE);
        assert_eq!(s.horizon.lod_distance, 24);
        // A distance between stops (from the file) sits on the next one.
        s.horizon.lod_distance = 3000;
        assert_eq!(item.slider(&s), Some(13.0 / 23.0));
        s.horizon.lod_distance = 12;
        assert_eq!(item.slider(&s), Some(1.0 / 23.0));
        assert_eq!(
            Item::Vanilla(Opt::RenderDistance).label(&s, &NONE).plain(),
            format!("Full Detail Distance: {near} chunks")
        );
    }

    #[test]
    fn blue_map_settings_sit_with_far_terrain() {
        let mut s = Settings::default();
        let find = Item::Horizon(H::BlueMapAutoDetect);
        let radius = Item::Horizon(H::BlueMapRadius);
        let distance: Vec<Item> = rows()
            .into_iter()
            .flat_map(|(_, groups)| groups)
            .find(|(name, _)| *name == "Distance")
            .unwrap()
            .1
            .iter()
            .map(|r| r.item)
            .collect();
        assert!(distance.contains(&find) && distance.contains(&radius));
        assert!(distance.contains(&Item::Horizon(H::FarDetail)));
        assert_eq!(find.switch(&s, &NONE), Some(false), "off by default");
        assert_eq!(radius.label(&s, &NONE).plain(), "BlueMap Radius: 64 chunks");
        find.cycle(&mut s, false, FB, &NONE);
        assert!(s.horizon.blue_map.auto_detect);
        radius.cycle(&mut s, false, FB, &NONE);
        assert_eq!(s.horizon.blue_map.radius, 128);
        radius.cycle(&mut s, true, FB, &NONE);
        radius.cycle(&mut s, true, FB, &NONE);
        assert_eq!(radius.label(&s, &NONE).plain(), "BlueMap Radius: 32 chunks");
        assert_eq!(s.horizon.preset, Preset::Custom);
    }

    #[test]
    fn presets_set_vanilla_and_horizon_options() {
        let mut s = Settings::default();
        Item::Horizon(H::Preset).cycle(&mut s, false, [854, 480], &NONE);
        assert_eq!(s.horizon.preset, Preset::Performance);
        assert_eq!(s.render_distance, 8);
        assert_eq!(s.clouds, "false");
        // Any quality change leaves the preset.
        Item::Vanilla(Opt::Clouds).cycle(&mut s, false, [854, 480], &NONE);
        assert_eq!(s.horizon.preset, Preset::Custom);
        // Display preferences keep it.
        Item::Horizon(H::Preset).cycle(&mut s, true, [854, 480], &NONE);
        Item::Vanilla(Opt::Vsync).cycle(&mut s, false, [854, 480], &NONE);
        assert_eq!(s.horizon.preset, Preset::Ultra);
    }

    #[test]
    fn screenshot_quality_cycles_1_2_4_8_and_keeps_the_preset() {
        let mut s = Settings::default();
        Item::Horizon(H::Preset).cycle(&mut s, false, [854, 480], &NONE);
        let mut seen = Vec::new();
        for _ in 0..4 {
            Item::Horizon(H::ScreenshotQuality).cycle(&mut s, false, [854, 480], &NONE);
            seen.push(s.horizon.screenshot_quality);
        }
        assert_eq!(seen, [2, 4, 8, 1]);
        Item::Horizon(H::ScreenshotQuality).cycle(&mut s, true, [854, 480], &NONE);
        assert_eq!(s.horizon.screenshot_quality, 8);
        assert_eq!(s.horizon.preset, Preset::Performance);
    }

    #[test]
    fn upload_budget_slider_steps() {
        let mut s = Settings::default();
        H::UploadBudget.set_slider(&mut s, 1.0);
        assert_eq!(s.horizon.upload_budget_kib, 16384);
        assert_eq!(H::UploadBudget.slider(&s), Some(1.0));
        H::UploadBudget.set_slider(&mut s, 0.0);
        assert_eq!(s.horizon.upload_budget_kib, 0);
    }

    #[test]
    fn antialiasing_cycles_off_fxaa_taa() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::Antialiasing);
        for label in ["FXAA", "TAA", "Off"] {
            item.cycle(&mut s, false, [854, 480], &NONE);
            assert_eq!(
                item.label(&s, &NONE).plain(),
                format!("Anti-aliasing: {label}")
            );
        }
        item.cycle(&mut s, true, [854, 480], &NONE);
        assert_eq!((s.horizon.fxaa, s.horizon.taa), (false, true));

        // Upscaling needs TAA: a ratio turns it on, leaving TAA goes back to Native.
        s.horizon.taa = false;
        let upscaling = Item::Horizon(H::Upscaling);
        upscaling.cycle(&mut s, false, [854, 480], &NONE);
        assert_eq!(upscaling.label(&s, &NONE).plain(), "Upscaling: Quality");
        assert!(s.horizon.taa);
        item.cycle(&mut s, false, [854, 480], &NONE);
        assert_eq!(s.horizon.upscaling, Upscaling::Native);

        // The Upscaling row is only on the screen while TAA is on.
        assert!(!shown(upscaling, &s));
        s.horizon.taa = true;
        assert!(shown(upscaling, &s));
    }

    #[test]
    fn entity_culling_modes_are_opt_in_and_cycle_from_its_row() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::EntityCulling);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Off);
        assert_eq!(item.label(&s, &NONE).plain(), "Entity Culling: Off");
        for mode in EntityCullingMode::ALL {
            s.horizon.entity_culling = mode;
            for preset in Preset::ALL {
                crate::graphics::apply_preset(preset, &mut s);
                assert_eq!(s.horizon.entity_culling, mode);
            }
        }
        s.horizon.entity_culling = EntityCullingMode::Off;
        let mut ui = ui();
        let r = row(&mut ui, &s, item);
        for mode in [
            EntityCullingMode::Synchronous,
            EntityCullingMode::Asynchronous,
        ] {
            s.horizon.preset = Preset::Balanced;
            assert_eq!(
                press(&mut ui, &mut s, (r[0] + 4.0, r[1] + 4.0)),
                [CLICK, CHANGED]
            );
            assert_eq!(s.horizon.entity_culling, mode);
            assert_eq!(s.horizon.preset, Preset::Custom);
        }
        assert_eq!(
            item.label(&s, &NONE).plain(),
            "Entity Culling: Asynchronous"
        );
    }

    #[test]
    fn particle_culling_modes_are_opt_in_and_cycle_from_its_row() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::ParticleCulling);
        assert_eq!(s.horizon.particle_culling, ParticleCullingMode::Off);
        assert_eq!(item.label(&s, &NONE).plain(), "Particle Culling: Off");
        for mode in ParticleCullingMode::ALL {
            s.horizon.particle_culling = mode;
            for preset in Preset::ALL {
                crate::graphics::apply_preset(preset, &mut s);
                assert_eq!(s.horizon.particle_culling, mode);
            }
            assert_eq!(
                crate::graphics::Graphics::parse(&s.horizon.serialize()).particle_culling,
                mode
            );
        }
        s.horizon.particle_culling = ParticleCullingMode::Off;
        let mut ui = ui();
        let r = row(&mut ui, &s, item);
        for (mode, label) in [
            (ParticleCullingMode::Synchronous, "Synchronous"),
            (ParticleCullingMode::Asynchronous, "Asynchronous"),
            (ParticleCullingMode::Off, "Off"),
        ] {
            s.horizon.preset = Preset::Balanced;
            assert_eq!(
                press(&mut ui, &mut s, (r[0] + 4.0, r[1] + 4.0)),
                [CLICK, CHANGED]
            );
            assert_eq!(s.horizon.particle_culling, mode);
            assert_eq!(s.horizon.preset, Preset::Custom);
            assert_eq!(
                item.label(&s, &NONE).plain(),
                format!("Particle Culling: {label}")
            );
        }
    }

    #[test]
    fn graphics_cycle_button_cycles_forward_and_shift_click_cycles_backward_with_wrapping() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::EntityCulling);
        let mut ui = ui();
        let r = row(&mut ui, &s, item);
        let at = (r[0] + 4.0, r[1] + 4.0);

        // 1. Feature without shift (normal click): cycles forward (+1)
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Off);
        assert_eq!(press_shift(&mut ui, &mut s, at, false), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Synchronous);

        assert_eq!(press_shift(&mut ui, &mut s, at, false), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Asynchronous);

        // 3. Wrap-around forward (at max value, normal click wraps to min / 0)
        assert_eq!(press_shift(&mut ui, &mut s, at, false), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Off);

        // 4. Wrap-around backward (at min / 0, shift-click wraps to max)
        assert_eq!(press_shift(&mut ui, &mut s, at, true), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Asynchronous);

        // 2. Feature with shift: cycles backward (-1)
        assert_eq!(press_shift(&mut ui, &mut s, at, true), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Synchronous);

        assert_eq!(press_shift(&mut ui, &mut s, at, true), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Off);
    }

    #[test]
    fn left_and_right_step_a_focused_choice_back_and_on() {
        let mut s = Settings::default();
        let mut ui = ui();
        row(&mut ui, &s, Item::Horizon(H::EntityCulling));
        focus_on(&mut ui, Item::Horizon(H::EntityCulling));
        assert_eq!(key(&mut ui, &mut s, K::ArrowLeft), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Asynchronous);
        assert_eq!(key(&mut ui, &mut s, K::ArrowRight), [CLICK, CHANGED]);
        assert_eq!(s.horizon.entity_culling, EntityCullingMode::Off);
        assert_eq!(ui.focused_row(), Some(Item::Horizon(H::EntityCulling)));
    }

    #[test]
    fn entity_culling_precheck_slider_is_only_live_for_async() {
        let item = Item::Horizon(H::EntityCullingPrecheck);
        let mut s = Settings::default();
        let initial = s.horizon.entity_culling_precheck;

        for mode in [EntityCullingMode::Off, EntityCullingMode::Synchronous] {
            s.horizon.entity_culling = mode;
            assert_eq!(
                item.blocked(&s, &NONE),
                Some("Only applies to asynchronous entity culling.")
            );

            let mut ui = ui();
            let row_rect = row(&mut ui, &s, item);
            assert!(press(&mut ui, &mut s, (row_rect[0] + 4.0, row_rect[1] + 4.0)).is_empty());
            item.set_slider(&mut s, 1.0, &NONE);
            assert_eq!(s.horizon.entity_culling_precheck, initial);

            assert!(!ui.tab_reaches(&s, SIZE, item));
            focus_on(&mut ui, item);
            key(&mut ui, &mut s, K::Enter);
            assert!(!ui.changes_value());
            key(&mut ui, &mut s, K::ArrowRight);
            assert_eq!(s.horizon.entity_culling_precheck, initial);
        }

        for (code, expected) in [
            ("en_us", "Only applies to asynchronous entity culling."),
            ("de_de", "Nur für asynchrones Objekt-Culling."),
            (
                "es_es",
                "Solo se aplica a la oclusión asíncrona de entidades.",
            ),
            (
                "fr_fr",
                "S'applique uniquement à l'occlusion asynchrone des entités.",
            ),
            ("pl_pl", "Dotyczy tylko asynchronicznego pomijania bytów."),
            (
                "ru_ru",
                "Применяется только при асинхронном отсечении сущностей.",
            ),
            ("tr_tr", "Yalnızca eşzamansız varlık elemesine uygulanır."),
            (
                "uk_ua",
                "Застосовується лише для асинхронного відсікання сутностей.",
            ),
            ("zh_cn", "仅适用于异步实体剔除。"),
        ] {
            let (shown, missed) = lang::scoped(lang::Language::load(&[], code), || {
                lang::get(item.blocked(&s, &NONE).unwrap())
            });
            assert_eq!(shown, expected, "{code}");
            assert!(missed.is_empty(), "{code}: {missed:?}");
        }

        s.horizon.entity_culling = EntityCullingMode::Asynchronous;
        assert_eq!(item.blocked(&s, &NONE), None);
        let mut ui = ui();
        let row_rect = row(&mut ui, &s, item);
        let (track_x, track_w) = Columns::of(SIZE.0, SIZE.1).track();
        assert_eq!(
            press(
                &mut ui,
                &mut s,
                (track_x + track_w - 4.0, row_rect[1] + 4.0)
            ),
            [DRAGGED]
        );
        assert_eq!(s.horizon.entity_culling_precheck, 512);
    }

    /// Translucent Batching: a switch on the Advanced page, on by default, no part of a
    /// preset, and greyed out, reading Off, where the device can't batch.
    #[test]
    fn translucent_batching_is_an_advanced_switch_where_the_device_can() {
        let item = Item::Horizon(H::TranslucentBatching);
        let page = items()
            .into_iter()
            .find(|(_, i)| *i == item)
            .map(|(p, _)| p);
        if cfg!(target_os = "macos") {
            // Metal draws flat: never offered, so not shown.
            assert_eq!(page, None);
            return;
        }
        assert_eq!(page, Some(Page::Advanced));
        let mut s = Settings::default();
        s.horizon.preset = Preset::Balanced;
        let mut ui = ui();
        // Where the device can't, the saved On reads Off, and neither a click nor Enter
        // after it does anything: no sound, no change, no save.
        crate::graphics::offer_translucent_batching(false);
        assert!(item.blocked(&s, &NONE).is_some());
        assert_eq!(item.label(&s, &NONE).plain(), "Translucent Batching: Off");
        assert_eq!(item.switch(&s, &NONE), Some(false));
        let r = row(&mut ui, &s, item);
        let point = (r[0] + 4.0, r[1] + 4.0);
        assert!(press(&mut ui, &mut s, point).is_empty());
        focus_on(&mut ui, item);
        assert!(key(&mut ui, &mut s, K::Enter).is_empty());
        assert!(s.horizon.translucent_batching);
        // Where it can, a click switches it and keeps the preset.
        crate::graphics::offer_translucent_batching(true);
        assert_eq!(item.label(&s, &NONE).plain(), "Translucent Batching: On");
        assert_eq!(item.switch(&s, &NONE), Some(true));
        assert_eq!(press(&mut ui, &mut s, point), [CLICK, CHANGED]);
        assert!(!s.horizon.translucent_batching);
        assert_eq!(item.label(&s, &NONE).plain(), "Translucent Batching: Off");
        assert_eq!(s.horizon.preset, Preset::Balanced);
        // No preset sets it.
        for on in [false, true] {
            s.horizon.translucent_batching = on;
            for preset in Preset::ALL {
                crate::graphics::apply_preset(preset, &mut s);
                assert_eq!(s.horizon.translucent_batching, on, "{preset:?}");
            }
        }
    }

    /// GPU Culling: Off, On or Auto right after Cave Culling, off by default, no part of a
    /// preset, and greyed out with its reason, reading Off, wherever it isn't offered.
    #[test]
    fn gpu_culling_steps_off_on_auto_where_it_is_offered() {
        use crate::graphics::GpuCullingOffer;
        let item = Item::Horizon(H::GpuCulling);
        let at = |item| items().into_iter().position(|(_, i)| i == item);
        if cfg!(target_os = "macos") {
            // Metal draws no count the GPU wrote: never offered, so not shown.
            assert_eq!(at(item), None);
            return;
        }
        let i = at(item).unwrap();
        assert_eq!(items()[i].0, Page::Performance);
        assert_eq!(at(Item::Horizon(H::CaveCulling)), Some(i - 1));
        let mut s = Settings::default();
        assert_eq!(item.switch(&s, &NONE), None);
        assert_eq!(s.horizon.gpu_culling, GpuCullingMode::Off);
        s.horizon.preset = Preset::Balanced;
        s.horizon.gpu_culling = GpuCullingMode::On;
        // Where it isn't offered, the saved On reads Off, and neither a click nor Enter after
        // it does anything.
        for offer in [
            GpuCullingOffer::Unsupported,
            GpuCullingOffer::Software,
            GpuCullingOffer::OutOfMemory,
            GpuCullingOffer::Dx12,
            GpuCullingOffer::DeviceLost,
        ] {
            crate::graphics::offer_gpu_culling(offer);
            assert!(offer.reason().is_some());
            assert_eq!(item.blocked(&s, &NONE), offer.reason());
            assert_eq!(item.label(&s, &NONE).plain(), "GPU Culling: Off");
            let mut ui = ui();
            let r = row(&mut ui, &s, item);
            assert!(press(&mut ui, &mut s, (r[0] + 4.0, r[1] + 4.0)).is_empty());
            focus_on(&mut ui, item);
            assert!(key(&mut ui, &mut s, K::Enter).is_empty());
            assert_eq!(s.horizon.gpu_culling, GpuCullingMode::On, "{offer:?}");
        }
        // Where it is, it steps through its three values and keeps the preset.
        crate::graphics::offer_gpu_culling(GpuCullingOffer::Offered);
        assert_eq!(item.blocked(&s, &NONE), None);
        for label in ["Auto", "Off", "On"] {
            item.cycle(&mut s, false, FB, &NONE);
            assert_eq!(
                item.label(&s, &NONE).plain(),
                format!("GPU Culling: {label}")
            );
            assert_eq!(
                crate::graphics::Graphics::parse(&s.horizon.serialize()).gpu_culling,
                s.horizon.gpu_culling
            );
        }
        item.cycle(&mut s, true, FB, &NONE);
        assert_eq!(s.horizon.gpu_culling, GpuCullingMode::Off);
        assert_eq!(s.horizon.preset, Preset::Balanced);
        // Auto says what it does.
        s.horizon.gpu_culling = GpuCullingMode::Auto;
        assert!(description(item, &s, &NONE, "").starts_with("Auto: "));
        // No preset sets it.
        for mode in GpuCullingMode::ALL {
            s.horizon.gpu_culling = mode;
            for preset in Preset::ALL {
                crate::graphics::apply_preset(preset, &mut s);
                assert_eq!(s.horizon.gpu_culling, mode, "{preset:?}");
            }
        }
    }

    #[test]
    fn opaque_ice_toggles_and_round_trips() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::OpaqueIce);
        assert_eq!(item.switch(&s, &NONE), Some(false));
        assert_eq!(item.label(&s, &NONE).plain(), "Opaque Ice: Off");
        item.cycle(&mut s, false, FB, &NONE);
        assert_eq!(item.label(&s, &NONE).plain(), "Opaque Ice: On");
        assert!(
            crate::graphics::Graphics::parse(&s.horizon.serialize()).opaque_ice,
            "the choice is saved"
        );
        item.cycle(&mut s, false, FB, &NONE);
        assert_eq!(item.switch(&s, &NONE), Some(false));
        assert!(!crate::graphics::Graphics::parse(&s.horizon.serialize()).opaque_ice);

        // It sits with the other culling switches on the Performance page.
        let group = rows()
            .into_iter()
            .flat_map(|(page, groups)| groups.into_iter().map(move |(_, b)| (page, b)))
            .find(|(_, b)| b.iter().any(|r| r.item == item))
            .unwrap();
        assert_eq!(group.0, Page::Performance);
        assert!(group.1.iter().any(|r| r.item == item));
    }

    #[test]
    fn leaf_culling_cycles_modes_beside_the_other_culling() {
        let mut s = Settings::default();
        let item = Item::Horizon(H::LeafCulling);
        assert_eq!(item.switch(&s, &NONE), None);
        assert_eq!(item.label(&s, &NONE).plain(), "Leaf Culling: Off");
        for label in ["Hidden Faces", "Solid", "Solid Aggressive", "Hollow", "Off"] {
            item.cycle(&mut s, false, FB, &NONE);
            assert_eq!(
                item.label(&s, &NONE).plain(),
                format!("Leaf Culling: {label}")
            );
            assert_eq!(
                crate::graphics::Graphics::parse(&s.horizon.serialize()).leaf_culling,
                s.horizon.leaf_culling
            );
        }
        item.cycle(&mut s, true, FB, &NONE);
        assert_eq!(s.horizon.leaf_culling, LeafCullingMode::Hollow);

        // The three culling settings share one group of the Performance page.
        let culling = rows()
            .into_iter()
            .flat_map(|(page, groups)| groups.into_iter().map(move |(_, b)| (page, b)))
            .find(|(_, b)| b.iter().any(|r| r.item == item))
            .unwrap();
        assert_eq!(culling.0, Page::Performance);
        let block: Vec<Item> = culling.1.iter().map(|r| r.item).collect();
        for h in [
            H::CaveCulling,
            H::EntityCulling,
            H::ParticleCulling,
            H::LeafCulling,
        ] {
            assert!(block.contains(&Item::Horizon(h)), "{h:?}");
        }
        // The block-entity caches are the group after it, in this order.
        let groups: Vec<(&str, Vec<Item>)> = rows()
            .into_iter()
            .find(|(page, _)| *page == Page::Performance)
            .unwrap()
            .1
            .into_iter()
            .map(|(name, rows)| (name, rows.iter().map(|r| r.item).collect()))
            .collect();
        let at = groups
            .iter()
            .position(|(name, _)| *name == "Culling")
            .unwrap();
        assert_eq!(
            groups[at + 1],
            (
                "Block Entities",
                [
                    H::CacheShulkerBoxes,
                    H::CacheSigns,
                    H::CacheDecoratedPots,
                    H::CacheBells,
                    H::CacheChests,
                    H::CacheBanners,
                    H::CacheCopperGolemStatues,
                ]
                .map(Item::Horizon)
                .to_vec()
            )
        );

        // The description names what the picked mode does.
        for mode in LeafCullingMode::ALL {
            s.horizon.leaf_culling = mode;
            let text = description(item, &s, &NONE, "fallback");
            assert_ne!(text, "fallback", "{mode:?}");
            assert!(text.len() < 80, "{mode:?}: {text}");
        }

        // A click cycles it and leaves the preset, as the other quality settings do.
        let mut ui = ui();
        s.horizon.preset = Preset::Balanced;
        s.horizon.leaf_culling = LeafCullingMode::Off;
        let r = row(&mut ui, &s, item);
        assert_eq!(
            press(&mut ui, &mut s, (r[0] + 4.0, r[1] + 4.0)),
            [CLICK, CHANGED]
        );
        assert_eq!(s.horizon.leaf_culling, LeafCullingMode::Hidden);
        assert_eq!(s.horizon.preset, Preset::Custom);
    }

    #[test]
    fn graphics_step_slider_matches_set_slider_for_all_options() {
        // HDR's sliders step only where the display offers HDR.
        let out = offering(true, None);
        let options = [
            H::ChunkUpdates,
            H::UploadBudget,
            H::Workers,
            H::ResolutionScale,
            H::UpscalingSharpness,
            H::HdrPaperWhite,
            H::HdrPeak,
            H::HdrBlackLevel,
            H::HdrUiBrightness,
        ];
        for h in options {
            let item = Item::Horizon(h);
            let steps = h.slider_steps().expect("steps defined");
            for i in 0..steps {
                let pos = i as f32 / steps as f32;
                let mut s1 = Settings::default();
                let mut s2 = Settings::default();
                s1.horizon.hdr = HdrMode::On;
                s2.horizon.hdr = HdrMode::On;
                h.set_slider(&mut s1, pos);

                item.step_slider(&mut s1, true, 200.0, &out);
                let next_pos = (i + 1) as f32 / steps as f32;
                h.set_slider(&mut s2, next_pos);
                assert_eq!(
                    item.slider(&s1),
                    item.slider(&s2),
                    "{h:?} step forward from {i}"
                );
            }
            for i in 1..=steps {
                let pos = i as f32 / steps as f32;
                let mut s1 = Settings::default();
                let mut s2 = Settings::default();
                s1.horizon.hdr = HdrMode::On;
                s2.horizon.hdr = HdrMode::On;
                h.set_slider(&mut s1, pos);

                item.step_slider(&mut s1, false, 200.0, &out);
                let prev_pos = (i - 1) as f32 / steps as f32;
                h.set_slider(&mut s2, prev_pos);
                assert_eq!(
                    item.slider(&s1),
                    item.slider(&s2),
                    "{h:?} step backward from {i}"
                );
            }
        }
    }
    #[test]
    fn hdr_sliders_step_through_every_value_they_can_be_set_to() {
        let out = offering(true, None);
        let value = |h: H, s: &Settings| match h {
            H::HdrPaperWhite => s.horizon.hdr_paper_white,
            H::HdrPeak => s.horizon.hdr_peak,
            H::HdrBlackLevel => s.horizon.hdr_black_level,
            H::HdrUiBrightness => s.horizon.hdr_ui_brightness,
            _ => unreachable!(),
        };
        let fresh = || {
            let mut s = Settings::default();
            s.horizon.hdr = HdrMode::On;
            s
        };
        for h in [
            H::HdrPaperWhite,
            H::HdrPeak,
            H::HdrBlackLevel,
            H::HdrUiBrightness,
        ] {
            // Everything a drag can land on, each read back where the slider puts it.
            let mut set = std::collections::BTreeSet::new();
            for i in 0..=10_000 {
                let mut s = fresh();
                h.set_slider(&mut s, i as f32 / 10_000.0);
                let v = value(h, &s);
                set.insert(v);
                let mut again = fresh();
                h.set_slider(&mut again, h.slider(&s).unwrap());
                assert_eq!(value(h, &again), v, "{h:?} round trip");
            }
            // Everything a key or the controller reaches, from the bottom to the top.
            let mut s = fresh();
            h.set_slider(&mut s, 0.0);
            let mut stepped = std::collections::BTreeSet::from([value(h, &s)]);
            for _ in 0..h.slider_steps().unwrap() {
                Item::Horizon(h).step_slider(&mut s, true, 200.0, &out);
                stepped.insert(value(h, &s));
            }
            assert_eq!(stepped, set, "{h:?}");
        }
        // Auto, then 10..480 by 10; Auto, then 200..2000 by 50 and 2500..10 000 by 500;
        // 0..1 nit by 0.01 with 0 no lift.
        let mut s = fresh();
        let reach = |h: H, s: &mut Settings| {
            h.set_slider(s, 0.0);
            let mut all = vec![value(h, s)];
            for _ in 0..h.slider_steps().unwrap() {
                Item::Horizon(h).step_slider(s, true, 200.0, &out);
                all.push(value(h, s));
            }
            all
        };
        let paper: Vec<u32> = std::iter::once(0).chain((10..=480).step_by(10)).collect();
        assert_eq!(reach(H::HdrPaperWhite, &mut s), paper);
        let peak: Vec<u32> = std::iter::once(0)
            .chain((200..=2000).step_by(50))
            .chain((2500..=10_000).step_by(500))
            .collect();
        assert_eq!(reach(H::HdrPeak, &mut s), peak);
        assert_eq!(
            reach(H::HdrBlackLevel, &mut s),
            (0..=100).collect::<Vec<u32>>()
        );
    }
    #[test]
    fn black_level_reads_as_its_lift() {
        let mut s = Settings::default();
        let label = |s: &Settings| Item::Horizon(H::HdrBlackLevel).label(s, &NONE).plain();
        assert_eq!(label(&s), "Black Level: 0.00");
        s.horizon.hdr_black_level = 5;
        assert_eq!(label(&s), "Black Level: 0.05");
    }
    /// Horizon's graphics captions are English sentences that the language table keys by
    /// themselves, so a caption, description or value that is not in `en_us.json` shows in
    /// English in every language. Every text the screen shows, with each setting cycled
    /// through its values (#1927).
    #[test]
    fn every_graphics_text_is_in_the_language_table() {
        let (_, missed) = lang::scoped(lang::Language::load(&[], "de_de"), || {
            for hdr in HdrMode::ALL {
                let mut s = Settings::default();
                s.horizon.hdr = hdr;
                s.horizon.taa = true;
                for _ in 0..8 {
                    for (page, groups) in rows() {
                        lang::get(page.name());
                        lang::get(page.about());
                        for (group, rows) in groups {
                            lang::get(group);
                            for row in rows {
                                // Vanilla's captions are vanilla's keys, not loaded here.
                                if let Item::Horizon(_) = row.item {
                                    row.item.label(&s, &NONE);
                                }
                                let text = description(row.item, &s, &NONE, row.text);
                                lang::get(text);
                                lang::get(row.cost.label());
                                if let Some(why) = row.item.blocked(&s, &NONE) {
                                    lang::get(why);
                                }
                                if let Item::Horizon(h) = row.item {
                                    h.cycle(&mut s, false, &NONE);
                                }
                            }
                        }
                    }
                }
            }
            for text in [
                "Search options",
                "Results for \"%s\"",
                "No settings match.",
                "Undo",
                "Done",
            ] {
                lang::get(text);
            }
            // Every reason GPU Culling's switch can give, whatever this device offers.
            use crate::graphics::GpuCullingOffer as O;
            for offer in [
                O::Unsupported,
                O::Software,
                O::OutOfMemory,
                O::Dx12,
                O::DeviceLost,
            ] {
                lang::get(offer.reason().unwrap());
            }
        });
        // `labelled` also looks up values that are already words in the language ("An") or
        // numbers with a unit ("12 chunks", "Auto (8)"); those are not keys, and "FSR 3.1" is
        // a name.
        let missed: Vec<_> = missed
            .into_iter()
            .filter(|m| !m.starts_with(|c: char| c.is_ascii_digit()) && !m.starts_with("Auto ("))
            .filter(|m| !["An", "Aus", "FSR 3.1", "key.keyboard.unknown"].contains(&m.as_str()))
            .collect();
        assert_eq!(missed, Vec::<String>::new());
    }
    /// Horizon's captions join label and value with the language's `options.generic_value` and
    /// `options.percent_value`, as vanilla's beside them on the same page do (#6072). The
    /// templates are 26.3's.
    #[test]
    fn horizon_captions_use_the_languages_separator() {
        struct Vanilla;
        impl lang::ResourceSource for Vanilla {
            fn read(&self, path: &str) -> Option<Vec<u8>> {
                let (generic, percent) = match path {
                    "assets/minecraft/lang/en_us.json" => ("%s: %s", "%s: %s%%"),
                    "assets/minecraft/lang/fr_fr.json" => ("%s : %s", "%s : %s %%"),
                    "assets/minecraft/lang/zh_cn.json" => ("%s：%s", "%s：%s%%"),
                    "assets/minecraft/lang/en_ud.json" => ("%2$s :%1$s", "%s: %s%%"),
                    _ => return None,
                };
                serde_json::to_vec(&serde_json::json!({
                    "options.generic_value": generic,
                    "options.percent_value": percent,
                }))
                .ok()
            }
        }
        let mut s = Settings::default();
        s.horizon.leaf_culling = LeafCullingMode::Hollow;
        s.horizon.resolution_scale = 75;
        for (code, leaf, scale) in [
            ("en_us", "Leaf Culling: Hollow", "Resolution Scale: 75%"),
            (
                "fr_fr",
                "Occlusion feuillage : Creux",
                "Échelle de résolution : 75 %",
            ),
            ("zh_cn", "树叶剔除：镂空", "渲染比例：75%"),
            // Value first.
            ("en_ud", "Hollow :Leaf Culling", "Resolution Scale: 75%"),
        ] {
            let (shown, _) = lang::scoped(lang::Language::load(&[&Vanilla], code), || {
                (
                    Item::Horizon(H::LeafCulling).label(&s, &NONE).plain(),
                    Item::Horizon(H::ResolutionScale).label(&s, &NONE).plain(),
                )
            });
            assert_eq!(shown, (leaf.into(), scale.into()), "{code}");
        }
    }
    /// Leaf Culling, Frame Generation and Shadow Cache came with English captions only (#1927).
    #[test]
    fn newer_graphics_settings_are_translated() {
        let mut s = Settings::default();
        s.horizon.leaf_culling = LeafCullingMode::Hollow;
        s.horizon.entity_culling = EntityCullingMode::Asynchronous;
        s.horizon.particle_culling = ParticleCullingMode::Asynchronous;
        for (code, leaf, entity, leaf_text, particle, particle_text) in [
            (
                "de_de",
                "Laub-Culling: Hohl",
                "Objekt-Culling: Asynchron",
                "Lässt Flächen zwischen Blättern weg: schneller, aber Lücken sind durchsichtig.",
                "Partikel-Culling: Asynchron",
                "Überspringt Partikel außerhalb des Bildes, unterhalb eines Pixels oder hinter Terrain und entlastet den Render-Thread.",
            ),
            (
                "pl_pl",
                "Pomijanie liści: Puste",
                "Pomijanie bytów: Asynchroniczne",
                "Pomija ściany między liśćmi: szybciej, ale luki są przezroczyste.",
                "Pomijanie cząsteczek: Asynchroniczne",
                "Pomija cząsteczki poza ekranem, mniejsze niż piksel i zasłonięte przez teren, odciążając wątek renderowania.",
            ),
            (
                "uk_ua",
                "Відсікання листя: Порожнє",
                "Відсік. сутностей: Асинхронно",
                "Прибирає грані між листям: швидше, але прогалини прозорі.",
                "Відсікання частинок: Асинхронно",
                "Пропускає частинки поза екраном, менші за піксель і закриті місцевістю, зменшуючи роботу потоку рендерингу.",
            ),
            (
                "zh_cn",
                "树叶剔除: 镂空",
                "实体剔除: 异步",
                "省略树叶之间的面：更快，但缝隙会透视。",
                "粒子剔除: 异步",
                "跳过屏幕外、小于一个像素和被地形遮挡的粒子，减少渲染线程的工作量。",
            ),
            (
                "fr_fr",
                "Occlusion feuillage: Creux",
                "Occlusion des entités: Asynchrone",
                "Supprime les faces entre feuilles : plus rapide, mais on voit à travers les trous.",
                "Occlusion des particules: Asynchrone",
                "Ignore les particules hors écran, trop petites ou masquées par le terrain avec moins de travail sur le thread de rendu.",
            ),
        ] {
            let (shown, _) = lang::scoped(lang::Language::load(&[], code), || {
                let item = |h| Item::Horizon(h);
                (
                    item(H::LeafCulling).label(&s, &NONE).plain(),
                    item(H::EntityCulling).label(&s, &NONE).plain(),
                    lang::get(description(item(H::LeafCulling), &s, &NONE, "")),
                    item(H::ParticleCulling).label(&s, &NONE).plain(),
                    lang::get(description(item(H::ParticleCulling), &s, &NONE, "")),
                )
            });
            assert_eq!(
                shown,
                (
                    leaf.into(),
                    entity.into(),
                    leaf_text.into(),
                    particle.into(),
                    particle_text.into(),
                ),
                "{code}"
            );
        }
    }
}

#[cfg(test)]
#[path = "graphics_ui_hdr_tests.rs"]
mod hdr_tests;
