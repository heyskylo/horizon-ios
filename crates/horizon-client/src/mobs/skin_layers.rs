//! 3D skin layers (off by default): the player's outer skin layer drawn as
//! its own pixels, each opaque texel extruded out of the model as a small
//! slab (`threeDSkinLayers` in horizon-extras.txt, Options → Skin Customization).
//!
//! Vanilla draws the outer layer as one box inflated by 0.25 px, so a hat,
//! jacket, sleeves and trousers read as a flat skin over the body. Here each
//! texel of those boxes that the skin paints becomes a slab one texel deep:
//! the face pushed out along the box's normal, its sides filled in where the
//! neighbouring texel is transparent. Hair, hoods and hems then have a
//! silhouette of their own, and the gaps between painted texels stay open.
//!
//! The geometry is built once per (skin texture, slim/wide) from the decoded
//! 64×64 RGBA skin and cached by the game; the model tree, part names and the
//! texture's normalised UVs are the ones the renderer already uses, so both
//! the CPU and the GPU (`super::skin`) pose paths draw it unchanged.
use super::{Part, Polygon};

/// The parts of `PlayerModel` that make up the outer layer, as the prepared
/// `entity-models.json` names them.
pub const OUTER_PARTS: [&str; 6] = [
    "hat",
    "jacket",
    "left_sleeve",
    "right_sleeve",
    "left_pants",
    "right_pants",
];

/// The size of a prepared skin image: `resample_64` always hands out this
/// many texels a side, wide and legacy skins alike.
pub const SIZE: usize = 64;

/// How far a texel is pushed out, in model units (1 = 1/16 block = one skin
/// texel): the depth the 3D Skin Layers mod uses by default.
pub const DEPTH: f32 = 1.0;

/// A texel this transparent is not drawn at all (`alpha` of 0–255); the ones
/// just above it stay, so a tinted glass-like layer keeps its translucency.
const ALPHA_CUTOFF: u8 = 8;

/// `model` with every outer-layer part replaced by its extruded geometry,
/// or `None` when the model has no outer layer to extrude.
pub fn extrude(model: &Part, skin: &[u8], size: usize, depth: f32) -> Option<Part> {
    if skin.len() < size * size * 4 {
        return None;
    }
    let mut changed = false;
    let mut out = model.clone();
    explode(&mut out, skin, size, depth, &mut changed);
    changed.then_some(out)
}

fn explode(part: &mut Part, skin: &[u8], size: usize, depth: f32, changed: &mut bool) {
    if OUTER_PARTS.contains(&part.name.as_str()) {
        let mut polygons = Vec::with_capacity(part.polygons.len() * 8);
        for polygon in &part.polygons {
            polygons.extend(extrude_polygon(polygon, skin, size, depth, changed));
        }
        part.polygons = polygons;
    }
    for child in &mut part.children {
        explode(child, skin, size, depth, changed);
    }
}

