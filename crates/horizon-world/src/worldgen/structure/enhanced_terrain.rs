//! Structure libraries' enhanced terrain adaptation: YUNG's API's and
//! MoogsStructureLib's, which their Beardifier mixins add to vanilla's
//! density. Carried as `"horizon:enhanced_terrain_adaptation"` on a structure
//! and on its pool elements (docs/mods.md); the kernel itself is
//! [`TerrainKernel`].
use super::{
    BoundingBox, Rotation,
    jigsaw::Projection,
    start::{Piece, StructureStart},
};
use crate::worldgen::DecodeError;
use horizon_core::{
    density::{KernelJunction, KernelPiece, StructureBounds, TerrainKernel},
    position::ChunkPos,
};
use serde_json::Value;
use std::sync::Arc;

/// One adaptation: the kernel, and how a piece's box is moved, padded or
/// cut to a band before the kernel measures from it.
#[derive(Debug)]
pub struct Adaptation {
    kernel: Arc<TerrainKernel>,
    /// `bottom_offset`, cast to whole blocks as the libraries do.
    offset: i32,
    /// `padding`: x, top, bottom, z.
    padding: [i32; 4],
    band: Option<Band>,
}
/// MoogsStructureLib's band: the kernel measures from `bottom..=top` above
/// the piece's floor instead of its whole height, for pieces as tall as one
/// of `heights` (any piece without it), and junctions add nothing.
#[derive(Debug)]
struct Band {
    bottom: i32,
    top: i32,
    heights: Option<Vec<i32>>,
}
impl Adaptation {
    pub fn decode(v: &Value) -> Result<Self, DecodeError> {
        let int = |v: &Value, key: &'static str| -> Result<i32, DecodeError> {
            v[key]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or(DecodeError::Field(key))
        };
        let optional = |v: &Value, key: &str| v[key].as_i64().map_or(0, |n| n as i32);
        let action = |key: &'static str| match v[key].as_str() {
            Some("bury") => Ok(1),
            Some("carve") => Ok(-1),
            Some("none") => Ok(0),
            _ => Err(DecodeError::Field(key)),
        };
        let size = int(v, "kernel_size")?;
        let distance = int(v, "kernel_distance")?;
        if size < 0 || distance < 0 {
            return Err(DecodeError::Field("kernel_size"));
        }
        let padding = &v["padding"];
        let band = match v.get("band") {
            Some(b) => Some(Band {
                bottom: int(b, "bottom")?,
                top: int(b, "top")?,
                heights: b["piece_heights"].as_array().map(|h| {
                    h.iter()
                        .filter_map(Value::as_i64)
                        .map(|n| n as i32)
                        .collect()
                }),
            }),
            None => None,
        };
        Ok(Self {
            kernel: Arc::new(TerrainKernel::new(
                size,
                distance,
                action("top")?,
                action("bottom")?,
            )),
            offset: v["bottom_offset"].as_f64().unwrap_or(0.) as i32,
            padding: ["x", "top", "bottom", "z"].map(|k| optional(padding, k)),
            band,
        })
    }
    fn radius(&self) -> i32 {
        self.kernel.radius()
    }
    /// The box the kernel measures from for a piece with this box and
    /// rotation; `None` for a band piece of another height.
    fn piece_box(&self, original: BoundingBox, rotation: Rotation) -> Option<BoundingBox> {
        let [x, top, bottom, z] = self.padding;
        // The x padding runs along the axis the piece's east turned to.
        let (px, pz) = match rotation {
            Rotation::None | Rotation::Clockwise180 => (x, z),
            Rotation::Clockwise90 | Rotation::Counterclockwise90 => (z, x),
        };
        let mut b = original.moved([0, self.offset, 0]);
        b.min[0] -= px;
        b.max[0] += px;
        b.min[2] -= pz;
        b.max[2] += pz;
        match &self.band {
            Some(band) => {
                let height = original.max[1] - original.min[1] + 1;
                if band.heights.as_ref().is_some_and(|h| !h.contains(&height)) {
                    return None;
                }
                let floor = original.min[1] + self.offset;
                b.min[1] = floor + band.bottom;
                b.max[1] = floor + band.top;
            }
            None => {
                b.max[1] += top;
                b.min[1] -= bottom;
            }
        }
        Some(b)
    }
}

