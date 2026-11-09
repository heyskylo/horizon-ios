//! The configuration packs write into their stage files (spec 1.7, 6.1): which buffers a fragment
//! shader writes (`/* DRAWBUFFERS:0257 */`, `/* RENDERTARGETS: 0,2 */`) and, per colortex buffer,
//! its format (`const int colortex2Format = RGBA16F;`, inside a block comment because the format
//! names are not GLSL), whether and to what it is cleared, and whether it gets mipmaps.
//!
//! Everything is read from live code only: the preprocessor has already dropped the false
//! branches, so a format chosen by an option is the one the option selects. Every stage file of
//! the programs that run is read (OQ-17), and the shadow programs' when the shadow pass doesn't
//! run; a later file overrides an earlier one, with a note when the values differ. A colortex
//! format counts only in a fragment stage, and in a block comment only where `const` starts its
//! line, as Iris 1.11.7 read our probes ([`ReadConfig::read_vertex`], [`ReadConfig::read`]).

use crate::Preprocessed;
pub use horizon_render_pipeline::config::{
    BUFFERS, Buffer, Config, Format, HALF_LIVES, Kind, SHADOW_COLORS, Shadow, buffer_index,
    shadow_color,
};

/// Reading a [`Config`] from a pack's stage files.
pub trait ReadConfig {
    fn read(&mut self, pp: &Preprocessed);
    fn read_formats(&mut self, pp: &Preprocessed);
    fn read_formats_and_shadow(&mut self, pp: &Preprocessed);
    fn read_vertex(&mut self, pp: &Preprocessed);
}

impl ReadConfig for Config {
    /// Reads one preprocessed fragment stage file.
    fn read(&mut self, pp: &Preprocessed) {
        self.read_stage(pp, true, true, true);
    }

    /// Reads only the colortex formats of one preprocessed fragment stage file, as [`Self::read`]
    /// reads them.
    fn read_formats(&mut self, pp: &Preprocessed) {
        self.read_stage(pp, true, false, false);
    }

    /// Reads the colortex formats and the shadow settings of one preprocessed fragment stage
    /// file: `shadow.fsh` of a pack without a shadow pass. Iris 1.11.7 applied `shadowDistance`
    /// and `sunPathRotation` declared only there (its shadow projection and sun position), with a
    /// probe pack of ours whose shadow buffers nothing read (#10490).
    fn read_formats_and_shadow(&mut self, pp: &Preprocessed) {
        self.read_stage(pp, true, true, false);
    }

    /// Reads one preprocessed vertex stage file. Its colortex formats don't count: Iris 1.11.7
    /// ignored a `colortexNFormat` declared only in `composite.vsh`, `final.vsh`, `deferred.vsh`,
    /// `gbuffers_terrain.vsh` or `shadow.vsh` of probe packs p9063a, p9063c and p9063s (ours),
    /// while it honoured the same line in each program's fragment shader. Nor do the shadow
    /// settings of [`VERTEX_IGNORED`].
    fn read_vertex(&mut self, pp: &Preprocessed) {
        self.read_stage(pp, false, false, true);
    }
}

/// The settings of one stage file, into a [`Config`].
trait ReadStage {
    fn read_stage(&mut self, pp: &Preprocessed, formats: bool, shadow: bool, others: bool);
    fn constant(&mut self, place: &str, ty: &str, name: &str, value: &str, reads: [bool; 3]);
}

