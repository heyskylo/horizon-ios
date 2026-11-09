//! Polytone screen sprite modifiers (`polytone/overlay_modifiers`, wiki "Screen Sprite Modifiers"):
//! a GUI sprite drawn moved, resized, recoloured, with other atlas UVs or another texture, and
//! extra sprites drawn over it. Measured on the real mod (Polytone 26.3-6.8.6): the size grows
//! as the sprite is drawn (a hotbar stretches), `u0`..`v1` are atlas UVs, `color` is the
//! vertex colour, and an overlay goes through its own texture's modifier.

use std::collections::HashMap;
use std::path::Path;

use horizon_polytone::sprites::{SpriteChange, SpriteModifier, sprite_id};

use super::{HudAtlas, Rect as SpriteRect};

/// The packs' modifiers, by the atlas key of the sprite each one changes.
#[derive(Debug, Default)]
pub struct SpriteModifiers {
    by_sprite: HashMap<String, (SpriteChange, Vec<SpriteChange>)>,
}

/// One sprite to draw in place of a modified one: where, which atlas rect and its colour (ABGR).
pub(crate) struct SpriteDraw {
    pub key: String,
    pub rect: SpriteRect,
    pub at: [f32; 4],
    pub color: u32,
}

/// `minecraft:hud/hotbar` as the atlas keys it (`gui/sprites/hud/hotbar`; another namespace's as
/// `ns:gui/sprites/...`).
fn atlas_key(id: &str) -> String {
    match id.split_once(':') {
        Some(("minecraft", path)) => format!("gui/sprites/{path}"),
        Some((namespace, path)) => format!("{namespace}:gui/sprites/{path}"),
        None => format!("gui/sprites/{id}"),
    }
}

/// The sprite id of an atlas key, when it is a GUI sprite.
fn sprite_of(key: &str) -> Option<String> {
    if let Some(path) = key.strip_prefix("gui/sprites/") {
        return Some(sprite_id(path));
    }
    let (namespace, rest) = key.split_once(':')?;
    Some(format!(
        "{namespace}:{}",
        rest.strip_prefix("gui/sprites/")?
    ))
}

impl SpriteModifiers {
    /// The modifiers of the packs merged at `root` for the GUI sprites among `keys` (the atlas's):
    /// for each sprite, the last file that targets it.
    pub fn read<'a>(root: &Path, keys: impl Iterator<Item = &'a String>) -> Self {
        let mut modifiers = Vec::new();
        let namespaces = std::fs::read_dir(root.join("assets"))
            .into_iter()
            .flatten()
            .flatten();
        for namespace in namespaces {
            let mut stack = vec![namespace.path().join("polytone/overlay_modifiers")];
            while let Some(dir) = stack.pop() {
                let mut entries: Vec<_> = std::fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .collect();
                entries.sort_by_key(|e| e.path());
                for entry in entries {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    let parsed = horizon_world::json_file::read::<serde_json::Value>(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|json| SpriteModifier::parse(&json));
                    match parsed {
                        Ok(m) => modifiers.push(m),
                        Err(e) => log::warn!("{}: {e}", path.display()),
                    }
                }
            }
        }
        Self::from_modifiers(modifiers, keys)
    }

    /// `modifiers` (in load order) for the GUI sprites among `keys`: for each sprite, the last that
    /// targets it.
    fn from_modifiers<'a>(
        modifiers: Vec<SpriteModifier>,
        keys: impl Iterator<Item = &'a String>,
    ) -> Self {
        let mut by_sprite = HashMap::new();
        if !modifiers.is_empty() {
            for key in keys {
                let Some(id) = sprite_of(key) else {
                    continue;
                };
                if let Some(m) = modifiers
                    .iter()
                    .rev()
                    .find(|m| m.targets.matches(&id, |_| false))
                {
                    by_sprite.insert(key.clone(), (m.change.clone(), m.overlays.clone()));
                }
            }
        }
        Self { by_sprite }
    }

    pub fn is_empty(&self) -> bool {
        self.by_sprite.is_empty()
    }
}