/// A structure's adaptation and its library's rules.
#[derive(Debug)]
pub struct StructureAdaptation {
    adaptation: Arc<Adaptation>,
    /// YUNG's API adds rigid pieces' junctions too; MoogsStructureLib only
    /// terrain-matching pieces'.
    rigid_junctions: bool,
    /// YUNG's API grows the start's box by the kernel radius, so chunks that
    /// far out reference the start.
    pub inflate: bool,
}
impl StructureAdaptation {
    pub fn decode(v: &Value) -> Result<Self, DecodeError> {
        Ok(Self {
            adaptation: Arc::new(Adaptation::decode(v)?),
            rigid_junctions: v["rigid_junctions"].as_bool().unwrap_or(false),
            inflate: v["inflate"].as_bool().unwrap_or(false),
        })
    }
    pub fn radius(&self) -> i32 {
        self.adaptation.radius()
    }
    /// EnhancedBeardifierHelper.forStructuresInChunk for one start: its
    /// kernel pieces and junctions near `chunk`, and the box they reach.
    pub fn collect(
        &self,
        start: &StructureStart,
        chunk: ChunkPos,
        pieces: &mut Vec<KernelPiece>,
        junctions: &mut Vec<KernelJunction>,
    ) -> Option<BoundingBox> {
        let reach = start
            .pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Pool(p) => p.element.enhanced_terrain().map(Adaptation::radius),
                Piece::Legacy(_) => None,
            })
            .fold(self.radius(), i32::max);
        if reach <= 0 {
            return None;
        }
        let (bx, bz) = (chunk.x * 16, chunk.z * 16);
        let mut affected: Option<BoundingBox> = None;
        let mut include = |b: BoundingBox, radius: i32| {
            let b = b.inflated(radius, radius);
            match &mut affected {
                Some(a) => a.encapsulate(&b),
                None => affected = Some(b),
            }
        };
        for piece in &start.pieces {
            let b = piece.bounding_box();
            // StructurePiece.isCloseToChunk.
            if !(b.max[0] >= bx - reach
                && b.min[0] <= bx + 15 + reach
                && b.max[2] >= bz - reach
                && b.min[2] <= bz + 15 + reach)
            {
                continue;
            }
            let Piece::Pool(pool) = piece else {
                if let Some(b) = self
                    .adaptation
                    .piece_box(b, Rotation::None)
                    .filter(|_| !self.adaptation.kernel.is_none())
                {
                    push_piece(pieces, &self.adaptation, b, &mut include);
                }
                continue;
            };
            let adaptation = pool.element.enhanced_terrain().unwrap_or(&self.adaptation);
            if adaptation.kernel.is_none() {
                continue;
            }
            let rigid = pool.element.projection() == Projection::Rigid;
            if rigid && let Some(b) = adaptation.piece_box(b, pool.rotation) {
                push_piece(pieces, adaptation, b, &mut include);
            }
            if (rigid && !self.rigid_junctions) || adaptation.band.is_some() {
                continue;
            }
            let radius = adaptation.radius();
            for j in &pool.junctions {
                if j.source_x > bx - radius
                    && j.source_z > bz - radius
                    && j.source_x < bx + 15 + radius
                    && j.source_z < bz + 15 + radius
                {
                    let source = [
                        j.source_x,
                        j.source_ground_y + adaptation.offset,
                        j.source_z,
                    ];
                    junctions.push(KernelJunction {
                        source,
                        kernel: adaptation.kernel.clone(),
                    });
                    include(BoundingBox::new(source, source), radius);
                }
            }
        }
        affected
    }
}

