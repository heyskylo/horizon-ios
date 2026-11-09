// Still-fluid tiling follows Catnip/Ponder 1.0.82 FluidRenderHelper.renderTiledFace.
// Copyright (c) 2022 The Create Team
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use horizon_core::geometry::Direction;

/// Controls a still-fluid box. The tint is stack ARGB, independent of biome tint.
#[derive(Clone, Copy, Debug)]
pub struct NativeFluidRenderOptions {
    pub argb: Option<u32>,
    pub render_bottom: bool,
    /// TextureAtlasSprite.uvShrinkRatio: 4 / the largest stitched atlas dimension.
    /// Explicit because Horizon's block sprites occupy arrays rather than a stitched atlas.
    pub uv_shrink_ratio: f32,
}

/// A corner before the caller's pose, with UVs within the still sprite, and its outward normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [i8; 3],
}

/// Preserve sky light and increase block light to the fluid's luminosity (native light packing).
pub fn fluid_light(light: u32, luminosity: u8) -> u32 {
    (light & 0xf0) | (((light >> 8 & 0xf0).max(u32::from(luminosity.min(15)) * 16)) << 8)
}

/// Emit whole tiled quads, or refuse invalid/unreasonably large geometry before emitting anything.
/// Negative coordinates and exact integer boundaries retain the source's local UV fractions.
pub fn fluid_box(
    bounds: [f32; 6],
    options: NativeFluidRenderOptions,
    mut emit: impl FnMut(FluidVertex),
) -> bool {
    if !options.uv_shrink_ratio.is_finite()
        || !(0.0..=1.0).contains(&options.uv_shrink_ratio)
        || bounds
            .iter()
            .any(|x| !x.is_finite() || x.abs() > 1_048_576.0)
        || (0..3).any(|i| bounds[i] > bounds[i + 3])
    {
        return false;
    }
    let tiles =
        std::array::from_fn::<_, 3, _>(|i| (bounds[i + 3].ceil() - bounds[i].floor()) as u64);
    let quads = tiles[0] * tiles[1] * 2
        + tiles[1] * tiles[2] * 2
        + tiles[0] * tiles[2] * if options.render_bottom { 2 } else { 1 };
    if quads > 65_536 {
        return false;
    }
    let [x0, y0, z0, x1, y1, z1] = bounds;
    for (side, face) in [
        (Direction::Down, [x0, z0, x1, z1, y0]),
        (Direction::Up, [x0, z0, x1, z1, y1]),
        (Direction::North, [x0, y0, x1, y1, z0]),
        (Direction::South, [x0, y0, x1, y1, z1]),
        (Direction::West, [z0, y0, z1, y1, x0]),
        (Direction::East, [z0, y0, z1, y1, x1]),
    ] {
        if side != Direction::Down || options.render_bottom {
            tiled_face(&mut emit, side, face, options.uv_shrink_ratio);
        }
    }
    true
}

fn tiled_face(out: &mut impl FnMut(FluidVertex), side: Direction, face: [f32; 5], ratio: f32) {
    let [a0, b0, a1, b1, plane] = face;
    let positive = matches!(side, Direction::Up | Direction::South | Direction::East);
    let shrink = ratio * 0.25;
    let lerp = |v| v + shrink * (0.5 - v);
    let mut a = a0;
    while a < a1 {
        let end_a = (a.floor() + 1.0).min(a1);
        let (u0, u1) = if matches!(side, Direction::North | Direction::East) {
            (end_a.ceil() - end_a, end_a.ceil() - a)
        } else {
            (a - a.floor(), end_a - a.floor())
        };
        let (u0, u1) = (lerp(u0), lerp(u1));
        let mut b = b0;
        while b < b1 {
            let end_b = (b.floor() + 1.0).min(b1);
            let (v0, v1) = if side == Direction::Up {
                (b - b.floor(), end_b - b.floor())
            } else {
                (end_b.ceil() - end_b, end_b.ceil() - b)
            };
            let (v0, v1) = (lerp(v0), lerp(v1));
            let positions = match side {
                Direction::West | Direction::East => {
                    let (near, far) = if positive { (end_a, a) } else { (a, end_a) };
                    [
                        [plane, end_b, near],
                        [plane, b, near],
                        [plane, b, far],
                        [plane, end_b, far],
                    ]
                }
                Direction::North | Direction::South => {
                    let (near, far) = if positive { (a, end_a) } else { (end_a, a) };
                    [
                        [near, end_b, plane],
                        [near, b, plane],
                        [far, b, plane],
                        [far, end_b, plane],
                    ]
                }
                Direction::Down | Direction::Up => {
                    let (near, far) = if positive { (b, end_b) } else { (end_b, b) };
                    [
                        [a, plane, near],
                        [a, plane, far],
                        [end_a, plane, far],
                        [end_a, plane, near],
                    ]
                }
            };
            let normal = match side {
                Direction::Down => [0, -1, 0],
                Direction::Up => [0, 1, 0],
                Direction::North => [0, 0, -1],
                Direction::South => [0, 0, 1],
                Direction::West => [-1, 0, 0],
                Direction::East => [1, 0, 0],
            };
            for (pos, uv) in positions
                .into_iter()
                .zip([[u0, v0], [u0, v1], [u1, v1], [u1, v0]])
            {
                out(FluidVertex { pos, uv, normal });
            }
            b = end_b;
        }
        a = end_a;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_tiling_is_outward_and_does_not_inset_geometry() {
        let bounds = [-1.25, 0.125, -0.25, 1.5, 1.125, 0.75];
        let mut result = Vec::new();
        assert!(fluid_box(
            bounds,
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: false,
                uv_shrink_ratio: 4.0 / 2048.0,
            },
            |v| result.push(v),
        ));
        assert_eq!(result.len(), (4 * 2 * 2 + 2 * 2 * 2 + 4 * 2) * 4);
        for quad in result.as_chunks::<4>().0 {
            let [a, b, c, _] = quad.map(|v| glam::Vec3::from_array(v.pos));
            assert!(
                (b - a)
                    .cross(c - a)
                    .dot(glam::Vec3::from_array(quad[0].normal.map(f32::from)))
                    > 0.0
            );
            assert!(
                quad.iter()
                    .all(|v| (0..3).all(|i| v.pos[i] >= bounds[i] && v.pos[i] <= bounds[i + 3]))
            );
        }
        assert!(
            result
                .iter()
                .any(|v| v.pos == [bounds[0], bounds[4], bounds[2]])
        );
        assert!(!fluid_box(
            [0.0, 0.0, 0.0, 1_000.0, 1_000.0, 1_000.0],
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: true,
                uv_shrink_ratio: 0.0,
            },
            |_| panic!("invalid geometry must fail before emitting"),
        ));
    }
    #[test]
    fn luminosity_changes_only_block_light() {
        assert_eq!(fluid_light(0x20a0, 15), 0xf0a0);
        assert_eq!(fluid_light(0x25a5, 15), 0xf0a0);
        assert_eq!(fluid_light(0xe030, 3), 0xe030);
    }
}
