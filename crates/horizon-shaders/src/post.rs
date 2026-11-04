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

use crate::config::{self, Config};
use crate::custom::Custom;
use crate::{
    ComputeProgram, preprocess, translate_composite, translate_compute, translate_gbuffers,
    translate_shadow,
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

/// `gbuffers_clouds`: no fallback, the game draws its clouds otherwise (OQ-3).
const CLOUDS: [&str; 1] = ["gbuffers_clouds"];

/// `gbuffers_hand` and its fallbacks (2.3).
const HAND: [&str; 4] = [
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

/// The sun and moon's default blend: added to the sky, as vanilla draws them.
pub const CELESTIAL_BLEND: Blend = [
    Factor::SrcAlpha,
    Factor::One,
    Factor::One,
    Factor::OneMinusSrcAlpha,
];

const GROUPS: [(Group, &str); 6] = [
    (Group::Setup, "setup"),
    (Group::Begin, "begin"),
    (Group::Prepare, "prepare"),
    (Group::Deferred, "deferred"),
    (Group::Composite, "composite"),
    (Group::Final, "final"),
];

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
        let valid = match size {
            ImageSize::Relative(s) => s.iter().all(|v| v.is_finite() && *v > 0.0),
            ImageSize::Absolute(s) => s.iter().all(|v| *v > 0),
            ImageSize::Volume(s) => s.iter().all(|v| *v > 0),
        };
        if !valid || !matches!(*clear, "true" | "false") {
            return None;
        }
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

pub struct Post {
    /// False when a required pass or resource failed. Do not run a partial effect.
    pub complete: bool,
    /// The folder the programs came from: `""` for `shaders` itself, else `world0`...
    pub folder: String,
    pub programs: Vec<PostProgram>,
    /// The flips of the virtual `deferred_pre` and `composite_pre` (6.1.6).
    pub deferred_pre: Vec<(u32, bool)>,
    pub composite_pre: Vec<(u32, bool)>,
    /// What draws solid and cutout terrain, when the game asked for it and one loads.
    pub terrain: Option<PostProgram>,
    /// What draws translucent terrain (water, stained glass) after deferred.
    pub water: Option<PostProgram>,
    /// What draws the sky (disc, sunset, stars, void) before terrain.
    pub sky: Option<PostProgram>,
    /// What draws the sun and moon, between the sunset and the stars.
    pub sky_textured: Option<PostProgram>,
    /// What draws entities, after terrain.
    pub entities: Option<PostProgram>,
    /// What draws block entities, after terrain.
    pub block: Option<PostProgram>,
    /// What draws translucent entities (name tags, shadows), after the others.
    pub entities_translucent: Option<PostProgram>,
    /// What draws particles.
    pub particles: Option<PostProgram>,
    /// What draws beacon and End gateway beams (after deferred, blended).
    pub beacon_beam: Option<PostProgram>,
    pub damaged_block: Option<PostProgram>,
    /// What draws the game's clouds.
    pub clouds: Option<PostProgram>,
    /// What draws lines (the block outline), with the pack's own transform and look.
    pub lines: Option<PostProgram>,
    /// What draws rain and snow, after deferred.
    pub weather: Option<PostProgram>,
    /// What draws the first-person hand and held items.
    pub hand: Option<PostProgram>,
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
            &self.water,
            &self.sky,
            &self.sky_textured,
            &self.entities,
            &self.block,
            &self.entities_translucent,
            &self.particles,
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
        complete: true,
        folder: folder.to_owned(),
        programs: Vec::new(),
        deferred_pre: flips(properties, "deferred_pre"),
        composite_pre: flips(properties, "composite_pre"),
        terrain: None,
        water: None,
        sky: None,
        sky_textured: None,
        entities: None,
        block: None,
        entities_translucent: None,
        particles: None,
        beacon_beam: None,
        damaged_block: None,
        clouds: None,
        lines: None,
        weather: None,
        hand: None,
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
    post.complete = post.notes.is_empty();
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
            if let Some(mut p) =
                loader.load(name, group, |v, f, _| translate_composite(v, f, target))
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
    let mut gbuffers = |names: &[&str], group, inputs: &GbuffersInputs| {
        names.iter().find_map(|name| {
            // `alphaTest.<program>` replaces the geometry's default test (OQ-23).
            let key = properties.get(&format!("alphaTest.{name}"));
            let test = key.and_then(|v| crate::AlphaTest::parse(v.trim()));
            let mut inputs = inputs.clone();
            if let Some(test) = test {
                inputs.alpha_test = test;
            }
            loader.load(name.to_string(), group, |v, f, config| {
                apply_terrain_lighting(&mut inputs, properties, config);
                translate_gbuffers(v, f, Target::Buffers, &inputs)
            })
        })
    };
    // The sky blends as vanilla draws its sunset fan and stars unless `blend.*` says otherwise.
    let sky = inputs
        .sky
        .as_ref()
        .and_then(|i| gbuffers(&SKY, Group::Gbuffers, i));
    let sky = sky.map(|mut p| {
        p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
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
    if let Some(inputs) = &inputs.terrain {
        // Solid and cutout terrain draw together: the cutout test, unless `alphaTest.*` says
        // otherwise (3.2).
        let mut cutout = inputs.clone();
        cutout.alpha_test = cutout.alpha_test.or(Some(crate::AlphaTest {
            op: ">",
            reference: 0.1,
        }));
        let terrain = gbuffers(&TERRAIN, Group::Gbuffers, &cutout);
        // gbuffers_water falls back to gbuffers_terrain's chain (2.3).
        let water = gbuffers(&WATER, Group::Translucent, inputs);
        loader.post.terrain = terrain;
        loader.post.water = water.map(|mut p| {
            p.blend = blend(properties, &p.name).unwrap_or(Some(TRANSLUCENT_BLEND));
            p
        });
        if let Some(p) = &mut loader.post.terrain {
            p.blend = blend(properties, &p.name).flatten();
        }
    }
    loader.post.sky = sky;
    loader.post.sky_textured = sky_textured;
    loader.post.entities = entities;
    loader.post.block = block;
    loader.post.entities_translucent = entities_translucent;
    loader.post.particles = particles;
    loader.post.beacon_beam = beacon_beam;
    loader.post.damaged_block = damaged_block;
    loader.post.clouds_off = properties.get("clouds").is_some_and(|v| v.trim() == "off");
    loader.post.clouds = clouds.filter(|_| !loader.post.clouds_off);
    loader.post.lines = lines;
    loader.post.weather = weather;
    loader.post.hand = hand;
    let flag = |key: &str| properties.get(key).map(|v| v.trim() == "true");
    loader.post.translucent_late = flag("separateEntityDraws").unwrap_or(false);
    // `after` when the pack has a deferred program, else `mixed`; ours are all translucent, so
    // `mixed` draws them after deferred too.
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
    // Missing optional programs retain vanilla geometry. A pack that actually
    // supplies a stage must have a usable program from that stage's fallback chain.
    let switches = crate::switches_on(pack);
    let chains: [(&str, &[&str], bool); 11] = [
        ("terrain", &TERRAIN, post.terrain.is_some()),
        ("water", &WATER, post.water.is_some()),
        ("sky", &SKY, post.sky.is_some()),
        ("celestial", &SKY_TEXTURED, post.sky_textured.is_some()),
        ("entities", &ENTITIES, post.entities.is_some()),
        ("block entities", &BLOCK, post.block.is_some()),
        (
            "translucent entities",
            &ENTITIES_TRANSLUCENT,
            post.entities_translucent.is_some(),
        ),
        ("particles", &PARTICLES, post.particles.is_some()),
        ("clouds", &CLOUDS, post.clouds_off || post.clouds.is_some()),
        ("weather", &WEATHER, post.weather.is_some()),
        ("hand", &HAND, post.hand.is_some()),
    ];
    for (kind, names, usable) in chains {
        let requested = names.iter().any(|name| {
            let path = if folder.is_empty() {
                format!("{name}.fsh")
            } else {
                format!("{folder}/{name}.fsh")
            };
            let vertex = path
                .strip_suffix(".fsh")
                .map(|path| format!("{path}.vsh"))
                .unwrap();
            (pack.exists(&path) || pack.exists(&vertex))
                && enabled(properties, folder, name, macros, &switches)
        });
        if requested && !usable {
            post.complete = false;
            post.notes.push(format!(
                "{kind}: no usable geometry program or renderer inputs"
            ));
        }
    }
    if post.shadow_pass {
        let keep = |key: &str| {
            properties
                .get(key)
                .is_none_or(|value| value.trim() != "false")
        };
        let set = |key: &str| {
            properties
                .get(key)
                .is_some_and(|value| value.trim() == "true")
        };
        let shadow_chains: [(&str, &[&str], bool, bool); 4] = [
            (
                "shadow terrain",
                &["shadow"],
                post.shadow.is_some(),
                keep("shadowTerrain"),
            ),
            (
                "shadow water",
                &["shadow_water", "shadow"],
                post.shadow_water.is_some(),
                keep("shadowTranslucent"),
            ),
            (
                "shadow entities",
                &["shadow_entities", "shadow"],
                post.shadow_entities.is_some(),
                keep("shadowEntities") || set("shadowPlayer"),
            ),
            (
                "shadow blocks",
                &["shadow_block", "shadow"],
                post.shadow_block.is_some(),
                keep("shadowBlockEntities") || set("shadowLightBlockEntities"),
            ),
        ];
        for (kind, names, usable, wanted) in shadow_chains {
            let requested = names.iter().any(|name| {
                let prefix = if folder.is_empty() {
                    name.to_string()
                } else {
                    format!("{folder}/{name}")
                };
                (pack.exists(&format!("{prefix}.vsh")) || pack.exists(&format!("{prefix}.fsh")))
                    && enabled(properties, folder, name, macros, &switches)
            });
            if wanted && requested && !usable {
                post.complete = false;
                post.notes.push(format!(
                    "{kind}: no usable shadow program or renderer inputs"
                ));
            }
        }
    }
    post.config = std::mem::take(&mut post.config).finish();
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
    // Discover compute stages first: they may themselves read the shadow buffers, or
    // update custom images without any shadow geometry. Do not compile unused raster
    // stages while shadows are off.
    let names: Vec<_> = std::iter::once("shadowcomp".to_owned())
        .chain((1..=99).map(|n| format!("shadowcomp{n}")))
        .collect();
    for name in &names {
        let compute = loader.compute(name);
        if !compute.is_empty() {
            loader.post.shadow_comp.push(PostProgram {
                name: name.clone(),
                group: Group::ShadowComp,
                program: None,
                buffers: Vec::new(),
                flips: flips(loader.properties, name),
                blend: None,
                buffer_blends: Vec::new(),
                mipmaps: 0,
                compute,
                geometry_triangles: 0,
                scale: FULL,
            });
        }
    }
    let post = &*loader.post;
    let gbuffers = [
        &post.terrain,
        &post.water,
        &post.sky,
        &post.sky_textured,
        &post.entities,
        &post.block,
        &post.entities_translucent,
        &post.particles,
        &post.beacon_beam,
        &post.damaged_block,
        &post.clouds,
        &post.weather,
        &post.hand,
    ];
    let reads = (post.programs.iter())
        .chain(&post.shadow_comp)
        .chain(gbuffers.into_iter().flatten())
        .flat_map(|p| {
            p.program
                .iter()
                .map(|q| &q.interface)
                .chain(p.compute.iter().map(|(_, c)| &c.interface))
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
        return;
    }
    let mut loaded = std::mem::take(&mut loader.post.shadow_comp)
        .into_iter()
        .map(|p| (p.name, p.compute))
        .collect::<std::collections::BTreeMap<_, _>>();
    for name in names {
        let compute = loaded.remove(&name).unwrap_or_default();
        let program = loader.load_with_compute(
            name,
            Group::ShadowComp,
            |v, f, _| translate_composite(v, f, Target::Buffers),
            Some(compute),
        );
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
                let p = loader.load(name.to_string(), Group::Shadow, |v, f, config| {
                    apply_terrain_lighting(&mut inputs, properties, config);
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
    let water = keep("shadowTranslucent")
        .then(|| load(&["shadow_water", "shadow"], terrain, true, false))
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

/// Apply pack policy only to vertex inputs which explicitly opt into terrain lighting.
fn apply_terrain_lighting(inputs: &mut GbuffersInputs, properties: &Properties, config: &Config) {
    if let Some(policy) = &mut inputs.terrain_lighting {
        let flag = |key: &str, default| match properties.get(key).map(str::trim) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        };
        policy.old_lighting = flag("oldLighting", true);
        policy.separate_ao = flag("separateAo", false);
        policy.ao_strength = config.ambient_occlusion_level;
    }
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
    /// The program `name` translated by `translate`, or `None` when it is missing, disabled or
    /// fails (with a note).
    fn load(
        &mut self,
        name: String,
        group: Group,
        translate: impl FnOnce(
            &Preprocessed,
            &Preprocessed,
            &Config,
        ) -> Result<Program, Vec<crate::Error>>,
    ) -> Option<PostProgram> {
        self.load_with_compute(name, group, translate, None)
    }

    fn load_with_compute(
        &mut self,
        name: String,
        group: Group,
        translate: impl FnOnce(
            &Preprocessed,
            &Preprocessed,
            &Config,
        ) -> Result<Program, Vec<crate::Error>>,
        cached_compute: Option<Vec<(String, ComputeProgram)>>,
    ) -> Option<PostProgram> {
        let path = |ext: &str| match self.folder {
            "" => format!("{name}.{ext}"),
            f => format!("{f}/{name}.{ext}"),
        };
        let (vsh, fsh) = (path("vsh"), path("fsh"));
        if !enabled(
            self.properties,
            self.folder,
            &name,
            &self.options,
            &self.switches,
        ) {
            return None;
        }
        let fullscreen = matches!(
            group,
            Group::Setup
                | Group::Begin
                | Group::Prepare
                | Group::Deferred
                | Group::Composite
                | Group::Final
                | Group::ShadowComp
        );
        let (vertex_exists, fragment_exists) = (self.pack.exists(&vsh), self.pack.exists(&fsh));
        if group == Group::Setup || !vertex_exists || !fragment_exists {
            if group != Group::Setup && fullscreen && (vertex_exists || fragment_exists) {
                self.post.complete = false;
                self.post
                    .notes
                    .push(format!("{name}: missing vertex or fragment stage"));
                return None;
            }
            if !fullscreen {
                return None;
            }
            // A suffix-only compute program is valid even without a base .csh.
            let compute = cached_compute.unwrap_or_else(|| self.compute(&name));
            return (!compute.is_empty()).then(|| PostProgram {
                buffer_blends: buffer_blends(self.properties, &name),
                flips: flips(self.properties, &name),
                name,
                group,
                program: None,
                buffers: Vec::new(),
                blend: None,
                mipmaps: 0,
                compute,
                geometry_triangles: 0,
                scale: FULL,
            });
        }
        if self.pack.exists(&path("gsh")) {
            let Some(inputs) = self.geometry.clone() else {
                if fullscreen {
                    self.post.complete = false;
                }
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
            if fullscreen {
                self.post.complete = false;
            }
            notes.extend(problems);
            notes.push(format!("{name}: skipped"));
            return None;
        }
        notes.extend([&vertex, &fragment].iter().flat_map(|pp| warnings(pp)));
        let mut config = self.post.config.clone();
        config.read(&vertex);
        config.read(&fragment);
        let program = match translate(&vertex, &fragment, &config) {
            Ok(p) => p,
            Err(errors) => {
                if fullscreen {
                    self.post.complete = false;
                }
                notes.extend(
                    errors
                        .iter()
                        .map(|e| format!("{}:{}: {}", e.file, e.line, e.message)),
                );
                notes.push(format!("{name}: skipped"));
                return None;
            }
        };
        self.post.config = config;
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
            | Group::ShadowComp => cached_compute.unwrap_or_else(|| self.compute(&name)),
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
            mipmaps,
            compute,
            geometry_triangles: 0,
        })
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
        let mut config = self.post.config.clone();
        for stage in &stages {
            config.read(stage);
        }
        let mut inputs = inputs.clone();
        apply_terrain_lighting(&mut inputs.inputs, self.properties, &config);
        let (program, triangles) =
            match crate::translate_geometry(vertex, geometry, fragment, &inputs) {
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
        self.post.config = config;
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
        if !enabled(
            self.properties,
            self.folder,
            name,
            &self.options,
            &self.switches,
        ) {
            return out;
        }
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
                self.post.complete = false;
                notes.extend(problems);
                notes.push(format!("{name}: skipped"));
                continue;
            }
            if self.properties.get(&format!("indirect.{name}")).is_some() {
                self.post.complete = false;
                notes.push(format!(
                    "{path}: indirect compute dispatch is not supported"
                ));
                continue;
            }
            match translate_compute(&pp) {
                Ok(program) => {
                    self.post.config.read(&pp);
                    out.push((name, program));
                }
                Err(errors) => {
                    self.post.complete = false;
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
    properties.program_enabled(folder, name, macros, switches)
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

/// Whether the pack names the `HIGHER_SHADOWCOLOR` feature, required or optional.
fn higher_shadowcolor(properties: &Properties) -> bool {
    ["iris.features.required", "iris.features.optional"]
        .iter()
        .filter_map(|k| properties.get(k))
        .any(|v| v.split_whitespace().any(|w| w == "HIGHER_SHADOWCOLOR"))
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
        assert!(
            off.complete,
            "intentional clouds=off must not disable the pack: {:?}",
            off.notes
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
        // gbuffers_skytextured falls back to gbuffers_textured, then gbuffers_basic.
        let sun = post.sky_textured.unwrap();
        assert_eq!(
            (sun.name.as_str(), sun.blend),
            ("gbuffers_textured", Some(CELESTIAL_BLEND))
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
        // gbuffers_clouds has no fallback (OQ-3): without it the game draws its clouds.
        assert!(post.clouds.is_none());
        // The hand falls back to gbuffers_textured and does not blend.
        let hand = post.hand.as_ref().unwrap();
        assert_eq!(
            (hand.name.as_str(), hand.group, hand.blend),
            ("gbuffers_textured", Group::Gbuffers, None)
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
    fn terrain_policy(program: &Program) -> Option<crate::TerrainLighting> {
        let literal = |name| {
            let (_, constant) = program
                .vertex
                .constants
                .iter()
                .find(|(_, c)| c.name.as_deref() == Some(name))?;
            match program.vertex.global_expressions[constant.init] {
                naga::Expression::Literal(value) => Some(value),
                _ => None,
            }
        };
        let naga::Literal::Bool(old_lighting) = literal("hz_old_lighting")? else {
            return None;
        };
        let naga::Literal::Bool(separate_ao) = literal("hz_separate_ao")? else {
            return None;
        };
        let naga::Literal::F32(ao_strength) = literal("hz_ao_strength")? else {
            return None;
        };
        Some(crate::TerrainLighting {
            old_lighting,
            separate_ao,
            ao_strength,
        })
    }

    #[test]
    #[ignore = "needs HORIZON_TEST_TERRAIN_PACK, a local pack never committed"]
    fn real_pack_terrain_lighting_policy() {
        let path =
            std::env::var_os("HORIZON_TEST_TERRAIN_PACK").expect("HORIZON_TEST_TERRAIN_PACK");
        let pack = Pack::open(std::path::Path::new(&path)).unwrap();
        let mut macros = Macros::standard();
        macros.undef("MC_OS_MAC");
        macros.define("MC_OS_OTHER", "");
        let folder = dimension_folder(&pack, 0);
        let properties = Properties::from_pack(&pack, &folder, &macros);
        let plain = GbuffersInputs {
            declarations: "layout(location=0) in vec3 hz_v_pos;".into(),
            body: "hz_Vertex=vec4(hz_v_pos,1.0);hz_Color=vec4(1.0);".into(),
            attributes: vec![("mc_Entity".into(), "vec4(-1.0)".into())],
            ..Default::default()
        };
        let terrain = GbuffersInputs {
            terrain_lighting: Some(crate::TerrainLighting::default()),
            ..plain.clone()
        };
        let inputs = GameInputs {
            terrain: Some(terrain),
            sky: Some(plain.clone()),
            celestial: Some(plain.clone()),
            entities: Some(plain.clone()),
            clouds: Some(plain.clone()),
            weather: Some(plain.clone()),
            hand: Some(plain),
            ..Default::default()
        };
        let post = load_post(&pack, &folder, &properties, &macros, &inputs);
        assert!(post.complete, "{:?} {:?}", post.notes, post.notes);
        let expected = Some(crate::TerrainLighting {
            old_lighting: properties.get("oldLighting") != Some("false"),
            separate_ao: properties.get("separateAo") == Some("true"),
            ao_strength: post.config.ambient_occlusion_level,
        });
        for pass in [&post.terrain, &post.water, &post.shadow, &post.shadow_water]
            .into_iter()
            .flatten()
        {
            let actual = terrain_policy(pass.program.as_ref().unwrap());
            println!("{}: {:?}", pass.name, actual);
            assert_eq!(actual, expected);
        }
        println!(
            "complete={} ambientOcclusionLevel={}",
            post.complete, post.config.ambient_occlusion_level
        );
    }

    #[test]
    fn terrain_lighting_policy_follows_options_dimension_and_shadow_fallbacks() {
        let mut files = Vec::<(String, String)>::new();
        for (folder, mode, ao) in [("world0", 2, 0.375), ("world-1", 1, 0.75)] {
            files.push((
                format!("shaders/{folder}/settings.glsl"),
                format!(
                    "#define POLICY_MODE {mode} //[1 2]\nconst float ambientOcclusionLevel={ao};"
                ),
            ));
            // Only basic exists: terrain and water must carry the policy through fallback.
            for name in ["gbuffers_basic", "shadow"] {
                files.push((format!("shaders/{folder}/{name}.vsh"), "#version 120\n#include \"settings.glsl\"\nvoid main(){gl_Position=gl_Vertex;gl_FrontColor=gl_Color;}".into()));
                files.push((
                    format!("shaders/{folder}/{name}.fsh"),
                    "#version 120\n#include \"settings.glsl\"\nvoid main(){gl_FragColor=gl_Color;}"
                        .into(),
                ));
            }
        }
        files.push(("shaders/shaders.properties".into(), "shadow.enabled=true\n#if POLICY_MODE == 2\noldLighting=false\nseparateAo=true\n#else\noldLighting=true\nseparateAo=false\n#endif".into()));
        let borrowed: Vec<_> = files
            .iter()
            .map(|(path, code)| (path.as_str(), code.as_str()))
            .collect();
        let pack = Pack::from_zip_bytes("lighting-policy", zip(&borrowed)).unwrap();
        let macros = Macros::standard();
        let plain = GbuffersInputs {
            body: "hz_Vertex=vec4(0.0,0.0,0.0,1.0);hz_Color=vec4(1.0);".into(),
            ..Default::default()
        };
        let terrain = GbuffersInputs {
            terrain_lighting: Some(crate::TerrainLighting::default()),
            ..plain.clone()
        };
        let inputs = GameInputs {
            terrain: Some(terrain),
            sky: Some(plain.clone()),
            celestial: Some(plain.clone()),
            entities: Some(plain.clone()),
            clouds: Some(plain.clone()),
            weather: Some(plain.clone()),
            hand: Some(plain),
            ..Default::default()
        };
        for (dimension, old_lighting, separate_ao, ao_strength) in
            [(0, false, true, 0.375), (-1, true, false, 0.75)]
        {
            let folder = dimension_folder(&pack, dimension);
            let properties = Properties::from_pack(&pack, &folder, &macros);
            let post = load_post(&pack, &folder, &properties, &macros, &inputs);
            assert!(post.complete, "{:?}", post.notes);
            let expected = Some(crate::TerrainLighting {
                old_lighting,
                separate_ao,
                ao_strength,
            });
            for pass in [&post.terrain, &post.water, &post.shadow, &post.shadow_water] {
                assert_eq!(
                    terrain_policy(pass.as_ref().unwrap().program.as_ref().unwrap()),
                    expected
                );
            }
            assert_eq!(post.terrain.as_ref().unwrap().name, "gbuffers_basic");
            assert!(terrain_policy(post.sky.as_ref().unwrap().program.as_ref().unwrap()).is_none());
            assert!(
                terrain_policy(post.entities.as_ref().unwrap().program.as_ref().unwrap()).is_none()
            );
            assert!(
                terrain_policy(
                    post.shadow_entities
                        .as_ref()
                        .unwrap()
                        .program
                        .as_ref()
                        .unwrap()
                )
                .is_none()
            );
            assert_eq!(
                inputs.terrain.as_ref().unwrap().terrain_lighting,
                Some(crate::TerrainLighting::default())
            );
            let defaults = Properties::parse("shadow.enabled=true", &macros);
            let post = load_post(&pack, &folder, &defaults, &macros, &inputs);
            assert_eq!(
                terrain_policy(post.terrain.unwrap().program.as_ref().unwrap()),
                Some(crate::TerrainLighting {
                    old_lighting: true,
                    separate_ao: false,
                    ao_strength
                })
            );
        }
    }

    #[test]
    fn incomplete_fullscreen_passes_fail_safely() {
        let pack = Pack::from_zip_bytes("test", zip(&[("shaders/final.vsh", VSH)])).unwrap();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &Macros::standard(),
            &GameInputs::default(),
        );
        assert!(!post.complete);
        assert!(
            post.notes
                .iter()
                .any(|note| note.contains("missing vertex or fragment"))
        );
    }
    #[test]
    fn requested_geometry_needs_renderer_inputs_and_usable_programs() {
        let pack = Pack::from_zip_bytes(
            "test",
            zip(&[
                ("shaders/gbuffers_terrain.vsh", VSH),
                ("shaders/gbuffers_terrain.fsh", FSH),
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
        assert!(!post.complete);
        let input = GbuffersInputs {
            body: "hz_Vertex=vec4(0.0,0.0,0.0,1.0); hz_MultiTexCoord0=vec4(0.0);".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain: Some(input.clone()),
            entities: Some(input),
            ..Default::default()
        };
        let post = load_post(&pack, "", &Properties::default(), &macros, &inputs);
        assert!(post.complete, "{:?}", post.notes);
        assert!(post.terrain.is_some());
    }

    #[test]
    fn shadow_failures_respect_part_flags_and_valid_fallbacks() {
        let fragment = "#version 120\nvoid main(){gl_FragColor=vec4(1.0);}";
        let inputs = GbuffersInputs {
            body: "hz_Vertex=vec4(0.0,0.0,0.0,1.0);hz_MultiTexCoord0=vec4(0.0);".into(),
            ..Default::default()
        };
        let inputs = GameInputs {
            terrain: Some(inputs.clone()),
            entities: Some(inputs),
            ..Default::default()
        };
        let macros = Macros::standard();
        let load = |files: &[(&str, &str)], flags: &str| {
            let pack = Pack::from_zip_bytes("test", zip(files)).unwrap();
            let properties = Properties::parse(flags, &macros);
            load_post(&pack, "", &properties, &macros, &inputs)
        };
        let bad = load(&[("shaders/shadow.vsh", VSH)], "shadow.enabled=true");
        assert!(!bad.complete);
        assert!(
            bad.notes
                .iter()
                .any(|note| note.contains("no usable shadow"))
        );
        let off = load(
            &[("shaders/shadow.vsh", VSH)],
            "shadow.enabled=true\nshadowTerrain=false\nshadowTranslucent=false\nshadowEntities=false\nshadowBlockEntities=false",
        );
        assert!(off.complete, "{:?}", off.notes);
        let fallback = load(
            &[
                ("shaders/shadow.vsh", VSH),
                ("shaders/shadow.fsh", fragment),
                ("shaders/shadow_water.vsh", VSH),
                ("shaders/shadow_water.fsh", "invalid"),
            ],
            "shadow.enabled=true",
        );
        assert!(fallback.complete, "{:?}", fallback.notes);
        assert_eq!(fallback.shadow_water.as_ref().unwrap().name, "shadow");
    }
    #[test]
    fn compute_phases_suffixes_and_dimension_configuration_survive_merge() {
        let code = "#version 430\nlayout(local_size_x=8) in;void main(){}";
        let configured = "#version 430\nlayout(local_size_x=8) in;\n/* const int colortex4Format=RGBA16F; */\nconst bool colortex4Clear=false;\nvoid main(){}";
        let pack = Pack::from_zip_bytes(
            "compute",
            zip(&[
                ("shaders/setup.csh", code),
                ("shaders/setup2_a.csh", code),
                ("shaders/begin_b.csh", code),
                ("shaders/composite_a.csh", code),
                ("shaders/final_a.csh", code),
                ("shaders/world-1/composite.csh", configured),
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
        assert!(post.complete, "{:?}", post.notes);
        assert_eq!(
            post.programs
                .iter()
                .filter(|p| p.group == Group::Setup)
                .flat_map(|p| p.compute.iter().map(|(n, _)| n.as_str()))
                .collect::<Vec<_>>(),
            ["setup", "setup2_a"]
        );
        assert_eq!(
            post.programs
                .iter()
                .find(|p| p.group == Group::Begin)
                .unwrap()
                .compute[0]
                .0,
            "begin_b"
        );
        assert_eq!(
            post.programs
                .iter()
                .filter(|p| !matches!(p.group, Group::Setup | Group::Begin))
                .map(|p| p.compute[0].0.as_str())
                .collect::<Vec<_>>(),
            ["composite_a", "final_a"]
        );
        let disabled = Properties::parse(
            "program.composite.enabled=false\nprogram.setup2.enabled=false",
            &macros,
        );
        let post = load_post(&pack, "", &disabled, &macros, &GameInputs::default());
        assert_eq!(
            post.programs
                .iter()
                .filter(|p| p.group == Group::Setup)
                .count(),
            1
        );
        assert_eq!(
            post.programs
                .iter()
                .filter(|p| !matches!(p.group, Group::Setup | Group::Begin))
                .count(),
            1
        );
        let post = load_post(
            &pack,
            "world-1",
            &Properties::default(),
            &macros,
            &GameInputs::default(),
        );
        assert!(
            !post
                .programs
                .iter()
                .any(|p| matches!(p.group, Group::Setup | Group::Begin))
        );
        assert_eq!(
            post.config.buffers[4].format,
            crate::Format::parse("RGBA16F").unwrap()
        );
        assert!(!post.config.buffers[4].clear);
    }

    #[test]
    fn compute_shadow_reads_are_live_and_failures_disable_partial_effects() {
        let code = "#version 430\nlayout(local_size_x=1) in;uniform sampler2D shadowtex0;layout(rgba8) writeonly uniform image2D result_img;void main(){imageStore(result_img,ivec2(0),texelFetch(shadowtex0,ivec2(0),0));}";
        let pack =
            Pack::from_zip_bytes("compute", zip(&[("shaders/composite_a.csh", code)])).unwrap();
        let macros = Macros::standard();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &macros,
            &GameInputs::default(),
        );
        assert!(post.complete && post.shadow_pass, "{:?}", post.notes);
        let properties = Properties::parse("indirect.composite_a=0 0", &macros);
        let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
        assert!(!post.complete);
        assert!(post.notes.iter().any(|n| n.contains("indirect compute")));
        let pack = Pack::from_zip_bytes("unused", zip(&[("shaders/composite.csh", "#version 430\nlayout(local_size_x=1) in;uniform sampler2D shadowtex0;void main(){}")])).unwrap();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &macros,
            &GameInputs::default(),
        );
        assert!(post.complete && !post.shadow_pass, "{:?}", post.notes);
    }

    #[test]
    fn custom_images_reject_invalid_dimensions_and_booleans() {
        for value in [
            "tex rgba rgba8 unsigned_byte true true NaN 1.0",
            "tex rgba rgba8 unsigned_byte true true -1.0 1.0",
            "tex rgba rgba8 unsigned_byte true false 0 8",
            "tex rgba rgba8 unsigned_byte maybe false 8 8",
        ] {
            assert!(CustomImage::parse("image", value).is_none(), "{value}");
        }
    }
    #[test]
    fn compute_only_shadowcomp_is_loaded_before_shadow_resource_inference() {
        let macros = Macros::standard();
        for (sampler, on) in [("shadowcolor0", true), ("colortex0", false)] {
            let code = format!(
                "#version 430\nlayout(local_size_x=1) in;uniform sampler2D {sampler};layout(rgba8) writeonly uniform image3D volume;void main(){{imageStore(volume,ivec3(0),texelFetch({sampler},ivec2(0),0));}}"
            );
            let pack = Pack::from_zip_bytes(
                "shadow-compute",
                zip(&[("shaders/shadowcomp_a.csh", code.as_str())]),
            )
            .unwrap();
            let post = load_post(
                &pack,
                "",
                &Properties::default(),
                &macros,
                &GameInputs::default(),
            );
            assert!(post.complete, "{:?}", post.notes);
            assert_eq!(post.shadow_pass, on, "{sampler}");
            assert_eq!(post.shadow_comp.len(), 1);
            assert!(post.shadow_comp[0].program.is_none());
            assert_eq!(post.shadow_comp[0].compute[0].0, "shadowcomp_a");
            let properties = Properties::parse("program.shadowcomp.enabled=false", &macros);
            let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
            assert!(post.shadow_comp.is_empty() && !post.shadow_pass);
            let properties = Properties::parse("shadow.enabled=false", &macros);
            let post = load_post(&pack, "", &properties, &macros, &GameInputs::default());
            assert!(!post.shadow_pass);
            assert_eq!(
                post.shadow_comp.len(),
                1,
                "custom image compute is independent of drawing shadows"
            );
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn begin_raster_and_compute_share_one_lifecycle_pass() {
        let compute = "#version 430\nlayout(local_size_x=1) in; layout(std430,binding=0) buffer Data{vec4 value;};void main(){value=vec4(1.0);}";
        let vertex = "#version 330\nvoid main(){gl_Position=gl_Vertex;}";
        let fragment = "#version 330\nvoid main(){gl_FragColor=vec4(1.0);}";
        let pack = Pack::from_zip_bytes(
            "lifecycle",
            crate::pack::tests::zip(&[
                ("shaders/setup.csh", compute),
                ("shaders/begin.csh", compute),
                ("shaders/begin.vsh", vertex),
                ("shaders/begin.fsh", fragment),
            ]),
        )
        .unwrap();
        let post = load_post(
            &pack,
            "",
            &Properties::default(),
            &Macros::standard(),
            &GameInputs::default(),
        );
        assert!(post.complete, "{:?}", post.notes);
        assert_eq!(post.programs.len(), 2);
        assert_eq!(post.programs[0].group, Group::Setup);
        assert!(post.programs[0].program.is_none());
        assert_eq!(post.programs[0].compute.len(), 1);
        assert_eq!(post.programs[1].group, Group::Begin);
        assert!(post.programs[1].program.is_some());
        assert_eq!(post.programs[1].compute.len(), 1);
        assert_eq!(post.buffer_minimums().get(&0), Some(&16));
    }

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
