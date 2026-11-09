//! The post-processing programs of one dimension, ready for the renderer: `prepare`, `deferred`,
//! `composite` and `final` with their numbered variants, in the order they run (spec 2.1), each
//! preprocessed and translated, with the buffers it writes and the flips that follow it, and the
//! buffer settings their files carry (6.1).
//!
//! Decisions (impl-plan.md):
//! - OQ-12: a dimension's folder (`world0`, `world-1`, `world1`) replaces the base folder for that
//!   dimension when it exists; every stage file is read from it.
//! - OQ-29: a program with a geometry stage is reported and treated as missing.
//! - A program that fails to translate is reported and treated as missing (2.5).
//! - Terrain draws with the first program of its fallback chain that loads (2.3).

use std::collections::BTreeSet;

use crate::config::{self, Config, ReadConfig};
use crate::custom::Custom;
use crate::{
    ComputeProgram, eval_condition, preprocess, translate_composite, translate_compute,
    translate_gbuffers, translate_shadow,
};
use crate::{GbuffersInputs, Macros, Pack, Preprocessed, Program, Properties, Target};

/// Which group of 2.1 a program belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Compute initialization at load and after target resize.
    Setup,
    /// Composite-style work before shadow rendering.
    Begin,
    Prepare,
    Deferred,
    Composite,
    Final,
    /// A gbuffers program: the game draws its geometry with it, before deferred.
    Gbuffers,
    /// A gbuffers program drawing translucent geometry, after deferred (2.1).
    Translucent,
    /// A shadow program: the game's geometry from the shadow light, into the shadow buffers.
    Shadow,
    /// `shadowcomp`..`shadowcomp99`: composite-style programs over the shadow buffers.
    ShadowComp,
}

impl Group {
    /// Whether the group writes the shadow buffers, where buffer n is shadowcolor n.
    pub fn shadow(self) -> bool {
        matches!(self, Group::Shadow | Group::ShadowComp)
    }
}

/// The samplers whose use turns the shadow pass on (2.5, 6.3).
const SHADOW_SAMPLERS: [&str; 9] = [
    "shadow",
    "watershadow",
    "shadowtex0",
    "shadowtex1",
    "shadowtex0HW",
    "shadowtex1HW",
    "shadowcolor",
    "shadowcolor0",
    "shadowcolor1",
];

/// A blend factor of `blend.<program>` (3.2), as GL names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Factor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturate,
}

impl Factor {
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "ZERO" => Self::Zero,
            "ONE" => Self::One,
            "SRC_COLOR" => Self::SrcColor,
            "ONE_MINUS_SRC_COLOR" => Self::OneMinusSrcColor,
            "DST_COLOR" => Self::DstColor,
            "ONE_MINUS_DST_COLOR" => Self::OneMinusDstColor,
            "SRC_ALPHA" => Self::SrcAlpha,
            "ONE_MINUS_SRC_ALPHA" => Self::OneMinusSrcAlpha,
            "DST_ALPHA" => Self::DstAlpha,
            "ONE_MINUS_DST_ALPHA" => Self::OneMinusDstAlpha,
            "SRC_ALPHA_SATURATE" => Self::SrcAlphaSaturate,
            _ => return None,
        })
    }
}

/// Source and destination factors for colour, then for alpha.
pub type Blend = [Factor; 4];

/// The default blend of translucent geometry (3.2).
pub const TRANSLUCENT_BLEND: Blend = [
    Factor::SrcAlpha,
    Factor::OneMinusSrcAlpha,
    Factor::One,
    Factor::OneMinusSrcAlpha,
];

