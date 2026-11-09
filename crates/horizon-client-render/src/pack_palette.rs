//! Polytone's dye, sheep and redstone colours (`dye.<name>.diffuse|text`, `sheep.<name>`,
//! `redstone.<power>`), for drawing only: the pack recolours collars, tropical fish, banners, wool,
//! sign text and redstone wire and dust, while gameplay (crafting, item data, the beacon) keeps
//! vanilla's `DyeColor`. Lock-free: the draw paths and the mesher's workers read it per face.
use std::path::Path;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use crate::block_entity_render::DYES;
use crate::signs::DYE_TEXT;
use horizon_polytone::colors::PackColors;

/// 0 is unset, else `1 << 32 | rgb`.
static DIFFUSE: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static TEXT: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static SHEEP: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static REDSTONE: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];

/// Takes the colours of the packs just loaded (vanilla's where they set none).
pub fn set(colors: &PackColors) {
    let store = |table: &[AtomicU64; 16], i: usize, color: Option<u32>| {
        table[i].store(
            color.map_or(0, |c| 1 << 32 | u64::from(c & 0xFF_FFFF)),
            Relaxed,
        );
    };
    for (i, (name, _)) in DYES.iter().enumerate() {
        store(&DIFFUSE, i, colors.dye(name, "diffuse"));
        store(&TEXT, i, colors.dye(name, "text"));
        store(&SHEEP, i, colors.sheep(name));
        store(&REDSTONE, i, colors.redstone(i as u8));
    }
}

/// An item modifier's `bar_color` of the `damage` type: a row of colours picked by the stack's
/// damage, the first pixel undamaged, the last at full damage (measured on the real mod).
struct DamageBar {
    targets: horizon_polytone::Targets,
    row: Vec<u32>,
}
static ITEM_BARS: RwLock<Vec<DamageBar>> = RwLock::new(Vec::new());

/// Reads the packs' item modifiers' durability bar colours under a pack overlay root: an
/// `<item>_bar.png` alone, or a json whose `bar_color` is `damage` with its `<name>_bar.png` (or
/// `<name>.png`) beside it. Colormap ids and inline colormaps aren't read.
pub fn set_item_bars(root: &Path) {
    let mut bars = Vec::new();
    for namespace in std::fs::read_dir(root.join("assets"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let base = namespace.path().join("polytone/item_modifiers");
        let mut stack = vec![base.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Some(bar) = item_bar(root, &base, &path) {
                    bars.push(bar);
                }
            }
        }
    }
    *ITEM_BARS.write().unwrap_or_else(|e| e.into_inner()) = bars;
}

