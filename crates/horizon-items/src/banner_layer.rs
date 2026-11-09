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
    // `BannerPatternLayers.CODEC` is a list codec: a layer it rejects (an
    // unknown colour, a missing field) is left out, and `TagValueInput.read`
    // keeps the partial list of the valid layers.
    list.iter()
        .filter_map(|e| {
            let text = |k: &str| match e.get(k) {
                Some(Tag::String(s)) => Some(s.to_string_lossy()),
                _ => None,
            };
            let pattern = text("pattern")?;
            let color = text("color")?;
            crate::firework::DYE_FIREWORK_COLORS
                .iter()
                .any(|(name, _)| *name == color)
                .then(|| {
                    BannerLayer::new(
                        &if pattern.contains(':') {
                            pattern
                        } else {
                            format!("minecraft:{pattern}")
                        },
                        &color,
                    )
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_with_an_unknown_colour_is_left_out() {
        let tag = Tag::List(vec![
            Tag::compound([
                ("pattern", Tag::String("brick".into())),
                ("color", Tag::String("red".into())),
            ]),
            Tag::compound([
                ("pattern", Tag::String("brick".into())),
                ("color", Tag::String("not_a_color".into())),
            ]),
        ]);
        let layers = layers_from_nbt(Some(&tag));
        assert_eq!(
            layers.len(),
            1,
            "the list codec's partial keeps the valid layer"
        );
        assert_eq!(layers[0], BannerLayer::new("minecraft:brick", "red"));
    }
}
