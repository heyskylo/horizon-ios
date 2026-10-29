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
    list.iter()
        .filter_map(|e| {
            let text = |k: &str| match e.get(k) {
                Some(Tag::String(s)) => Some(s.to_string_lossy()),
                _ => None,
            };
            let pattern = text("pattern")?;
            let color = text("color")?;
            Some(BannerLayer::new(
                &if pattern.contains(':') {
                    pattern
                } else {
                    format!("minecraft:{pattern}")
                },
                &color,
            ))
        })
        .collect()
}
