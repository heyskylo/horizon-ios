//! The pipeline's buffers and shadow map (spec 1.7, 6.1, 6.3, 7.1): each colortex buffer's
//! format, clearing and mipmaps, the shadow map's settings, and the names packs give them. A pack
//! format's reader fills a [`Config`] (`horizon-shaders` reads them from a `shaders/` pack's stage
//! files); the renderer makes its targets from it.

/// colortex buffers (6.1.1).
pub const BUFFERS: usize = 16;

/// The shadowcolor buffers there are: two, and `shadowcolor2` to `shadowcolor7` for a pack that
/// names the `HIGHER_SHADOWCOLOR` feature.
pub const SHADOW_COLORS: usize = 8;

/// A colour format of 6.7, as channels, bits per channel and kind. Packed formats are described
/// by their widest channel: `RGB10_A2` is 4 × 10 bits, `R11F_G11F_B10F` 3 × 11 bits float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub channels: u8,
    pub bits: u8,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Unorm,
    Snorm,
    Float,
    Int,
    Uint,
}

impl Format {
    /// The default `RGBA` (6.1.3).
    pub const RGBA8: Format = Format {
        channels: 4,
        bits: 8,
        kind: Kind::Unorm,
    };

    /// A format name of 6.7, in any case.
    pub fn parse(name: &str) -> Option<Format> {
        let name = name.trim().to_ascii_uppercase();
        let packed = |channels, bits, kind| {
            Some(Format {
                channels,
                bits,
                kind,
            })
        };
        match name.as_str() {
            "RGBA" => return Some(Self::RGBA8),
            "R3_G3_B2" => return packed(3, 3, Kind::Unorm),
            "RGB5_A1" => return packed(4, 5, Kind::Unorm),
            "RGB10_A2" => return packed(4, 10, Kind::Unorm),
            "RGB10_A2UI" => return packed(4, 10, Kind::Uint),
            "R11F_G11F_B10F" => return packed(3, 11, Kind::Float),
            "RGB9_E5" => return packed(3, 9, Kind::Float),
            "RGBA4" => return packed(4, 4, Kind::Unorm),
            "RGB565" => return packed(3, 6, Kind::Unorm),
            "RGBA2" => return packed(4, 2, Kind::Unorm),
            _ => {}
        }
        let letters = name.bytes().take_while(u8::is_ascii_alphabetic).count();
        let channels = match &name[..letters] {
            "R" => 1,
            "RG" => 2,
            "RGB" => 3,
            "RGBA" => 4,
            _ => return None,
        };
        let rest = &name[letters..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let bits: u8 = rest[..digits].parse().ok()?;
        let kind = match &rest[digits..] {
            "" => Kind::Unorm,
            "_SNORM" => Kind::Snorm,
            "F" => Kind::Float,
            "I" => Kind::Int,
            "UI" => Kind::Uint,
            _ => return None,
        };
        let valid = match kind {
            Kind::Unorm | Kind::Snorm => matches!(bits, 8 | 16),
            Kind::Float => matches!(bits, 16 | 32),
            Kind::Int | Kind::Uint => matches!(bits, 8 | 16 | 32),
        };
        valid.then_some(Format {
            channels,
            bits,
            kind,
        })
    }
}

/// The settings of one colortex buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct Buffer {
    pub format: Format,
    /// Cleared every frame (`<buffer>Clear`, default true).
    pub clear: bool,
    /// `<buffer>ClearColor`, else 6.1.5's default: colortex0 the fog colour, colortex1 white,
    /// the others transparent black.
    pub clear_color: Option<[f32; 4]>,
    /// `<buffer>MipmapEnabled`.
    pub mipmap: bool,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            format: Format::RGBA8,
            clear: true,
            clear_color: None,
            mipmap: false,
        }
    }
}