/// The slabs a face's texels make, or the face itself when it cannot be read
/// as a texel grid (a model whose UVs are not an axis-aligned rectangle).
fn extrude_polygon(
    polygon: &Polygon,
    skin: &[u8],
    size: usize,
    depth: f32,
    changed: &mut bool,
) -> Vec<Polygon> {
    let (u0, u1, v0, v1) = polygon
        .vertices
        .iter()
        .fold((f32::MAX, f32::MIN, f32::MAX, f32::MIN), |a, v| {
            (a.0.min(v[3]), a.1.max(v[3]), a.2.min(v[4]), a.3.max(v[4]))
        });
    let texels_u = ((u1 - u0) * size as f32).round() as i32;
    let texels_v = ((v1 - v0) * size as f32).round() as i32;
    if texels_u <= 0 || texels_v <= 0 || texels_u > 32 || texels_v > 32 {
        return vec![polygon.clone()];
    }
    let (Some(a), Some(b), Some(c)) = (
        corner(polygon, u0, v0),
        corner(polygon, u1, v0),
        corner(polygon, u0, v1),
    ) else {
        return vec![polygon.clone()];
    };
    let n = glam::Vec3::from_array(polygon.normal).normalize_or_zero();
    if n == glam::Vec3::ZERO {
        return vec![polygon.clone()];
    }
    // The face's own axes, from its UV grid, so the skirt normals point away
    // from the slab no matter how the quad is wound.
    let du = (b - a).normalize_or_zero();
    let dv = (c - a).normalize_or_zero();
    let span_u = (u1 - u0).max(f32::EPSILON);
    let span_v = (v1 - v0).max(f32::EPSILON);
    let at = |u: f32, v: f32| a + (b - a) * ((u - u0) / span_u) + (c - a) * ((v - v0) / span_v);
    let base_u = (u0 * size as f32).round() as i32;
    let base_v = (v0 * size as f32).round() as i32;
    let opaque = |i: i32, j: i32| {
        if i < 0 || j < 0 || i >= texels_u || j >= texels_v {
            return false;
        }
        let x = (base_u + i) as usize;
        let y = (base_v + j) as usize;
        x < size && y < size && skin[(y * size + x) * 4 + 3] > ALPHA_CUTOFF
    };
    let mut out = Vec::with_capacity((texels_u * texels_v) as usize);
    let step = 1.0 / size as f32;
    for j in 0..texels_v {
        for i in 0..texels_u {
            if !opaque(i, j) {
                continue;
            }
            let (ua, va) = (u0 + i as f32 * step, v0 + j as f32 * step);
            let (ub, vb) = (ua + step, va + step);
            let (p00, p10) = (at(ua, va), at(ub, va));
            let (p11, p01) = (at(ub, vb), at(ua, vb));
            let lift = n * depth;
            out.push(quad(
                [p00 + lift, p10 + lift, p11 + lift, p01 + lift],
                [(ua, va), (ub, va), (ub, vb), (ua, vb)],
                polygon.normal,
            ));
            // Sides only where the neighbour is not painted: the ones between
            // two texels would be hidden inside the layer and cost fill rate.
            let (uc, vc) = ((ua + ub) * 0.5, (va + vb) * 0.5);
            let sides = [
                ((i - 1, j), -du, [p00, p01]),
                ((i + 1, j), du, [p10, p11]),
                ((i, j - 1), -dv, [p01, p11]),
                ((i, j + 1), dv, [p00, p10]),
            ];
            for ((ni, nj), normal, [s0, s1]) in sides {
                if opaque(ni, nj) {
                    continue;
                }
                let normal = normal.normalize_or_zero().to_array();
                let uv = (uc, vc);
                out.push(quad(
                    [s0, s1, s1 + lift, s0 + lift],
                    [uv, uv, uv, uv],
                    normal,
                ));
            }
        }
    }
    if out.is_empty() {
        // A layer the skin leaves blank on this face: nothing to draw there.
        *changed = true;
        return out;
    }
    *changed = true;
    out
}

fn quad(positions: [glam::Vec3; 4], uvs: [(f32, f32); 4], normal: [f32; 3]) -> Polygon {
    Polygon {
        normal,
        vertices: std::array::from_fn(|k| {
            [
                positions[k].x,
                positions[k].y,
                positions[k].z,
                uvs[k].0,
                uvs[k].1,
            ]
        }),
        cull_back: false,
    }
}

fn corner(polygon: &Polygon, u: f32, v: f32) -> Option<glam::Vec3> {
    polygon
        .vertices
        .iter()
        .find(|a| (a[3] - u).abs() < 1e-4 && (a[4] - v).abs() < 1e-4)
        .map(|a| glam::Vec3::new(a[0], a[1], a[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2-texel face facing +Z, its four corners at the given depth.
    fn face() -> Polygon {
        Polygon {
            normal: [0.0, 0.0, 1.0],
            vertices: [
                [-1.0, -1.0, 0.5, 0.0, 0.0],
                [1.0, -1.0, 0.5, 0.5, 0.0],
                [1.0, 1.0, 0.5, 0.5, 0.5],
                [-1.0, 1.0, 0.5, 0.0, 0.5],
            ],
            cull_back: false,
        }
    }

    fn skin(alpha: [u8; 4]) -> Vec<u8> {
        let mut skin = vec![0; 64 * 64 * 4];
        for (i, a) in alpha.iter().enumerate() {
            skin[i * 4 + 3] = *a;
        }
        skin
    }

    #[test]
    fn one_slab_and_four_sides_for_a_single_painted_texel() {
        let model = Part {
            name: "hat".into(),
            pose: [0.0; 9],
            polygons: vec![face()],
            children: Vec::new(),
        };
        let out = extrude(&model, &skin([255, 0, 0, 0]), 64, 1.0).expect("a hat");
        // One face plus four skirts.
        assert_eq!(out.polygons.len(), 5);
        let top = out.polygons[0].vertices;
        for v in top {
            assert!(v[2] > 1.4, "the slab is pushed out: {v:?}");
        }
    }

    #[test]
    fn a_transparent_face_draws_nothing() {
        let model = Part {
            name: "jacket".into(),
            pose: [0.0; 9],
            polygons: vec![face()],
            children: Vec::new(),
        };
        let out = extrude(&model, &skin([0, 0, 0, 0]), 64, 1.0).expect("a jacket");
        assert!(out.polygons.is_empty());
    }

    #[test]
    fn a_part_that_is_not_an_outer_layer_is_untouched() {
        let model = Part {
            name: "head".into(),
            pose: [0.0; 9],
            polygons: vec![face()],
            children: Vec::new(),
        };
        assert!(extrude(&model, &skin([255, 255, 255, 255]), 64, 1.0).is_none());
    }
}