impl ReadStage for Config {
    /// `formats`: whether colortex formats count; `shadow`: whether the shadow settings do (those
    /// of [`VERTEX_IGNORED`] only with it); `others`: whether every other setting does.
    fn read_stage(&mut self, pp: &Preprocessed, formats: bool, shadow: bool, others: bool) {
        let line = |at: usize| pp.locate(code_line(&pp.code, at));
        for (at, ty, name, value) in constants(&pp.code, false) {
            let place = line(at);
            self.constant(&place, ty, name, value, [formats, shadow, others]);
        }
        for (origin, comment) in &pp.comments {
            for (at, ty, name, value) in constants(comment, true) {
                let file = pp
                    .files
                    .get(origin.file as usize)
                    .map_or("?", String::as_str);
                let place = format!("{file}:{}", origin.line);
                // Our probes' Iris 1.11.7 values fit this (a fit, not a rule Iris states): a
                // `colortexNFormat` in a block comment counts only when `const` starts its line.
                // Iris ignored one on the line of the `/*` (`/* const int colortex2Format =
                // RGBA16F; */`), after a ` * `, and after another declaration on its line; it
                // honoured one indented, one followed by text or by `*/`, and the first of two.
                let starts_line = (comment[..at].rfind('\n'))
                    .is_some_and(|n| comment[n + 1..at].trim().is_empty());
                let reads = [formats && starts_line, shadow, others];
                self.constant(&place, ty, name, value, reads);
            }
        }
    }

    /// `reads`: whether a colortex format declared here counts, whether a shadow setting does
    /// (those of [`VERTEX_IGNORED`] only with it), and whether any other setting does.
    fn constant(&mut self, place: &str, ty: &str, name: &str, value: &str, reads: [bool; 3]) {
        let [formats, shadow, others] = reads;
        // Like a colortex format, it counts from fragment shaders only, those of shadow programs
        // with no shadow pass too: Iris 1.11.7 made `noisetex` 64 wide from a declaration in our
        // probes' `composite.fsh`, `deferred.fsh`, `gbuffers_textured.fsh`, `final.fsh` or
        // `shadow.fsh` (no pass), and kept 256 from one in `final.vsh`.
        if name == "noiseTextureResolution" && ty == "int" {
            if let Some(n) = number(value).filter(|n| *n >= 1.0 && formats) {
                self.noise_resolution = Some(n as u32);
            }
            return;
        }
        let format = ty == "int" && name.strip_suffix("Format").and_then(buffer_index).is_some();
        if !shadow && VERTEX_IGNORED.contains(&name) {
            return;
        }
        // Any other shadow setting (`shadow_constant` knows the names) where `shadow` or `others`.
        if Config::default().shadow_constant(ty, name, value) {
            if shadow || others {
                self.shadow_constant(ty, name, value);
            }
            return;
        }
        if !others && !format {
            return;
        }
        if let Some(i) = HALF_LIVES.iter().position(|n| *n == name) {
            if others && let Some(n) = number(value).filter(|n| *n >= 0.0) {
                self.half_lives[i] = Some(n);
            }
            return;
        }
        let settings = [
            ("Format", "int"),
            ("ClearColor", "vec4"),
            ("Clear", "bool"),
            ("MipmapEnabled", "bool"),
        ];
        let Some((buffer, setting)) = settings.iter().find_map(|(suffix, want)| {
            let buffer = buffer_index(name.strip_suffix(suffix)?)?;
            (ty == *want).then_some((buffer, *suffix))
        }) else {
            return;
        };
        if setting == "Format" && !formats {
            return;
        }
        let b = &mut self.buffers[buffer];
        let ok = match setting {
            "Format" => Format::parse(value).map(|f| b.format = f),
            "ClearColor" => vec4(value).map(|c| b.clear_color = Some(c)),
            "Clear" => boolean(value).map(|v| b.clear = v),
            _ => boolean(value).map(|v| b.mipmap = v),
        };
        if ok.is_none() {
            let problem = match setting {
                "Format" => format!("unknown format `{value}`"),
                "ClearColor" => format!("`{name}` is not a vec4: `{value}`"),
                _ => format!("`{name}` is not a bool: `{value}`"),
            };
            self.notes.push(format!("{place}: {problem}"));
            return;
        }
        // The same setting under another name (an alias) counts as the same.
        let key = format!("{buffer}{setting}");
        match self.settings.iter_mut().find(|(k, _)| *k == key) {
            Some((_, old)) if old != value => {
                self.notes.push(format!(
                    "{place}: `{name}` changed by a later file to `{value}`"
                ));
                *old = value.to_owned();
            }
            Some(_) => {}
            None => self.settings.push((key, value.to_owned())),
        }
    }
}

