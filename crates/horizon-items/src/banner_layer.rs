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
    // `BannerPatternLayers.CODEC` reads the whole list: one layer the codec
    // rejects (an unknown colour, a missing field) leaves no layers at all.
    let mut layers = Vec::with_capacity(list.len());
    for e in list {
        let text = |k: &str| match e.get(k) {
            Some(Tag::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        let Some(pattern) = text("pattern") else {
            return Vec::new();
        };
        let Some(color) = text("color") else {
            return Vec::new();
        };
        if !crate::firework::DYE_FIREWORK_COLORS
            .iter()
            .any(|(name, _)| *name == color)
        {
            return Vec::new();
        }
        layers.push(BannerLayer::new(
            &if pattern.contains(':') {
                pattern
            } else {
                format!("minecraft:{pattern}")
            },
            &color,
        ));
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invalid_layer_clears_the_saved_layers() {
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
        assert!(
            layers_from_nbt(Some(&tag)).is_empty(),
            "one layer the codec rejects leaves no layers"
        );
    }
}