/// The shadow map's settings (6.3, 7.1), from `const` lines of the stage files.
#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    /// `shadowMapResolution`: the map's side in pixels.
    pub resolution: u32,
    /// `shadowDistance`: half the side of the area the map covers, in blocks.
    pub distance: f32,
    /// `shadowDistanceRenderMul`: when > 0, geometry beyond `distance × mul` is not drawn.
    pub render_mul: f32,
    /// `voxelDistance`: with `shadow.culling=reversed`, nothing within it is culled.
    pub voxel_distance: f32,
    /// `shadowIntervalSize`: the shadow camera moves in steps of this many blocks (0: none).
    pub interval: f32,
    /// Native packs: the snap's remainder is euclidean, so the centre is within half an interval
    /// of the player at negative coordinates too. `shaders/` packs keep Iris's.
    pub euclidean_snap: bool,
    /// `sunPathRotation`, degrees.
    pub sun_path_rotation: f32,
    /// `shadowMapFov`: a perspective projection of this many degrees instead of orthographic.
    pub fov: Option<f32>,
    /// `shadowNearPlane`, `shadowFarPlane`: the orthographic projection's planes, as JOML's
    /// `zNear`/`zFar`. None when the pack doesn't set one: Iris 1.11.7 then uses -100.05 and
    /// 156 (measured with a probe pack), each on its own.
    pub near: Option<f32>,
    pub far: Option<f32>,
    /// `shadowHardwareFiltering(0|1)`: shadowtex0/1 sampled with depth comparison.
    pub hardware_filtering: [bool; 2],
    /// `shadowtex0Nearest` (or `shadowtexNearest`, `shadow0MinMagNearest`) and
    /// `shadowtex1Nearest` (or `shadow1MinMagNearest`): shadowtex0/1 read through a plain sampler
    /// take the nearest texel. Otherwise Iris 1.11.7 filters them linearly (measured).
    pub nearest: [bool; 2],
    /// `shadowcolor0` to `shadowcolor7`: format, cleared every frame, clear colour (default
    /// white).
    pub colors: [Buffer; SHADOW_COLORS],
}

impl Default for Shadow {
    fn default() -> Self {
        let white = Buffer {
            clear_color: Some([1.0; 4]),
            ..Buffer::default()
        };
        Self {
            resolution: 1024,
            distance: 160.0,
            render_mul: -1.0,
            voxel_distance: 160.0,
            interval: 2.0,
            euclidean_snap: false,
            sun_path_rotation: 0.0,
            fov: None,
            near: None,
            far: None,
            hardware_filtering: [false; 2],
            nearest: [false; 2],
            colors: std::array::from_fn(|_| white.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub buffers: [Buffer; BUFFERS],
    pub shadow: Shadow,
    /// `noiseTextureResolution`: the side of the `noisetex` Horizon makes when the pack gives none.
    pub noise_resolution: Option<u32>,
    /// The smoothed uniforms' half-lives in ticks, in [`HALF_LIVES`] order, when the pack sets
    /// them (`const float wetnessHalflife = 30.0;`...).
    pub half_lives: [Option<f32>; 4],
    /// Conflicting or unreadable settings, as `file:line: message`.
    pub notes: Vec<String>,
    /// Each colortex setting the pack's files set so far and its value, so that its reader can
    /// report a later file changing it.
    pub settings: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            buffers: std::array::from_fn(|_| Buffer::default()),
            shadow: Shadow::default(),
            noise_resolution: None,
            half_lives: [None; 4],
            notes: Vec::new(),
            settings: Vec::new(),
        }
    }
}

/// The shadowcolor buffer a sampler or image name reads: `shadowcolor` and `shadowcolor0` to
/// `shadowcolor7`.
pub fn shadow_color(name: &str) -> Option<usize> {
    match name.strip_prefix("shadowcolor")?.as_bytes() {
        [] => Some(0),
        &[d @ b'0'..=b'7'] => Some(usize::from(d - b'0')),
        _ => None,
    }
}

/// The colortex index of a buffer name or alias (6.1.1).
pub fn buffer_index(name: &str) -> Option<usize> {
    let alias = [
        "gcolor",
        "gdepth",
        "gnormal",
        "composite",
        "gaux1",
        "gaux2",
        "gaux3",
        "gaux4",
    ];
    if let Some(i) = alias.iter().position(|a| *a == name) {
        return Some(i);
    }
    let n: usize = name.strip_prefix("colortex")?.parse().ok()?;
    (n < BUFFERS).then_some(n)
}

/// The `const float` half-lives (ShaderDoc, OptiFine's shaders.txt) of `wetness` rising and
/// falling, `eyeBrightnessSmooth` and `centerDepthSmooth`, in ticks.
pub const HALF_LIVES: [&str; 4] = [
    "wetnessHalflife",
    "drynessHalflife",
    "eyeBrightnessHalflife",
    "centerDepthHalflife",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_parse() {
        let f = |channels, bits, kind| {
            Some(Format {
                channels,
                bits,
                kind,
            })
        };
        assert_eq!(Format::parse("rgba16f"), f(4, 16, Kind::Float));
        assert_eq!(Format::parse("R32UI"), f(1, 32, Kind::Uint));
        assert_eq!(Format::parse("RG8_SNORM"), f(2, 8, Kind::Snorm));
        assert_eq!(Format::parse("R11F_G11F_B10F"), f(3, 11, Kind::Float));
        assert_eq!(Format::parse("RGB16"), f(3, 16, Kind::Unorm));
        assert_eq!(Format::parse("RGBA12"), None);
        assert_eq!(Format::parse("R16_SNORMX"), None);
    }
}
