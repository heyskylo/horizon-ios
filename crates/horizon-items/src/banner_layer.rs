//! Banner pattern layers (`BannerPatternLayers`) and their NBT form.
use horizon_world::nbt::Tag;
use std::sync::Arc;

/// `BannerPatternLayers.Layer`: a pattern id and the dye name it is painted in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BannerLayer {
    pub pattern: Arc<str>,
    pub color: Arc<str>,
}
impl BannerLayer {
    pub fn new(pattern: &str, color: &str) -> Self {
        Self {
            pattern: Arc::from(pattern),
            color: Arc::from(color),
        }
    }
}

/// The `banner_patterns` NBT list of layers.
pub fn layers_to_nbt(layers: &[BannerLayer]) -> Tag {
    Tag::List(
        layers
            .iter()
            .map(|l| {
                Tag::compound([
                    ("pattern", Tag::String((&*l.pattern).into())),
                    ("color", Tag::String((&*l.color).into())),
                ])
            })
            .collect(),
    )
}
pub fn layers_from_nbt(tag: Option<&Tag>) -> Vec<BannerLayer> {
    let Some(Tag::List(list)) = tag else {
        return Vec::new();
    };
    // `BannerPatternLayers.CODEC` is `Layer.CODEC.listOf()`: one layer the codec
    // rejects (a missing field, an unknown dye name) fails the whole list, and
    // `BannerBlockEntity.loadAdditional` then falls back to
    // `BannerPatternLayers.EMPTY`.
    let layer = |e: &Tag| -> Option<BannerLayer> {
        let text = |k: &str| match e.get(k) {
            Some(Tag::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        let pattern = text("pattern")?;
        let color = text("color")?;
        if !crate::firework::DYE_FIREWORK_COLORS
            .iter()
            .any(|(name, _)| *name == color)
        {
            return None;
        }
        Some(BannerLayer::new(
            &if pattern.contains(':') {
                pattern
            } else {
                format!("minecraft:{pattern}")
            },
            &color,
        ))
    };
    let mut layers = Vec::with_capacity(list.len());
    for e in list {
        match layer(e) {
            Some(layer) => layers.push(layer),
            None => return Vec::new(),
        }
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(pattern: &str, color: &str) -> Tag {
        Tag::compound([
            ("pattern", Tag::String(pattern.into())),
            ("color", Tag::String(color.into())),
        ])
    }

    /// `BannerPatternLayers.CODEC` is a list codec: one layer it rejects (an
    /// unknown dye name, a missing field) fails the whole list, and
    /// `BannerBlockEntity.loadAdditional` keeps `BannerPatternLayers.EMPTY`, so
    /// the valid layer before it is dropped too.
    #[test]
    fn an_invalid_layer_clears_the_saved_layers() {
        let mixed = Tag::List(vec![layer("brick", "red"), layer("brick", "not_a_color")]);
        assert!(
            layers_from_nbt(Some(&mixed)).is_empty(),
            "one rejected layer means no layers"
        );
        // The same list without the bad layer survives, so this is not an empty
        // answer to every list.
        let valid = Tag::List(vec![layer("brick", "red"), layer("globe", "blue")]);
        let layers = layers_from_nbt(Some(&valid));
        assert_eq!(layers.len(), 2, "both valid layers are kept");
        assert_eq!(layers[0], BannerLayer::new("minecraft:brick", "red"));
        // A layer missing its colour fails the list as well.
        let missing = Tag::List(vec![
            layer("brick", "red"),
            Tag::compound([("pattern", Tag::String("brick".into()))]),
        ]);
        assert!(layers_from_nbt(Some(&missing)).is_empty());
        // And the round trip of a valid list is unchanged.
        assert_eq!(layers_from_nbt(Some(&layers_to_nbt(&layers))), layers);
    }
}
