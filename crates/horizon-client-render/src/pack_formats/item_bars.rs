//! Polytone's item modifiers' durability bar colours, on the engine's
//! [`crate::pack_palette::ItemBars`].
use std::path::Path;
use std::sync::Arc;

/// An item modifier's `bar_color` of the `damage` type: a row of colours picked by the stack's
/// damage, the first pixel undamaged, the last at full damage (measured on the real mod).
struct DamageBar {
    targets: horizon_polytone::Targets,
    row: Vec<u32>,
}

/// The packs' damage bars, the last read first.
struct Bars(Vec<DamageBar>);

impl crate::pack_palette::ItemBars for Bars {
    fn color(&self, item: &str, damage: f32) -> Option<u32> {
        let bar = self
            .0
            .iter()
            .rev()
            .find(|b| b.targets.matches(item, |_| false))?;
        let last = (bar.row.len() - 1) as f32;
        Some(bar.row[(damage.clamp(0.0, 1.0) * last).round() as usize])
    }
}

/// Reads the packs' item modifiers' durability bar colours under a pack overlay root: an
/// `<item>_bar.png` alone, or a json whose `bar_color` is `damage` with its `<name>_bar.png` (or
/// `<name>.png`) beside it. Colormap ids and inline colormaps aren't read. `None` without any.
pub fn read(root: &Path) -> Option<Arc<dyn crate::pack_palette::ItemBars>> {
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
    (!bars.is_empty()).then(|| Arc::new(Bars(bars)) as _)
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let bars = read(&dir).unwrap();
        // The real mod: damage 200 of 1561 shows the first pixel, 1400 the last.
        assert_eq!(
            bars.color("minecraft:diamond_pickaxe", 200.0 / 1561.0),
            Some(0xFF0000)
        );
        assert_eq!(
            bars.color("minecraft:diamond_pickaxe", 1400.0 / 1561.0),
            Some(0xFFFF00)
        );
        assert_eq!(bars.color("minecraft:iron_axe", 0.9), Some(0x040506));
        assert_eq!(bars.color("minecraft:diamond_shovel", 0.5), None);
        assert!(read(&dir.join("none")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