/// `gbuffers_water` and its fallbacks (2.3).
const WATER: [&str; 5] = [
    "gbuffers_water",
    "gbuffers_terrain",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_terrain` and its fallbacks (2.3).
const TERRAIN: [&str; 4] = [
    "gbuffers_terrain",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_terrain_solid` and its fallbacks (ShaderDoc's "Iris Additional Programs", as
/// #9137 quotes it): solid terrain, the quads over opaque texels.
const TERRAIN_SOLID: [&str; 5] = [
    "gbuffers_terrain_solid",
    "gbuffers_terrain",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_terrain_cutout` and its fallbacks (the same): cutout terrain.
const TERRAIN_CUTOUT: [&str; 5] = [
    "gbuffers_terrain_cutout",
    "gbuffers_terrain",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_line` and its fallback, as in Iris: lines such as the block outline.
const LINES: [&str; 2] = ["gbuffers_line", "gbuffers_basic"];

/// `gbuffers_skybasic` and its fallback (2.3).
const SKY: [&str; 2] = ["gbuffers_skybasic", "gbuffers_basic"];

/// `gbuffers_skytextured` and its fallbacks (2.3).
const SKY_TEXTURED: [&str; 3] = [
    "gbuffers_skytextured",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_entities` and its fallbacks (2.3).
const ENTITIES: [&str; 4] = [
    "gbuffers_entities",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_block` and its fallbacks (2.3).
const BLOCK: [&str; 5] = [
    "gbuffers_block",
    "gbuffers_terrain",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_clouds` and its fallbacks, as in Iris (its ShaderDoc's gbuffers table): without
/// any of them the game draws its clouds.
const CLOUDS: [&str; 3] = ["gbuffers_clouds", "gbuffers_textured", "gbuffers_basic"];

/// `gbuffers_hand` and its fallbacks (2.3).
const HAND: [&str; 4] = [
    "gbuffers_hand",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_hand_water` and its fallbacks (2.3): translucent held items, after deferred.
const HAND_WATER: [&str; 5] = [
    "gbuffers_hand_water",
    "gbuffers_hand",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_weather` and its fallbacks (2.3).
const WEATHER: [&str; 4] = [
    "gbuffers_weather",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_entities_translucent` and its fallbacks (2.3).
const ENTITIES_TRANSLUCENT: [&str; 5] = [
    "gbuffers_entities_translucent",
    "gbuffers_entities",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// `gbuffers_beaconbeam` and its fallbacks (the shaders.properties reference): beacon and End
/// gateway beams.
const BEACON_BEAM: [&str; 3] = ["gbuffers_beaconbeam", "gbuffers_textured", "gbuffers_basic"];

/// `gbuffers_particles` and its fallbacks (2.3).
const PARTICLES: [&str; 4] = [
    "gbuffers_particles",
    "gbuffers_textured_lit",
    "gbuffers_textured",
    "gbuffers_basic",
];

/// The sun and moon's default blend: added to the sky, and their own alpha replacing the
/// buffer's, as vanilla draws them (`RenderPipelines.CELESTIAL`, `BlendFunction.OVERLAY`). Iris
/// 1.11.7 left the sun's alpha (2.0) in an RGBA32F buffer the sky had written 1.0 to, with a
/// probe pack of ours (#10490).
pub const CELESTIAL_BLEND: Blend = [Factor::SrcAlpha, Factor::One, Factor::One, Factor::Zero];

const GROUPS: [(Group, &str); 6] = [
    (Group::Setup, "setup"),
    (Group::Begin, "begin"),
    (Group::Prepare, "prepare"),
    (Group::Deferred, "deferred"),
    (Group::Composite, "composite"),
    (Group::Final, "final"),
];

/// The stars' blend: added to the sky, their own alpha replacing the buffer's, as vanilla draws
/// them (`RenderPipelines.STARS`, `BlendFunction.OVERLAY`). Iris 1.11.7 drew them so with a probe
/// pack of ours (#10490): colour 0.25 × 0.5 over the disc's 1.0 read 1.125, alpha 0.5.
pub const STARS_BLEND: Blend = [Factor::SrcAlpha, Factor::One, Factor::One, Factor::Zero];

#[derive(Clone)]
pub struct PostProgram {
    /// The program's name (`composite3`).
    pub name: String,
    pub group: Group,
    /// The vertex and fragment stages; `None` for a program of compute stages only.
    pub program: Option<Program>,
    /// colortex buffer of each fragment output location (location n writes `buffers[n]`).
    pub buffers: Vec<u32>,
    /// `flip.<name>.<buffer>`: buffers forced to flip (true) or kept (false) after this program.
    pub flips: Vec<(u32, bool)>,
    /// How the program's output blends into its buffers; `None` writes them.
    pub blend: Option<Blend>,
    /// `gbuffers_skybasic` without a `blend.*` of its own: each part of the sky blends as
    /// vanilla draws it, the disc and the void without blending (`RenderPipelines.SKY`), the stars
    /// with [`STARS_BLEND`] and the sunset fan with `blend`.
    pub vanilla_sky_blends: bool,
    /// Per-buffer blend overrides, keyed by colortex index (not output location).
    pub buffer_blends: Vec<(u32, Option<Blend>)>,
    /// Bit n: `colortex<n>MipmapEnabled` in its fragment stage, so buffer n's mip chain is
    /// regenerated right before it (6.1.7; not for gbuffers programs).
    pub mipmaps: u16,
    /// Compute programs that run before this one, in order: `<name>.csh`, then `_a` to `_z`.
    pub compute: Vec<(String, ComputeProgram)>,
    /// A geometry stage folded into the vertex stage (`translate_geometry`): drawn non-indexed,
    /// six vertices per quad, with this many instances. 0 without one.
    pub geometry_triangles: u32,
    /// `scale.<name>`: (scale, offset x, offset y), the part of its buffers a program draws, as
    /// fractions of their size from GL's bottom-left corner; [1, 0, 0] (all of them) without.
    pub scale: [f32; 3],
}

/// `size.buffer.<buffer>`: a size of its own, relative to the render size (floats) or in
/// pixels (integers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BufferSize {
    Relative([f32; 2]),
    Absolute([u32; 2]),
}

impl BufferSize {
    fn parse(value: &str) -> Option<Self> {
        let words: Vec<&str> = value.split_whitespace().collect();
        let [w, h] = words[..] else {
            return None;
        };
        if w.contains('.') || h.contains('.') {
            let (w, h): (f32, f32) = (w.parse().ok()?, h.parse().ok()?);
            let fine = |f: f32| f.is_finite() && f > 0.0;
            (fine(w) && fine(h)).then_some(Self::Relative([w, h]))
        } else {
            Some(Self::Absolute([w.parse().ok()?, h.parse().ok()?]))
        }
    }

    /// The size for a render size of `render`, at least one pixel and at most
    /// [`BufferSize::MAX`].
    pub fn resolve(self, render: [u32; 2]) -> [u32; 2] {
        let size = match self {
            Self::Relative(s) => [0, 1].map(|k| (render[k] as f32 * s[k]) as u32),
            Self::Absolute(s) => s,
        };
        size.map(|n| n.clamp(1, Self::MAX))
    }

    /// The largest side a buffer may have, the size every device takes for a 2D texture.
    pub const MAX: u32 = 8192;
}

/// `bufferObject.<n>`: an SSBO's size in bytes, or bytes per pixel of the render size scaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BufferObject {
    Bytes(u64),
    PerPixel { bytes: u64, scale: [f32; 2] },
}

impl BufferObject {
    fn parse(value: &str) -> Option<Self> {
        let words: Vec<&str> = value.split_whitespace().collect();
        let bytes = words.first()?.parse().ok()?;
        match words[..] {
            [_] | [_, "false", ..] => Some(Self::Bytes(bytes)),
            [_, "true", x, y] => Some(Self::PerPixel {
                bytes,
                scale: [x.parse().ok()?, y.parse().ok()?],
            }),
            _ => None,
        }
    }

    /// The size in bytes for a render size of `render`, at least 16.
    pub fn size(self, render: [u32; 2]) -> u64 {
        match self {
            Self::Bytes(b) => b,
            Self::PerPixel { bytes, scale } => {
                let [w, h] = [0, 1].map(|k| (render[k] as f32 * scale[k]) as u64);
                bytes.saturating_mul(w).saturating_mul(h)
            }
        }
        .max(16)
    }
}

/// `image.<name>`: an image of the pack's, read and written as `name`, sampled as `sampler`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomImage {
    pub name: String,
    pub sampler: Option<String>,
    pub format: crate::Format,
    /// The internal format's GLSL name (`r32i`), for declarations without a format.
    pub qualifier: String,
    /// Relative to the render size (width and height), or in pixels.
    pub size: ImageSize,
    /// Cleared to zero before each frame.
    pub clear: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageSize {
    Relative([f32; 2]),
    Absolute([u32; 2]),
    /// Three sizes: a 3D image, whatever its depth.
    Volume([u32; 3]),
    /// One size in pixels: a 1D image (a lookup table by index).
    Line(u32),
}

impl CustomImage {
    fn parse(name: &str, value: &str) -> Option<Self> {
        let w: Vec<&str> = value.split_whitespace().collect();
        let [sampler, _format, internal, _ty, clear, relative, rest @ ..] = &w[..] else {
            return None;
        };
        let size = match (*relative, rest) {
            ("true", [x, y, ..]) => ImageSize::Relative([x.parse().ok()?, y.parse().ok()?]),
            ("false", [x]) => ImageSize::Line(x.parse().ok()?),
            ("false", [x, y]) => ImageSize::Absolute([x.parse().ok()?, y.parse().ok()?]),
            ("false", [x, y, z]) => {
                ImageSize::Volume([x.parse().ok()?, y.parse().ok()?, z.parse().ok()?])
            }
            _ => return None,
        };
        Some(Self {
            name: name.to_owned(),
            sampler: (*sampler != "none").then(|| sampler.to_string()),
            format: crate::Format::parse(internal)?,
            qualifier: internal.to_ascii_lowercase(),
            size,
            clear: *clear == "true",
        })
    }

    /// Width, height and depth for a render size of `render`.
    pub fn extent(&self, render: [u32; 2]) -> [u32; 3] {
        match self.size {
            ImageSize::Relative(s) => {
                let [w, h] = [0, 1].map(|k| ((render[k] as f32 * s[k]) as u32).max(1));
                [w, h, 1]
            }
            ImageSize::Absolute([w, h]) => [w.max(1), h.max(1), 1],
            ImageSize::Volume(s) => s.map(|n| n.max(1)),
            ImageSize::Line(w) => [w.max(1), 1, 1],
        }
    }

    /// A 1D image (`image1D`), not a 2D one.
    pub fn is_1d(&self) -> bool {
        matches!(self.size, ImageSize::Line(_))
    }

    /// A 3D image (`image3D`), not a 2D one.
    pub fn is_3d(&self) -> bool {
        matches!(self.size, ImageSize::Volume(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShadowCulling {
    On,
    Off,
    Reversed,
}

#[derive(Clone)]
pub struct Post {
    /// The folder the programs came from: `""` for `shaders` itself, else `world0`...
    pub folder: String,
    pub programs: Vec<PostProgram>,
    /// The flips of the virtual `deferred_pre` and `composite_pre` (6.1.6).
    pub deferred_pre: Vec<(u32, bool)>,
    pub composite_pre: Vec<(u32, bool)>,
    /// What draws solid and cutout terrain, when the game asked for it and one loads; when
    /// terrain draws in two parts, solid terrain only.
    pub terrain: Option<PostProgram>,
    /// What draws cutout terrain when the game draws terrain in two parts
    /// (`GameInputs::terrain_parts`); `terrain` then draws the solid part. A pack with
    /// `gbuffers_terrain_solid` or `gbuffers_terrain_cutout` and no program for one part has
    /// only the other.
    pub terrain_cutout: Option<PostProgram>,
    /// What draws translucent terrain (water, stained glass) after deferred.
    pub water: Option<PostProgram>,
    /// The pack has no program for solid and cutout terrain (none of `gbuffers_terrain`'s
    /// chain, nor `_solid` or `_cutout`), or for translucent terrain (none of
    /// `gbuffers_water`'s): nothing draws that terrain, into no buffer nor depth. Iris 1.11.7
    /// drew no terrain with our probe packs having no gbuffers programs, at the root or in a
    /// dimension's folder (#10455). A program that is there but doesn't load still leaves its
    /// terrain to the game.
    pub terrain_undrawn: bool,
    pub water_undrawn: bool,
    /// What draws the sky (disc, sunset, stars, void) before terrain.
    pub sky: Option<PostProgram>,
    /// What draws the sun and moon, between the sunset and the stars.
    pub sky_textured: Option<PostProgram>,
    /// What draws entities, after terrain.
    pub entities: Option<PostProgram>,
    /// What draws block entities, after terrain.
    pub block: Option<PostProgram>,
    /// An authored translucent block variant; absent or failed keeps the ordinary block path.
    pub block_translucent: Option<PostProgram>,
    /// What draws translucent entities (name tags, shadows), after the others.
    pub entities_translucent: Option<PostProgram>,
    /// What draws particles.
    pub particles: Option<PostProgram>,
    /// The translucent particle variant for explicit mixed ordering, when it loads.
    pub particles_translucent: Option<PostProgram>,
    /// What draws beacon and End gateway beams (after deferred, blended).
    pub beacon_beam: Option<PostProgram>,
    pub damaged_block: Option<PostProgram>,
    /// What draws the game's clouds.
    pub clouds: Option<PostProgram>,
    /// What draws lines (the block outline), with the pack's own transform and look.
    pub lines: Option<PostProgram>,
    /// What draws rain and snow, after deferred. They never write depth, whatever `rain.depth`
    /// says: Iris 1.11.7 left none of either in `depthtex0` (probe packs of ours, #10492).
    pub weather: Option<PostProgram>,
    /// What draws the first-person hand and held items.
    pub hand: Option<PostProgram>,
    /// What draws their translucent parts (stained glass, ice...), after deferred and blended.
    pub hand_water: Option<PostProgram>,
    /// `clouds=off`: no clouds at all, neither the game's nor the pack's program for them.
    pub clouds_off: bool,
    /// Some program reads a shadow buffer, or `shadow.enabled=true`: the shadow pass runs (2.5).
    pub shadow_pass: bool,
    /// When it runs: what draws solid and cutout terrain from the light (`shadow`), and
    /// translucent terrain (`shadow_water` or `shadow`), into the shadow buffers (2.4).
    pub shadow: Option<PostProgram>,
    pub shadow_water: Option<PostProgram>,
    /// What draws entities (`shadow_entities` or `shadow`) and block entities (`shadow_block` or
    /// `shadow`) from the light, unless `shadowEntities`/`shadowBlockEntities` is false.
    pub shadow_entities: Option<PostProgram>,
    pub shadow_block: Option<PostProgram>,
    /// `shadowEntities=false` with `shadowPlayer=true`: of the entities, only the player casts.
    pub shadow_only_player: bool,
    /// `shadowBlockEntities=false` with `shadowLightBlockEntities=true`: only block entities
    /// whose block emits light cast.
    pub shadow_only_lit_blocks: bool,
    /// `shadowcomp`..`shadowcomp99`, after the shadow pass (2.1).
    pub shadow_comp: Vec<PostProgram>,
    /// Translucent entities draw after deferred (`separateEntityDraws`, 3.2).
    pub translucent_late: bool,
    /// Particles draw after deferred (`particles.ordering`, `particles.before.deferred`, 3.2).
    pub particles_late: bool,
    /// The custom uniforms of `shaders.properties` (3.4).
    pub custom: Custom,
    /// `texture.*`: the textures the pack binds to sampler names.
    pub textures: Vec<crate::CustomTexture>,
    /// `size.buffer.*`, by buffer.
    pub sizes: [Option<BufferSize>; crate::BUFFERS],
    /// The shadowcolor buffers the shadow programs have: 2, or all 8 when the pack names the
    /// `HIGHER_SHADOWCOLOR` feature (`iris.features.required` or `.optional`).
    pub shadow_colors: usize,
    /// `bufferObject.*`, by binding.
    pub buffer_objects: std::collections::BTreeMap<u32, BufferObject>,
    /// `image.*`.
    pub images: Vec<CustomImage>,
    /// `shadow.culling`: `false` draws everything within the shadow distance, every face, not
    /// only what casts a shadow into view; `reversed` within `voxelDistance` of the player.
    pub shadow_culling: ShadowCulling,
    pub config: Config,
    /// Programs skipped and why, as `program: message` or `file:line: message`.
    pub notes: Vec<String>,
}

impl Post {
    /// Every program that loaded: the passes', what draws the game's geometry, the shadow
    /// pass's and its composites.
    fn every_program(&self) -> impl Iterator<Item = &PostProgram> {
        let draws = [
            &self.terrain,
            &self.terrain_cutout,
            &self.water,
            &self.sky,
            &self.sky_textured,
            &self.entities,
            &self.block,
            &self.block_translucent,
            &self.entities_translucent,
            &self.particles,
            &self.particles_translucent,
            &self.damaged_block,
            &self.clouds,
            &self.weather,
            &self.hand,
            &self.shadow,
            &self.shadow_water,
            &self.shadow_entities,
            &self.shadow_block,
        ];
        self.programs
            .iter()
            .chain(draws.into_iter().flatten())
            .chain(&self.shadow_comp)
    }

    /// The fewest bytes each storage buffer (`bufferObject.<n>`, set 4) needs for every stage
    /// that binds it: its block's size as the stage lays it out (std430). OpenGL binds a smaller buffer and leaves what lies
    /// past its end undefined; wgpu refuses to draw or dispatch with one.
    pub fn buffer_minimums(&self) -> std::collections::BTreeMap<u32, u64> {
        let mut sizes = std::collections::BTreeMap::new();
        for p in self.every_program() {
            let stages = p.program.iter().flat_map(|s| [&s.vertex, &s.fragment]);
            for module in stages.chain(p.compute.iter().map(|(_, c)| &c.module)) {
                for (_, var) in module.global_variables.iter() {
                    if let (naga::AddressSpace::Storage { .. }, Some(b)) = (var.space, &var.binding)
                        && b.group == 4
                    {
                        let size = u64::from(module.types[var.ty].inner.size(module.to_ctx()));
                        let least = sizes.entry(b.binding).or_insert(0);
                        *least = size.max(*least);
                    }
                }
            }
        }
        sizes
    }
}

/// The folder for one of Minecraft's legacy dimension numbers. Named mappings use the
/// corresponding Minecraft identifier; callers with an identifier should use
/// [`dimension_folder_named`] instead.
pub fn dimension_folder(pack: &Pack, id: i32) -> String {
    let name = match id {
        -1 => "minecraft:the_nether",
        1 => "minecraft:the_end",
        0 => "minecraft:overworld",
        _ => "",
    };
    dimension_folder_named(pack, name, id)
}

/// Selects a dimension's program folder. `dimension.properties` mappings take precedence
/// over legacy folders: explicit identifiers beat `*`, and an unmapped identifier uses the
/// base folder. Without that file, preserve the `world<legacy_id>` selection.
pub fn dimension_folder_named(pack: &Pack, id: &str, legacy_id: i32) -> String {
    if let Some(text) = pack.read_text("dimension.properties") {
        let mappings = Properties::parse(&text, &Macros::default());
        let mut fallback = "";
        for entry in &mappings.entries {
            let Some(folder) = entry.key.strip_prefix("dimension.") else {
                continue;
            };
            // A folder must stay inside shaders/, including on folder-backed packs.
            let Some(path) = crate::pack::normalize(folder).filter(|p| !p.is_empty()) else {
                continue;
            };
            for name in entry.value.split_whitespace() {
                if name == "*" {
                    fallback = folder;
                } else if name == id
                    || (!name.contains(':') && id.strip_prefix("minecraft:") == Some(name))
                {
                    return path;
                }
            }
        }
        return crate::pack::normalize(fallback).unwrap_or_default();
    }
    let folder = format!("world{legacy_id}");
    if !pack.list(&folder).is_empty() {
        folder
    } else {
        String::new()
    }
}

/// The game's geometry a pack may draw with its gbuffers programs: each kind's vertex inputs,
/// when the game can draw it with a pack.
#[derive(Clone, Debug, Default)]
pub struct GameInputs {
    pub terrain: Option<GbuffersInputs>,
    pub sky: Option<GbuffersInputs>,
    pub celestial: Option<GbuffersInputs>,
    /// Entities and block entities (one vertex format).
    pub entities: Option<GbuffersInputs>,
    /// The clouds' mesh.
    pub clouds: Option<GbuffersInputs>,
    /// Rain and snow.
    pub weather: Option<GbuffersInputs>,
    /// Lines: the block outline.
    pub lines: Option<GbuffersInputs>,
    /// The first-person hand and held items.
    pub hand: Option<GbuffersInputs>,
    /// `terrain`, drawing only its solid quads, then only its cutout ones: given, every pack
    /// draws terrain in these two parts.
    pub terrain_parts: Option<[GbuffersInputs; 2]>,
    /// Terrain read vertex by vertex, for a shadow program with a geometry stage.
    pub terrain_geometry: Option<crate::GeometryInputs>,
    /// Entities and block entities read vertex by vertex, for the same.
    pub entity_geometry: Option<crate::GeometryInputs>,
}

/// Loads the post-processing programs of `folder` and, for the geometry `inputs` describes, the
/// gbuffers programs it draws with.
pub fn load_post(
    pack: &Pack,
    folder: &str,
    properties: &Properties,
    macros: &Macros,
    inputs: &GameInputs,
) -> Post {
    let mut post = Post {
        folder: folder.to_owned(),
        programs: Vec::new(),
        deferred_pre: flips(properties, "deferred_pre"),
        composite_pre: flips(properties, "composite_pre"),
        terrain: None,
        terrain_cutout: None,
        water: None,
        terrain_undrawn: false,
        water_undrawn: false,
        sky: None,
        sky_textured: None,
        entities: None,
        block: None,
        block_translucent: None,
        entities_translucent: None,
        particles: None,
        particles_translucent: None,
        beacon_beam: None,
        damaged_block: None,
        clouds: None,
        lines: None,
        weather: None,
        hand: None,
        hand_water: None,
        clouds_off: false,
        shadow_pass: false,
        shadow: None,
        shadow_water: None,
        shadow_entities: None,
        shadow_block: None,
        shadow_only_player: false,
        shadow_only_lit_blocks: false,
        shadow_comp: Vec::new(),
        translucent_late: false,
        particles_late: false,
        custom: Custom::parse(properties),
        textures: Vec::new(),
        sizes: [None; crate::BUFFERS],
        shadow_colors: if higher_shadowcolor(properties) {
            crate::SHADOW_COLORS
        } else {
            2
        },
        buffer_objects: Default::default(),
        images: Vec::new(),
        shadow_culling: match properties.get("shadow.culling").map(str::trim) {
            Some("false") => ShadowCulling::Off,
            Some("reversed") => ShadowCulling::Reversed,
            _ => ShadowCulling::On,
        },
        config: Config::default(),
        notes: Vec::new(),
    };
    let options = crate::with_options(pack, macros);
    for e in &properties.entries {
        if let Some(n) = e.key.strip_prefix("bufferObject.") {
            match (n.parse(), BufferObject::parse(&e.value)) {
                (Ok(n), Some(b)) => _ = post.buffer_objects.insert(n, b),
                _ => post.notes.push(format!("`{}`: not a buffer object", e.key)),
            }
            continue;
        }
        if let Some(name) = e.key.strip_prefix("image.") {
            match CustomImage::parse(name, &options.expand(&e.value)) {
                Some(i) => post.images.push(i),
                None => post.notes.push(format!("`{}`: not an image", e.key)),
            }
            continue;
        }
        let Some(name) = e.key.strip_prefix("size.buffer.") else {
            continue;
        };
        match (crate::buffer_index(name), BufferSize::parse(&e.value)) {
            (Some(b), Some(size)) => post.sizes[b] = Some(size),
            _ => post
                .notes
                .push(format!("`{}`: not a buffer and a size", e.key)),
        }
    }
    let (textures, notes) = crate::load_textures(pack, properties);
    post.textures = textures;
    post.notes.extend(notes);
    let mut loader = Loader {
        pack,
        folder,
        properties,
        macros,
        switches: crate::switches_on(pack),
        options,
        geometry: None,
        post: &mut post,
    };
    for (group, base) in GROUPS {
        let names: Vec<String> = if group == Group::Final {
            vec![base.to_owned()]
        } else {
            std::iter::once(base.to_owned())
                .chain((1..=99).map(|n| format!("{base}{n}")))
                .collect()
        };
        let target = match group {
            Group::Final => Target::Window,
            _ => Target::Buffers,
        };
        for name in names {
            if let Some(mut p) = loader.load(name, group, |v, f| translate_composite(v, f, target))
            {
                // `blend.<program>` blends every buffer the program writes. Iris 1.11.7, probed:
                // it applies to begin, prepare, deferred and composite programs, and
                // `blend.<program>.<buffer>` doesn't apply to deferred and composite ones; the
                // other cases are assumed to match.
                p.blend = blend(loader.properties, &p.name).flatten();
                p.buffer_blends.clear();
                loader.post.programs.push(p);
            }
        }
    }
    let split_terrain = ["gbuffers_terrain_solid", "gbuffers_terrain_cutout"]
        .iter()
        .any(|n| loader.has(n));
    // Whether the pack has a program for each part, loaded or not.
    let [has_solid, has_cutout] =
        [&TERRAIN_SOLID, &TERRAIN_CUTOUT].map(|chain| chain.iter().any(|n| loader.has(n)));
    let mut gbuffers = |names: &[&str], group, inputs: &GbuffersInputs| {
        names.iter().find_map(|name| {
            // `alphaTest.<program>` replaces the geometry's default test (OQ-23).
            let key = properties.get(&format!("alphaTest.{name}"));
            let test = key.and_then(|v| crate::AlphaTest::parse(v.trim()));
            let mut inputs = inputs.clone();
            if let Some(test) = test {
                inputs.alpha_test = test;
            }
            loader.load(name.to_string(), group, |v, f| {
                translate_gbuffers(v, f, Target::Buffers, &inputs)
            })
        })
    };
    // The sky's parts blend as vanilla draws them (the sunset fan translucent, the stars
    // [`STARS_BLEND`], the disc and the void not at all), unless `blend.*` says otherwise. Iris 1.11.7 left exactly what
    // `gbuffers_skybasic` wrote on the sky disc, alpha 0.25 included, with a probe pack of ours
    // (#10490).
    let sky = inputs
        .sky
        .as_ref()
        .and_then(|i| gbuffers(&SKY, Group::Gbuffers, i));
    let sky = sky.map(|mut p| {
        let own = blend(properties, &p.name);
        p.vanilla_sky_blends = own.is_none();
        p.blend = own.unwrap_or(Some(TRANSLUCENT_BLEND));
        p
    });
    let celestial = inputs.celestial.as_ref();
    let sky_textured = celestial.and_then(|i| gbuffers(&SKY_TEXTURED, Group::Gbuffers, i));
    let sky_textured = sky_textured.map(|mut p| {
        p.blend = blend(properties, &p.name).unwrap_or(Some(CELESTIAL_BLEND));
        p
    });
    // Entities and block entities draw without blending, as terrain does and as vanilla's
    // cutout entity types do, unless `blend.*` says otherwise (OQ-23).
    let entity_blend = |p: Option<PostProgram>| {
        p.map(|mut p| {
            p.blend = blend(properties, &p.name).flatten();
            p
        })
    };
    let (entities, block) = match &inputs.entities {
        Some(i) => (
            entity_blend(gbuffers(&ENTITIES, Group::Gbuffers, i)),
            entity_blend(gbuffers(&BLOCK, Group::Gbuffers, i)),
        ),
        None => (None, None),
    };
    let damaged_block = inputs
        .entities
        .as_ref()
        .and_then(|i| gbuffers(&["gbuffers_damagedblock"], Group::Gbuffers, i))
        .map(|mut p| {
            p.blend = blend(properties, &p.name).unwrap_or(Some([
                Factor::DstColor,
                Factor::SrcColor,
                Factor::One,
                Factor::Zero,
            ]));
            p
        });
    // Translucent entities and particles blend (OQ-23). Particles keep vanilla's alpha cut at
    // 0.1; translucent entities lose only fully transparent texels (a shadow fades out).
    let translucent =
        |names: &[&str],
         cut: f32,
         gbuffers: &mut dyn FnMut(&[&str], &GbuffersInputs) -> Option<PostProgram>| {
            let i = inputs.entities.as_ref()?;
            let mut i = i.clone();
            i.alpha_test = Some(crate::AlphaTest {
                op: ">",
                reference: cut,
            });
            gbuffers(names, &i).map(|mut p| {
                p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
                p
            })
        };
    let mut gbuffers_dyn = |names: &[&str], i: &GbuffersInputs| gbuffers(names, Group::Gbuffers, i);
    let entities_translucent = translucent(&ENTITIES_TRANSLUCENT, 0.0, &mut gbuffers_dyn);
    let particles = translucent(&PARTICLES, 0.1, &mut gbuffers_dyn);
    // Optional slots deliberately have no fallback chain: an absent, disabled or rejected
    // variant must leave the legacy material grouping and state intact.
    let block_translucent = translucent(&["gbuffers_block_translucent"], 0.0, &mut gbuffers_dyn);
    let mixed = properties.get("particles.ordering").map(str::trim) == Some("mixed");
    let particles_translucent = mixed
        .then(|| translucent(&["gbuffers_particles_translucent"], 0.1, &mut gbuffers_dyn))
        .flatten();
    let beacon_beam = translucent(&BEACON_BEAM, 0.0, &mut gbuffers_dyn);
    // Clouds blend, as vanilla's do (OQ-23).
    let clouds = inputs
        .clouds
        .as_ref()
        .and_then(|i| gbuffers_dyn(&CLOUDS, i));
    let clouds = clouds.map(|mut p| {
        p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
        p
    });
    // The block outline blends, as vanilla's does.
    let lines = inputs
        .lines
        .as_ref()
        .and_then(|i| gbuffers_dyn(&LINES, i))
        .map(|mut p| {
            p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
            p
        });
    // Rain and snow draw after deferred and blend, as vanilla's do (OQ-23).
    let weather = inputs
        .weather
        .as_ref()
        .and_then(|i| gbuffers(&WEATHER, Group::Translucent, i));
    let weather = weather.map(|mut p| {
        p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
        p
    });
    // The hand draws as entities do: no blending unless `blend.*` says otherwise (OQ-23).
    let hand = inputs
        .hand
        .as_ref()
        .and_then(|i| gbuffers(&HAND, Group::Gbuffers, i))
        .map(|mut p| {
            p.blend = blend(properties, &p.name).flatten();
            p
        });
    // Translucent held items draw after deferred as translucent terrain does, and blend.
    let hand_water = inputs
        .hand
        .as_ref()
        .and_then(|i| gbuffers(&HAND_WATER, Group::Translucent, i))
        .map(|mut p| {
            p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
            p
        });
    if let Some(terrain) = &inputs.terrain {
        // Cutout terrain's alpha test, unless `alphaTest.*` says otherwise (3.2): alpha above
        // 0.5, which `alphaTestRef` reads. Iris 1.11.7 with probe packs of ours (#9307), through
        // gbuffers_terrain alone or gbuffers_terrain_cutout: cutout fragments written with alpha
        // 0.5 and below were discarded, 0.51 and above kept, and alphaTestRef read 0.5. Vanilla's
        // cutout terrain cuts at 0.5 too (`RenderPipelines.CUTOUT_TERRAIN`'s `ALPHA_CUTOUT`).
        let cutout = |inputs: &GbuffersInputs| {
            let mut cutout = inputs.clone();
            cutout.alpha_test = cutout.alpha_test.or(Some(crate::AlphaTest {
                op: ">",
                reference: 0.5,
            }));
            cutout
        };
        // Solid and cutout terrain draw apart. A pack with Iris's gbuffers_terrain_solid or
        // gbuffers_terrain_cutout draws each with its own, falling back to gbuffers_terrain's
        // chain (Iris 1.11.7 with probe packs having both, or one of them); any other pack draws
        // both parts with gbuffers_terrain's chain, each with its own alpha test (#9307). Solid
        // terrain gets no default alpha test, and alphaTestRef 0: in those probes Iris kept
        // solid fragments written with alpha 0.05, through either program, and also 0.0
        // through gbuffers_terrain alone or gbuffers_terrain_solid (#9307).
        let parts = inputs.terrain_parts.as_ref().filter(|_| split_terrain);
        let mut lone = None;
        let (solid, terrain_cutout) = match parts {
            Some([solid, cut]) => {
                let solid = gbuffers(&TERRAIN_SOLID, Group::Gbuffers, solid);
                let cut = gbuffers(&TERRAIN_CUTOUT, Group::Gbuffers, &cutout(cut));
                // One part loaded: its name, the other part, and whether the pack has a
                // program for that one.
                let one = match (&solid, &cut) {
                    (Some(p), None) => Some((p.name.clone(), "cutout", has_cutout)),
                    (None, Some(p)) => Some((p.name.clone(), "solid", has_solid)),
                    _ => None,
                };
                match one {
                    // No program in the other part's chain: that part draws nothing, and this
                    // one draws alone. Iris 1.11.7 left the other part's terrain undrawn with
                    // our probes having only gbuffers_terrain_solid, or only _cutout.
                    Some((name, other, false)) => {
                        lone = Some(format!(
                            "{name}: no program for {other} terrain; nothing draws it"
                        ));
                        (solid, cut)
                    }
                    // The other part's program didn't load: this one doesn't draw alone, which
                    // would leave that part's terrain undrawn. The game draws all terrain, as
                    // when an unsplit pack's terrain program doesn't load.
                    Some((name, other, true)) => {
                        lone = Some(format!(
                            "{name}: {other} terrain's program didn't load; the game draws all terrain"
                        ));
                        (None, None)
                    }
                    None => (solid, cut),
                }
            }
            None => match &inputs.terrain_parts {
                // One program draws both parts, so both load or neither does.
                Some([solid, cut]) => match (
                    gbuffers(&TERRAIN, Group::Gbuffers, solid),
                    gbuffers(&TERRAIN, Group::Gbuffers, &cutout(cut)),
                ) {
                    (Some(solid), Some(cut)) => (Some(solid), Some(cut)),
                    _ => (None, None),
                },
                // A game that draws terrain in one part: all of it with the cutout test.
                None => (gbuffers(&TERRAIN, Group::Gbuffers, &cutout(terrain)), None),
            },
        };
        // gbuffers_water falls back to gbuffers_terrain's chain (2.3).
        // Water discards what lies behind the boats' water patches (`GbuffersInputs::water_mask`).
        let water = gbuffers(
            &WATER,
            Group::Translucent,
            &GbuffersInputs {
                water_mask: true,
                ..terrain.clone()
            },
        );
        loader.post.notes.extend(lone);
        loader.post.terrain_undrawn = !has_solid && !has_cutout;
        loader.post.water_undrawn = !WATER.iter().any(|n| loader.has(n));
        for (undrawn, part) in [
            (loader.post.terrain_undrawn, "terrain"),
            (loader.post.water_undrawn, "translucent terrain"),
        ] {
            if undrawn {
                loader
                    .post
                    .notes
                    .push(format!("no program for {part}; nothing draws it"));
            }
        }
        loader.post.terrain = solid;
        loader.post.water = water.map(|mut p| {
            p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
            p
        });
        loader.post.terrain_cutout = terrain_cutout;
        let programs = [&mut loader.post.terrain, &mut loader.post.terrain_cutout];
        for p in programs.into_iter().flatten() {
            p.blend = blend(properties, &p.name).flatten();
        }
    }
    loader.post.sky = sky;
    loader.post.sky_textured = sky_textured;
    loader.post.entities = entities;
    loader.post.block = block;
    loader.post.block_translucent = block_translucent;
    loader.post.entities_translucent = entities_translucent;
    loader.post.particles = particles;
    loader.post.particles_translucent = particles_translucent;
    loader.post.beacon_beam = beacon_beam;
    loader.post.damaged_block = damaged_block;
    loader.post.clouds_off = properties.get("clouds").is_some_and(|v| v.trim() == "off");
    loader.post.clouds = clouds.filter(|_| !loader.post.clouds_off);
    loader.post.lines = lines;
    loader.post.weather = weather;
    loader.post.hand = hand;
    loader.post.hand_water = hand_water;
    let flag = |key: &str| properties.get(key).map(|v| v.trim() == "true");
    loader.post.translucent_late = flag("separateEntityDraws").unwrap_or(false);
    // Keep the legacy timing here. The renderer moves ordinary mixed particles earlier
    // only after it has successfully built their translucent variant.
    loader.post.particles_late = match properties.get("particles.ordering").map(str::trim) {
        Some("before") => false,
        Some(_) => true,
        None => !flag("particles.before.deferred").unwrap_or(false),
    };
    load_shadow(
        &mut loader,
        inputs.terrain.as_ref(),
        inputs.entities.as_ref(),
        [&inputs.terrain_geometry, &inputs.entity_geometry].map(Option::as_ref),
    );
    post
}

/// Whether the shadow pass runs (2.5) and, when it does, its programs: `shadow_solid` and
/// `shadow_cutout` are not used (2.4, Legacy); the game's solid and cutout terrain is one draw.
/// `shadow_entities` and `shadow_block` read the entities' vertices (`entities`). A program
/// with a geometry stage reads its vertices itself: `geometry` holds how, for terrain and for
/// entities.
fn load_shadow(
    loader: &mut Loader<'_>,
    terrain: Option<&GbuffersInputs>,
    entities: Option<&GbuffersInputs>,
    geometry: [Option<&crate::GeometryInputs>; 2],
) {
    let post = &*loader.post;
    let gbuffers = [
        &post.terrain,
        &post.terrain_cutout,
        &post.water,
        &post.sky,
        &post.sky_textured,
        &post.entities,
        &post.block,
        &post.block_translucent,
        &post.entities_translucent,
        &post.particles,
        &post.particles_translucent,
        &post.beacon_beam,
        &post.damaged_block,
        &post.clouds,
        &post.weather,
        &post.hand,
        &post.hand_water,
    ];
    // A program's compute stages count as its other stages do: Iris 1.11.7 drew the shadow map
    // for a probe pack of ours whose only reader of it was `prepare.csh` (#10490).
    let reads = (post.programs.iter())
        .chain(gbuffers.into_iter().flatten())
        .flat_map(|p| {
            let stages = p.program.iter().map(|q| &q.interface);
            stages.chain(p.compute.iter().map(|(_, c)| &c.interface))
        })
        .flat_map(|i| &i.samplers)
        .any(|s| {
            SHADOW_SAMPLERS.contains(&s.name.as_str())
                || config::shadow_color(&s.name).is_some_and(|n| n < loader.post.shadow_colors)
        });
    let forced = match loader.properties.get("shadow.enabled").map(str::trim) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    };
    let on = forced.unwrap_or(reads);
    loader.post.shadow_pass = on;
    if !on {
        // Observed in probe packs of ours (p9063a, p9063c), not a rule Iris states: Iris 1.11.7
        // honoured a `colortexNFormat` declared only in `shadow.fsh` or `shadowcomp.fsh` of a
        // pack Horizon runs without a shadow pass. So the colortex formats of the shadow
        // programs' fragment shaders are read here, with `shadow.fsh`'s shadow settings
        // (`shadowDistance` and `sunPathRotation` probed, #10490), and nothing else of them
        // (nothing else was probed). The other shadow programs' files were not probed; they are
        // read as `shadow`, formats only.
        let names = ["shadow", "shadow_water", "shadow_entities", "shadow_block"]
            .map(str::to_owned)
            .into_iter()
            .chain(std::iter::once("shadowcomp".to_owned()))
            .chain((1..=99).map(|n| format!("shadowcomp{n}")));
        for name in names {
            // `shadow.fsh`'s shadow settings count too (`Config::read_formats_and_shadow`).
            let shadow = name == "shadow";
            loader.read_formats(&name, shadow);
        }
        return;
    }
    let names =
        std::iter::once("shadowcomp".to_owned()).chain((1..=99).map(|n| format!("shadowcomp{n}")));
    for name in names {
        let program = loader.load(name, Group::ShadowComp, |v, f| {
            translate_composite(v, f, Target::Buffers)
        });
        loader.post.shadow_comp.extend(program);
    }
    let properties = loader.properties;
    let mut load =
        |names: &[&str], inputs: Option<&GbuffersInputs>, terrain: bool, cutout: bool| {
            let inputs = inputs?;
            names.iter().find_map(|name| {
                // Cutout geometry (leaves) casts the shadow of its texels only: the cutout test,
                // unless `alphaTest.<program>` says otherwise, as for the gbuffers programs.
                // Translucent terrain keeps no test, as gbuffers_water does.
                let test = match properties.get(&format!("alphaTest.{name}")) {
                    Some(v) => crate::AlphaTest::parse(v.trim()).flatten(),
                    None if !cutout => inputs.alpha_test,
                    None => inputs.alpha_test.or(Some(crate::AlphaTest {
                        op: ">",
                        reference: 0.1,
                    })),
                };
                let mut inputs = inputs.clone();
                inputs.alpha_test = test;
                let geometry = if terrain { geometry[0] } else { geometry[1] };
                loader.geometry = geometry.cloned().map(|mut g| {
                    g.inputs.alpha_test = test;
                    g
                });
                let p = loader.load(name.to_string(), Group::Shadow, |v, f| {
                    translate_shadow(v, f, &inputs)
                });
                p.map(|mut p| {
                    p.blend = blend(properties, &p.name).flatten();
                    p
                })
            })
        };
    // `shadowTerrain`, `shadowTranslucent`, `shadowEntities` and `shadowBlockEntities` (3.2)
    // leave their part out of the shadow pass; all are on by default (OQ-19). `shadowPlayer`
    // and `shadowLightBlockEntities` (E, off by default) keep the player, and block entities
    // that emit light, when the first two are off.
    let keep = |key: &str| properties.get(key).is_none_or(|v| v.trim() != "false");
    let set = |key: &str| properties.get(key).is_some_and(|v| v.trim() == "true");
    let only_player = !keep("shadowEntities") && set("shadowPlayer");
    let only_lit = !keep("shadowBlockEntities") && set("shadowLightBlockEntities");
    let shadow = keep("shadowTerrain")
        .then(|| load(&["shadow"], terrain, true, true))
        .flatten();
    // Its water keeps out of the boats' water patches (`GbuffersInputs::water_patches`).
    let water_inputs = terrain.map(|t| GbuffersInputs {
        water_patches: true,
        ..t.clone()
    });
    let water = keep("shadowTranslucent")
        .then(|| {
            load(
                &["shadow_water", "shadow"],
                water_inputs.as_ref(),
                true,
                false,
            )
        })
        .flatten();
    let shadow_entities = (keep("shadowEntities") || only_player)
        .then(|| load(&["shadow_entities", "shadow"], entities, false, true))
        .flatten();
    let shadow_block = (keep("shadowBlockEntities") || only_lit)
        .then(|| load(&["shadow_block", "shadow"], entities, false, true))
        .flatten();
    if shadow.is_none() && terrain.is_some() {
        loader
            .post
            .notes
            .push("shadow: no program; the shadow map stays clear".to_owned());
    }
    loader.geometry = None;
    loader.post.shadow = shadow;
    loader.post.shadow_water = water;
    loader.post.shadow_entities = shadow_entities;
    loader.post.shadow_block = shadow_block;
    loader.post.shadow_only_player = only_player;
    loader.post.shadow_only_lit_blocks = only_lit;
}

struct Loader<'a> {
    pack: &'a Pack,
    folder: &'a str,
    properties: &'a Properties,
    macros: &'a Macros,
    switches: BTreeSet<String>,
    /// `macros` with the pack's options, for the program switches.
    options: Macros,
    /// The game's geometry read vertex by vertex, while loading shadow programs.
    geometry: Option<crate::GeometryInputs>,
    post: &'a mut Post,
}

impl Loader<'_> {
    /// The pack has both stages of the program `name` in this folder, and doesn't turn it off
    /// (`program.<name>.enabled`), as `load` reads them.
    fn has(&self, name: &str) -> bool {
        ["vsh", "fsh"].iter().all(|ext| match self.folder {
            "" => self.pack.exists(&format!("{name}.{ext}")),
            f => self.pack.exists(&format!("{f}/{name}.{ext}")),
        }) && enabled(
            self.properties,
            self.folder,
            name,
            &self.options,
            &self.switches,
        )
    }

    /// The program `name` translated by `translate`, or `None` when it is missing, disabled or
    /// fails (with a note).
    fn load(
        &mut self,
        name: String,
        group: Group,
        translate: impl FnOnce(&Preprocessed, &Preprocessed) -> Result<Program, Vec<crate::Error>>,
    ) -> Option<PostProgram> {
        let path = |ext: &str| match self.folder {
            "" => format!("{name}.{ext}"),
            f => format!("{f}/{name}.{ext}"),
        };
        let (vsh, fsh) = (path("vsh"), path("fsh"));
        if group == Group::Setup || !self.pack.exists(&fsh) || !self.pack.exists(&vsh) {
            let on = enabled(
                self.properties,
                self.folder,
                &name,
                &self.options,
                &self.switches,
            );
            // A program of compute stages only (`.csh` without `.vsh` and `.fsh`).
            let composite = matches!(
                group,
                Group::Setup
                    | Group::Begin
                    | Group::Prepare
                    | Group::Deferred
                    | Group::Composite
                    | Group::ShadowComp
            );
            if !on || !composite {
                return None;
            }
            let compute = self.compute(&name);
            return (!compute.is_empty()).then(|| PostProgram {
                buffer_blends: buffer_blends(self.properties, &name),
                flips: flips(self.properties, &name),
                name,
                group,
                program: None,
                buffers: Vec::new(),
                blend: None,
                vanilla_sky_blends: false,
                mipmaps: 0,
                compute,
                geometry_triangles: 0,
                scale: FULL,
            });
        }
        if !enabled(
            self.properties,
            self.folder,
            &name,
            &self.options,
            &self.switches,
        ) {
            return None;
        }
        if self.pack.exists(&path("gsh")) {
            let Some(inputs) = self.geometry.clone() else {
                self.post.notes.push(format!(
                    "{name}: uses a geometry stage (supported for the shadow pass's terrain and entities only); skipped"
                ));
                return None;
            };
            let (vsh, gsh) = (path("vsh"), path("gsh"));
            return self.load_geometry(name, group, &vsh, &gsh, &fsh, &inputs);
        }
        let vertex = self.preprocess(&vsh);
        let fragment = self.preprocess(&fsh);
        let notes = &mut self.post.notes;
        let problems: Vec<String> = [&vertex, &fragment]
            .iter()
            .flat_map(|pp| diagnostics(pp))
            .collect();
        if !problems.is_empty() {
            notes.extend(problems);
            notes.push(format!("{name}: skipped"));
            return None;
        }
        notes.extend([&vertex, &fragment].iter().flat_map(|pp| warnings(pp)));
        let program = match translate(&vertex, &fragment) {
            Ok(p) => p,
            Err(errors) => {
                notes.extend(
                    errors
                        .iter()
                        .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
                );
                notes.push(format!("{name}: skipped"));
                return None;
            }
        };
        notes.extend(
            program
                .notes
                .iter()
                .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
        );
        self.post.config.read_vertex(&vertex);
        self.post.config.read(&fragment);
        let mut buffers =
            config::draw_buffers(&fragment).unwrap_or_else(|| default_draw_buffers(group.shadow()));
        if group.shadow() {
            // Output n writes shadowcolor n (6.3); `shadow_buffers` says how many there are.
            let kept = buffers
                .iter()
                .position(|&b| b as usize >= self.shadow_buffers(&fragment))
                .unwrap_or(buffers.len());
            buffers.truncate(kept);
        }
        let mipmaps = match group {
            Group::Gbuffers | Group::Translucent | Group::Shadow | Group::ShadowComp => 0,
            _ => {
                let mut own = Config::default();
                own.read(&fragment);
                (own.buffers.iter().enumerate())
                    .filter(|b| b.1.mipmap)
                    .fold(0, |m, (i, _)| m | 1 << i)
            }
        };
        let compute = match group {
            Group::Setup
            | Group::Begin
            | Group::Prepare
            | Group::Deferred
            | Group::Composite
            | Group::Final
            | Group::ShadowComp => self.compute(&name),
            _ => Vec::new(),
        };
        Some(PostProgram {
            buffer_blends: buffer_blends(self.properties, &name),
            flips: flips(self.properties, &name),
            scale: scale(self.properties, &name, group),
            name,
            group,
            program: Some(program),
            buffers,
            blend: None,
            vanilla_sky_blends: false,
            mipmaps,
            compute,
            geometry_triangles: 0,
        })
    }

    /// Reads the colortex formats of program `name` without loading it, when it is there and
    /// enabled: those of its fragment shader, as [`Self::load`] reads them; with `shadow`, its
    /// shadow settings too.
    fn read_formats(&mut self, name: &str, shadow: bool) {
        let path = |ext: &str| match self.folder {
            "" => format!("{name}.{ext}"),
            f => format!("{f}/{name}.{ext}"),
        };
        let (vsh, fsh) = (path("vsh"), path("fsh"));
        let on = enabled(
            self.properties,
            self.folder,
            name,
            &self.options,
            &self.switches,
        );
        if !on || !self.pack.exists(&fsh) || !self.pack.exists(&vsh) {
            return;
        }
        let fragment = self.preprocess(&fsh);
        if shadow {
            self.post.config.read_formats_and_shadow(&fragment);
        } else {
            self.post.config.read_formats(&fragment);
        }
    }

    /// Stage file `path` through the preprocessor; the pack's images declared without a format
    /// (GLSL lets a write-only image omit it) get the one `image.*` gives them.
    fn preprocess(&self, path: &str) -> Preprocessed {
        let mut pp = preprocess(self.pack, path, self.macros);
        if self.post.images.is_empty() || !pp.code.contains("image") {
            return pp;
        }
        let source: Vec<&str> = pp.code.split('\n').collect();
        let lines: Vec<String> = source
            .iter()
            .enumerate()
            .map(|(n, &line)| {
                // A qualifier on the line before (`layout(r32ui)` then `uniform uimage3D v;`).
                let qualified = line.contains("layout")
                    || source[..n]
                        .iter()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .is_some_and(|l| l.trim_start().starts_with("layout") && !l.contains(';'));
                let words: Vec<&str> = line
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .collect();
                let image = self
                    .post
                    .images
                    .iter()
                    .find(|i| words.contains(&i.name.as_str()));
                match image {
                    Some(i)
                        if words.contains(&"uniform")
                            && !qualified
                            && words.iter().any(|w| {
                                ["image1D", "image2D", "image3D"]
                                    .iter()
                                    .any(|d| w.ends_with(d))
                            }) =>
                    {
                        format!("layout({}) {line}", i.qualifier)
                    }
                    _ => line.to_owned(),
                }
            })
            .collect();
        pp.code = lines.join("\n");
        pp
    }

    /// A program with a geometry stage, folded into its vertex stage.
    fn load_geometry(
        &mut self,
        name: String,
        group: Group,
        vsh: &str,
        gsh: &str,
        fsh: &str,
        inputs: &crate::GeometryInputs,
    ) -> Option<PostProgram> {
        let stages = [vsh, gsh, fsh].map(|p| self.preprocess(p));
        let notes = &mut self.post.notes;
        let problems: Vec<String> = stages.iter().flat_map(diagnostics).collect();
        if !problems.is_empty() {
            notes.extend(problems);
            notes.push(format!("{name}: skipped"));
            return None;
        }
        let [vertex, geometry, fragment] = &stages;
        let (program, triangles) =
            match crate::translate_geometry(vertex, geometry, fragment, inputs) {
                Ok(p) => p,
                Err(errors) => {
                    notes.extend(
                        errors
                            .iter()
                            .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
                    );
                    notes.push(format!("{name}: skipped"));
                    return None;
                }
            };
        notes.extend(
            program
                .notes
                .iter()
                .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
        );
        self.post.config.read_vertex(vertex);
        self.post.config.read(fragment);
        let mut buffers =
            config::draw_buffers(fragment).unwrap_or_else(|| default_draw_buffers(true));
        let kept = buffers
            .iter()
            .position(|&b| b as usize >= self.shadow_buffers(fragment))
            .unwrap_or(buffers.len());
        buffers.truncate(kept);
        Some(PostProgram {
            buffer_blends: buffer_blends(self.properties, &name),
            flips: flips(self.properties, &name),
            scale: scale(self.properties, &name, group),
            name,
            group,
            program: Some(program),
            buffers,
            blend: None,
            vanilla_sky_blends: false,
            mipmaps: 0,
            compute: Vec::new(),
            geometry_triangles: triangles,
        })
    }

    /// The shadowcolor buffers a shadow program's outputs can write: all the pack has with a
    /// `RENDERTARGETS` or `DRAWBUFFERS` directive, else shadowcolor0 and 1. Iris 1.11.7 with
    /// `HIGHER_SHADOWCOLOR`: a `shadow` program without a directive leaves shadowcolor2 to 7
    /// alone, one with `RENDERTARGETS: 0,1,2,3,4` writes all five (probe packs of our own).
    fn shadow_buffers(&self, fragment: &Preprocessed) -> usize {
        match config::draw_buffers(fragment) {
            Some(_) => self.post.shadow_colors,
            None => 2,
        }
    }

    /// The compute programs of composite-style program `name` that are on and translate.
    fn compute(&mut self, name: &str) -> Vec<(String, ComputeProgram)> {
        let mut out = Vec::new();
        let names =
            std::iter::once(name.to_owned()).chain(('a'..='z').map(|c| format!("{name}_{c}")));
        for name in names {
            let path = match self.folder {
                "" => format!("{name}.csh"),
                f => format!("{f}/{name}.csh"),
            };
            if !self.pack.exists(&path)
                || !enabled(
                    self.properties,
                    self.folder,
                    &name,
                    &self.options,
                    &self.switches,
                )
            {
                continue;
            }
            let pp = self.preprocess(&path);
            let problems = diagnostics(&pp);
            let notes = &mut self.post.notes;
            if !problems.is_empty() {
                notes.extend(problems);
                notes.push(format!("{name}: skipped"));
                continue;
            }
            match translate_compute(&pp) {
                Ok(program) => {
                    notes.extend(
                        program
                            .notes
                            .iter()
                            .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
                    );
                    out.push((name, program));
                }
                Err(errors) => {
                    notes.extend(
                        errors
                            .iter()
                            .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
                    );
                    notes.push(format!("{name}: skipped"));
                }
            }
        }
        out
    }
}

/// The buffers a program writes without `RENDERTARGETS` or `DRAWBUFFERS`: only buffer 0, except
/// in the shadow pass's own programs, where outputs 0 and 1 write shadowcolor0 and shadowcolor1.
/// Measured in Iris 1.11.7 with probe packs: a `gbuffers_terrain`, `begin`, `prepare`, `deferred`
/// or `composite` program that writes `gl_FragData[1..]` or `layout(location = n)` leaves those
/// buffers untouched, while the second output of `shadow` and `shadowcomp` lands in shadowcolor1.
fn default_draw_buffers(shadow: bool) -> Vec<u32> {
    if shadow { (0..8).collect() } else { vec![0] }
}

fn diagnostics(pp: &Preprocessed) -> Vec<String> {
    located(pp, &pp.diagnostics)
}

/// `pp`'s warnings (conditions read as false, unbalanced conditionals), located.
fn warnings(pp: &Preprocessed) -> Vec<String> {
    located(pp, &pp.warnings)
}

fn located(pp: &Preprocessed, list: &[crate::preprocess::Diagnostic]) -> Vec<String> {
    list.iter()
        .map(|d| {
            let file = pp
                .files
                .get(d.origin.file as usize)
                .map_or("?", String::as_str);
            format!("{file}:{}: {}", d.origin.line, d.message)
        })
        .collect()
}

/// `program.<name>.enabled` (or `program.<folder>/<name>.enabled`): a boolean expression over
/// switch options (3.3): those on by default (8.1) and the defined macros.
fn enabled(
    properties: &Properties,
    folder: &str,
    name: &str,
    macros: &Macros,
    switches: &BTreeSet<String>,
) -> bool {
    let keys = [
        format!("program.{folder}/{name}.enabled"),
        format!("program.{name}.enabled"),
    ];
    let Some(expr) = keys.iter().find_map(|k| properties.get(k)) else {
        return true;
    };
    // `true`/`false` as numbers, every other name as `defined(name)`, then the `#if` arithmetic.
    let mut condition = String::new();
    for token in crate::macros::lex(expr) {
        match token {
            "true" => condition.push('1'),
            "false" => condition.push('0'),
            t if switches.contains(t) => condition.push('1'),
            t if t.starts_with(|c: char| c.is_alphabetic() || c == '_') => {
                condition.push_str(&format!("defined({t})"));
            }
            t => condition.push_str(t),
        }
    }
    eval_condition(&condition, macros).ok() != Some(0)
}

/// `blend.<program>`: `Some(None)` for `off`, `Some(factors)`, or `None` when absent or not
/// four known factors.
fn blend(properties: &Properties, program: &str) -> Option<Option<Blend>> {
    let value = properties.get(&format!("blend.{program}"))?.trim();
    if value == "off" {
        return Some(None);
    }
    let factors: Vec<Factor> = value
        .split_whitespace()
        .map(Factor::parse)
        .collect::<Option<_>>()?;
    Some(Some(factors.try_into().ok()?))
}

/// Blend overrides apply to buffer names even when DRAWBUFFERS remaps locations.
fn buffer_blends(properties: &Properties, program: &str) -> Vec<(u32, Option<Blend>)> {
    let prefix = format!("blend.{program}.");
    properties
        .entries
        .iter()
        .filter_map(|e| {
            let buffer = config::buffer_index(e.key.strip_prefix(&prefix)?)?;
            let value = blend(
                properties,
                &format!("{program}.{}", e.key.strip_prefix(&prefix)?),
            )?;
            Some((buffer as u32, value))
        })
        .collect()
}

/// A program drawing all of its buffers.
const FULL: [f32; 3] = [1.0, 0.0, 0.0];

/// `scale.<program>=<scale> [<offsetX> <offsetY>]` of a deferred or composite program (OptiFine's
/// "Composite render scale"); prepare programs are treated the same, unverified. Iris 1.11.7
/// applies the offsets too (a probe pack of our own, composite). A value that is not a scale in (0, 1] with offsets in [0, 1) draws all of them.
fn scale(properties: &Properties, program: &str, group: Group) -> [f32; 3] {
    if !matches!(group, Group::Prepare | Group::Deferred | Group::Composite) {
        return FULL;
    }
    let Some(value) = properties.get(&format!("scale.{program}")) else {
        return FULL;
    };
    let numbers: Option<Vec<f32>> = value.split_whitespace().map(|n| n.parse().ok()).collect();
    let fraction = |s: f32| s > 0.0 && s <= 1.0;
    let offset = |o: f32| (0.0..1.0).contains(&o);
    match numbers.as_deref() {
        Some(&[s]) if fraction(s) => [s, 0.0, 0.0],
        Some(&[s, x, y]) if fraction(s) && offset(x) && offset(y) => [s, x, y],
        _ => FULL,
    }
}

/// Whether the pack names the `HIGHER_SHADOWCOLOR` feature, required or optional, in any case.
fn higher_shadowcolor(properties: &Properties) -> bool {
    ["iris.features.required", "iris.features.optional"]
        .iter()
        .filter_map(|k| properties.get(k))
        .any(|v| {
            v.split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("HIGHER_SHADOWCOLOR"))
        })
}

/// `flip.<program>.<buffer>=<bool>` keys of one program.
fn flips(properties: &Properties, program: &str) -> Vec<(u32, bool)> {
    let prefix = format!("flip.{program}.");
    properties
        .entries
        .iter()
        .filter_map(|e| {
            let buffer = config::buffer_index(e.key.strip_prefix(&prefix)?)?;
            let value = match e.value.trim() {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            Some((buffer as u32, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_defaults_are_notes_without_skipping_render_or_compute() {
        let pack = Pack::from_zip_bytes(
            "original-default-control",
            crate::pack::tests::zip(&[
                ("shaders/composite.vsh", "#version 430\nvoid main() { gl_Position = vec4(0.0); }"),
                ("shaders/composite.fsh", "#version 430\nuniform double unknown = double(2.0);\nout vec4 color; void main() { color = vec4(1.0); }"),
                ("shaders/composite.csh", "#version 430\nlayout(local_size_x=1) in; uniform double unknown = double(2.0); void main() {}"),
            ]),
        ).unwrap();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &Macros::standard(),
            &GameInputs::default(),
        );
        let program = post
            .programs
            .iter()
            .find(|p| p.name == "composite")
            .unwrap();
        assert!(program.program.is_some());
        assert_eq!(program.compute.len(), 1);
        for file in ["composite.fsh", "composite.csh"] {
            assert!(
                post.notes
                    .iter()
                    .any(|n| n.contains(file) && n.contains("unknown") && n.contains("using zero")),
                "{:?}",
                post.notes
            );
        }
        assert!(
            !post.notes.iter().any(|n| n.contains("skipped")),
            "{:?}",
            post.notes
        );
    }

    /// #10455, with probe packs of ours in Iris 1.11.7: a pack (or the dimension folder it
    /// uses) with no gbuffers programs drew no terrain, solid or translucent, into any buffer or
    /// depth, and the root folder's gbuffers programs didn't draw the Nether's terrain when
    /// `world-1` had none. A `gbuffers_water` alone draws translucent terrain only (Horizon's
    /// choice; not measured in Iris).
    #[test]
    fn a_folder_without_terrain_programs_draws_no_terrain() {
        const VSH: &str = "#version 120\nvoid main() { gl_Position = ftransform(); }";
        const FSH: &str =
            "#version 120\n/* DRAWBUFFERS:0 */\nvoid main() { gl_FragData[0] = vec4(1.0); }";
        let pack = Pack::from_zip_bytes(
            "original-undrawn-terrain",
            crate::pack::tests::zip(&[
                ("shaders/gbuffers_basic.vsh", VSH),
                ("shaders/gbuffers_basic.fsh", FSH),
                ("shaders/world-1/composite.vsh", VSH),
                ("shaders/world-1/composite.fsh", FSH),
                ("shaders/world1/gbuffers_water.vsh", VSH),
                ("shaders/world1/gbuffers_water.fsh", FSH),
            ]),
        )
        .unwrap();
        let inputs = GameInputs {
            terrain: Some(GbuffersInputs::default()),
            ..Default::default()
        };
        let load = |folder: &str| {
            let post = load_post(
                &pack,
                folder,
                &Properties::default(),
                &Macros::standard(),
                &inputs,
            );
            (
                post.terrain.is_some(),
                post.water.is_some(),
                post.terrain_undrawn,
                post.water_undrawn,
            )
        };
        assert_eq!(load(""), (true, true, false, false));
        assert_eq!(load("world-1"), (false, false, true, true));
        assert_eq!(load("world1"), (false, true, true, false));
    }

    #[test]
    fn named_dimension_folder_reaches_the_program_loader() {
        let pack = Pack::from_zip_bytes(
            "original-dimension-control",
            crate::pack::tests::zip(&[
                (
                    "shaders/dimension.properties",
                    "dimension.netherShaders = minecraft:the_nether\n",
                ),
                (
                    "shaders/final.vsh",
                    "#version 120\nvoid main() { gl_Position = gl_Vertex; }",
                ),
                (
                    "shaders/final.fsh",
                    "#version 120\n/* DRAWBUFFERS:0 */\nvoid main() { gl_FragColor = vec4(0.0); }",
                ),
                (
                    "shaders/netherShaders/final.vsh",
                    "#version 120\nvoid main() { gl_Position = gl_Vertex; }",
                ),
                (
                    "shaders/netherShaders/final.fsh",
                    "#version 120\n/* DRAWBUFFERS:3 */\nvoid main() { gl_FragColor = vec4(1.0); }",
                ),
            ]),
        )
        .unwrap();
        let folder = dimension_folder_named(&pack, "minecraft:the_nether", -1);
        assert_eq!(folder, "netherShaders");
        let post = load_post(
            &pack,
            &folder,
            &Properties::default(),
            &Macros::standard(),
            &GameInputs::default(),
        );
        assert!(post.notes.is_empty(), "{:?}", post.notes);
        assert_eq!(post.programs.len(), 1);
        assert_eq!(post.programs[0].buffers, [3]);
    }

    #[test]
    fn named_dimensions_precede_wildcards_and_legacy_folders() {
        for mappings in [
            "dimension.other = *\ndimension.specific = the_nether example:moon\n",
            "dimension.specific = the_nether example:moon\ndimension.other = *\n",
        ] {
            let pack = Pack::from_zip_bytes(
                "t",
                crate::pack::tests::zip(&[
                    ("shaders/dimension.properties", mappings),
                    ("shaders/world-1/final.fsh", "legacy"),
                ]),
            )
            .unwrap();
            assert_eq!(dimension_folder(&pack, -1), "specific");
            assert_eq!(dimension_folder_named(&pack, "example:moon", 0), "specific");
            assert_eq!(
                dimension_folder_named(&pack, "example:the_nether", -1),
                "other"
            );
            assert_eq!(dimension_folder(&pack, 1), "other");
        }
        let pack = Pack::from_zip_bytes(
            "t",
            crate::pack::tests::zip(&[
                (
                    "shaders/dimension.properties",
                    "dimension.specific = the_end\ndimension.../../escape = *\n",
                ),
                ("shaders/world-1/final.fsh", "legacy"),
            ]),
        )
        .unwrap();
        assert_eq!(dimension_folder(&pack, -1), "");
        assert_eq!(dimension_folder(&pack, 1), "specific");
        let legacy = Pack::from_zip_bytes(
            "t",
            crate::pack::tests::zip(&[("shaders/world-1/final.fsh", "legacy")]),
        )
        .unwrap();
        assert_eq!(dimension_folder(&legacy, -1), "world-1");
        assert_eq!(dimension_folder(&legacy, 0), "");
    }

    #[test]
    fn dimension_mapping_paths_and_unknown_legacy_ids() {
        let mapped = |rules: &str| {
            Pack::from_zip_bytes(
                "t",
                crate::pack::tests::zip(&[("shaders/dimension.properties", rules)]),
            )
            .unwrap()
        };
        let pack =
            mapped("dimension.over = minecraft:overworld\ndimension.nested/./fallback = *\n");
        assert_eq!(dimension_folder(&pack, 0), "over");
        assert_eq!(dimension_folder(&pack, 2), "nested/fallback");
        assert_eq!(
            dimension_folder_named(&pack, "mod:moon", 2),
            "nested/fallback"
        );
        let pack = mapped("dimension.over = overworld\ndimension.../escape = *\n");
        assert_eq!(dimension_folder(&pack, 2), "");
        let pack = mapped("dimension.over = overworld\n");
        assert_eq!(dimension_folder(&pack, 2), "");
    }

    #[test]
    fn image_dimensions_expand_default_and_chosen_options() {
        let bytes =
            crate::pack::tests::zip(&[("shaders/options.glsl", "#define SIZE 16 // [8 16]\n")]);
        for (choice, expected) in [(None, [16, 16]), (Some("8"), [8, 8])] {
            let mut pack = Pack::from_zip_bytes("image.zip", bytes.clone()).unwrap();
            if let Some(value) = choice {
                pack = pack.with_chosen(std::collections::BTreeMap::from([(
                    "SIZE".into(),
                    value.into(),
                )]));
            }
            let macros = Macros::standard();
            let properties = Properties::parse(
                "image.grid = none RGBA RGBA8 BYTE false false SIZE SIZE",
                &macros,
            );
            let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
            assert!(post.notes.is_empty(), "{:?}", post.notes);
            assert_eq!(post.images.len(), 1);
            assert_eq!(post.images[0].size, ImageSize::Absolute(expected));
        }
    }

    #[test]
    fn image_dimensions_keep_literals_and_reject_unknown_macros() {
        let pack = Pack::from_zip_bytes(
            "image.zip",
            crate::pack::tests::zip(&[("shaders/placeholder.glsl", "")]),
        )
        .unwrap();
        let macros = Macros::standard();
        let properties = Properties::parse(
            "image.grid = none RGBA RGBA8 BYTE false false 8 16",
            &macros,
        );
        let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
        assert!(post.notes.is_empty(), "{:?}", post.notes);
        assert_eq!(post.images[0].size, ImageSize::Absolute([8, 16]));
        let properties = Properties::parse(
            "image.grid = none RGBA RGBA8 BYTE false false UNKNOWN UNKNOWN",
            &macros,
        );
        let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
        assert!(post.images.is_empty());
        assert!(!post.notes.is_empty());
    }
    use crate::pack::tests::zip;

    const VSH: &str = "#version 120\nvarying vec2 uv;\nvoid main() {\n\
                       gl_Position = ftransform();\nuv = gl_MultiTexCoord0.xy;\n}\n";
    const FSH: &str = "#version 120\nuniform sampler2D colortex0;\nvarying vec2 uv;\n\
                       void main() {\n/* DRAWBUFFERS:3 */\n\
                       gl_FragData[0] = texture2D(colortex0, uv);\n}\n";

    #[test]
    fn buffer_objects_and_images_parse() {
        assert_eq!(BufferObject::parse("4096"), Some(BufferObject::Bytes(4096)));
        assert_eq!(
            BufferObject::parse("64 false 1.0 1.0"),
            Some(BufferObject::Bytes(64))
        );
        assert_eq!(
            BufferObject::parse("16 true 0.5 0.25"),
            Some(BufferObject::PerPixel {
                bytes: 16,
                scale: [0.5, 0.25]
            })
        );
        assert_eq!(BufferObject::parse("16 true 0.5"), None);
        assert_eq!(BufferObject::parse("many"), None);
        let voxels = CustomImage::parse(
            "voxels",
            "voxelSampler RED_INTEGER R32UI UNSIGNED_INT true false 64 32 16",
        )
        .unwrap();
        assert_eq!(voxels.sampler.as_deref(), Some("voxelSampler"));
        assert_eq!(voxels.size, ImageSize::Volume([64, 32, 16]));
        assert!(voxels.clear);
        let half =
            CustomImage::parse("half", "none RGBA RGBA16F HALF_FLOAT false true 0.5 0.5").unwrap();
        assert_eq!(
            (half.sampler, half.size),
            (None, ImageSize::Relative([0.5, 0.5]))
        );
        assert!(!half.clear);
        assert!(
            CustomImage::parse("bad", "none RGBA RGBA17Q HALF_FLOAT false true 0.5 0.5").is_none()
        );
        assert!(CustomImage::parse("short", "none RGBA RGBA16F HALF_FLOAT false").is_none());
        let flat = CustomImage::parse("flat", "none RGBA RGBA8 BYTE false false 8 8 1").unwrap();
        assert!(flat.is_3d());
        assert_eq!(flat.extent([1, 1]), [8, 8, 1]);
        assert!(
            !CustomImage::parse("plain", "none RGBA RGBA8 BYTE false false 8 8")
                .unwrap()
                .is_3d()
        );
        // One size: a 1D image, a table read by index through its sampler.
        let table = CustomImage::parse(
            "imgTable",
            "texTable RED_INTEGER R32UI UNSIGNED_INT false false 4096",
        )
        .unwrap();
        assert_eq!(table.sampler.as_deref(), Some("texTable"));
        assert_eq!(table.size, ImageSize::Line(4096));
        assert!(table.is_1d() && !table.is_3d());
        assert_eq!(table.extent([1920, 1080]), [4096, 1, 1]);
        assert!(!flat.is_1d());
        // A relative size needs both.
        assert!(CustomImage::parse("r", "none RGBA RGBA8 BYTE false true 0.5").is_none());
    }

    /// A pack's 1D image and its sampler: the image is loaded, its `uimage1D` declaration
    /// gets the image's format, and the program reading both translates.
    #[test]
    fn a_1d_image_and_its_sampler_load() {
        let properties =
            "image.imgTable = texTable RED_INTEGER R32UI UNSIGNED_INT false false 64\n";
        let csh = "#version 430\nlayout(local_size_x = 64) in;\nwriteonly uniform uimage1D imgTable;\n\
                   void main() { imageStore(imgTable, int(gl_GlobalInvocationID.x), uvec4(gl_GlobalInvocationID.x)); }\n";
        let fsh = "#version 430\nuniform usampler1D texTable;\nuniform int blockId;\n\
                   /* RENDERTARGETS: 0 */\nlayout(location = 0) out vec4 color;\n\
                   void main() { color = vec4(texelFetch(texTable, blockId % 64, 0).r); }\n";
        let vsh = "#version 430\nout vec2 uv;\nvoid main() { uv = vec2(0.0); gl_Position = ftransform(); }\n";
        let pack = Pack::from_zip_bytes(
            "table.zip",
            crate::pack::tests::zip(&[
                ("shaders/shaders.properties", properties),
                ("shaders/setup.csh", csh),
                ("shaders/composite.vsh", vsh),
                ("shaders/composite.fsh", fsh),
            ]),
        )
        .unwrap();
        let mut macros = Macros::standard();
        let (properties, notes) = crate::read_properties(&pack, &mut macros);
        assert!(notes.is_empty(), "{notes:?}");
        let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
        assert!(post.notes.is_empty(), "{:?}", post.notes);
        assert_eq!(post.images.len(), 1);
        assert!(post.images[0].is_1d());
        let composite = post
            .programs
            .iter()
            .find(|p| p.name == "composite")
            .unwrap();
        let program = composite.program.as_ref().unwrap();
        let sampler = program
            .interface
            .samplers
            .iter()
            .find(|s| s.name == "texTable")
            .unwrap();
        assert_eq!(sampler.ty, "usampler1D");
    }

    /// A storage buffer declared smaller than the block a stage binds it as: the block's size
    /// as the stage lays it out is what the buffer needs (std430 rounds a block to its
    /// largest member's alignment), the most of every stage and program that binds it.
    #[test]
    fn storage_buffers_need_their_blocks_size() {
        let compute = "#version 430\nlayout(local_size_x = 1) in;\n\
                       layout(std430, binding = 0) buffer Sums { vec4 a; vec4 b; float c; } sums;\n\
                       layout(std430, binding = 1) buffer Cells { uint count; uvec4 cells[3]; } grid;\n\
                       void main() {\n    sums.c = sums.a.x + float(grid.count + grid.cells[0].x);\n}\n";
        let fsh = "#version 430\nlayout(std430, binding = 0) buffer Sums { vec4 a; } sums;\n\
                   uniform sampler2D colortex0;\nvarying vec2 uv;\n\
                   void main() {\n/* DRAWBUFFERS:3 */\n\
                   gl_FragData[0] = texture2D(colortex0, uv) + sums.a;\n}\n";
        let pack = Pack::from_zip_bytes(
            "t",
            zip(&[
                ("shaders/composite.vsh", VSH),
                ("shaders/composite.fsh", fsh),
                ("shaders/composite.csh", compute),
            ]),
        )
        .unwrap();
        let macros = Macros::standard();
        let properties = Properties::parse("bufferObject.0 = 40\nbufferObject.1 = 4\n", &macros);
        let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
        assert!(post.notes.is_empty(), "{:?}", post.notes);
        assert_eq!(post.programs[0].compute.len(), 1);
        // `Sums`: 36 bytes rounded to vec4's 16; `Cells`: a uint padded to the array's 16,
        // then three elements.
        assert_eq!(
            post.buffer_minimums(),
            std::collections::BTreeMap::from([(0, 48), (1, 64)])
        );
    }

    #[test]
    fn buffer_sizes_are_bounded() {
        let relative = BufferSize::parse("0.5 0.25").unwrap();
        assert_eq!(relative.resolve([1920, 1080]), [960, 270]);
        assert_eq!(
            BufferSize::parse("1e30 1.0").unwrap().resolve([1920, 1080])[0],
            BufferSize::MAX
        );
        assert_eq!(BufferSize::parse("inf 1.0"), None);
        assert_eq!(BufferSize::parse("-0.5 1.0"), None);
        assert_eq!(
            BufferSize::parse("100000 0").unwrap().resolve([1, 1]),
            [BufferSize::MAX, 1]
        );
    }

    /// `blend.<program>` blends every buffer a composite-style program writes, while
    /// `blend.<program>.<buffer>` does not apply to them (Iris 1.11.7, probed: a buffer's `off`
    /// under a program blend still blends, and a buffer's factors alone do not).
    #[test]
    fn composite_programs_blend_as_their_program_says() {
        let pack = Pack::from_zip_bytes(
            "t",
            zip(&[
                ("shaders/deferred.vsh", VSH),
                ("shaders/deferred.fsh", FSH),
                ("shaders/composite.vsh", VSH),
                ("shaders/composite.fsh", FSH),
                ("shaders/composite1.vsh", VSH),
                ("shaders/composite1.fsh", FSH),
                ("shaders/composite2.vsh", VSH),
                ("shaders/composite2.fsh", FSH),
            ]),
        )
        .unwrap();
        let properties = Properties::parse(
            "blend.deferred = ONE ONE ZERO ONE
blend.composite = SRC_ALPHA ONE_MINUS_SRC_ALPHA ONE ZERO
\
             blend.composite.colortex3 = off
blend.composite1.colortex3 = ONE ONE ONE ONE
\
             blend.composite2 = off
",
            &Macros::default(),
        );
        let post = load_post(
            &pack,
            "",
            &properties,
            &Macros::standard(),
            &GameInputs::default(),
        );
        use Factor::*;
        let blends: Vec<(&str, Option<Blend>, usize)> = post
            .programs
            .iter()
            .map(|p| (p.name.as_str(), p.blend, p.buffer_blends.len()))
            .collect();
        assert_eq!(
            blends,
            [
                ("deferred", Some([One, One, Zero, One]), 0),
                (
                    "composite",
                    Some([SrcAlpha, OneMinusSrcAlpha, One, Zero]),
                    0
                ),
                ("composite1", None, 0),
                ("composite2", None, 0),
            ]
        );
    }

    #[test]
    fn programs_load_in_order_with_their_settings() {
        let pack = Pack::from_zip_bytes(
            "t",
            zip(&[
                ("shaders/composite.vsh", VSH),
                ("shaders/composite.fsh", FSH),
                ("shaders/composite2.vsh", VSH),
                ("shaders/composite2.fsh", FSH),
                ("shaders/deferred.vsh", VSH),
                (
                    "shaders/deferred.fsh",
                    "#version 120\nconst bool colortex5MipmapEnabled = true;\n\
                     void main() { gl_FragColor = vec4(1.0); }\n",
                ),
                ("shaders/composite1.fsh", FSH),
                ("shaders/composite3.vsh", VSH),
                ("shaders/composite3.fsh", FSH),
                ("shaders/composite4.vsh", VSH),
                ("shaders/composite4.fsh", FSH),
                ("shaders/composite4.gsh", "x"),
                ("shaders/composite5.vsh", VSH),
                (
                    "shaders/composite5.fsh",
                    "#version 120\nvoid main() { undefined(); }\n",
                ),
                ("shaders/final.vsh", VSH),
                ("shaders/final.fsh", FSH),
                ("shaders/world-1/final.vsh", VSH),
                ("shaders/world-1/final.fsh", FSH),
            ]),
        )
        .unwrap();
        let properties = Properties::parse(
            "program.composite3.enabled = HQ && !LOW\nflip.composite2.gaux1=true\n\
             flip.composite.colortex3 = false\nflip.composite_pre.colortex1=true\n",
            &Macros::default(),
        );
        assert_eq!(dimension_folder(&pack, 0), "");
        assert_eq!(dimension_folder(&pack, -1), "world-1");
        let post = load_post(
            &pack,
            "",
            &properties,
            &Macros::standard(),
            &GameInputs::default(),
        );
        let names: Vec<&str> = post.programs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["deferred", "composite", "composite2", "final"]);
        let p = &post.programs;
        assert_eq!(p[0].buffers, [0]);
        assert_eq!((p[0].mipmaps, p[1].mipmaps), (1 << 5, 0));
        assert_eq!(p[1].buffers, [3]);
        assert_eq!(p[1].flips, [(3, false)]);
        assert_eq!(p[2].flips, [(4, true)]);
        assert_eq!(post.composite_pre, [(1, true)]);
        assert_eq!(p[3].group, Group::Final);
        let notes = post.notes.join("\n");
        assert!(
            notes.contains(
                "composite4: uses a geometry stage (supported for the shadow pass's terrain and entities only)"
            ),
            "{notes}"
        );
        assert!(notes.contains("composite5.fsh:2:"), "{notes}");
        assert!(notes.contains("composite5: skipped"), "{notes}");

        let mut macros = Macros::standard();
        macros.define("HQ", "");
        let post = load_post(
            &pack,
            "world-1",
            &properties,
            &macros,
            &GameInputs::default(),
        );
        let names: Vec<&str> = post.programs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["final"]);
        let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
        assert!(post.programs.iter().any(|p| p.name == "composite3"));
    }

    #[test]
    fn scale_keys_give_composite_style_programs_a_viewport() {
        let names = [
            "deferred",
            "composite",
            "composite1",
            "composite2",
            "composite3",
            "final",
        ];
        let files: Vec<(String, &str)> = names
            .iter()
            .flat_map(|n| {
                [
                    (format!("shaders/{n}.vsh"), VSH),
                    (format!("shaders/{n}.fsh"), FSH),
                ]
            })
            .collect();
        let files: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), *t)).collect();
        let pack = Pack::from_zip_bytes("t", zip(&files)).unwrap();
        let properties = Properties::parse(
            "scale.deferred = 0.5\nscale.composite = 0.25 0.5 0.25\nscale.composite1 = 2\n\
             scale.composite2 = 0.5 0.5\nscale.final = 0.5\n",
            &Macros::default(),
        );
        let post = load_post(
            &pack,
            "",
            &properties,
            &Macros::standard(),
            &GameInputs::default(),
        );
        let scales: Vec<(&str, [f32; 3])> = post
            .programs
            .iter()
            .map(|p| (p.name.as_str(), p.scale))
            .collect();
        assert_eq!(
            scales,
            [
                ("deferred", [0.5, 0.0, 0.0]),
                ("composite", [0.25, 0.5, 0.25]),
                // Not a scale, an offset missing, a program that draws the screen: all of it.
                ("composite1", FULL),
                ("composite2", FULL),
                ("composite3", FULL),
                ("final", FULL),
            ]
        );
    }

    #[test]
    fn image_declarations_get_their_format_once() {
        let csh = |decl: &str| {
            format!(
                "#version 430\nlayout(local_size_x = 1) in;\n{decl}\nvoid main() {{ imageStore(voxels, ivec3(0), uvec4(1u)); }}\n"
            )
        };
        let bare = csh("writeonly uniform uimage3D voxels;");
        let split = csh("layout(r32ui)\nwriteonly uniform uimage3D voxels;");
        let pack = Pack::from_zip_bytes(
            "P.zip",
            zip(&[
                ("shaders/composite.vsh", VSH),
                (
                    "shaders/composite.fsh",
                    "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n",
                ),
                ("shaders/composite.csh", &bare),
                ("shaders/composite_a.csh", &split),
            ]),
        )
        .unwrap();
        let properties = Properties::parse(
            "image.voxels = none RED_INTEGER R32UI UNSIGNED_INT false false 4 4 4\n",
            &Macros::standard(),
        );
        let post = load_post(
            &pack,
            "",
            &properties,
            &Macros::standard(),
            &GameInputs::default(),
        );
        let compute: Vec<&str> = post.programs[0]
            .compute
            .iter()
            .map(|c| c.0.as_str())
            .collect();
        assert_eq!(compute, ["composite", "composite_a"], "{:?}", post.notes);
    }

    /// Without RENDERTARGETS or DRAWBUFFERS, a program writes colortex0 only, whatever outputs it
    /// assigns, while the shadow program's outputs 0 and 1 still write shadowcolor0 and 1. The
    /// programs are those of probe pack p8107 run in Iris 1.11.7 (the PR has the readings).
    #[test]
    fn a_program_without_a_directive_writes_buffer_0_only() {
        let full = "#version 120\nvarying vec2 uv;\nvoid main() {\n\
                    gl_Position = ftransform();\nuv = gl_MultiTexCoord0.xy;\n}\n";
        let deferred = "#version 120\nuniform sampler2D colortex0;\nvarying vec2 uv;\nvoid main() {\n\
                        gl_FragData[0] = texture2D(colortex0, uv);\n\
                        gl_FragData[3] = vec4(1.1875);\ngl_FragData[7] = vec4(1.4375);\n}\n";
        let composite = "#version 330 compatibility\nuniform sampler2D colortex0;\nin vec2 uv;\n\
                         void main() {\ngl_FragData[0] = texture(colortex0, uv);\n\
                         gl_FragData[4] = vec4(2.25);\ngl_FragData[5] = vec4(2.3125);\n}\n";
        let composite1 = "#version 330 compatibility\nuniform sampler2D colortex0;\nin vec2 uv;\n\
                          layout(location = 0) out vec4 o0;\nlayout(location = 2) out vec4 o2;\n\
                          void main() {\no0 = texture(colortex0, uv);\no2 = vec4(3.125);\n}\n";
        let composite3 = "#version 330 compatibility\nuniform sampler2D colortex0;\nin vec2 uv;\n\
                          /* RENDERTARGETS: 0,12 */\nvoid main() {\n\
                          gl_FragData[0] = texture(colortex0, uv);\ngl_FragData[1] = vec4(6.5);\n}\n";
        let terrain_vsh = "#version 120\nvarying vec2 texcoord;\nvarying vec4 color;\nvoid main() {\n\
                           gl_Position = ftransform();\n\
                           texcoord = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;\ncolor = gl_Color;\n}\n";
        let terrain = "#version 120\nuniform sampler2D texture;\nvarying vec2 texcoord;\n\
                       varying vec4 color;\nvoid main() {\n\
                       vec4 c = texture2D(texture, texcoord) * color;\nif (c.a < 0.1) discard;\n\
                       gl_FragData[0] = c;\ngl_FragData[6] = vec4(5.0);\n}\n";
        let shadow_vsh = "#version 120\nvoid main() {\ngl_Position = ftransform();\n}\n";
        let shadow = "#version 120\nvoid main() {\n\
                      gl_FragData[0] = vec4(0.25);\ngl_FragData[1] = vec4(0.5);\n}\n";
        let prepare = "#version 120\nvoid main() {\n\
                       gl_FragData[0] = vec4(0.375);\ngl_FragData[2] = vec4(8.5);\n}\n";
        let shadowcomp = "#version 120\nvoid main() {\n\
                          gl_FragData[0] = vec4(0.75);\ngl_FragData[1] = vec4(0.875);\n}\n";
        let reads_shadow = "#version 120\nuniform sampler2D shadowcolor1;\nvarying vec2 uv;\n\
                            void main() { gl_FragColor = texture2D(shadowcolor1, uv); }\n";
        let pack = Pack::from_zip_bytes(
            "t",
            zip(&[
                ("shaders/deferred.vsh", full),
                ("shaders/deferred.fsh", deferred),
                ("shaders/composite.vsh", full),
                ("shaders/composite.fsh", composite),
                ("shaders/composite1.vsh", full),
                ("shaders/composite1.fsh", composite1),
                ("shaders/composite3.vsh", full),
                ("shaders/composite3.fsh", composite3),
                ("shaders/gbuffers_terrain.vsh", terrain_vsh),
                ("shaders/gbuffers_terrain.fsh", terrain),
                ("shaders/shadow.vsh", shadow_vsh),
                ("shaders/shadow.fsh", shadow),
                ("shaders/prepare.vsh", full),
                ("shaders/prepare.fsh", prepare),
                ("shaders/shadowcomp.vsh", full),
                ("shaders/shadowcomp.fsh", shadowcomp),
                ("shaders/final.vsh", full),
                ("shaders/final.fsh", reads_shadow),
            ]),
        )
        .unwrap();
        let game = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n    hz_Color = vec4(1.0);\n".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain: Some(game.clone()),
            entities: Some(game),
            ..Default::default()
        };
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &Macros::standard(),
            &inputs,
        );
        let written: Vec<(&str, &[u32])> = (post.programs.iter())
            .chain(&post.terrain)
            .chain(&post.shadow)
            .chain(&post.shadow_comp)
            .map(|p| (p.name.as_str(), &p.buffers[..]))
            .collect();
        assert_eq!(
            written,
            [
                ("prepare", &[0][..]),
                ("deferred", &[0]),
                ("composite", &[0]),
                ("composite1", &[0]),
                ("composite3", &[0, 12]),
                ("final", &[0]),
                ("gbuffers_terrain", &[0]),
                ("shadow", &[0, 1]),
                ("shadowcomp", &[0, 1]),
            ],
            "{:?}",
            post.notes
        );
    }

    /// The shadow pass runs when a program reads a shadow buffer or `shadow.enabled` says so;
    /// then `shadow`, `shadow_water` and the `shadowcomp` series load, writing shadowcolor0/1.
    #[test]
    fn shadow_programs_load_when_a_shadow_buffer_is_read() {
        let reads = "#version 120\nuniform sampler2D shadowtex0;\nvarying vec2 uv;\n\
                     void main() { gl_FragColor = texture2D(shadowtex0, uv); }\n";
        let shadow = "#version 120\nvarying vec2 uv;\nvoid main() {\n/* RENDERTARGETS: 1,0,4 */\n\
                      gl_FragData[0] = vec4(1.0); gl_FragData[1] = vec4(0.5); gl_FragData[2] = vec4(0.0);\n}\n";
        let files = [
            ("shaders/composite.vsh", VSH),
            ("shaders/composite.fsh", reads),
            ("shaders/shadow.vsh", VSH),
            ("shaders/shadow.fsh", shadow),
            ("shaders/shadowcomp1.vsh", VSH),
            ("shaders/shadowcomp1.fsh", FSH),
            ("shaders/shadow_block.vsh", VSH),
            ("shaders/shadow_block.fsh", FSH),
        ];
        let pack = Pack::from_zip_bytes("t", zip(&files)).unwrap();
        let game = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain: Some(game.clone()),
            entities: Some(game),
            ..Default::default()
        };
        let load = |properties: &str| {
            let properties = Properties::parse(properties, &Macros::default());
            load_post(&pack, "", &properties, &Macros::standard(), &inputs)
        };
        let post = load("");
        assert!(post.shadow_pass);
        let shadow = post.shadow.as_ref().expect("shadow");
        assert_eq!(
            (shadow.group, &shadow.buffers[..]),
            (Group::Shadow, &[1, 0][..])
        );
        // shadow_water falls back to shadow.
        assert_eq!(
            post.shadow_water.as_ref().map(|p| p.name.as_str()),
            Some("shadow")
        );
        // Entities fall back to shadow; block entities have their own program.
        let name = |p: &Option<PostProgram>| p.as_ref().map(|p| p.name.clone());
        assert_eq!(
            (name(&post.shadow_entities), name(&post.shadow_block)),
            (Some("shadow".into()), Some("shadow_block".into()))
        );
        let off = load("shadowEntities=false\nshadowBlockEntities = false");
        assert!(
            off.shadow.is_some() && off.shadow_entities.is_none() && off.shadow_block.is_none()
        );
        // shadowPlayer and shadowLightBlockEntities keep a part of each.
        let some = load(
            "shadowEntities=false\nshadowPlayer=true\nshadowBlockEntities=false\nshadowLightBlockEntities=true",
        );
        assert!(some.shadow_entities.is_some() && some.shadow_only_player);
        assert!(some.shadow_block.is_some() && some.shadow_only_lit_blocks);
        assert!(!post.shadow_only_player && !post.shadow_only_lit_blocks);
        let comp: Vec<_> = post
            .shadow_comp
            .iter()
            .map(|p| (p.name.as_str(), p.group))
            .collect();
        assert_eq!(comp, [("shadowcomp1", Group::ShadowComp)]);
        // DRAWBUFFERS:3 names no shadowcolor buffer.
        assert!(post.shadow_comp[0].buffers.is_empty());
        let post = load("shadow.enabled = false");
        assert!(!post.shadow_pass && post.shadow.is_none() && post.shadow_comp.is_empty());
        // Nothing reads a shadow buffer.
        let pack = Pack::from_zip_bytes("t", zip(&files[2..])).unwrap();
        let none = |properties: &str| {
            let properties = Properties::parse(properties, &Macros::default());
            load_post(&pack, "", &properties, &Macros::standard(), &inputs)
        };
        assert!(!none("").shadow_pass);
        assert!(none("shadow.enabled=true").shadow.is_some());
        let post = none("shadow.enabled=true\nshadowTranslucent=false");
        assert!(post.shadow.is_some() && post.shadow_water.is_none());
    }

    /// A compute stage that samples a shadow buffer turns the shadow pass on as a fragment stage
    /// does. Iris 1.11.7 with probe packs of ours (#10490), where nothing else read them: a
    /// `prepare.csh` alone sampling shadowtex0 read the terrain's depth there (0.3956 at our noon
    /// scene), a `composite.csh` beside a `composite.fsh` that reads none read the same, and a
    /// `prepare.csh` sampling shadowcolor0 read what `shadow.fsh` wrote (0.251); Horizon read an
    /// empty map (1.0) in all three.
    #[test]
    fn a_compute_stage_reading_a_shadow_buffer_turns_the_shadow_pass_on() {
        let csh = |sampler: &str| {
            format!(
                "#version 430\nlayout(local_size_x = 1) in;\nconst ivec3 workGroups = ivec3(1, 1, 1);\n\
                 uniform sampler2D {sampler};\nlayout(rgba8) writeonly uniform image2D colorimg5;\n\
                 void main() {{ imageStore(colorimg5, ivec2(0), texture({sampler}, vec2(0.5))); }}\n"
            )
        };
        let load = |files: &[(&str, &str)]| {
            let mut files = files.to_vec();
            files.extend([
                ("shaders/final.vsh", VSH),
                ("shaders/final.fsh", FSH),
                ("shaders/shadow.vsh", VSH),
                ("shaders/shadow.fsh", FSH),
            ]);
            let pack = Pack::from_zip_bytes("t", zip(&files)).unwrap();
            let properties = Properties::parse("", &Macros::default());
            load_post(
                &pack,
                "",
                &properties,
                &Macros::standard(),
                &GameInputs::default(),
            )
        };
        let prepare = csh("shadowtex0");
        let post = load(&[("shaders/prepare.csh", &prepare)]);
        assert!(
            post.programs.iter().any(|p| p.name == "prepare"),
            "{:?}",
            post.notes
        );
        assert!(post.shadow_pass, "{:?}", post.notes);
        let post = load(&[
            ("shaders/composite.vsh", VSH),
            ("shaders/composite.fsh", FSH),
            ("shaders/composite.csh", &prepare),
        ]);
        let composite = post.programs.iter().find(|p| p.name == "composite");
        assert_eq!(composite.map(|p| p.compute.len()), Some(1));
        assert!(post.shadow_pass, "{:?}", post.notes);
        let color = csh("shadowcolor0");
        assert!(load(&[("shaders/prepare.csh", &color)]).shadow_pass);
        // A compute stage that reads none leaves it off, as before.
        let colortex = csh("colortex1");
        assert!(!load(&[("shaders/prepare.csh", &colortex)]).shadow_pass);
    }

    /// `HIGHER_SHADOWCOLOR` (required or optional) gives the shadow programs shadowcolor2 to 7,
    /// with their own settings; a directive reaches them, no directive still writes 0 and 1.
    /// Without the feature, outputs stop at shadowcolor1 as before. As in Iris 1.11.7 with
    /// probe packs p7951a to c (the PR has the readings).
    #[test]
    fn higher_shadowcolor_gives_shadow_programs_eight_buffers() {
        let shadow_vsh = "#version 120\nvoid main() {\ngl_Position = ftransform();\n}\n";
        let writes = |targets: Option<&str>| {
            let directive =
                targets.map_or(String::new(), |t| format!("/* RENDERTARGETS: {t} */\n"));
            format!(
                "#version 120\nconst vec4 shadowcolor5ClearColor = vec4(0.3, 0.4, 0.5, 0.6);\n\
                 /* const int shadowcolor6Format = RGBA32F; */\n{directive}void main() {{\n\
                 gl_FragData[0] = vec4(0.0625);\ngl_FragData[1] = vec4(0.125);\n\
                 gl_FragData[2] = vec4(0.1875);\ngl_FragData[3] = vec4(0.25);\n\
                 gl_FragData[4] = vec4(0.3125);\n}}\n"
            )
        };
        let full = "#version 120\nvarying vec2 uv;\nvoid main() {\n\
                    gl_Position = ftransform();\nuv = gl_MultiTexCoord0.xy;\n}\n";
        let shadowcomp = "#version 120\n/* RENDERTARGETS: 7 */\n\
                          void main() {\ngl_FragData[0] = vec4(0.9375);\n}\n";
        let reads = "#version 120\nuniform sampler2D shadowcolor7;\nvarying vec2 uv;\n\
                     void main() { gl_FragColor = texture2D(shadowcolor7, uv); }\n";
        let game = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain: Some(game.clone()),
            entities: Some(game),
            ..Default::default()
        };
        let load = |properties: &str, targets: Option<&str>| {
            let shadow = writes(targets);
            let pack = Pack::from_zip_bytes(
                "t",
                zip(&[
                    ("shaders/shadow.vsh", shadow_vsh),
                    ("shaders/shadow.fsh", &shadow),
                    ("shaders/shadowcomp.vsh", full),
                    ("shaders/shadowcomp.fsh", shadowcomp),
                    ("shaders/final.vsh", full),
                    ("shaders/final.fsh", reads),
                ]),
            )
            .unwrap();
            let properties = Properties::parse(properties, &Macros::default());
            load_post(&pack, "", &properties, &Macros::standard(), &inputs)
        };
        let buffers = |post: &Post| {
            let shadow = post.shadow.as_ref().map(|p| p.buffers.clone());
            let comp: Vec<_> = post.shadow_comp.iter().map(|p| p.buffers.clone()).collect();
            (post.shadow_colors, shadow, comp)
        };
        let required = "iris.features.required = HIGHER_SHADOWCOLOR\n";
        let post = load(required, Some("0,1,2,3,4"));
        // Reading shadowcolor7 alone turns the shadow pass on.
        assert!(post.shadow_pass, "{:?}", post.notes);
        assert_eq!(
            buffers(&post),
            (8, Some(vec![0, 1, 2, 3, 4]), vec![vec![7]])
        );
        let s = &post.config.shadow;
        assert_eq!(s.colors[5].clear_color, Some([0.3, 0.4, 0.5, 0.6]));
        assert_eq!(s.colors[6].format, crate::Format::parse("RGBA32F").unwrap());
        assert_eq!(s.colors[7].clear_color, Some([1.0; 4]));
        let optional = "iris.features.optional = HIGHER_SHADOWCOLOR\n";
        let post = load(optional, Some("0,1,2,3,4"));
        assert_eq!(buffers(&post).1, Some(vec![0, 1, 2, 3, 4]));
        // Iris 1.11.7 takes the name in any case (#8996, a probe pack of our own).
        let post = load(
            "iris.features.optional = higher_shadowcolor\n",
            Some("0,1,2,3,4"),
        );
        assert_eq!(buffers(&post).1, Some(vec![0, 1, 2, 3, 4]));
        // No directive: shadowcolor0 and 1, as without the feature.
        let post = load(required, None);
        assert_eq!(buffers(&post).1, Some(vec![0, 1]));
        // Without the feature: two buffers; shadowcolor7 doesn't turn the shadow pass on, and
        // with it forced on, RENDERTARGETS: 7 writes nothing.
        let post = load("", Some("0,1,2,3,4"));
        assert!(!post.shadow_pass);
        let post = load("shadow.enabled = true\n", Some("0,1,2,3,4"));
        assert_eq!(buffers(&post), (2, Some(vec![0, 1]), vec![vec![]]));
    }

    /// A pack without a shadow pass still takes `shadow.fsh`'s shadow settings, not
    /// `shadow.vsh`'s: Iris 1.11.7 applied `shadowDistance` and `sunPathRotation` from the first
    /// (shadow projection, sun position) and not from the second, with probe packs of ours whose
    /// shadow buffers nothing read (#10490).
    #[test]
    fn shadow_fsh_settings_count_without_a_shadow_pass() {
        let settings = "const float shadowDistance = 48.0;\nconst float sunPathRotation = 20.0;\n";
        let load = |vsh: &str, fsh: &str| {
            let files = [
                ("shaders/final.vsh", VSH),
                ("shaders/final.fsh", FSH),
                ("shaders/shadow.vsh", vsh),
                ("shaders/shadow.fsh", fsh),
            ];
            let pack = Pack::from_zip_bytes("t", zip(&files)).unwrap();
            let properties = Properties::parse("", &Macros::default());
            load_post(
                &pack,
                "",
                &properties,
                &Macros::standard(),
                &GameInputs::default(),
            )
        };
        let with = |text: &str, file: &str| format!("#version 120\n{text}{}", &file[13..]);
        let post = load(VSH, &with(settings, FSH));
        assert!(!post.shadow_pass);
        let s = &post.config.shadow;
        assert_eq!((s.distance, s.sun_path_rotation), (48.0, 20.0));
        let post = load(&with(settings, VSH), FSH);
        let (s, d) = (&post.config.shadow, crate::config::Shadow::default());
        assert_eq!(
            (s.distance, s.sun_path_rotation),
            (d.distance, d.sun_path_rotation)
        );
    }

    /// A shadow program with a geometry stage draws terrain, entities and block entities, each
    /// read vertex by vertex as the game says; a draw whose vertices can't be read so skips it.
    #[test]
    fn a_shadow_geometry_stage_draws_entities_and_block_entities() {
        let reads = "#version 120\nuniform sampler2D shadowtex0;\nvarying vec2 uv;\n\
                     void main() { gl_FragColor = texture2D(shadowtex0, uv); }\n";
        let vsh = "#version 330\nout vec2 vUv;\nvoid main() { gl_Position = ftransform(); vUv = vec2(0.0); }\n";
        let gsh = "#version 330\nlayout(triangles) in;\nlayout(triangle_strip, max_vertices = 3) out;\n\
                   in vec2 vUv[];\nout vec2 gUv;\nvoid main() {\n    for (int i = 0; i < 3; i++) {\n        \
                   gl_Position = gl_in[i].gl_Position;\n        gUv = vUv[i];\n        EmitVertex();\n    }\n    \
                   EndPrimitive();\n}\n";
        let fsh = "#version 330\nin vec2 gUv;\nlayout(location = 0) out vec4 c;\nvoid main() { c = vec4(gUv, 0.0, 1.0); }\n";
        let files = [
            ("shaders/composite.vsh", VSH),
            ("shaders/composite.fsh", reads),
            ("shaders/shadow.vsh", vsh),
            ("shaders/shadow.gsh", gsh),
            ("shaders/shadow.fsh", fsh),
        ];
        let pack = Pack::from_zip_bytes("t", zip(&files)).unwrap();
        let game = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;\n\
                           layout(set = 1, binding = 0) readonly buffer V { uint v[]; };\n"
                .into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n".into(),
            ..Default::default()
        };
        let geometry = crate::GeometryInputs {
            inputs: game.clone(),
            pull: "        hz_v_pos = uintBitsToFloat(uvec3(v[hz_vertex_index], v[hz_vertex_index], v[hz_vertex_index]));\n"
                .into(),
            index: "uint(@)".into(),
        };
        let mut inputs = GameInputs {
            terrain: Some(game.clone()),
            entities: Some(game),
            terrain_geometry: Some(geometry.clone()),
            entity_geometry: Some(geometry),
            ..Default::default()
        };
        let load = |inputs: &GameInputs| {
            load_post(
                &pack,
                "",
                &Properties::default(),
                &Macros::standard(),
                inputs,
            )
        };
        let post = load(&inputs);
        let triangles = |p: &Option<PostProgram>| p.as_ref().map(|p| p.geometry_triangles);
        assert_eq!(
            [&post.shadow, &post.shadow_entities, &post.shadow_block].map(triangles),
            [Some(1); 3],
            "{:?}",
            post.notes
        );
        inputs.entity_geometry = None;
        let post = load(&inputs);
        assert!(post.shadow.is_some());
        assert!(post.shadow_entities.is_none() && post.shadow_block.is_none());
        assert!(
            post.notes
                .iter()
                .any(|n| n.starts_with("shadow: uses a geometry stage")),
            "{:?}",
            post.notes
        );
    }

    /// Water falls back to the terrain chain and blends by default, as the sky does; entities
    /// and block entities fall back along theirs; `blend.*` overrides.
    /// `clouds=off` hides the game's clouds and drops the pack's own cloud program.
    #[test]
    fn clouds_off_draws_no_clouds() {
        let vsh = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";
        let fsh = "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n";
        let pack = Pack::from_zip_bytes(
            "clouds.zip",
            crate::pack::tests::zip(&[
                ("shaders/gbuffers_clouds.vsh", vsh),
                ("shaders/gbuffers_clouds.fsh", fsh),
            ]),
        )
        .unwrap();
        let macros = Macros::standard();
        let inputs = GameInputs {
            clouds: Some(crate::GbuffersInputs {
                declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
                body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let on = load_post(&pack, "", &Properties::default(), &macros, &inputs);
        assert!(on.clouds.is_some() && !on.clouds_off);
        let off = Properties::parse("clouds = off\n", &macros);
        let off = load_post(&pack, "", &off, &macros, &inputs);
        assert!(off.clouds.is_none() && off.clouds_off);
    }

    /// Without `gbuffers_weather` or the textured programs, rain and snow draw with
    /// `gbuffers_basic`, blended, whatever `rain.depth` says.
    #[test]
    fn weather_falls_back_to_basic_whatever_rain_depth_says() {
        let pack = Pack::from_zip_bytes(
            "original-weather-depth",
            crate::pack::tests::zip(&[
                (
                    "shaders/gbuffers_basic.vsh",
                    "#version 120\nvoid main(){gl_Position=vec4(0.0,0.0,0.0,1.0);}",
                ),
                (
                    "shaders/gbuffers_basic.fsh",
                    "#version 120\nvoid main(){gl_FragColor=vec4(1.0);}",
                ),
            ]),
        )
        .unwrap();
        let macros = Macros::standard();
        let inputs = GameInputs {
            weather: Some(GbuffersInputs::default()),
            ..Default::default()
        };
        for text in ["", "rain.depth = true", "rain.depth = false"] {
            let properties = Properties::parse(text, &macros);
            let post = load_post(&pack, "", &properties, &macros, &inputs);
            let weather = post.weather.expect("weather fallback loads");
            assert_eq!(weather.name, "gbuffers_basic");
            assert_eq!(weather.group, Group::Translucent);
            assert_eq!(weather.blend, Some(TRANSLUCENT_BLEND));
        }
    }

    /// Without `gbuffers_clouds` the clouds draw with `gbuffers_textured`, else `gbuffers_basic`,
    /// blended; a pack with none of them leaves the clouds to the game.
    #[test]
    fn clouds_fall_back_to_textured_then_basic() {
        let vsh = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";
        let fsh = "#version 120\nvoid main() { gl_FragColor = vec4(1.0); }\n";
        let macros = Macros::standard();
        let inputs = GameInputs {
            clouds: Some(crate::GbuffersInputs {
                declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
                body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let clouds = |programs: &[&str]| {
            let files: Vec<(String, &str)> = programs
                .iter()
                .flat_map(|p| {
                    [
                        (format!("shaders/{p}.vsh"), vsh),
                        (format!("shaders/{p}.fsh"), fsh),
                    ]
                })
                .collect();
            let files: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), *t)).collect();
            let pack = Pack::from_zip_bytes("clouds.zip", crate::pack::tests::zip(&files)).unwrap();
            load_post(&pack, "", &Properties::default(), &macros, &inputs)
                .clouds
                .map(|p| (p.name, p.blend))
        };
        let blended = |name: &str| Some((name.to_owned(), Some(TRANSLUCENT_BLEND)));
        assert_eq!(
            clouds(&["gbuffers_textured", "gbuffers_basic"]),
            blended("gbuffers_textured")
        );
        assert_eq!(
            clouds(&["gbuffers_textured_lit", "gbuffers_basic"]),
            blended("gbuffers_basic")
        );
        assert_eq!(
            clouds(&["gbuffers_clouds", "gbuffers_textured"]),
            blended("gbuffers_clouds")
        );
        assert_eq!(clouds(&["gbuffers_terrain", "composite"]), None);
    }

    const TERRAIN_VSH: &str = "#version 120\nvoid main() { gl_Position = ftransform(); }\n";
    const TERRAIN_FSH: &str = "#version 120\nvoid main() { gl_FragData[0] = vec4(1.0); }\n";

    /// The pack of `files` (under `shaders/`), with `properties`, loaded with the game's terrain
    /// inputs and, with `parts`, its two parts.
    fn terrain_files(files: &[(String, &str)], properties: &str, parts: bool) -> Post {
        let files: Vec<_> = (files.iter())
            .map(|(name, source)| (format!("shaders/{name}"), *source))
            .collect();
        let files: Vec<_> = files.iter().map(|(n, s)| (n.as_str(), *s)).collect();
        let pack = Pack::from_zip_bytes("terrain-parts", crate::pack::tests::zip(&files)).unwrap();
        let inputs = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n    hz_Color = vec4(1.0);\n".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain_parts: parts.then(|| [inputs.clone(), inputs.clone()]),
            terrain: Some(inputs),
            ..Default::default()
        };
        let macros = Macros::standard();
        let properties = Properties::parse(properties, &macros);
        load_post(&pack, "", &properties, &macros, &inputs)
    }

    /// `terrain_files` with `programs`, each writing white.
    fn terrain_pack(programs: &[&str], properties: &str, parts: bool) -> Post {
        let files: Vec<_> = (programs.iter())
            .flat_map(|p| {
                [
                    (format!("{p}.vsh"), TERRAIN_VSH),
                    (format!("{p}.fsh"), TERRAIN_FSH),
                ]
            })
            .collect();
        terrain_files(&files, properties, parts)
    }

    /// What draws solid (or all) terrain and what draws cutout terrain.
    fn terrain_names(programs: &[&str], properties: &str, parts: bool) -> [Option<String>; 2] {
        let post = terrain_pack(programs, properties, parts);
        [post.terrain, post.terrain_cutout].map(|p| p.map(|p| p.name))
    }

    const TERRAIN_PROGRAMS: [&str; 3] = [
        "gbuffers_terrain",
        "gbuffers_terrain_solid",
        "gbuffers_terrain_cutout",
    ];

    /// Iris 1.11.7 with our probe packs (#9137): `gbuffers_terrain_solid` draws solid terrain
    /// and `gbuffers_terrain_cutout` cutout terrain, each falling back to `gbuffers_terrain`;
    /// a pack with only `gbuffers_terrain` draws both parts with it, each with its own alpha
    /// test (#9307).
    #[test]
    fn terrain_solid_and_cutout_fall_back_to_terrain() {
        let names = |programs: &[&str], parts: bool| terrain_names(programs, "", parts);
        let some = |name: &str| Some(name.to_owned());
        let [terrain, solid, cutout] = TERRAIN_PROGRAMS;
        assert_eq!(
            names(&[terrain, solid, cutout], true),
            [some(solid), some(cutout)]
        );
        assert_eq!(names(&[terrain, solid], true), [some(solid), some(terrain)]);
        assert_eq!(
            names(&[terrain, cutout], true),
            [some(terrain), some(cutout)]
        );
        assert_eq!(names(&[terrain], true), [some(terrain), some(terrain)]);
        // Without the game's two parts, terrain draws whole.
        assert_eq!(
            names(&[terrain, solid, cutout], false),
            [some(terrain), None]
        );
    }

    /// A pack whose only terrain program is `gbuffers_terrain_solid` (or `_cutout`) has nothing
    /// for the other part: that part draws alone, and nothing draws the other, as Iris 1.11.7
    /// did with our probes `tsc-solidonly` and `tsc-cutoutonly`.
    #[test]
    fn a_lone_split_program_draws_only_its_part() {
        let [_, solid, cutout] = TERRAIN_PROGRAMS;
        let some = |name: &str| Some(name.to_owned());
        assert_eq!(terrain_names(&[solid], "", true), [some(solid), None]);
        assert_eq!(terrain_names(&[cutout], "", true), [None, some(cutout)]);
        let post = terrain_pack(&[solid], "", true);
        let note = "gbuffers_terrain_solid: no program for cutout terrain; nothing draws it";
        assert!(post.notes.iter().any(|n| n == note), "{:?}", post.notes);
    }

    /// When the other part has a program that fails to load, the part that loaded doesn't draw
    /// alone: the game draws all terrain (#9176's review).
    #[test]
    fn a_split_part_that_fails_to_load_takes_the_other_with_it() {
        let [_, solid, cutout] = TERRAIN_PROGRAMS;
        // `cutout` with a stage that doesn't translate.
        let files = [
            (format!("{solid}.vsh"), TERRAIN_VSH),
            (format!("{solid}.fsh"), TERRAIN_FSH),
            (format!("{cutout}.vsh"), TERRAIN_VSH),
            (
                format!("{cutout}.fsh"),
                "void main() { gl_FragData[0] = vec4(1.0) }\n",
            ),
        ];
        let post = terrain_files(&files, "", true);
        assert!(post.terrain.is_none() && post.terrain_cutout.is_none());
        let note = "gbuffers_terrain_solid: cutout terrain's program didn't load; the game draws all terrain";
        assert!(post.notes.iter().any(|n| n == note), "{:?}", post.notes);
    }

    /// `program.<name>.enabled` turning off the only split program leaves both parts to
    /// `gbuffers_terrain`, as for a pack without it: Iris 1.11.7 drew all terrain with
    /// `gbuffers_terrain` for our probe `tsc-disabled-false`.
    #[test]
    fn a_disabled_split_program_leaves_terrain_whole() {
        let [terrain, solid, _] = TERRAIN_PROGRAMS;
        let off = "program.gbuffers_terrain_solid.enabled = false\n";
        let some = |name: &str| Some(name.to_owned());
        assert_eq!(
            terrain_names(&[terrain, solid], off, true),
            [some(terrain), some(terrain)]
        );
        let on = "program.gbuffers_terrain_solid.enabled = true\n";
        assert_eq!(
            terrain_names(&[terrain, solid], on, true),
            [some(solid), some(terrain)]
        );
    }

    /// The cutout part's storage blocks count towards the buffers' sizes, like every other
    /// program's.
    #[test]
    fn the_cutout_parts_storage_blocks_size_their_buffers() {
        let fsh = "#version 430\nlayout(std430, binding = 0) buffer Sums { vec4 a[4]; } sums;\n\
                   void main() {\n/* DRAWBUFFERS:0 */\ngl_FragData[0] = sums.a[3];\n}\n";
        let files = [
            ("gbuffers_terrain.vsh".into(), TERRAIN_VSH),
            ("gbuffers_terrain.fsh".into(), TERRAIN_FSH),
            ("gbuffers_terrain_cutout.vsh".into(), TERRAIN_VSH),
            ("gbuffers_terrain_cutout.fsh".into(), fsh),
        ];
        let post = terrain_files(&files, "bufferObject.0 = 16\n", true);
        let cutout = post.terrain_cutout.as_ref().map(|p| p.name.as_str());
        assert_eq!(cutout, Some("gbuffers_terrain_cutout"), "{:?}", post.notes);
        assert_eq!(
            post.buffer_minimums(),
            std::collections::BTreeMap::from([(0, 64)])
        );
    }

    /// A pack's own `gbuffers_hand_water` draws the translucent held items: after deferred,
    /// blended, beside its opaque `gbuffers_hand`.
    #[test]
    fn translucent_held_items_draw_with_gbuffers_hand_water() {
        let vsh = "#version 120\nvarying vec2 uv;\nvoid main() {\n    gl_Position = ftransform();\n    uv = gl_MultiTexCoord0.xy;\n}\n";
        let fsh = "#version 120\nuniform sampler2D texture;\nvarying vec2 uv;\nvoid main() {\n    gl_FragData[0] = texture2D(texture, uv);\n}\n";
        let pack = Pack::from_zip_bytes(
            "P.zip",
            zip(&[
                ("shaders/gbuffers_hand.vsh", vsh),
                ("shaders/gbuffers_hand.fsh", fsh),
                ("shaders/gbuffers_hand_water.vsh", vsh),
                ("shaders/gbuffers_hand_water.fsh", fsh),
            ]),
        )
        .unwrap();
        let hand = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n    hz_Color = vec4(1.0);\n".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            hand: Some(hand),
            ..Default::default()
        };
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &Macros::standard(),
            &inputs,
        );
        let (opaque, translucent) = (post.hand.unwrap(), post.hand_water.unwrap());
        assert_eq!(
            (opaque.name.as_str(), opaque.blend),
            ("gbuffers_hand", None)
        );
        assert_eq!(
            (
                translucent.name.as_str(),
                translucent.group,
                translucent.blend
            ),
            (
                "gbuffers_hand_water",
                Group::Translucent,
                Some(TRANSLUCENT_BLEND)
            )
        );
    }

    #[test]
    fn water_falls_back_to_terrain_and_blends() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/packs/terrain-test");
        let pack = Pack::open(&dir).unwrap();
        let macros = Macros::standard();
        let inputs = crate::GbuffersInputs {
            declarations: "layout(location = 0) in vec3 hz_v_pos;".into(),
            body: "    hz_Vertex = vec4(hz_v_pos, 1.0);\n    hz_Color = vec4(1.0);\n".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain: Some(inputs.clone()),
            sky: Some(inputs.clone()),
            celestial: Some(inputs.clone()),
            entities: Some(inputs.clone()),
            clouds: Some(inputs.clone()),
            weather: Some(inputs.clone()),
            lines: Some(inputs.clone()),
            hand: Some(inputs),
            terrain_parts: None,
            terrain_geometry: None,
            entity_geometry: None,
        };
        let post = load_post(&pack, "", &Properties::default(), &macros, &inputs);
        let (terrain, water) = (post.terrain.unwrap(), post.water.unwrap());
        assert_eq!((terrain.group, terrain.blend), (Group::Gbuffers, None));
        assert_eq!(water.name, "gbuffers_terrain");
        let sky = post.sky.unwrap();
        assert_eq!(
            (sky.name.as_str(), sky.group, sky.blend),
            (
                "gbuffers_skybasic",
                Group::Gbuffers,
                Some(TRANSLUCENT_BLEND)
            )
        );
        // Its parts blend as vanilla's: the stars with BlendFunction.OVERLAY.
        assert!(sky.vanilla_sky_blends);
        assert_eq!(
            STARS_BLEND,
            [Factor::SrcAlpha, Factor::One, Factor::One, Factor::Zero]
        );
        // gbuffers_skytextured falls back to gbuffers_textured, then gbuffers_basic.
        let sun = post.sky_textured.unwrap();
        assert_eq!(
            (sun.name.as_str(), sun.blend),
            ("gbuffers_textured", Some(CELESTIAL_BLEND))
        );
        // Vanilla's `BlendFunction.OVERLAY`: colour added, alpha the sun's own.
        assert_eq!(
            sun.blend,
            Some([Factor::SrcAlpha, Factor::One, Factor::One, Factor::Zero])
        );
        assert_eq!(
            (water.group, water.blend),
            (Group::Translucent, Some(TRANSLUCENT_BLEND))
        );
        // Entities fall back to gbuffers_textured, block entities to gbuffers_terrain; both
        // draw without blending.
        let (entities, block) = (post.entities.unwrap(), post.block.unwrap());
        assert_eq!(
            (entities.name.as_str(), entities.blend),
            ("gbuffers_textured", None)
        );
        assert_eq!(
            (block.name.as_str(), block.blend),
            ("gbuffers_terrain", None)
        );
        // Translucent entity parts and particles blend; particles draw after deferred unless
        // the pack says otherwise, translucent entities before unless `separateEntityDraws`.
        let (translucent, particles) = (
            post.entities_translucent.as_ref().unwrap(),
            post.particles.as_ref().unwrap(),
        );
        assert_eq!(
            (translucent.name.as_str(), translucent.blend),
            ("gbuffers_textured", Some(TRANSLUCENT_BLEND))
        );
        assert_eq!(particles.blend, Some(TRANSLUCENT_BLEND));
        assert!(post.particles_late && !post.translucent_late);
        // Clouds fall back to gbuffers_textured, blended.
        let clouds = post.clouds.as_ref().unwrap();
        assert_eq!(
            (clouds.name.as_str(), clouds.blend),
            ("gbuffers_textured", Some(TRANSLUCENT_BLEND))
        );
        // The hand falls back to gbuffers_textured and does not blend; its translucent items
        // fall back to the same program, after deferred and blended.
        let hand = post.hand.as_ref().unwrap();
        assert_eq!(
            (hand.name.as_str(), hand.group, hand.blend),
            ("gbuffers_textured", Group::Gbuffers, None)
        );
        let hand_water = post.hand_water.as_ref().unwrap();
        assert_eq!(
            (hand_water.name.as_str(), hand_water.group, hand_water.blend),
            (
                "gbuffers_textured",
                Group::Translucent,
                Some(TRANSLUCENT_BLEND)
            )
        );
        // Weather falls back to gbuffers_textured, after deferred, blended.
        let weather = post.weather.as_ref().unwrap();
        assert_eq!(
            (weather.name.as_str(), weather.group, weather.blend),
            (
                "gbuffers_textured",
                Group::Translucent,
                Some(TRANSLUCENT_BLEND)
            )
        );
        let ordering = Properties::parse(
            "particles.ordering = before\nseparateEntityDraws = true\n",
            &macros,
        );
        let post = load_post(&pack, "", &ordering, &macros, &inputs);
        assert!(!post.particles_late && post.translucent_late);
        let properties = Properties::parse(
            "blend.gbuffers_terrain = ONE ONE ZERO ONE\nblend.gbuffers_water = off\n",
            &macros,
        );
        let terrain_blend = blend(&properties, "gbuffers_terrain");
        assert_eq!(
            terrain_blend,
            Some(Some([Factor::One, Factor::One, Factor::Zero, Factor::One]))
        );
        assert_eq!(blend(&properties, "gbuffers_water"), Some(None));
        assert_eq!(blend(&properties, "gbuffers_basic"), None);
    }

    #[test]
    fn formats_come_from_the_fragment_shaders_iris_reads() {
        // Cut down from probe packs p9063a, p9063b and p9063c (ours). Iris 1.11.7: colortex8,
        // declared only in `composite.vsh`, kept RGBA8; colortex12 and colortex6, declared only
        // in `shadow.fsh` and `shadowcomp.fsh` of a pack whose shadow pass Horizon doesn't run,
        // took RGBA16F; colortex1 with `gdepth` declared and no format kept RGBA8. Other
        // settings of those shadow files weren't probed, and Horizon leaves them out.
        let declare = |n: u32| format!("/*\nconst int colortex{n}Format = RGBA16F;\n*/\n");
        let write = "#version 120\n/* RENDERTARGETS: 1,6,8,12 */\nvoid main() { gl_FragData[0] = \
                     vec4(2.5); gl_FragData[1] = vec4(2.5); gl_FragData[2] = vec4(2.5); \
                     gl_FragData[3] = vec4(2.5); }\n";
        let pack = Pack::from_zip_bytes(
            "original-format-files",
            zip(&[
                (
                    "shaders/composite.vsh",
                    format!("{}{VSH}", declare(8)).as_str(),
                ),
                ("shaders/composite.fsh", write),
                ("shaders/shadow.vsh", VSH),
                (
                    "shaders/shadow.fsh",
                    format!(
                        "{}const bool colortex12Clear = false;\n{write}",
                        declare(12)
                    )
                    .as_str(),
                ),
                ("shaders/shadowcomp.vsh", VSH),
                (
                    "shaders/shadowcomp.fsh",
                    format!("{}{write}", declare(6)).as_str(),
                ),
                ("shaders/final.vsh", VSH),
                (
                    "shaders/final.fsh",
                    "#version 120\nuniform sampler2D gdepth;\nvarying vec2 uv;\n\
                     void main() { gl_FragColor = texture2D(gdepth, uv); }\n",
                ),
            ]),
        )
        .unwrap();
        let macros = Macros::standard();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &macros,
            &GameInputs::default(),
        );
        assert!(!post.shadow_pass);
        let half = crate::Format::parse("RGBA16F").unwrap();
        let formats: Vec<bool> = [1, 6, 8, 12]
            .iter()
            .map(|&n| post.config.buffers[n].format == half)
            .collect();
        assert_eq!(formats, [false, true, false, true]);
        assert_eq!(post.config.buffers[1].format, crate::Format::RGBA8);
        assert!(post.config.buffers[12].clear);
    }

    #[test]
    fn horizons_test_pack_loads() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/packs/post-test");
        let pack = Pack::open(&dir).unwrap();
        let macros = Macros::standard();
        let properties = Properties::parse(&pack.read_text("shaders.properties").unwrap(), &macros);
        let post = load_post(
            &pack,
            &dimension_folder(&pack, 0),
            &properties,
            &macros,
            &GameInputs::default(),
        );
        assert!(post.notes.is_empty(), "{:?}", post.notes);
        let names: Vec<&str> = post.programs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["composite", "composite1", "final"]);
        assert_eq!(post.programs[0].buffers, [0, 2]);
        assert_eq!(post.programs[1].buffers, [0]);
        assert_eq!(
            post.config.buffers[2].format,
            crate::Format::parse("RGBA16F").unwrap()
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn setup_and_begin_compute_load_before_prepare_even_with_only_a_suffix() {
        let compute = "#version 430\nlayout(local_size_x=1) in; void main(){}";
        let zip = crate::pack::tests::zip(&[
            ("shaders/setup.csh", compute),
            ("shaders/begin_a.csh", compute),
            ("shaders/prepare.csh", compute),
        ]);
        let pack = Pack::from_zip_bytes("lifecycle.zip", zip).unwrap();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &Macros::standard(),
            &GameInputs::default(),
        );
        assert!(post.notes.is_empty(), "{:?}", post.notes);
        assert_eq!(
            post.programs
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["setup", "begin", "prepare"]
        );
    }
}

#[cfg(test)]
mod buffer_blend_tests {
    use super::*;
    #[test]
    fn buffer_blends_use_colortex_numbers_and_preserve_explicit_off() {
        let props = Properties::parse(
            "blend.gbuffers_water=off\nblend.gbuffers_water.colortex13=ONE ONE_MINUS_SRC_ALPHA ONE ONE_MINUS_SRC_ALPHA\nblend.gbuffers_water.colortex2=off\n",
            &Macros::standard(),
        );
        let overrides = buffer_blends(&props, "gbuffers_water");
        assert_eq!(blend(&props, "gbuffers_water"), Some(None));
        assert_eq!(
            overrides,
            vec![
                (
                    13,
                    Some([
                        Factor::One,
                        Factor::OneMinusSrcAlpha,
                        Factor::One,
                        Factor::OneMinusSrcAlpha
                    ])
                ),
                (2, None)
            ]
        );
    }
}

#[cfg(test)]
mod material_variant_tests;
