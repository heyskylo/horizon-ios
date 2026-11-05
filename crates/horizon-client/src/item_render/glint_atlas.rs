//! Vanilla item-atlas coordinates for GUI glint; the HUD's unrelated packing cannot supply them.
use crate::{atlas_sources, hud::Rect};
use horizon_assets::animation::AnimationMetadata;
use std::{collections::HashMap, path::Path};

type Entry = (String, u32, u32);

/// `SpriteSourceList.list` and `SpriteResourceLoader.loadSprite` for the non-mipmapped ITEMS atlas.
pub(super) fn load(
    root: &Path,
    padding: u32,
    maximum: u32,
) -> crate::Result<HashMap<String, Rect>> {
    let sources = atlas_sources::atlases(root);
    let sources = sources
        .get("minecraft:items")
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let textures = atlas_sources::list_textures(root);
    let decode = |name: &str, dir: &str| {
        crate::hud::decode(&atlas_sources::texture_path(root, name, dir)).ok()
    };
    let mut entries = vec![("minecraft:missingno".into(), 16, 16)];
    let mut names = Vec::new();
    for (id, origin) in atlas_sources::list(sources, &textures) {
        names.push(id.clone());
        let dimensions = match &origin {
            atlas_sources::SpriteOrigin::File(texture) => (|| {
                let (w, h, _) = decode(texture, "textures")?;
                let path = atlas_sources::texture_path(root, texture, "textures")
                    .with_extension("png.mcmeta");
                let metadata = horizon_core::prepared_files::read_to_string(&path)
                    .ok()
                    .map(|text| serde_json::from_str::<serde_json::Value>(&text))
                    .transpose()
                    .ok()?;
                if let Some(metadata) = &metadata {
                    metadata.as_object()?;
                    if let Some(texture) = metadata.get("texture") {
                        texture_metadata(texture)?;
                    }
                }
                let size = match metadata.as_ref().and_then(|v| v.get("animation")) {
                    Some(animation) => AnimationMetadata::from_json(&animation.to_string())
                        .ok()?
                        .frame_size(w as i32, h as i32)
                        .map(|x| x as u32),
                    None => [w, h],
                };
                (size[0] > 0
                    && size[1] > 0
                    && w.is_multiple_of(size[0])
                    && h.is_multiple_of(size[1]))
                .then_some((size[0], size[1]))
            })(),
            // Generated sprites have empty resource metadata, so their whole image is one frame.
            _ => atlas_sources::generate(&origin, &decode).map(|(w, h, _)| (w, h)),
        };
        if let Some((w, h)) = dimensions {
            entries.push((id, w, h));
        }
    }
    let mut sprites = stitch(&entries, padding, maximum).ok_or_else(|| {
        std::io::Error::other(format!(
            "item atlas does not fit texture limit {maximum} with padding {padding}"
        ))
    })?;
    // TextureAtlas.getSprite returns missingno for rejected or unplaced source sprites.
    if let Some(missing) = sprites.get("missingno").copied() {
        for name in names {
            sprites
                .entry(super::sprite_name(&name).to_owned())
                .or_insert(missing);
        }
    }
    Ok(sprites)
}

/// `TextureMetadataSection.CODEC`: invalid fields reject the sprite, even without mipmaps.
fn texture_metadata(section: &serde_json::Value) -> Option<()> {
    let fields = section.as_object()?;
    for (name, value) in fields.iter().filter(|(_, value)| !value.is_null()) {
        let valid = match name.as_str() {
            "blur" | "clamp" => value.is_boolean(),
            "mipmap_strategy" => matches!(
                value.as_str(),
                Some("auto" | "mean" | "cutout" | "strict_cutout" | "dark_cutout")
            ),
            "alpha_cutoff_bias" => value.is_number(),
            _ => true,
        };
        if !valid {
            return None;
        }
    }
    Some(())
}

