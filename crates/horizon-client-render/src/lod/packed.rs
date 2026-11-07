//! The compact layout of a far tile's solid quads in the arena: one 24-byte
//! record each instead of four 24-byte vertices.
//!
//! A far tile's quads are not blocks' faces -- they are merged rectangles of a
//! voxel column's exposed side, hundreds of blocks wide -- so none of them fits
//! `mesher::packed`'s record, whose coordinates are a block grid of 16. They
//! are far more regular than a chunk's quads instead: a rectangle in an
//! axis-aligned plane, with one colour, one light and one sprite layer across
//! all four corners, and UVs that `lod::mesh::emit_quad` derives from the
//! positions. So a record holds what that function was given, and
//! terrain.wgsl's `far_corner` does what it did:
//!
//! - word 0: the colour (RGBA8), the same for all four corners;
//! - word 1: the sprite layer, and the light in the high half;
//! - word 2: the plane, the coordinate all four corners share, as an `i32`;
//! - word 3: the rectangle's low edge along the face's first axis, as an `i32`;
//! - word 4: its low edge along the second, as an `i32`;
//! - word 5: the plane's axis, whether the corners are reversed, the
//!   rectangle's width and height, and `FULL` clear.
//!
//! Nothing here reasons about which record is right for a quad: `pack` builds
//! one, rebuilds the four vertices from it with the same code the shader
//! mirrors, and keeps it only if they come back bit for bit. A quad no record
//! reproduces -- a fractional coordinate, a rectangle too wide, a face whose
//! UVs were scaled down to fit (`lod::mesh::MAX_PIECES`) -- is kept as its four
//! vertices, exactly as `mesher::packed` keeps one.

use crate::mesher::{TerrainVertex, packed};

/// Word 5's bit of a quad kept as four vertices; word 0 is then the slots from
/// the record to its first vertex. The same bit `mesher::packed` uses, so the
/// two layouts place their records and spilled vertices alike.
pub use crate::mesher::packed::FULL;

/// The draws' base vertex of a compact far mesh, above `packed::PACKED_BASE`:
/// `FAR_BASE + 4 * slot + corner` is a corner of the quad whose record is at
/// `slot`. terrain.wgsl's `section_corner` reads the layout off the index, so
/// a far tile and a chunk section can share one arena and one pipeline.
pub const FAR_BASE: u32 = 1 << 29;

/// Whether far tiles meshed from now on get this layout: `HORIZON_FAR_PACK=0`
/// stops them and leaves chunk sections' own layout alone, so the two can be
/// told apart in one build. `HORIZON_PACKED_QUADS=0` stops both.
pub fn far_packing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_FAR_PACK").as_deref() != Ok("0"))
}

/// The widest rectangle a record's 11-bit width and height hold. `lod::mesh`
/// splits a face at `MAX_FACE` = 1024 blocks unless it scales its UVs instead,
/// and a scaled face does not pack, so this is never the binding limit.
const MAX_EXTENT: i64 = (1 << 11) - 1;

/// Which axis a face's plane lies on, and what `lod::mesh::emit_quad` makes of
/// the other two. The names are that function's: `u` is the rectangle's first
/// axis and `v` its second, and the UV of a corner is `[u, -v]` of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    /// Up and Down: the plane is y, u is x and v is z.
    Y,
    /// North and South: the plane is z, u is x and v is y.
    Z,
    /// West and East: the plane is x, u is z and v is y.
    X,
}

impl Axis {
    const ALL: [Self; 3] = [Self::Y, Self::Z, Self::X];

    fn bits(self) -> u32 {
        match self {
            Self::Y => 0,
            Self::Z => 1,
            Self::X => 2,
        }
    }

    fn from_bits(bits: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.bits() == bits)
    }
}

/// A face as `lod::mesh::emit_quad` was given it, which is all a record holds.
#[derive(Clone, Copy, Debug)]
struct Face {
    axis: Axis,
    /// Whether the corners go round the other way, as they do for Down, South
    /// and East (that function's `points.reverse()`).
    reversed: bool,
    plane: i32,
    u0: i32,
    v0: i32,
    du: u32,
    dv: u32,
    layer: u16,
    light: u16,
    color: u32,
}

