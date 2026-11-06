//! Custom colours: `optifine/color.properties`, the single colours a pack sets for particles,
//! fog and sky, potions, spawn eggs, map, sheep, collar and dye colours, text and the loading
//! screen, and the custom biome palettes it assigns to blocks (`palette.block.*`).
//!
//! Every key is optional; a key the pack leaves out, or whose value is not a colour, keeps
//! vanilla's. Values are read here once, with the pack; each feature asks for its own.
//!
//! Written from OptiFine's public documentation of the format (`OptiFineDoc/doc/color.properties`
//! and `colormap.properties`); no OptiFine code was read.

use std::collections::HashMap;
use std::path::Path;

use crate::matchers::{BlockMatch, namespaced};
use crate::paths::resolve;
use crate::properties::Properties;

/// Where the file is read, first found first.
const FILES: [&str; 2] = [
    "assets/minecraft/optifine/color.properties",
    "assets/minecraft/mcpatcher/color.properties",
];

/// `0xRRGGBB`.
pub type Rgb = u32;

/// The sixteen dye colours by name, in vanilla's `DyeColor` order.
pub const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

#[derive(Debug, Default, PartialEq)]
pub struct ColorProperties {
    /// The resource path the file was read from (`None` without one).
    pub file: Option<String>,
    pub particle_water: Option<Rgb>,
    pub particle_portal: Option<Rgb>,
    pub fog_nether: Option<Rgb>,
    pub fog_end: Option<Rgb>,
    pub sky_end: Option<Rgb>,
    pub lilypad: Option<Rgb>,
    /// By effect id (`minecraft:speed`).
    pub potions: HashMap<String, Rgb>,
    /// A plain bottle of water (`potion.water`).
    pub potion_water: Option<Rgb>,
    /// Spawn eggs by entity id (`minecraft:creeper`): shell and spots.
    pub eggs: HashMap<String, Egg>,
    /// Map colours by `MapColor` name (`grass`, `white_terracotta`), aliases resolved.
    pub map: HashMap<String, Rgb>,
    /// By `DYES` index.
    pub sheep: [Option<Rgb>; 16],
    pub collar: [Option<Rgb>; 16],
    pub dye: [Option<Rgb>; 16],
    pub text_xpbar: Option<Rgb>,
    pub text_boss: Option<Rgb>,
    pub text_sign: Option<Rgb>,
    /// `§0`..`§f` (0..16) and their shadows (16..32).
    pub text_codes: [Option<Rgb>; 32],
    pub loading: Loading,
    pub clouds: Option<Clouds>,
    /// Milliseconds of an experience orb's colour cycle (`xporb.time`; vanilla's is 628).
    pub xporb_time: Option<u32>,
    /// The default format of custom block colormaps (`palette.format`).
    pub palette_format: Option<Format>,
    /// `palette.block.<image>=<blocks>`: a colormap image (resource path) and its blocks.
    pub palette_blocks: Vec<(String, Vec<BlockMatch>)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Egg {
    pub shell: Option<Rgb>,
    pub spots: Option<Rgb>,
}

/// The resource loading screen's colours and its logo's blend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loading {
    pub background: Option<Rgb>,
    pub bar: Option<Rgb>,
    pub outline: Option<Rgb>,
    pub progress: Option<Rgb>,
    /// `None`: vanilla's; `Some(None)`: `off`; otherwise `src dst srcA dstA`.
    pub blend: Option<Option<[BlendFactor; 4]>>,
}

/// OpenGL's blend factors, as `screen.loading.blend` names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
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

impl BlendFactor {
    fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_uppercase().as_str() {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clouds {
    Fast,
    Fancy,
    Off,
}

/// How a custom colormap is read (`format=`, or `palette.format`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// Vanilla's temperature and downfall triangle, 256x256.
    #[default]
    Vanilla,
    /// A column per biome id, a row per height.
    Grid,
    /// One colour, no image.
    Fixed,
}

impl Format {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "vanilla" => Self::Vanilla,
            "grid" => Self::Grid,
            "fixed" => Self::Fixed,
            _ => return None,
        })
    }
}

/// `rrggbb`, with or without a leading `#` or `0x`.
pub fn rgb(value: &str) -> Option<Rgb> {
    let v = value.trim();
    let v = v
        .strip_prefix('#')
        .or_else(|| v.strip_prefix("0x"))
        .unwrap_or(v);
    if v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit()) {
        u32::from_str_radix(v, 16).ok()
    } else {
        None
    }
}

/// A dye's index in [`DYES`], through the old names (`silver`, `lightBlue`).
fn dye_index(name: &str) -> Option<usize> {
    let name = match name {
        "silver" => "light_gray",
        "lightBlue" => "light_blue",
        other => other,
    };
    DYES.iter().position(|d| *d == name)
}

/// A map colour's name, through the aliases the docs list.
fn map_name(name: &str) -> &str {
    match name {
        "snow" => "white",
        "adobe" => "orange",
        "silver" => "light_gray",
        "lightBlue" => "light_blue",
        other => other,
    }
}