#[derive(Default)]
struct Region {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    entry: Option<usize>,
    children: Vec<Region>,
}
impl Region {
    fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            ..Self::default()
        }
    }
    /// `Stitcher.Region.add`: first fitting child, with vanilla's split order and no rotation.
    fn add(&mut self, index: usize, w: u32, h: u32) -> bool {
        if self.entry.is_some() || w > self.w || h > self.h {
            return false;
        }
        if w == self.w && h == self.h {
            self.entry = Some(index);
            return true;
        }
        if self.children.is_empty() {
            let (dw, dh) = (self.w - w, self.h - h);
            self.children.push(Self::new(self.x, self.y, w, h));
            if dw > 0 && dh > 0 {
                if self.h.max(dw) >= self.w.max(dh) {
                    self.children.push(Self::new(self.x, self.y + h, w, dh));
                    self.children
                        .push(Self::new(self.x + w, self.y, dw, self.h));
                } else {
                    self.children.push(Self::new(self.x + w, self.y, dw, h));
                    self.children
                        .push(Self::new(self.x, self.y + h, self.w, dh));
                }
            } else if dw == 0 {
                self.children.push(Self::new(self.x, self.y + h, w, dh));
            } else {
                self.children.push(Self::new(self.x + w, self.y, dw, h));
            }
        }
        self.children.iter_mut().any(|r| r.add(index, w, h))
    }
    fn gather(
        &self,
        entries: &[Entry],
        size: [u32; 2],
        padding: u32,
        out: &mut HashMap<String, Rect>,
    ) {
        if let Some(i) = self.entry {
            let (name, w, h) = &entries[i];
            let (x, y) = (self.x + padding, self.y + padding);
            out.insert(
                super::sprite_name(name).to_owned(),
                Rect {
                    u0: x as f32 / size[0] as f32,
                    v0: y as f32 / size[1] as f32,
                    u1: (x + w) as f32 / size[0] as f32,
                    v1: (y + h) as f32 / size[1] as f32,
                    w: *w,
                    h: *h,
                },
            );
        }
        for child in &self.children {
            child.gather(entries, size, padding, out);
        }
    }
}

/// `Stitcher.stitch/expand`: mip level zero, padding from the filtering option.
fn stitch(entries: &[Entry], padding: u32, maximum: u32) -> Option<HashMap<String, Rect>> {
    let mut order: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(i, (name, w, h))| {
            let (ns, path) = name.split_once(':').unwrap_or(("minecraft", name));
            (i, w + padding * 2, h + padding * 2, path, ns)
        })
        .collect();
    order.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then(b.1.cmp(&a.1))
            .then(a.3.cmp(b.3))
            .then(a.4.cmp(b.4))
    });
    let mut regions: Vec<Region> = Vec::new();
    let (mut width, mut height) = (0u32, 0u32);
    let pow2 = |x: u32| if x == 0 { 0 } else { x.next_power_of_two() };
    for (index, w, h, _, _) in order {
        if regions.iter_mut().any(|r| r.add(index, w, h)) {
            continue;
        }
        let (next_w, next_h) = (pow2(width.checked_add(w)?), pow2(height.checked_add(h)?));
        let (can_w, can_h) = (next_w <= maximum, next_h <= maximum);
        if !can_w && !can_h {
            return None;
        }
        let (grow_w, grow_h) = (
            can_w && pow2(width) != next_w,
            can_h && pow2(height) != next_h,
        );
        let horizontal = if grow_w != grow_h {
            grow_w
        } else {
            can_w && pow2(width) <= pow2(height)
        };
        let mut region = if horizontal {
            if height == 0 {
                height = next_h;
            }
            let r = Region::new(width, 0, next_w - width, height);
            width = next_w;
            r
        } else {
            let r = Region::new(0, height, width, next_h - height);
            height = next_h;
            r
        };
        // Pinned expand ignores add's result for a sprite wider than the device limit.
        region.add(index, w, h);
        regions.push(region);
    }
    let mut out = HashMap::new();
    for r in regions {
        r.gather(entries, [width, height], padding, &mut out);
    }
    Some(out)
}
