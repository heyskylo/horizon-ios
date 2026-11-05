//! EquipmentLayerRenderer's dyeable-layer color decision, in the renderer's ABGR order.

/// Presence distinguishes an optionally dyed layer from a permanently white layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dyeable {
    /// No default means the layer is invisible until the stack has a dye color.
    pub color_when_undyed: Option<u32>,
}

/// Zero means skip the layer entirely. An explicit RGB zero remains opaque black.
pub fn layer_color(dyeable: Option<Dyeable>, dyed: Option<u32>) -> u32 {
    let Some(dyeable) = dyeable else {
        return 0xFFFF_FFFF;
    };
    let Some(rgb) = dyed.or(dyeable.color_when_undyed) else {
        return 0;
    };
    0xFF00_0000 | (rgb & 0xFF) << 16 | (rgb & 0xFF00) | (rgb >> 16) & 0xFF
}