impl ColorProperties {
    /// The pack's file under `root`, or vanilla's colours (all `None`) without one.
    pub fn load(root: &Path) -> Self {
        for file in FILES {
            if let Ok(text) = std::fs::read_to_string(root.join(file)) {
                let mut out = Self::parse(file, &Properties::parse(&text));
                out.file = Some(file.to_owned());
                return out;
            }
        }
        Self::default()
    }

    /// The colours of `p`, a file at resource path `file`.
    pub fn parse(file: &str, p: &Properties) -> Self {
        let get = |key: &str| p.get(key).and_then(rgb);
        let mut out = Self {
            particle_water: get("particle.water"),
            particle_portal: get("particle.portal"),
            fog_nether: get("fog.nether"),
            fog_end: get("fog.end"),
            sky_end: get("sky.end"),
            lilypad: get("lilypad"),
            potion_water: get("potion.water"),
            text_xpbar: get("text.xpbar"),
            text_boss: get("text.boss"),
            text_sign: get("text.sign"),
            loading: Loading {
                background: get("screen.loading"),
                bar: get("screen.loading.bar"),
                outline: get("screen.loading.outline"),
                progress: get("screen.loading.progress"),
                blend: p.get("screen.loading.blend").and_then(loading_blend),
            },
            clouds: p.get("clouds").and_then(|v| match v.trim() {
                "fast" => Some(Clouds::Fast),
                "fancy" => Some(Clouds::Fancy),
                "off" => Some(Clouds::Off),
                _ => None,
            }),
            xporb_time: p
                .get("xporb.time")
                .and_then(|v| v.trim().parse().ok())
                .filter(|&ms| ms > 0),
            palette_format: p.get("palette.format").and_then(Format::parse),
            ..Self::default()
        };
        for (key, value) in &p.entries {
            if let Some(effect) = key.strip_prefix("potion.") {
                if effect != "water"
                    && let Some(c) = rgb(value)
                {
                    out.potions.insert(namespaced(effect), c);
                }
            } else if let Some(rest) = key.strip_prefix("egg.") {
                let Some((part, entity)) = rest.split_once('.') else {
                    continue;
                };
                let Some(c) = rgb(value) else { continue };
                let egg = out.eggs.entry(namespaced(entity)).or_default();
                match part {
                    "shell" => egg.shell = Some(c),
                    "spots" => egg.spots = Some(c),
                    _ => {}
                }
            } else if let Some(name) = key.strip_prefix("map.") {
                if let Some(c) = rgb(value) {
                    out.map.insert(map_name(name).to_owned(), c);
                }
            } else if let Some(n) = key.strip_prefix("text.code.") {
                if let (Ok(n), Some(c)) = (n.parse::<usize>(), rgb(value))
                    && n < 32
                {
                    out.text_codes[n] = Some(c);
                }
            } else if let Some(image) = key.strip_prefix("palette.block.") {
                let blocks = BlockMatch::parse_list(value);
                match resolve(file, image) {
                    Some(path) if !blocks.is_empty() => {
                        let path = if path.ends_with(".png") {
                            path
                        } else {
                            format!("{path}.png")
                        };
                        out.palette_blocks.push((path, blocks));
                    }
                    _ => log::warn!("{file}: {key} names no colormap or no block"),
                }
            } else {
                for (prefix, table) in [
                    ("sheep.", &mut out.sheep),
                    ("collar.", &mut out.collar),
                    ("dye.", &mut out.dye),
                ] {
                    if let Some(name) = key.strip_prefix(prefix)
                        && let (Some(i), Some(c)) = (dye_index(name), rgb(value))
                    {
                        table[i] = Some(c);
                    }
                }
            }
        }
        out
    }

    /// A text colour code's colour (`0..16`), or its shadow's (`shadow`).
    pub fn text_code(&self, code: usize, shadow: bool) -> Option<Rgb> {
        self.text_codes
            .get(code + if shadow { 16 } else { 0 })
            .copied()
            .flatten()
    }
}

