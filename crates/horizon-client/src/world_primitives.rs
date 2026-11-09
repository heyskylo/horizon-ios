//! What a client mod draws in the world each frame through `native_world_primitives`
//! (docs/native-mods.md): lines, box outlines, filled boxes, beacon-style beams and text facing
//! the camera, in world coordinates with ARGB colours. The game tessellates them
//! (`game/world_primitives.rs`) and the renderer draws them with its own pipelines
//! (`gfx::world_primitives`), so a mod never touches wgpu for them.

/// The primitives of one frame, in the order the mods added them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldPrimitives {
    primitives: Vec<WorldPrimitive>,
}

/// One primitive and how it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldPrimitive {
    pub shape: PrimitiveShape,
    /// Seen through walls: drawn over everything, without a depth test. Otherwise the world
    /// in front of it hides it.
    pub through_walls: bool,
}

/// A primitive's shape, in world coordinates. Colours are ARGB (`0xAARRGGBB`): an alpha below
/// 255 blends it over the world.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum PrimitiveShape {
    /// A line `width` pixels wide.
    Line {
        from: [f64; 3],
        to: [f64; 3],
        color: u32,
        width: f32,
    },
    /// The twelve edges of a box, `width` pixels wide.
    BoxOutline {
        min: [f64; 3],
        max: [f64; 3],
        color: u32,
        width: f32,
    },
    /// The six faces of a box, seen from inside too.
    FilledBox {
        min: [f64; 3],
        max: [f64; 3],
        color: u32,
    },
    /// A beacon's beam: its rotating core and translucent glow, scrolling as a beacon's does,
    /// `height` blocks up from `base` (the centre of its foot). The colour's alpha scales the
    /// beacon's own (opaque core, faint glow).
    Beam {
        base: [f64; 3],
        height: u32,
        color: u32,
    },
    /// Text in the game's font, facing the camera, centred on `at`. `scale` 1 is a name tag's
    /// size; with `scale_with_distance` it grows with its distance from the camera, keeping
    /// on screen the size it has [`TEXT_REFERENCE_DISTANCE`] blocks away. `background` puts a
    /// name tag's translucent black behind it.
    Text {
        at: [f64; 3],
        text: String,
        color: u32,
        scale: f32,
        scale_with_distance: bool,
        background: bool,
    },
}

/// Where text with `scale_with_distance` has the size it keeps.
pub const TEXT_REFERENCE_DISTANCE: f64 = 8.0;

/// The tallest beam drawn, as a beacon's last section.
pub const MAX_BEAM_HEIGHT: u32 = 2048;

impl WorldPrimitives {
    /// Adds a primitive, depth tested unless it says otherwise.
    pub fn push(&mut self, shape: PrimitiveShape) -> &mut WorldPrimitive {
        self.primitives.push(WorldPrimitive {
            shape,
            through_walls: false,
        });
        self.primitives.last_mut().expect("just pushed")
    }
    pub fn line(
        &mut self,
        from: [f64; 3],
        to: [f64; 3],
        color: u32,
        width: f32,
    ) -> &mut WorldPrimitive {
        self.push(PrimitiveShape::Line {
            from,
            to,
            color,
            width,
        })
    }
    pub fn box_outline(
        &mut self,
        min: [f64; 3],
        max: [f64; 3],
        color: u32,
        width: f32,
    ) -> &mut WorldPrimitive {
        self.push(PrimitiveShape::BoxOutline {
            min,
            max,
            color,
            width,
        })
    }
    pub fn filled_box(&mut self, min: [f64; 3], max: [f64; 3], color: u32) -> &mut WorldPrimitive {
        self.push(PrimitiveShape::FilledBox { min, max, color })
    }
    pub fn beam(&mut self, base: [f64; 3], height: u32, color: u32) -> &mut WorldPrimitive {
        self.push(PrimitiveShape::Beam {
            base,
            height,
            color,
        })
    }
    /// Text of a name tag's size times `scale`, without a background.
    pub fn text(
        &mut self,
        at: [f64; 3],
        text: impl Into<String>,
        color: u32,
        scale: f32,
    ) -> &mut WorldPrimitive {
        self.push(PrimitiveShape::Text {
            at,
            text: text.into(),
            color,
            scale,
            scale_with_distance: false,
            background: false,
        })
    }
    pub fn iter(&self) -> std::slice::Iter<'_, WorldPrimitive> {
        self.primitives.iter()
    }
    pub fn len(&self) -> usize {
        self.primitives.len()
    }
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }
}

impl WorldPrimitive {
    /// Seen through walls ([`Self::through_walls`]).
    pub fn through_walls(&mut self) -> &mut Self {
        self.through_walls = true;
        self
    }
    /// Text keeps its size on screen as it moves away; nothing for other shapes.
    pub fn scale_with_distance(&mut self) -> &mut Self {
        if let PrimitiveShape::Text {
            scale_with_distance,
            ..
        } = &mut self.shape
        {
            *scale_with_distance = true;
        }
        self
    }
    /// Text on a name tag's background; nothing for other shapes.
    pub fn background(&mut self) -> &mut Self {
        if let PrimitiveShape::Text { background, .. } = &mut self.shape {
            *background = true;
        }
        self
    }
}

/// The world size of one font pixel of text at `scale`, `distance` blocks from the camera: a
/// name tag's 0.025 times `scale`, times the distance over [`TEXT_REFERENCE_DISTANCE`] when it
/// scales with distance. Non-finite or negative scales draw nothing (0).
pub fn text_unit(scale: f32, scale_with_distance: bool, distance: f64) -> f32 {
    if !scale.is_finite() || scale <= 0.0 {
        return 0.0;
    }
    let unit = 0.025 * scale;
    if scale_with_distance {
        unit * (distance / TEXT_REFERENCE_DISTANCE) as f32
    } else {
        unit
    }
}

/// ARGB as the world's vertex colour (RGBA bytes in memory), its alpha times `alpha` / 255.
pub fn vertex_color(argb: u32, alpha: u32) -> u32 {
    let a = (argb >> 24) * alpha / 255;
    let [r, g, b] = [argb >> 16 & 255, argb >> 8 & 255, argb & 255];
    a << 24 | b << 16 | g << 8 | r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_its_screen_size_only_when_it_scales_with_distance() {
        // A name tag's font pixel is 0.025 blocks.
        assert_eq!(text_unit(1.0, false, 50.0), 0.025);
        assert_eq!(text_unit(2.0, false, 1.0), 0.05);
        // Twice as far, twice as big: the same angle, so the same size on screen.
        assert_eq!(text_unit(1.0, true, 8.0), 0.025);
        assert_eq!(text_unit(1.0, true, 16.0), 0.05);
        assert_eq!(text_unit(f32::NAN, false, 1.0), 0.0);
        assert_eq!(text_unit(-1.0, true, 1.0), 0.0);
    }

    #[test]
    fn argb_becomes_rgba_bytes() {
        assert_eq!(
            vertex_color(0x80FF_2010, 255).to_le_bytes(),
            [0xFF, 0x20, 0x10, 0x80]
        );
        assert_eq!(vertex_color(0xFFFF_FFFF, 32).to_le_bytes()[3], 32);
    }
}