/// Shadow settings a vertex stage's `const` doesn't set: Iris 1.11.7 ignored each declared only in
/// `final.vsh`, `gbuffers_textured.vsh` or `shadow.vsh` of probe packs of ours (#10490), with
/// and without a shadow pass. The other shadow settings were not probed there.
const VERTEX_IGNORED: [&str; 5] = [
    "shadowDistance",
    "sunPathRotation",
    "shadowIntervalSize",
    "shadowNearPlane",
    "shadowFarPlane",
];

/// The shadow settings of 6.3 and 7.1, into a [`Config`].
trait ShadowSetting {
    fn shadow_constant(&mut self, ty: &str, name: &str, value: &str) -> bool;
}

impl ShadowSetting for Config {
    /// A shadow setting of 6.3 or 7.1; true when `name` is one (read or not).
    fn shadow_constant(&mut self, ty: &str, name: &str, value: &str) -> bool {
        let s = &mut self.shadow;
        let float = |v: &mut f32| {
            if let Some(n) = number(value) {
                *v = n;
            }
        };
        match name {
            "shadowMapResolution" => {
                if let Some(n) = number(value).filter(|n| *n >= 1.0) {
                    s.resolution = (n as u32).min(16384);
                }
            }
            "shadowDistance" => float(&mut s.distance),
            "shadowDistanceRenderMul" => float(&mut s.render_mul),
            "voxelDistance" => float(&mut s.voxel_distance),
            "shadowIntervalSize" => float(&mut s.interval),
            "sunPathRotation" => float(&mut s.sun_path_rotation),
            "shadowNearPlane" => s.near = number(value).or(s.near),
            "shadowFarPlane" => s.far = number(value).or(s.far),
            "shadowMapFov" => s.fov = number(value).filter(|f| *f > 0.0),
            "shadowHardwareFiltering" => {
                if let Some(b) = boolean(value) {
                    s.hardware_filtering = [b; 2];
                }
            }
            "shadowHardwareFiltering0" | "shadowHardwareFiltering1" => {
                if let Some(b) = boolean(value) {
                    s.hardware_filtering[usize::from(name.ends_with('1'))] = b;
                }
            }
            "shadowtexNearest" | "shadowtex0Nearest" | "shadow0MinMagNearest" => {
                if let Some(b) = boolean(value) {
                    s.nearest[0] = b;
                }
            }
            "shadowtex1Nearest" | "shadow1MinMagNearest" => {
                if let Some(b) = boolean(value) {
                    s.nearest[1] = b;
                }
            }
            _ => {
                // `shadowcolor` is shadowcolor0.
                let color =
                    name.strip_prefix("shadowcolor")
                        .map(|rest| match rest.as_bytes().first() {
                            Some(&d @ b'0'..=b'7') => (usize::from(d - b'0'), &rest[1..]),
                            _ => (0, rest),
                        });
                let Some((buffer, setting)) = color else {
                    return false;
                };
                let b = &mut s.colors[buffer];
                match (setting, ty) {
                    ("Format", "int") => {
                        if let Some(f) = Format::parse(value) {
                            b.format = f;
                        }
                    }
                    ("Clear", "bool") => {
                        if let Some(v) = boolean(value) {
                            b.clear = v;
                        }
                    }
                    ("ClearColor", "vec4") => {
                        if let Some(c) = vec4(value) {
                            b.clear_color = Some(c);
                        }
                    }
                    _ => return false,
                }
            }
        }
        true
    }
}