impl Face {
    /// The four vertices of it, as `lod::mesh::emit_quad` writes them. Kept
    /// here and not shared with that function because it is what the shader
    /// mirrors: the two have to agree, and `a_record_rebuilds_what_the_mesher_
    /// emitted` is what holds them to it.
    fn vertices(self) -> [TerrainVertex; 4] {
        let (p, u0, v0) = (self.plane as f32, self.u0 as f32, self.v0 as f32);
        let u1 = u0 + self.du as f32;
        let v1 = v0 + self.dv as f32;
        let mut points = match self.axis {
            Axis::Y => [[u0, p, v1], [u1, p, v1], [u1, p, v0], [u0, p, v0]],
            Axis::Z => [[u0, v1, p], [u1, v1, p], [u1, v0, p], [u0, v0, p]],
            Axis::X => [[p, v1, u1], [p, v1, u0], [p, v0, u0], [p, v0, u1]],
        };
        if self.reversed {
            points.reverse();
        }
        let uvs = points.map(|pos| match self.axis {
            Axis::Y => [pos[0], -pos[2]],
            Axis::Z => [pos[0], -pos[1]],
            Axis::X => [pos[2], -pos[1]],
        });
        let corner = [0, 1].map(|i| {
            uvs.iter()
                .map(|uv| uv[i])
                .fold(f32::INFINITY, f32::min)
                .floor()
        });
        let mut out = [TerrainVertex::tiled([0.0; 3], [0.0; 2], 0, 0, 0); 4];
        for ((to, pos), uv) in out.iter_mut().zip(points).zip(uvs) {
            *to = TerrainVertex::tiled(
                pos,
                [uv[0] - corner[0], uv[1] - corner[1]],
                self.layer,
                self.color,
                self.light,
            );
        }
        out
    }

    fn words(self) -> [u32; 6] {
        [
            self.color,
            u32::from(self.layer) | u32::from(self.light) << 16,
            self.plane as u32,
            self.u0 as u32,
            self.v0 as u32,
            self.axis.bits() | u32::from(self.reversed) << 2 | self.du << 3 | self.dv << 14,
        ]
    }

    /// The face a record names, or `None` for a record that spills its quad's
    /// vertices (`FULL`) or that does not name an axis.
    fn from_words(w: [u32; 6]) -> Option<Self> {
        if w[5] & FULL != 0 {
            return None;
        }
        Some(Self {
            axis: Axis::from_bits(w[5] & 3)?,
            reversed: (w[5] >> 2) & 1 != 0,
            plane: w[2] as i32,
            u0: w[3] as i32,
            v0: w[4] as i32,
            du: (w[5] >> 3) & 0x7ff,
            dv: (w[5] >> 14) & 0x7ff,
            layer: w[1] as u16,
            light: (w[1] >> 16) as u16,
            color: w[0],
        })
    }
}

/// A whole number in `i32`, or `None`.
fn whole(v: f32) -> Option<i32> {
    (v.fract() == 0.0 && (f32::from(i16::MIN) * 16.0..=f32::from(i16::MAX) * 16.0).contains(&v))
        .then_some(v as i32)
}