fn item_bar(root: &Path, base: &Path, path: &Path) -> Option<DamageBar> {
    let name = path.file_name()?.to_str()?;
    let row = |png: &Path| {
        let (width, _, rgba) = crate::sprites::read_rgba(png).ok()?;
        let row: Vec<u32> = rgba[..width as usize * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
            .collect();
        (!row.is_empty()).then_some(row)
    };
    if let Some(stem) = name.strip_suffix("_bar.png") {
        // A json of the same name owns its png.
        if path.with_file_name(format!("{stem}.json")).exists() {
            return None;
        }
        let relative = path.parent()?.strip_prefix(base).ok()?.join(stem);
        let namespace = base.parent()?.parent()?.file_name()?.to_str()?;
        let id = format!("{namespace}:{}", relative.to_str()?.replace('\\', "/"));
        let targets = horizon_polytone::Targets::parse(None, Some(&id)).ok()?;
        return Some(DamageBar {
            targets,
            row: row(path)?,
        });
    }
    let stem = name.strip_suffix(".json")?;
    let json: serde_json::Value = horizon_world::json_file::read(path).ok()?;
    if json.get("bar_color")?.as_str()? != "damage" {
        return None;
    }
    let png = [format!("{stem}_bar.png"), format!("{stem}.png")]
        .into_iter()
        .map(|png| path.with_file_name(png))
        .find(|png| png.exists())?;
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
    let implicit = horizon_polytone::implicit_id(&relative, "item_modifiers");
    let targets =
        horizon_polytone::Targets::parse(json.get("targets"), implicit.as_deref()).ok()?;
    Some(DamageBar {
        targets,
        row: row(&png)?,
    })
}

/// A pack's durability bar colour (RGB) for an item at this damage (0 to 1).
pub fn item_bar_color(item: &str, damage: f32) -> Option<u32> {
    let bars = ITEM_BARS.read().unwrap_or_else(|e| e.into_inner());
    let bar = bars
        .iter()
        .rev()
        .find(|b| b.targets.matches(item, |_| false))?;
    let last = (bar.row.len() - 1) as f32;
    Some(bar.row[(damage.clamp(0.0, 1.0) * last).round() as usize])
}

fn read(table: &[AtomicU64; 16], i: usize) -> Option<u32> {
    let v = table[i & 15].load(Relaxed);
    (v != 0).then_some(v as u32)
}

/// `DyeColor.getTextureDiffuseColor` (RGB) as the packs draw it.
pub fn diffuse(dye: usize) -> u32 {
    read(&DIFFUSE, dye).unwrap_or(DYES[dye & 15].1)
}

/// `DyeColor.getTextColor` (RGB) as the packs draw it.
pub fn text(dye: usize) -> u32 {
    read(&TEXT, dye).unwrap_or(DYE_TEXT[dye & 15].1)
}

/// A pack's `RedStoneWireBlock.COLORS` entry for this power (RGB), when it sets one.
pub fn redstone(power: u8) -> Option<u32> {
    read(&REDSTONE, usize::from(power))
}

/// `ColorLerper.Type.SHEEP` wool colour: a pack's `sheep.<name>`, else white 0xE6E6E6 and the
/// others 75% of vanilla's dye (a pack's `dye.*.diffuse` doesn't reach wool).
pub fn sheep_wool(dye: u8) -> u32 {
    let i = usize::from(dye & 15);
    if let Some(color) = read(&SHEEP, i) {
        return color;
    }
    if i == 0 {
        return 0xE6E6E6;
    }
    let src = DYES[i].1;
    let f = |c: u32| ((c as f32 * 0.75).floor() as u32).min(255);
    f(src >> 16 & 255) << 16 | f(src >> 8 & 255) << 8 | f(src & 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_recolours_only_what_it_names() {
        // Dyes no other test draws: the table is process-wide.
        let dir = std::env::temp_dir().join(format!("pack-dyes-{}", std::process::id()));
        let polytone = dir.join("assets/obs/polytone");
        std::fs::create_dir_all(&polytone).unwrap();
        std::fs::write(
            polytone.join("colors.json"),
            r#"{"dye": {"cyan": {"diffuse": "00ff00", "text": "0000ff"}}, "sheep": {"pink": "2040ff"}, "redstone": {"7": "0x123456"}}"#,
        )
        .unwrap();
        set(&PackColors::read(&dir));
        let (cyan, pink, purple) = (9, 6, 10);
        assert_eq!(diffuse(cyan), 0x00ff00);
        assert_eq!(text(cyan), 0x0000ff);
        assert_eq!(sheep_wool(pink as u8), 0x2040ff);
        // Wool reads vanilla's dye, not the pack's diffuse: 75% of 0x169C9C.
        assert_eq!(sheep_wool(cyan as u8), 0x107575);
        assert_eq!(diffuse(purple), 0x8932B8);
        assert_eq!(text(purple), 0xA020F0);
        assert_eq!((redstone(7), redstone(15)), (Some(0x123456), None));
        set(&PackColors::default());
        assert_eq!(redstone(7), None);
        assert_eq!(diffuse(cyan), 0x169C9C);
        assert_eq!(sheep_wool(pink as u8), 0xB6687F);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn png_row(path: &Path, pixels: &[[u8; 3]]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, pixels.len() as u32, 1);
        encoder.set_color(png::ColorType::Rgba);
        let data: Vec<u8> = pixels
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&data)
            .unwrap();
    }

    #[test]
    fn a_damage_bar_picks_its_pixel_by_the_stacks_damage() {
        let dir = std::env::temp_dir().join(format!("pack-bars-{}", std::process::id()));
        let modifiers = dir.join("assets/minecraft/polytone/item_modifiers");
        let rgby = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
        png_row(&modifiers.join("diamond_pickaxe_bar.png"), &rgby);
        std::fs::write(
            modifiers.join("tools.json"),
            r#"{"targets": ["minecraft:iron_axe"], "bar_color": "damage"}"#,
        )
        .unwrap();
        png_row(&modifiers.join("tools_bar.png"), &[[1, 2, 3], [4, 5, 6]]);
        set_item_bars(&dir);
        // The real mod: damage 200 of 1561 shows the first pixel, 1400 the last.
        assert_eq!(
            item_bar_color("minecraft:diamond_pickaxe", 200.0 / 1561.0),
            Some(0xFF0000)
        );
        assert_eq!(
            item_bar_color("minecraft:diamond_pickaxe", 1400.0 / 1561.0),
            Some(0xFFFF00)
        );
        assert_eq!(item_bar_color("minecraft:iron_axe", 0.9), Some(0x040506));
        assert_eq!(item_bar_color("minecraft:diamond_shovel", 0.5), None);
        set_item_bars(&dir.join("none"));
        assert_eq!(item_bar_color("minecraft:diamond_pickaxe", 0.5), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