/// The buffers a fragment shader writes, from the last `DRAWBUFFERS` or `RENDERTARGETS` comment
/// of its live code, or `None` without one (then the program loader decides). 6.1.4.
pub fn draw_buffers(pp: &Preprocessed) -> Option<Vec<u32>> {
    let mut found = None;
    for (_, comment) in &pp.comments {
        let text = comment.trim();
        if let Some(digits) = text.strip_prefix("DRAWBUFFERS:") {
            let list: Option<Vec<u32>> = digits.trim().chars().map(|c| c.to_digit(10)).collect();
            found = list.or(found);
        } else if let Some(list) = text.strip_prefix("RENDERTARGETS:") {
            let list: Option<Vec<u32>> = list
                .split(',')
                .map(|n| {
                    n.trim()
                        .parse()
                        .ok()
                        .filter(|&n: &u32| (n as usize) < BUFFERS)
                })
                .collect();
            found = list.or(found);
        }
    }
    found.map(|mut list| {
        list.truncate(8);
        list
    })
}

/// `const <type> <name> = <value>;` anywhere in `text`: (byte offset, type, name, value).
/// `lines`: a declaration also ends at the end of its line, as in a comment, where packs list
/// settings with no `;` (`const vec4 colortex6ClearColor = vec4(0.0, 0.0, 0.0, 1.0)`).
fn constants(text: &str, lines: bool) -> Vec<(usize, &str, &str, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("const") {
        let at = from + found;
        from = at + 5;
        let before = text[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let rest = &text[from..];
        let end = if lines {
            rest.find([';', '\n']).unwrap_or(rest.len())
        } else {
            let Some(end) = rest.find(';') else { break };
            end
        };
        let decl = &rest[..end];
        let Some((left, value)) = decl.split_once('=') else {
            continue;
        };
        let mut words = left.split_whitespace();
        if let (Some(ty), Some(name), None) = (words.next(), words.next(), words.next()) {
            out.push((at, ty, name, value.trim()));
        }
    }
    out
}

fn code_line(code: &str, at: usize) -> usize {
    code[..at].matches('\n').count()
}

fn boolean(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn number(text: &str) -> Option<f32> {
    let text = text.trim();
    text.strip_suffix(['f', 'F']).unwrap_or(text).parse().ok()
}

/// `vec4(r, g, b, a)` or `vec4(x)`.
fn vec4(value: &str) -> Option<[f32; 4]> {
    let inner = value
        .strip_prefix("vec4")?
        .trim()
        .strip_prefix('(')?
        .strip_suffix(')')?;
    let parts: Option<Vec<f32>> = inner.split(',').map(number).collect();
    match parts?.as_slice() {
        [x] => Some([*x; 4]),
        [r, g, b, a] => Some([*r, *g, *b, *a]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::tests::zip;
    use crate::{Macros, Pack, preprocess};

    fn stage(text: &str) -> Preprocessed {
        let pack = Pack::from_zip_bytes("t", zip(&[("shaders/composite.fsh", text)])).unwrap();
        preprocess(&pack, "composite.fsh", &Macros::standard())
    }

    #[test]
    fn draw_buffers_and_constants_come_from_live_code() {
        let pp = stage(
            "#version 120\n\
             #define HQ\n\
             /*\n\
             const int colortex2Format = RGBA16F;\n\
             const int gaux4Format = R32F;\n\
             */\n\
             #ifdef HQ\n\
             /* RENDERTARGETS: 0,2,11 */\n\
             #else\n\
             /* DRAWBUFFERS:01 */\n\
             const bool colortex5Clear = true;\n\
             #endif\n\
             const bool colortex5Clear = false;\n\
             const vec4 colortex3ClearColor = vec4(0.5, 0.25, 0.0, 1.0f);\n\
             const bool colortex3MipmapEnabled = true;\n\
             const vec4 gnormalClearColor = vec4(1.0);\n\
             uniform sampler2D gdepth;\n\
             void main() {}\n",
        );
        assert_eq!(draw_buffers(&pp), Some(vec![0, 2, 11]));
        let mut config = Config::default();
        config.read(&pp);
        let b = &config.buffers;
        assert_eq!(b[2].format, Format::parse("RGBA16F").unwrap());
        assert_eq!(b[7].format, Format::parse("R32F").unwrap());
        assert!(!b[5].clear && b[0].clear);
        assert_eq!(b[3].clear_color, Some([0.5, 0.25, 0.0, 1.0]));
        assert!(b[3].mipmap && !b[2].mipmap);
        assert_eq!(b[2].clear_color, Some([1.0; 4]));
        // Iris 1.11.7 kept RGBA8 for `gdepth` (colortex1) with no format given, in probe pack
        // p9063b of ours; OptiFine's RGBA32F rule doesn't apply.
        assert_eq!(b[1].format, Format::RGBA8, "gdepth");
        assert!(config.notes.is_empty(), "{:?}", config.notes);
    }

    #[test]
    fn half_lives_are_read_from_consts() {
        let text = "const float wetnessHalflife = 30.0f;\nconst float drynessHalflife = 50;\n\
                    const int eyeBrightnessHalflife = 4;\n";
        let mut config = Config::default();
        config.read(&stage(text));
        assert_eq!(config.half_lives, [Some(30.0), Some(50.0), Some(4.0), None]);
        assert!(config.notes.is_empty(), "{:?}", config.notes);
    }

    /// Iris 1.11.7 with probe packs of ours (#10490): these five set in a vertex stage didn't
    /// change its shadow projection, shadow camera or sun; in a fragment stage they did.
    #[test]
    fn some_shadow_settings_count_only_in_fragment_shaders() {
        let text = "const float shadowDistance = 48.0;\nconst float sunPathRotation = 20.0;\n\
                    const float shadowIntervalSize = 8.0;\nconst float shadowNearPlane = 0.5;\n\
                    const float shadowFarPlane = 64.0;\nconst int shadowMapResolution = 1024;\n";
        let settings = |s: &Shadow| (s.distance, s.sun_path_rotation, s.interval, s.near, s.far);
        let mut vertex = Config::default();
        vertex.read_vertex(&stage(text));
        assert_eq!(
            settings(&vertex.shadow),
            settings(&Config::default().shadow)
        );
        // One not probed in a vertex stage still counts there, as before.
        assert_eq!(vertex.shadow.resolution, 1024);
        let mut fragment = Config::default();
        fragment.read(&stage(text));
        assert_eq!(
            settings(&fragment.shadow),
            (48.0, 20.0, 8.0, Some(0.5), Some(64.0))
        );
    }

    #[test]
    fn shadow_settings() {
        let mut c = Config::default();
        c.read(&stage(
            "const int shadowMapResolution = 2048;\nconst float shadowDistance = 128.0f;\n\
             const float sunPathRotation = -40.0;\nconst bool shadowHardwareFiltering = true;\n\
             /* const int shadowcolor1Format = RGBA16F; */\nconst bool shadowcolor0Clear = false;\n",
        ));
        let s = c.shadow;
        assert_eq!(
            (s.resolution, s.distance, s.sun_path_rotation),
            (2048, 128.0, -40.0)
        );
        assert_eq!(
            (s.interval, s.fov, s.hardware_filtering),
            (2.0, None, [true; 2])
        );
        assert!(!s.colors[0].clear && s.colors[1].clear);
        assert_eq!(s.colors[1].format, Format::parse("RGBA16F").unwrap());
        assert_eq!(s.colors[1].clear_color, Some([1.0; 4]));
        assert_eq!(s.nearest, [false; 2]);
    }

    #[test]
    fn shadow_depth_filtering_settings() {
        let nearest = |text: &str| {
            let mut c = Config::default();
            c.read(&stage(text));
            c.shadow.nearest
        };
        assert_eq!(
            nearest("const bool shadowtex0Nearest = true;\n"),
            [true, false]
        );
        assert_eq!(
            nearest("const bool shadowtexNearest = true;\n"),
            [true, false]
        );
        assert_eq!(
            nearest("const bool shadow0MinMagNearest = true;\n"),
            [true, false]
        );
        assert_eq!(
            nearest("const bool shadowtex1Nearest = true;\n"),
            [false, true]
        );
        assert_eq!(
            nearest("const bool shadow1MinMagNearest = true;\n"),
            [false, true]
        );
        assert_eq!(
            nearest("const bool shadowtex0Nearest = false;\n"),
            [false; 2]
        );
    }

    #[test]
    fn no_directive_and_conflicts() {
        let pp = stage("void main() {}\n");
        assert_eq!(draw_buffers(&pp), None);
        let a = stage(
            "/*\nconst int colortex1Format = RGB8;\n*/\nconst bool colortex1Clear = maybe;\n",
        );
        let b = stage(
            "/*\nconst int colortex1Format = RGBA16;\nconst int colortex2Format = XYZ;\n*/\n",
        );
        let mut config = Config::default();
        config.read(&a);
        config.read(&b);
        let notes = config.notes.join("\n");
        assert!(
            notes.contains("composite.fsh:4: `colortex1Clear` is not a bool"),
            "{notes}"
        );
        assert!(
            notes.contains("composite.fsh:1: `colortex1Format` changed"),
            "{notes}"
        );
        assert!(notes.contains("unknown format `XYZ`"), "{notes}");
        assert_eq!(config.buffers[1].format, Format::parse("RGBA16").unwrap());
    }

    #[test]
    fn a_format_in_a_comment_counts_where_const_starts_its_line() {
        // The placements of probe packs p9063a and p9063c (ours): Iris 1.11.7 kept RGBA8 for
        // buffers 1, 2, 5 and 6, and took RGBA16F for 3, 4 and 7 to 11.
        let pp = stage(
            "/*\n * const int colortex1Format = RGBA16F;\n */\n\
             /* const int colortex2Format = RGBA16F; */\n\
             /*\nconst int colortex3Format = RGBA16F;\n    const int colortex4Format = RGBA16F;\n\
             const int colortex16Dummy = 1; const int colortex5Format = RGBA16F;\n*/\n\
             /* const int colortex6Format = RGBA16F;\n*/\n\
             /*\nconst int colortex7Format = RGBA16F; */\n\
             /*\nconst int colortex8Format = RGBA16F; trailing words\n\
             \tconst int colortex9Format = RGBA16F;\n\
             const  int  colortex10Format  =  RGBA16F ;\n\
             const int colortex11Format = RGBA16F; const int colortex16Dummy = 1;\n*/\n",
        );
        let mut config = Config::default();
        config.read(&pp);
        let half = Format::parse("RGBA16F").unwrap();
        let read: Vec<bool> = (1..=11).map(|n| config.buffers[n].format == half).collect();
        assert_eq!(
            read,
            [
                false, false, true, true, false, false, true, true, true, true, true
            ]
        );
    }

    #[test]
    fn a_setting_in_a_comment_may_end_at_its_line() {
        let pp = stage(
            "/*\nconst vec4 colortex6ClearColor = vec4(0.0, 0.0, 0.0, 1.0)\n\
             const vec4 colortex7ClearColor = vec4(0.5)\n\nconst bool colortex7Clear = false\n*/\n\
             const vec4 colortex8ClearColor =\n    vec4(0.25, 0.0, 0.0, 1.0);\n",
        );
        let mut config = Config::default();
        config.read(&pp);
        assert!(config.notes.is_empty(), "{:?}", config.notes);
        let b = &config.buffers;
        assert_eq!(b[6].clear_color, Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(b[7].clear_color, Some([0.5; 4]));
        assert!(!b[7].clear);
        // Live code still reads up to the `;`.
        assert_eq!(b[8].clear_color, Some([0.25, 0.0, 0.0, 1.0]));
    }
}