/// `quad`'s record, if one gives back its four vertices exactly.
pub fn pack(quad: &[TerrainVertex; 4]) -> Option<[u32; 6]> {
    let first = quad[0];
    // One colour, one light and one layer across the four corners, which is
    // what lets a record hold them once.
    if quad
        .iter()
        .any(|v| (v.color, v.light, v.layer) != (first.color, first.light, first.layer))
    {
        return None;
    }
    let lo = [0, 1, 2].map(|k| quad.iter().map(|v| v.pos[k]).fold(f32::INFINITY, f32::min));
    let hi = [0, 1, 2].map(|k| {
        quad.iter()
            .map(|v| v.pos[k])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    // Every axis and winding the mesher can have emitted; the round trip below
    // says which one it was, so nothing here has to work it out.
    for axis in Axis::ALL {
        let (plane, u, v) = match axis {
            Axis::Y => (1, 0, 2),
            Axis::Z => (2, 0, 1),
            Axis::X => (0, 2, 1),
        };
        if lo[plane] != hi[plane] {
            continue;
        }
        let (Some(plane), Some(u0), Some(v0)) = (whole(lo[plane]), whole(lo[u]), whole(lo[v]))
        else {
            continue;
        };
        let (Some(u1), Some(v1)) = (whole(hi[u]), whole(hi[v])) else {
            continue;
        };
        let (du, dv) = (i64::from(u1 - u0), i64::from(v1 - v0));
        if du > MAX_EXTENT || dv > MAX_EXTENT {
            continue;
        }
        for reversed in [false, true] {
            let face = Face {
                axis,
                reversed,
                plane,
                u0,
                v0,
                du: du as u32,
                dv: dv as u32,
                layer: first.layer,
                light: first.light,
                color: first.color,
            };
            let words = face.words();
            // The record is kept only where it gives the quad back exactly, so
            // a quad this layout cannot hold is spilled rather than altered.
            if bytemuck::cast::<_, [[u32; 6]; 4]>(face.vertices())
                == bytemuck::cast::<_, [[u32; 6]; 4]>(*quad)
            {
                return Some(words);
            }
        }
    }
    None
}

/// The four corners a record names, as the shader rebuilds them; `None` for a
/// record that spills its quad's vertices. For the tests that hold this layout
/// and `lod::mesh` to each other.
#[cfg(test)]
pub fn corners(words: [u32; 6]) -> Option<[TerrainVertex; 4]> {
    Face::from_words(words).map(Face::vertices)
}

/// Lays a far tile's solid quads out compactly in `mesh.packed`, where the
/// renderer reads that layout. Call it after `SectionMesh::sort_solid`, which
/// reorders the quads and clears whatever layout was there.
pub fn lay_out(mesh: &mut crate::mesher::SectionMesh) {
    if !packed::packing() || !far_packing() {
        return;
    }
    mesh.packed = layout(&mesh.solid);
    mesh.packed_far = !mesh.packed.is_empty();
}

/// A far tile's solid quads laid out compactly: a record each in mesh order,
/// then the vertices of the quads no record fits. Empty where that would not
/// be smaller than the vertices.
pub fn layout(solid: &[TerrainVertex]) -> Vec<[u32; 6]> {
    let quads = solid.as_chunks::<4>().0;
    let records: Vec<Option<[u32; 6]>> = quads.iter().map(pack).collect();
    packed::place(&records, quads)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four corners as bits: `TerrainVertex` has no `PartialEq`, and bit
    /// identity is the guarantee the layout makes in any case.
    fn words_of(quad: [TerrainVertex; 4]) -> [[u32; 6]; 4] {
        bytemuck::cast(quad)
    }

    fn face() -> Face {
        Face {
            axis: Axis::Y,
            reversed: false,
            plane: 71,
            u0: -48,
            v0: 112,
            du: 16,
            dv: 7,
            layer: 3,
            light: 0xF0F0,
            color: 0xFF_80_40_20,
        }
    }

    /// Every axis and winding survives the words, so `from_words` and the
    /// shader's `far_corner` read what `words` wrote.
    #[test]
    fn a_face_survives_its_record() {
        for axis in Axis::ALL {
            for reversed in [false, true] {
                let want = Face {
                    axis,
                    reversed,
                    ..face()
                };
                let got = Face::from_words(want.words()).expect("a record names a face");
                assert_eq!(
                    words_of(got.vertices()),
                    words_of(want.vertices()),
                    "{axis:?} {reversed}"
                );
                assert_eq!((got.axis, got.reversed), (axis, reversed));
                assert_eq!((got.plane, got.u0, got.v0), (want.plane, want.u0, want.v0));
                assert_eq!((got.du, got.dv), (want.du, want.dv));
                assert_eq!(
                    (got.layer, got.light, got.color),
                    (want.layer, want.light, want.color)
                );
            }
        }
    }

    /// The widest and the most distant rectangle a record claims to hold comes
    /// back exactly, so the bit fields are wide enough for what they say.
    #[test]
    fn the_extremes_a_record_claims_come_back() {
        let want = Face {
            du: MAX_EXTENT as u32,
            dv: MAX_EXTENT as u32,
            plane: i32::from(i16::MAX),
            u0: -8_000_000,
            v0: 8_000_000,
            layer: u16::MAX,
            light: u16::MAX,
            color: u32::MAX,
            ..face()
        };
        let got = Face::from_words(want.words()).expect("a record names a face");
        assert_eq!((got.du, got.dv), (want.du, want.dv));
        assert_eq!((got.plane, got.u0, got.v0), (want.plane, want.u0, want.v0));
        assert_eq!(
            (got.layer, got.light, got.color),
            (want.layer, want.light, want.color)
        );
    }

    /// Any quad the mesher could have emitted packs, and unpacks to the very
    /// vertices it emitted: this is the whole guarantee the layout makes.
    #[test]
    fn a_record_rebuilds_what_the_mesher_emitted() {
        let mut packed = 0;
        for axis in Axis::ALL {
            for reversed in [false, true] {
                for (du, dv) in [(1, 1), (16, 1), (1, 16), (37, 91), (1024, 1024)] {
                    let want = Face {
                        axis,
                        reversed,
                        du,
                        dv,
                        ..face()
                    };
                    let quad = want.vertices();
                    let words = pack(&quad).expect("the mesher's own quad packs");
                    let back = Face::from_words(words).expect("a record names a face");
                    assert_eq!(
                        words_of(back.vertices()),
                        words_of(quad),
                        "{axis:?} {reversed} {du}x{dv}"
                    );
                    packed += 1;
                }
            }
        }
        assert_eq!(packed, 30, "every case packed");
    }

    /// A quad this layout cannot hold is refused rather than altered: the
    /// corners disagreeing on a colour, a fractional coordinate, and a
    /// rectangle wider than the width field.
    #[test]
    fn a_quad_no_record_fits_is_refused() {
        let mut uneven = face().vertices();
        uneven[2].color ^= 1;
        assert!(pack(&uneven).is_none(), "corners differ in colour");

        let mut fraction = face().vertices();
        fraction[0].pos[0] += 0.5;
        assert!(pack(&fraction).is_none(), "a fractional coordinate");

        let wide = Face {
            du: MAX_EXTENT as u32 + 1,
            ..face()
        };
        assert!(pack(&wide.vertices()).is_none(), "wider than the field");
    }

    /// A tile of quads that all pack takes a quarter of the slots its vertices
    /// would, which is the point of the layout.
    #[test]
    fn a_tile_that_packs_takes_a_quarter_of_the_slots() {
        let mut solid = Vec::new();
        for n in 0..64 {
            solid.extend(
                Face {
                    u0: n * 4,
                    ..face()
                }
                .vertices(),
            );
        }
        let out = layout(&solid);
        assert_eq!(out.len(), solid.len() / 4, "a slot a quad, none spilled");
        for (quad, record) in solid.as_chunks::<4>().0.iter().zip(&out) {
            let face = Face::from_words(*record).expect("a record names a face");
            assert_eq!(
                words_of(face.vertices()),
                words_of(*quad),
            );
        }
    }

    /// A tile whose quads mostly spill keeps the vertices and gives up on the
    /// layout rather than growing: `place` is what decides, and this holds it
    /// to the same rule `mesher::packed` follows.
    #[test]
    fn a_tile_that_mostly_spills_keeps_its_vertices() {
        let mut solid = Vec::new();
        for n in 0..64 {
            let mut quad = Face {
                u0: n * 4,
                ..face()
            }
            .vertices();
            quad[1].color ^= 1;
            solid.extend(quad);
        }
        assert!(layout(&solid).is_empty(), "no layout smaller than vertices");
    }
}