fn push_piece(
    pieces: &mut Vec<KernelPiece>,
    adaptation: &Adaptation,
    b: BoundingBox,
    include: &mut impl FnMut(BoundingBox, i32),
) {
    // Java's BoundingBox puts inverted bounds (a padding below zero) in order.
    let b = BoundingBox::from_corners(b.min, b.max);
    let Some(bounds) = StructureBounds::new(b.min, b.max) else {
        return;
    };
    pieces.push(KernelPiece {
        bounds,
        kernel: adaptation.kernel.clone(),
    });
    include(b, adaptation.radius());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::structure::{
        LazyTemplate, StructureTemplate,
        jigsaw::{Junction, PoolElement, PoolPiece},
    };
    use serde_json::json;

    fn adaptation(v: Value) -> Adaptation {
        Adaptation::decode(&v).unwrap()
    }

    #[test]
    fn piece_boxes_move_pad_and_cut_to_bands() {
        let a = adaptation(
            json!({"kernel_size": 24, "kernel_distance": 24, "top": "carve",
            "bottom": "none", "bottom_offset": 2.9,
            "padding": {"x": 2, "z": 5, "top": 3, "bottom": 1}}),
        );
        let b = BoundingBox::new([0, 10, 0], [9, 19, 9]);
        // Offset 2 (truncated), then the paddings.
        let none = a.piece_box(b, Rotation::None).unwrap();
        assert_eq!((none.min, none.max), ([-2, 11, -5], [11, 24, 14]));
        // A quarter turn puts the x padding along z.
        let turned = a.piece_box(b, Rotation::Clockwise90).unwrap();
        assert_eq!((turned.min, turned.max), ([-5, 11, -2], [14, 24, 11]));
        let band = adaptation(
            json!({"kernel_size": 16, "kernel_distance": 8, "top": "carve",
            "bottom": "carve", "band": {"bottom": 17, "top": 23, "piece_heights": [24, 25]}}),
        );
        let tall = band
            .piece_box(BoundingBox::new([0, 30, 0], [9, 53, 9]), Rotation::None)
            .unwrap();
        assert_eq!((tall.min[1], tall.max[1]), (47, 53));
        assert!(band.piece_box(b, Rotation::None).is_none());
        assert!(Adaptation::decode(&json!({"kernel_size": 24})).is_err());
    }

    fn piece(element: &Arc<PoolElement>, x: i32, junction: [i32; 3]) -> Piece {
        Piece::Pool(PoolPiece {
            element: element.clone(),
            position: [x, 64, 0],
            ground_level_delta: 1,
            rotation: Rotation::None,
            bounding_box: BoundingBox::new([x, 64, 0], [x + 9, 73, 9]),
            junctions: vec![Junction {
                source_x: junction[0],
                source_ground_y: junction[1],
                source_z: junction[2],
                delta_y: 0,
                dest_projection: Projection::Rigid,
            }],
            waterlogging: false,
        })
    }

    fn element(projection: Projection, own: Option<Adaptation>) -> Arc<PoolElement> {
        Arc::new(PoolElement::Single {
            template: LazyTemplate::loaded(Arc::new(StructureTemplate::empty())),
            processors: Default::default(),
            processors_source: serde_json::Value::Null,
            legacy: false,
            projection,
            waterlogging: None,
            enhanced_terrain: own.map(Arc::new),
            ground_level: 1,
        })
    }

    /// YUNG's API adds every piece's junctions, MoogsStructureLib only those
    /// of pieces that follow the terrain; a piece's own NONE adds nothing,
    /// and pieces beyond the kernel's reach of the chunk are left out.
    #[test]
    fn collects_pieces_and_junctions_as_each_library_does() {
        let rigid = element(Projection::Rigid, None);
        let matching = element(Projection::TerrainMatching, None);
        let none = element(
            Projection::Rigid,
            Some(adaptation(json!({"kernel_size": 0, "kernel_distance": 0,
                "top": "none", "bottom": "none"}))),
        );
        let start = StructureStart {
            structure: 0,
            name: "test".into(),
            chunk: ChunkPos::new(0, 0),
            references: 0,
            pieces: vec![
                piece(&rigid, 0, [5, 64, 5]),
                piece(&matching, 10, [12, 65, 5]),
                piece(&none, 20, [22, 64, 5]),
                piece(&rigid, 60, [62, 64, 5]),
            ],
            bounding_box: BoundingBox::new([0, 64, 0], [69, 73, 9]),
        };
        let kernel = json!({"kernel_size": 24, "kernel_distance": 24, "top": "carve",
            "bottom": "none", "bottom_offset": -1});
        for (yung, junctions) in [(true, 2), (false, 1)] {
            let mut v = kernel.clone();
            v["rigid_junctions"] = yung.into();
            let s = StructureAdaptation::decode(&v).unwrap();
            let (mut pieces, mut found) = (Vec::new(), Vec::new());
            let affected = s
                .collect(&start, ChunkPos::new(0, 0), &mut pieces, &mut found)
                .unwrap();
            assert_eq!(pieces.len(), 1, "the rigid piece near the chunk");
            assert_eq!(pieces[0].bounds.minimum(), [0, 63, 0]);
            assert_eq!(found.len(), junctions);
            assert_eq!(found.last().unwrap().source, [12, 64, 5]);
            assert_eq!((affected.min, affected.max), ([-12, 51, -12], [24, 84, 21]));
        }
    }
}