/// `change` on a sprite at `key` drawn over `at` in `color`: its other texture, atlas UVs,
/// rectangle and colour.
fn apply(
    sprites: &HashMap<String, SpriteRect>,
    key: &str,
    rect: Option<SpriteRect>,
    change: &SpriteChange,
    at: [f32; 4],
    color: u32,
) -> Option<SpriteDraw> {
    // The caller's own cut of the sprite, unless the modifier draws another texture.
    let (key, mut rect) = match (change.texture.as_deref(), rect) {
        (None, Some(rect)) => (key.to_owned(), rect),
        (texture, _) => {
            let key = texture.map_or_else(|| key.to_owned(), atlas_key);
            let rect = *sprites.get(&key)?;
            (key, rect)
        }
    };
    let [u0, v0, u1, v1] = change.uv;
    rect.u0 = u0.unwrap_or(rect.u0);
    rect.v0 = v0.unwrap_or(rect.v0);
    rect.u1 = u1.unwrap_or(rect.u1);
    rect.v1 = v1.unwrap_or(rect.v1);
    let [dx, dy, dw, dh] = change.offset.map(|v| v as f32);
    Some(SpriteDraw {
        key,
        rect,
        at: [at[0] + dx, at[1] + dy, at[2] + dw, at[3] + dh],
        color: change.color.map_or(color, crate::hud_layout::abgr),
    })
}

/// What drawing the sprite `key` (whose atlas `rect` the caller already cut, as a bar's fill) over
/// `at` in `color` (ABGR) draws under the packs' modifiers: the
/// sprite as its modifier changes it, then its overlays (each as its own texture's modifier also
/// changes it). `None` when no modifier targets the sprite.
pub(crate) fn draws(
    atlas: &HudAtlas,
    key: &str,
    rect: SpriteRect,
    at: [f32; 4],
    color: u32,
) -> Option<Vec<SpriteDraw>> {
    atlas
        .sprite_modifiers
        .draws(&atlas.sprites, key, rect, at, color)
}