/// `off`, or four blend factors.
fn loading_blend(value: &str) -> Option<Option<[BlendFactor; 4]>> {
    if value.trim().eq_ignore_ascii_case("off") {
        return Some(None);
    }
    let factors: Vec<BlendFactor> = value
        .split_whitespace()
        .map(BlendFactor::parse)
        .collect::<Option<_>>()?;
    let [a, b, c, d] = factors[..] else {
        return None;
    };
    Some(Some([a, b, c, d]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "assets/minecraft/optifine/color.properties";

    fn parse(text: &str) -> ColorProperties {
        ColorProperties::parse(FILE, &Properties::parse(text))
    }

    #[test]
    fn colours_are_six_hex_digits() {
        assert_eq!(rgb("334cff"), Some(0x334cff));
        assert_eq!(rgb(" #FF00aa "), Some(0xff00aa));
        assert_eq!(rgb("0x000001"), Some(1));
        assert_eq!(rgb("fff"), None);
        assert_eq!(rgb("12345g"), None);
        assert_eq!(rgb(""), None);
    }

    #[test]
    fn scalars_and_absent_keys() {
        let c = parse(
            "particle.water=ffffff\nfog.nether=330707\nsky.end=zz0000\nlilypad=208030\n\
             text.xpbar=80ff20\nxporb.time=1000\nclouds=fancy\npalette.format=grid",
        );
        assert_eq!(c.particle_water, Some(0xffffff));
        assert_eq!(c.fog_nether, Some(0x330707));
        assert_eq!(c.sky_end, None, "not a colour: vanilla's");
        assert_eq!(c.fog_end, None);
        assert_eq!(c.lilypad, Some(0x208030));
        assert_eq!(c.text_xpbar, Some(0x80ff20));
        assert_eq!(c.xporb_time, Some(1000));
        assert_eq!(c.clouds, Some(Clouds::Fancy));
        assert_eq!(c.palette_format, Some(Format::Grid));
        assert_eq!(parse(""), ColorProperties::default());
    }

    #[test]
    fn potions_eggs_and_text_codes() {
        let c = parse(
            "potion.speed=33ebff\npotion.water=385dc6\npotion.mymod\\:glow=010203\n\
             egg.shell.creeper=0da70b\negg.spots.minecraft\\:creeper=000000\n\
             egg.spots.zombie=nothex\ntext.code.4=ff0000\ntext.code.20=110000\ntext.code.40=ffffff",
        );
        assert_eq!(c.potions["minecraft:speed"], 0x33ebff);
        assert_eq!(c.potions["mymod:glow"], 0x010203);
        assert!(!c.potions.contains_key("minecraft:water"));
        assert_eq!(c.potion_water, Some(0x385dc6));
        assert_eq!(
            c.eggs["minecraft:creeper"],
            Egg {
                shell: Some(0x0da70b),
                spots: Some(0)
            }
        );
        assert!(!c.eggs.contains_key("minecraft:zombie"));
        assert_eq!(c.text_code(4, false), Some(0xff0000));
        assert_eq!(c.text_code(4, true), Some(0x110000));
        assert_eq!(c.text_code(5, false), None);
    }

    #[test]
    fn dye_tables_and_map_read_the_old_names() {
        let c = parse(
            "sheep.silver=111111\nsheep.lightBlue=222222\ncollar.black=333333\ndye.red=444444\n\
             dye.chartreuse=555555\nmap.snow=666666\nmap.grass=777777\nmap.adobe=888888",
        );
        assert_eq!(c.sheep[8], Some(0x111111));
        assert_eq!(c.sheep[3], Some(0x222222));
        assert_eq!(c.collar[15], Some(0x333333));
        assert_eq!(c.dye[14], Some(0x444444));
        assert_eq!(c.dye.iter().flatten().count(), 1);
        assert_eq!(c.map["white"], 0x666666);
        assert_eq!(c.map["grass"], 0x777777);
        assert_eq!(c.map["orange"], 0x888888);
    }

    #[test]
    fn loading_screen() {
        let c = parse(
            "screen.loading=000000\nscreen.loading.progress=e22837\n\
             screen.loading.blend=SRC_ALPHA ONE_MINUS_SRC_ALPHA one zero",
        );
        assert_eq!(c.loading.background, Some(0));
        assert_eq!(c.loading.progress, Some(0xe22837));
        assert_eq!(
            c.loading.blend,
            Some(Some([
                BlendFactor::SrcAlpha,
                BlendFactor::OneMinusSrcAlpha,
                BlendFactor::One,
                BlendFactor::Zero
            ]))
        );
        assert_eq!(parse("screen.loading.blend=off").loading.blend, Some(None));
        assert_eq!(parse("screen.loading.blend=ONE ONE").loading.blend, None);
    }

    #[test]
    fn palette_blocks_resolve_their_images() {
        let c = parse(
            "palette.block.~/colormap/stone.png=stone gold_ore:lit=false\n\
             palette.block./colormap/x=dirt\npalette.block.~/colormap/none.png=",
        );
        assert_eq!(c.palette_blocks.len(), 1);
        let (image, blocks) = &c.palette_blocks[0];
        assert_eq!(image, "assets/minecraft/optifine/colormap/stone.png");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].id, "minecraft:gold_ore");
    }

    #[test]
    fn load_prefers_optifine_over_mcpatcher() {
        let root = std::env::temp_dir().join(format!("horizon-color-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (top, colour) in [("optifine", "010101"), ("mcpatcher", "020202")] {
            let dir = root.join(format!("assets/minecraft/{top}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("color.properties"), format!("lilypad={colour}")).unwrap();
        }
        let c = ColorProperties::load(&root);
        std::fs::remove_dir_all(root.join("assets/minecraft/optifine")).unwrap();
        let legacy = ColorProperties::load(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(c.lilypad, Some(0x010101));
        assert_eq!(c.file.as_deref(), Some(FILE));
        assert_eq!(legacy.lilypad, Some(0x020202));
        assert_eq!(ColorProperties::load(&root), ColorProperties::default());
    }
}
