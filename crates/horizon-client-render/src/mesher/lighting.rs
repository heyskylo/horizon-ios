//! Vertex light and colour of model quads, following `BlockModelLighter`
//! (smooth light and ambient occlusion, including the partial-face weights)
//! and `ModelBlockRenderer`'s integer colour arithmetic (`ARGB.gray`,
//! `scaleRGB`, `multiply`).
use super::{Info, Padded, pi};
use crate::models::catalog::QuadShape;
use horizon_assets::model::Direction as D;

/// `LightCoordsUtil.FULL_BRIGHT`.
pub(super) const FULL_BRIGHT: u32 = 0x00F0_00F0;

/// `CardinalLighting.DEFAULT`/`NETHER` by face.
pub(super) fn cardinal(nether: bool, face: D) -> f32 {
    match face {
        D::Down if nether => 0.9,
        D::Up if nether => 0.9,
        D::Down => 0.5,
        D::Up => 1.0,
        D::North | D::South => 0.8,
        D::West | D::East => 0.6,
    }
}
/// A face's brightness when a shader pack lights the faces (`Mesher::shader_pack_lighting`): no
/// directional shading, every face at the dimension's up-face value (1.0, the Nether's 0.9).
/// Our probe pack's Iris 1.11.7 values fit this: 0.9 on every Nether face, 1.0 in the overworld.
pub(super) fn pack_flat(nether: bool) -> f32 {
    cardinal(nether, D::Up)
}
/// The normals of `D`'s variants, in the order [`ADJACENCY`] is indexed by.
const NORMALS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
fn normal(face: D) -> [i32; 3] {
    NORMALS[face as usize]
}
pub(super) fn step(p: [i32; 3], face: D) -> [i32; 3] {
    let n = normal(face);
    [p[0] + n[0], p[1] + n[1], p[2] + n[2]]
}
/// The block state of the padded cell at index `i` (see `pi`, `step_index`).
#[inline]
pub(super) fn state_at<'a>(pad: &Padded, info: &'a [Info], i: usize) -> &'a Info {
    &info[pad.blocks[i] as usize]
}
/// `LightCoordsUtil.getLightCoords`: light at `p` as seen through `state`
/// (block level << 4 | sky level << 20).
pub(super) fn coords(pad: &Padded, state: &Info, p: [i32; 3]) -> u32 {
    packed_coords(state, pad.light[pi(p[0], p[1], p[2])])
}
/// `coords` of the cell at index `i`, as seen through `state`.
#[inline]
pub(super) fn coords_at(pad: &Padded, state: &Info, i: usize) -> u32 {
    packed_coords(state, pad.light[i])
}
/// `coords` of a stored light byte (sky level << 4 | block level).
pub(super) fn packed_coords(state: &Info, packed: u8) -> u32 {
    if state.emissive {
        return FULL_BRIGHT;
    }
    let block = u32::from((packed & 15).max(state.emission));
    block << 4 | u32::from(packed >> 4) << 20
}
/// `LightCoordsUtil.max`.
pub(super) fn max_coords(a: u32, b: u32) -> u32 {
    (a >> 4 & 15).max(b >> 4 & 15) << 4 | (a >> 20 & 15).max(b >> 20 & 15) << 20
}
/// `LightCoordsUtil.lightCoordsWithEmission` (a quad's element light emission), with
/// NeoForge's separate block and sky minimums: `[emission; 2]` is vanilla's.
pub(super) fn with_min(coords: u32, [block, sky]: [u8; 2]) -> u32 {
    if block == 0 && sky == 0 {
        return coords;
    }
    (coords >> 4 & 15).max(u32::from(block)) << 4 | (coords >> 20 & 15).max(u32::from(sky)) << 20
}
/// `LightCoordsUtil.smoothBlend`: zero samples take the centre; samples without
/// sky light borrow the centre's sky.
pub(super) fn smooth_blend(mut a: u32, mut b: u32, mut c: u32, center: u32) -> u32 {
    if center >> 20 & 15 > 2 || center >> 4 & 15 > 2 {
        for n in [&mut a, &mut b, &mut c] {
            if *n == 0 {
                *n = center;
            } else if *n >> 20 & 15 == 0 {
                *n |= center & 0x00FF_0000;
            }
        }
    }
    (a + b + c + center) >> 2 & 0x00FF_00FF
}
/// `LightCoordsUtil.smoothWeightedBlend` (Java float sums and int narrowing).
fn weighted_blend(c: [u32; 4], w: [f32; 4]) -> u32 {
    let channel = |shift: u32| {
        let v = |i: usize| (c[i] >> shift & 0xFF) as f32 * w[i];
        (v(0) + v(1) + v(2) + v(3)) as i32 as u32 & 0xFF
    };
    channel(0) | channel(16) << 16
}
/// `ARGB.gray` channel: `Mth.floor(brightness * 255)`. Without the `floorf`
/// call x86-64's baseline instruction set makes of `f32::floor`: truncation,
/// one lower where that rounded a negative value up (saturating as `as` does).
pub(super) fn gray(brightness: f32) -> i32 {
    let v = brightness * 255.0;
    let whole = v as i32;
    if whole as f32 > v {
        whole.saturating_sub(1)
    } else {
        whole
    }
}
/// One channel of `ARGB.scaleRGB(color, float)`.
pub(super) fn scale(channel: i32, by: f32) -> i32 {
    ((channel as f32 * by) as i32).clamp(0, 255)
}
// SizeInfo indices into the face shape.
const DOWN: u8 = 0;
const UP: u8 = 1;
const NORTH: u8 = 2;
const SOUTH: u8 = 3;
const WEST: u8 = 4;
const EAST: u8 = 5;
const FLIP_DOWN: u8 = 6;
const FLIP_UP: u8 = 7;
const FLIP_NORTH: u8 = 8;
const FLIP_SOUTH: u8 = 9;
const FLIP_WEST: u8 = 10;
const FLIP_EAST: u8 = 11;
struct Adjacency {
    corners: [D; 4],
    /// Face-shape factor pairs for each vertex's four corner weights.
    weights: [[u8; 8]; 4],
    /// Output vertex for each of the four computed corners.
    remap: [usize; 4],
}
const fn adjacency(face: D) -> Adjacency {
    use D::*;
    match face {
        Down => Adjacency {
            corners: [West, East, North, South],
            weights: [
                [
                    FLIP_WEST, SOUTH, FLIP_WEST, FLIP_SOUTH, WEST, FLIP_SOUTH, WEST, SOUTH,
                ],
                [
                    FLIP_WEST, NORTH, FLIP_WEST, FLIP_NORTH, WEST, FLIP_NORTH, WEST, NORTH,
                ],
                [
                    FLIP_EAST, NORTH, FLIP_EAST, FLIP_NORTH, EAST, FLIP_NORTH, EAST, NORTH,
                ],
                [
                    FLIP_EAST, SOUTH, FLIP_EAST, FLIP_SOUTH, EAST, FLIP_SOUTH, EAST, SOUTH,
                ],
            ],
            remap: [0, 1, 2, 3],
        },
        Up => Adjacency {
            corners: [East, West, North, South],
            weights: [
                [
                    EAST, SOUTH, EAST, FLIP_SOUTH, FLIP_EAST, FLIP_SOUTH, FLIP_EAST, SOUTH,
                ],
                [
                    EAST, NORTH, EAST, FLIP_NORTH, FLIP_EAST, FLIP_NORTH, FLIP_EAST, NORTH,
                ],
                [
                    WEST, NORTH, WEST, FLIP_NORTH, FLIP_WEST, FLIP_NORTH, FLIP_WEST, NORTH,
                ],
                [
                    WEST, SOUTH, WEST, FLIP_SOUTH, FLIP_WEST, FLIP_SOUTH, FLIP_WEST, SOUTH,
                ],
            ],
            remap: [2, 3, 0, 1],
        },
        North => Adjacency {
            corners: [Up, Down, East, West],
            weights: [
                [UP, FLIP_WEST, UP, WEST, FLIP_UP, WEST, FLIP_UP, FLIP_WEST],
                [UP, FLIP_EAST, UP, EAST, FLIP_UP, EAST, FLIP_UP, FLIP_EAST],
                [
                    DOWN, FLIP_EAST, DOWN, EAST, FLIP_DOWN, EAST, FLIP_DOWN, FLIP_EAST,
                ],
                [
                    DOWN, FLIP_WEST, DOWN, WEST, FLIP_DOWN, WEST, FLIP_DOWN, FLIP_WEST,
                ],
            ],
            remap: [3, 0, 1, 2],
        },
        South => Adjacency {
            corners: [West, East, Down, Up],
            weights: [
                [UP, FLIP_WEST, FLIP_UP, FLIP_WEST, FLIP_UP, WEST, UP, WEST],
                [
                    DOWN, FLIP_WEST, FLIP_DOWN, FLIP_WEST, FLIP_DOWN, WEST, DOWN, WEST,
                ],
                [
                    DOWN, FLIP_EAST, FLIP_DOWN, FLIP_EAST, FLIP_DOWN, EAST, DOWN, EAST,
                ],
                [UP, FLIP_EAST, FLIP_UP, FLIP_EAST, FLIP_UP, EAST, UP, EAST],
            ],
            remap: [0, 1, 2, 3],
        },
        West => Adjacency {
            corners: [Up, Down, North, South],
            weights: [
                [
                    UP, SOUTH, UP, FLIP_SOUTH, FLIP_UP, FLIP_SOUTH, FLIP_UP, SOUTH,
                ],
                [
                    UP, NORTH, UP, FLIP_NORTH, FLIP_UP, FLIP_NORTH, FLIP_UP, NORTH,
                ],
                [
                    DOWN, NORTH, DOWN, FLIP_NORTH, FLIP_DOWN, FLIP_NORTH, FLIP_DOWN, NORTH,
                ],
                [
                    DOWN, SOUTH, DOWN, FLIP_SOUTH, FLIP_DOWN, FLIP_SOUTH, FLIP_DOWN, SOUTH,
                ],
            ],
            remap: [3, 0, 1, 2],
        },
        East => Adjacency {
            corners: [Down, Up, North, South],
            weights: [
                [
                    FLIP_DOWN, SOUTH, FLIP_DOWN, FLIP_SOUTH, DOWN, FLIP_SOUTH, DOWN, SOUTH,
                ],
                [
                    FLIP_DOWN, NORTH, FLIP_DOWN, FLIP_NORTH, DOWN, FLIP_NORTH, DOWN, NORTH,
                ],
                [
                    FLIP_UP, NORTH, FLIP_UP, FLIP_NORTH, UP, FLIP_NORTH, UP, NORTH,
                ],
                [
                    FLIP_UP, SOUTH, FLIP_UP, FLIP_SOUTH, UP, FLIP_SOUTH, UP, SOUTH,
                ],
            ],
            remap: [1, 2, 3, 0],
        },
    }
}
const ADJACENCY: [Adjacency; 6] = [
    adjacency(D::Down),
    adjacency(D::Up),
    adjacency(D::North),
    adjacency(D::South),
    adjacency(D::West),
    adjacency(D::East),
];
fn shape(positions: &[[f32; 3]; 4], face: D, full: bool) -> QuadShape {
    QuadShape::new(positions, face, full)
}
/// `prepareQuadFlat` for a quad without a cull face: light from the
/// neighbour when the quad is flush with the face (cubic), else from the block
/// itself. `shape` is the quad's baked [`QuadShape`].
pub(super) fn flat_shaped(
    pad: &Padded,
    own: &Info,
    pos: [i32; 3],
    face: D,
    shape: &QuadShape,
) -> u32 {
    let at = if shape.cubic { step(pos, face) } else { pos };
    coords(pad, own, at)
}
/// Where `tesselateFlat` reads the light of a quad of a block at `pos`: the
/// neighbour on its cull face, else `prepareQuadFlat`'s choice (the neighbour
/// when the quad is cubic, else the block itself). `full` is the block's
/// `isCollisionShapeFullBlock`.
pub(super) fn flat_position(
    pos: [i32; 3],
    positions: &[[f32; 3]; 4],
    face: D,
    cull: Option<D>,
    full: bool,
) -> [i32; 3] {
    match cull {
        Some(d) => step(pos, d),
        None if shape(positions, face, full).cubic => step(pos, face),
        None => pos,
    }
}
/// A face's AO sample offsets from the sample base, in `pi` index space, so a
/// sample is `base + offset` rather than a `pi` per coordinate triple.
struct Samples {
    /// The four corners (`adjacency`'s).
    corners: [isize; 4],
    /// Each corner stepped out of the face (the permeability sample).
    out: [isize; 4],
    /// Corner `a` stepped toward corner `b`.
    diagonal: [[isize; 4]; 4],
    /// `step(pos, face)` from the block's own cell.
    step: isize,
}
const fn samples(face: D) -> Samples {
    let mut corners = [0isize; 4];
    let mut out = [0isize; 4];
    let mut diagonal = [[0isize; 4]; 4];
    let step = super::step_index(NORMALS[face as usize]);
    let mut i = 0;
    while i < 4 {
        let c = super::step_index(NORMALS[ADJACENCY[face as usize].corners[i] as usize]);
        corners[i] = c;
        out[i] = c + step;
        let mut j = 0;
        while j < 4 {
            diagonal[i][j] =
                c + super::step_index(NORMALS[ADJACENCY[face as usize].corners[j] as usize]);
            j += 1;
        }
        i += 1;
    }
    Samples {
        corners,
        out,
        diagonal,
        step,
    }
}
const SAMPLES: [Samples; 6] = [
    samples(D::Down),
    samples(D::Up),
    samples(D::North),
    samples(D::South),
    samples(D::West),
    samples(D::East),
];
/// `prepareQuadAmbientOcclusion`: per-vertex AO brightness (before `ARGB.gray`)
/// and smooth light coords, by vertex index.
pub(super) fn ambient_occlusion(
    pad: &Padded,
    info: &[Info],
    own: &Info,
    pos: [i32; 3],
    shape: &QuadShape,
    face: D,
) -> ([f32; 4], [u32; 4]) {
    let samples = &SAMPLES[face as usize];
    // The block's own cell and the sample base (the neighbour when the quad is
    // cubic): every corner and diagonal is a constant step from it.
    let cell = pi(pos[0], pos[1], pos[2]) as isize;
    let base = if shape.cubic {
        cell + samples.step
    } else {
        cell
    };
    let adjacency = &ADJACENCY[face as usize];
    let corner = |i: usize| base + samples.corners[i];
    let sample = |i: isize| {
        let i = i as usize;
        let s = state_at(pad, info, i);
        (coords_at(pad, s, i), s.shade)
    };
    let (light0, shade0) = sample(corner(0));
    let (light1, shade1) = sample(corner(1));
    let (light2, shade2) = sample(corner(2));
    let (light3, shade3) = sample(corner(3));
    let permeable = |i: usize| state_at(pad, info, (base + samples.out[i]) as usize).permeable;
    let (p0, p1, p2, p3) = (permeable(0), permeable(1), permeable(2), permeable(3));
    let diagonal = |a: usize, b: usize, open: bool| {
        if open {
            sample(base + samples.diagonal[a][b])
        } else {
            (light0, shade0)
        }
    };
    let (light02, shade02) = diagonal(0, 2, p2 || p0);
    let (light03, shade03) = diagonal(0, 3, p3 || p0);
    let (light12, shade12) = diagonal(1, 2, p2 || p1);
    let (light13, shade13) = diagonal(1, 3, p3 || p1);
    let next = (cell + samples.step) as usize;
    let next_state = state_at(pad, info, next);
    let light_center = if shape.cubic || !next_state.solid {
        coords_at(pad, next_state, next)
    } else {
        coords_at(pad, own, cell as usize)
    };
    let shade_center = if shape.cubic {
        state_at(pad, info, base as usize).shade
    } else {
        own.shade
    };
    let ao = [
        (shade3 + shade0 + shade03 + shade_center) * 0.25,
        (shade2 + shade0 + shade02 + shade_center) * 0.25,
        (shade2 + shade1 + shade12 + shade_center) * 0.25,
        (shade3 + shade1 + shade13 + shade_center) * 0.25,
    ];
    let blended = [
        smooth_blend(light3, light0, light03, light_center),
        smooth_blend(light2, light0, light02, light_center),
        smooth_blend(light2, light1, light12, light_center),
        smooth_blend(light3, light1, light13, light_center),
    ];
    let mut brightness = [0f32; 4];
    let mut light = [0u32; 4];
    for (k, &vertex) in adjacency.remap.iter().enumerate() {
        if shape.partial {
            let f = |i: usize| shape.bounds[adjacency.weights[k][i] as usize];
            let w = [f(0) * f(1), f(2) * f(3), f(4) * f(5), f(6) * f(7)];
            brightness[vertex] =
                (ao[0] * w[0] + ao[1] * w[1] + ao[2] * w[2] + ao[3] * w[3]).clamp(0.0, 1.0);
            light[vertex] = weighted_blend(blended, w);
        } else {
            brightness[vertex] = ao[k];
            light[vertex] = blended[k];
        }
    }
    (brightness, light)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ao_offsets_match_coordinate_samples_at_section_edges() {
        // Explicit coordinate directions, without NORMALS, ADJACENCY or step_index.
        let (down, up, north, south, west, east) = (
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
            [-1, 0, 0],
            [1, 0, 0],
        );
        let faces = [
            (D::Down, down, [west, east, north, south]),
            (D::Up, up, [east, west, north, south]),
            (D::North, north, [up, down, east, west]),
            (D::South, south, [west, east, down, up]),
            (D::West, west, [up, down, north, south]),
            (D::East, east, [down, up, north, south]),
        ];
        let add = |a: [i32; 3], b: [i32; 3]| std::array::from_fn(|i| a[i] + b[i]);
        let index = |p: [i32; 3]| pi(p[0], p[1], p[2]);
        for x in [0, 1, 14, 15] {
            for y in [0, 1, 14, 15] {
                for z in [0, 1, 14, 15] {
                    let pos = [x, y, z];
                    for (face, normal, corners) in faces {
                        let samples = &SAMPLES[face as usize];
                        let cell = index(pos) as isize;
                        assert_eq!((cell + samples.step) as usize, index(add(pos, normal)));
                        for cubic in [false, true] {
                            let base_pos = if cubic { add(pos, normal) } else { pos };
                            let base = if cubic { cell + samples.step } else { cell };
                            for (i, corner) in corners.into_iter().enumerate() {
                                let p = add(base_pos, corner);
                                assert_eq!((base + samples.corners[i]) as usize, index(p));
                                assert_eq!((base + samples.out[i]) as usize, index(add(p, normal)));
                            }
                            for (a, b) in [(0, 2), (0, 3), (1, 2), (1, 3)] {
                                assert_eq!(
                                    (base + samples.diagonal[a][b]) as usize,
                                    index(add(add(base_pos, corners[a]), corners[b]))
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    const fn packed(block: u32, sky: u32) -> u32 {
        block << 4 | sky << 20
    }
    #[test]
    fn smooth_blend_replaces_dark_samples_with_the_centre() {
        let center = packed(2, 15);
        // A zero sample takes the centre; a sky-less sample borrows its sky.
        let blended = smooth_blend(0, packed(8, 0), packed(4, 15), center);
        assert_eq!(blended & 0xFF, (32 + 128 + 64 + 32) / 4);
        assert_eq!(blended >> 16 & 0xFF, 240);
        // A dark centre leaves samples untouched.
        assert_eq!(
            smooth_blend(0, 0, packed(15, 15), packed(1, 1)) & 0xFF,
            (240 + 16) / 4
        );
    }
    #[test]
    fn weighted_blend_narrows_like_java_int_casts() {
        let c = [packed(15, 15); 4];
        assert_eq!(weighted_blend(c, [0.25; 4]), packed(15, 15));
        // Negative weights (geometry past the block) wrap through the 0xFF mask.
        assert_eq!(
            weighted_blend([packed(1, 0); 4], [-0.25; 4]) & 0xFF,
            (-16i32 as u32) & 0xFF
        );
    }
    #[test]
    fn gray_floors_like_f32_floor() {
        let mut values = vec![
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            1.0,
            -1.0,
            1e10,
            -1e10,
        ];
        for bits in (0..u32::MAX).step_by(7919) {
            values.push(f32::from_bits(bits));
        }
        for i in -3000..3000 {
            values.push(i as f32 / 1024.0);
        }
        for v in values {
            assert_eq!(gray(v), (v * 255.0).floor() as i32, "{v:e}");
        }
    }
    #[test]
    fn colour_arithmetic_matches_argb_helpers() {
        assert_eq!(gray(1.0), 255);
        assert_eq!(gray(0.8), 204);
        assert_eq!(scale(255, 0.5), 127);
        assert_eq!(scale(gray(0.6), 0.8), 122);
        assert_eq!(cardinal(true, D::Down), 0.9);
        assert_eq!(cardinal(false, D::Down), 0.5);
    }
    #[test]
    fn emission_raises_both_channels_and_drops_fractions() {
        assert_eq!(with_min(0x0087_0013, [0; 2]), 0x0087_0013);
        assert_eq!(with_min(0x0087_0013, [3; 2]), packed(3, 8));
        assert_eq!(max_coords(packed(3, 9), packed(7, 2)), packed(7, 9));
    }
    #[test]
    fn block_and_sky_minimums_raise_their_own_channel() {
        assert_eq!(with_min(0x0087_0013, [15, 0]), packed(15, 8));
        assert_eq!(with_min(0x0087_0013, [0, 15]), packed(1, 15));
        assert_eq!(with_min(0x0087_0013, [15, 15]), packed(15, 15));
    }
    #[test]
    fn flat_light_reads_the_neighbour_a_face_looks_at() {
        let pos = [4, 64, -2];
        let full_top = [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ];
        // A cull face samples its neighbour, whatever the quad's shape.
        let inset = full_top.map(|[x, _, z]| [x, 0.75, z]);
        assert_eq!(
            flat_position(pos, &inset, D::Up, Some(D::Up), false),
            [4, 65, -2]
        );
        // A flush quad without a cull face samples its neighbour too: a block
        // drawn at a cell that is now opaque (a moving block, #3002) still
        // takes the light around it.
        assert_eq!(
            flat_position(pos, &full_top, D::Up, None, false),
            [4, 65, -2]
        );
        let west = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
        ];
        assert_eq!(flat_position(pos, &west, D::West, None, false), [3, 64, -2]);
        // An inset quad (a piston head's arm) takes the block's own light.
        assert_eq!(flat_position(pos, &inset, D::Up, None, false), pos);
        assert_eq!(flat_position(pos, &inset, D::Up, None, true), [4, 65, -2]);
    }
    #[test]
    fn quad_shape_classifies_partial_and_cubic_faces() {
        let full_top = [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ];
        let s = shape(&full_top, D::Up, false);
        assert!(s.cubic && !s.partial);
        let slab_top = full_top.map(|[x, _, z]| [x, 0.5, z]);
        let s = shape(&slab_top, D::Up, false);
        assert!(!s.cubic && !s.partial);
        assert!(shape(&slab_top, D::Up, true).cubic);
        let small = slab_top.map(|[x, y, z]| [x * 0.5, y, z]);
        let s = shape(&small, D::Up, false);
        assert!(s.partial);
        assert_eq!(s.bounds[EAST as usize], 0.5);
        assert_eq!(s.bounds[FLIP_EAST as usize], 0.5);
    }
}