impl SpriteModifiers {
    fn draws(
        &self,
        sprites: &HashMap<String, SpriteRect>,
        key: &str,
        rect: SpriteRect,
        at: [f32; 4],
        color: u32,
    ) -> Option<Vec<SpriteDraw>> {
        let (change, overlays) = self.by_sprite.get(key)?;
        let base = apply(sprites, key, Some(rect), change, at, color);
        let base_at = base.as_ref().map_or(at, |b| b.at);
        let mut out: Vec<SpriteDraw> = base.into_iter().collect();
        for overlay in overlays {
            let Some(texture) = overlay.texture.as_deref() else {
                continue;
            };
            let key = atlas_key(texture);
            // The overlay's texture as its own modifier draws it, then the overlay's own changes.
            let own = self
                .by_sprite
                .get(&key)
                .map(|(own, _)| SpriteChange {
                    texture: None,
                    ..own.clone()
                })
                .unwrap_or_default();
            let Some(mut d) = apply(sprites, &key, None, &own, base_at, color) else {
                continue;
            };
            let [dx, dy, dw, dh] = overlay.offset.map(|v| v as f32);
            d.at = [d.at[0] + dx, d.at[1] + dy, d.at[2] + dw, d.at[3] + dh];
            let uvs = [
                &mut d.rect.u0,
                &mut d.rect.v0,
                &mut d.rect.u1,
                &mut d.rect.v1,
            ];
            for (slot, uv) in uvs.into_iter().zip(overlay.uv) {
                if let Some(uv) = uv {
                    *slot = uv;
                }
            }
            if let Some(c) = overlay.color {
                d.color = crate::hud_layout::abgr(c);
            }
            out.push(d);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(u0: f32) -> SpriteRect {
        SpriteRect {
            u0,
            v0: 0.0,
            u1: u0 + 0.1,
            v1: 0.1,
            w: 9,
            h: 9,
        }
    }

    #[test]
    fn a_modified_sprite_draws_moved_resized_recoloured_and_then_its_overlays() {
        let sprites: HashMap<String, SpriteRect> = [
            ("gui/sprites/hud/hotbar", rect(0.0)),
            ("gui/sprites/hud/hotbar_selection", rect(0.2)),
            ("gui/sprites/hud/heart/full", rect(0.4)),
            ("gui/sprites/hud/food_full", rect(0.6)),
        ]
        .map(|(k, r)| (k.to_owned(), r))
        .into();

        let modifiers: Vec<SpriteModifier> = [
            serde_json::json!({"texture": "minecraft:hud/hotbar", "x_inc": -20, "width_inc": 40}),
            serde_json::json!({"texture": "minecraft:hud/heart/full", "u0": 0, "u1": 0.5}),
            serde_json::json!({"texture": "minecraft:hud/food_full", "new_texture": "minecraft:hud/heart/full", "color": -65536}),
            serde_json::json!({"texture": "minecraft:hud/hotbar_selection", "overlays": [{"texture": "minecraft:hud/heart/full", "y_inc": -30}]}),
        ]
        .iter()
        .map(|j| SpriteModifier::parse(j).unwrap())
        .collect();
        let mods = SpriteModifiers::from_modifiers(modifiers, sprites.keys());
        let white = 0xFFFF_FFFF;
        // Moved left 20 and 40 wider, the rest as it was.
        let hotbar = &mods
            .draws(
                &sprites,
                "gui/sprites/hud/hotbar",
                rect(0.0),
                [100.0, 200.0, 182.0, 22.0],
                white,
            )
            .unwrap()[0];
        assert_eq!(hotbar.at, [80.0, 200.0, 222.0, 22.0]);
        assert_eq!((hotbar.rect.u0, hotbar.color), (0.0, white));
        // The heart's atlas UVs: u0 and u1 replaced, v kept.
        let heart = &mods
            .draws(
                &sprites,
                "gui/sprites/hud/heart/full",
                rect(0.4),
                [0.0; 4],
                white,
            )
            .unwrap()[0];
        assert_eq!(
            [heart.rect.u0, heart.rect.u1, heart.rect.v1],
            [0.0, 0.5, 0.1]
        );
        // Food drawn with the heart's texture (its own rect, not the heart modifier's UVs), red.
        let food = &mods
            .draws(
                &sprites,
                "gui/sprites/hud/food_full",
                rect(0.6),
                [0.0; 4],
                white,
            )
            .unwrap()[0];
        assert_eq!(
            (food.key.as_str(), food.rect.u0),
            ("gui/sprites/hud/heart/full", 0.4)
        );
        assert_eq!(food.color, crate::hud_layout::abgr(0xFFFF_0000));
        // The selection, then the heart over it 30 higher, with the heart's own UVs.
        let selection = mods
            .draws(
                &sprites,
                "gui/sprites/hud/hotbar_selection",
                rect(0.2),
                [10.0, 50.0, 24.0, 23.0],
                white,
            )
            .unwrap();
        assert_eq!(selection.len(), 2);
        assert_eq!(selection[0].at, [10.0, 50.0, 24.0, 23.0]);
        assert_eq!(selection[1].at, [10.0, 20.0, 24.0, 23.0]);
        assert_eq!((selection[1].rect.u0, selection[1].rect.u1), (0.0, 0.5));
        // A sprite no modifier aims at draws as it is.
        assert!(
            mods.draws(
                &sprites,
                "gui/sprites/hud/crosshair",
                rect(0.9),
                [0.0; 4],
                white
            )
            .is_none()
        );
    }

    #[test]
    fn sprite_ids_and_atlas_keys_turn_into_each_other() {
        assert_eq!(atlas_key("minecraft:hud/hotbar"), "gui/sprites/hud/hotbar");
        assert_eq!(atlas_key("hud/hotbar"), "gui/sprites/hud/hotbar");
        assert_eq!(atlas_key("mypack:hud/bar"), "mypack:gui/sprites/hud/bar");
        assert_eq!(
            sprite_of("gui/sprites/hud/hotbar").as_deref(),
            Some("minecraft:hud/hotbar")
        );
        assert_eq!(
            sprite_of("mypack:gui/sprites/hud/bar").as_deref(),
            Some("mypack:hud/bar")
        );
        assert_eq!(sprite_of("block/stone"), None);
    }
}
