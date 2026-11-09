//! Screen sprite modifiers (`assets/<namespace>/polytone/overlay_modifiers/**.json`, wiki "Screen
//! Sprite Modifiers"): a GUI sprite moved, resized, recoloured, given other UVs or another
//! texture, and extra sprites drawn over it.

use crate::Targets;
use serde_json::Value as Json;

/// What a modifier does to one sprite: the base's, or one of its overlays'.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpriteChange {
    /// `x_inc`, `y_inc`, `width_inc`, `height_inc`, in GUI pixels.
    pub offset: [i32; 4],
    /// `u0`, `v0`, `u1`, `v1`: the atlas UVs that replace the sprite's (observed on the real mod:
    /// they are the atlas's, not the sprite's own 0 to 1).
    pub uv: [Option<f32>; 4],
    /// `color`: ARGB.
    pub color: Option<u32>,
    /// `new_texture`, or an overlay's `texture`: the sprite drawn instead (relative to
    /// `textures/gui/sprites/`).
    pub texture: Option<String>,
}

/// A screen sprite modifier: the sprites it targets, what it does to them and its overlays.
#[derive(Clone, Debug)]
pub struct SpriteModifier {
    pub targets: Targets,
    pub change: SpriteChange,
    pub overlays: Vec<SpriteChange>,
}

impl SpriteModifier {
    pub fn parse(json: &Json) -> Result<Self, String> {
        if json.get("texture").is_none() {
            return Err("a screen sprite modifier needs a texture".into());
        }
        let targets = Targets::parse(json.get("texture"), None)?;
        let overlays = match json.get("overlays") {
            None => Vec::new(),
            Some(Json::Array(list)) => list
                .iter()
                .map(|o| {
                    let mut change = change(o)?;
                    change.texture = Some(
                        o.get("texture")
                            .and_then(Json::as_str)
                            .ok_or("an overlay needs a texture")?
                            .to_owned(),
                    );
                    Ok(change)
                })
                .collect::<Result<_, String>>()?,
            Some(_) => return Err("overlays is not a list".into()),
        };
        let mut base = change(json)?;
        base.texture = json
            .get("new_texture")
            .and_then(Json::as_str)
            .map(str::to_owned);
        Ok(Self {
            targets,
            change: base,
            overlays,
        })
    }
}

fn change(json: &Json) -> Result<SpriteChange, String> {
    let int = |key: &str| json.get(key).and_then(Json::as_f64).map_or(0, |v| v as i32);
    let float = |key: &str| json.get(key).and_then(Json::as_f64).map(|v| v as f32);
    let color = match json.get("color") {
        None => None,
        Some(v) => Some(
            v.as_i64()
                .map(|c| c as u32)
                .or_else(|| {
                    v.as_str()
                        .and_then(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
                })
                .ok_or("color is not an int")?,
        ),
    };
    Ok(SpriteChange {
        offset: ["x_inc", "y_inc", "width_inc", "height_inc"].map(int),
        uv: ["u0", "v0", "u1", "v1"].map(float),
        color,
        texture: None,
    })
}

/// A sprite id as targets name it: `hud/hotbar` is `minecraft:hud/hotbar`.
pub fn sprite_id(path: &str) -> String {
    if path.contains(':') {
        path.to_owned()
    } else {
        format!("minecraft:{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_modifier_reads_its_changes_and_overlays() {
        let m = SpriteModifier::parse(&json!({
            "texture": "minecraft:hud/hotbar", "x_inc": -5, "width_inc": 10, "color": -65536,
            "u1": 0.5, "new_texture": "my_pack:hud/bar",
            "overlays": [{"texture": "my_pack:hud/glow", "y_inc": -2, "height_inc": 4}]
        }))
        .unwrap();
        assert!(m.targets.matches("minecraft:hud/hotbar", |_| false));
        assert!(
            !m.targets
                .matches("minecraft:hud/hotbar_selection", |_| false)
        );
        assert_eq!(m.change.offset, [-5, 0, 10, 0]);
        assert_eq!(m.change.uv, [None, None, Some(0.5), None]);
        assert_eq!(m.change.color, Some(0xFFFF_0000));
        assert_eq!(m.change.texture.as_deref(), Some("my_pack:hud/bar"));
        assert_eq!(m.overlays[0].texture.as_deref(), Some("my_pack:hud/glow"));
        assert_eq!(m.overlays[0].offset, [0, -2, 0, 4]);
        assert!(SpriteModifier::parse(&json!({"x_inc": 1})).is_err());
        assert!(
            SpriteModifier::parse(&json!({"texture": "a:b", "overlays": [{"x_inc": 1}]})).is_err()
        );
    }
}
