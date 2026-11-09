//! Animated burning-entity flames; the first-person overlay uses screen_effects.
use crate::{hud::Rect, mesher::Vertex};
use std::collections::HashMap;

/// Test the flame's own envelope, which is taller and wider than the entity's box.
/// This only rejects camera-invisible geometry; callers keep other-view shader/mod paths.
pub fn in_view(
    view: &crate::gfx::Frustum,
    camera: glam::DVec3,
    position: [f64; 3],
    width: f32,
    height: f32,
) -> bool {
    let origin = (glam::DVec3::from(position) - camera).as_vec3();
    bounds(origin, width, height).is_none_or(|(lo, hi)| view.intersects(lo, hi))
}

fn bounds(origin: glam::Vec3, width: f32, height: f32) -> Option<(glam::Vec3, glam::Vec3)> {
    let scale = width * 1.4;
    let remaining = height / scale;
    // Unusual/invalid dimensions keep the original path. The ordinary aspect ratios also
    // bound accumulated float error in the 0.45-step loop well below the spare 0.1 scale.
    if !origin.is_finite()
        || !scale.is_finite()
        || !remaining.is_finite()
        || scale <= 0.0
        || !(0.0..=64.0).contains(&remaining)
    {
        return None;
    }
    // half starts at 0.5 and shrinks. z starts at 0.3 - trunc(remaining)*0.02 and
    // recedes by 0.03 per layer. Two extra steps cover float rounding at layer boundaries.
    let layers = (remaining / 0.45).ceil() + 2.0;
    let z = 0.3 - remaining.trunc() * 0.02;
    let depth = z.abs().max((z - layers * 0.03).abs());
    // |rotated x/z| <= |x| + |z|, for every billboard yaw. The last layer starts
    // below height and reaches 1.4*scale above its start; keep extra numerical slack.
    let radius = (0.5 + depth) * scale;
    let padding = glam::Vec3::splat(0.05 + origin.abs().max_element() * f32::EPSILON * 4.0);
    Some((
        origin - glam::Vec3::new(radius, 0.0, radius) - padding,
        origin + glam::Vec3::new(radius, height + 1.5 * scale, radius) + padding,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn flames(
    position: [f64; 3],
    width: f32,
    height: f32,
    camera: glam::DVec3,
    yaw: f32,
    sprites: &HashMap<String, Rect>,
    scene_light: u8,
    out: &mut Vec<Vertex>,
) {
    let (Some(a), Some(b)) = (sprites.get("block/fire_0"), sprites.get("block/fire_1")) else {
        return;
    };
    let scale = width * 1.4;
    if scale <= 0.0 {
        return;
    }
    let origin = glam::DVec3::from(position) - camera;
    let rotation = glam::Mat3::from_rotation_y(std::f32::consts::PI - yaw.to_radians());
    let mut remaining = height / scale;
    let mut half = 0.5f32;
    let mut y = 0.0f32;
    let mut z = 0.3 - (remaining as i32) as f32 * 0.02;
    let mut index = 0;
    while remaining > 0.0 {
        let r = if index % 2 == 0 { a } else { b };
        let (u0, u1) = if (index / 2) % 2 == 0 {
            (r.u0, r.u1)
        } else {
            (r.u1, r.u0)
        };
        for (p, uv) in [
            ([-half, y, z], [u0, r.v1]),
            ([half, y, z], [u1, r.v1]),
            ([half, y + 1.4, z], [u1, r.v0]),
            ([-half, y + 1.4, z], [u0, r.v0]),
        ] {
            let p = origin.as_vec3() + rotation * (glam::Vec3::from(p) * scale);
            out.push(Vertex {
                pos: p.to_array(),
                uv,
                layer: 0,
                color: 0xffff_ffff,
                light: 0xf000 | (u32::from(scene_light >> 4) * 16),
                normal: [0; 4],

                glint_uv: [0.0, 0.0],
            });
        }
        remaining -= 0.45;
        y += 0.45;
        half *= 0.9;
        z -= 0.03;
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn culling_envelope_contains_emitted_flames_at_every_billboard_yaw() {
        let rect = super::Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
            w: 16,
            h: 16,
        };
        let sprites =
            super::HashMap::from([("block/fire_0".into(), rect), ("block/fire_1".into(), rect)]);
        for (width, height) in [
            (0.25, 0.25),
            (0.5, 1.975),
            (0.25, 0.9875),
            (0.6, 1.8),
            (0.9, 1.4),
            (2.0, 1.9),
            (5.0, 448.0),
        ] {
            for scale in [0.0625, 1.0, 16.0] {
                let (width, height) = (width * scale, height * scale);
                for camera in [
                    glam::DVec3::ZERO,
                    glam::DVec3::new(30_000_000.0, -64.0, -30_000_000.0),
                ] {
                    let position = camera + glam::DVec3::new(7.25, -2.5, -12.0);
                    let (lo, hi) =
                        super::bounds((position - camera).as_vec3(), width, height).unwrap();
                    for yaw in (0..360).step_by(15) {
                        let mut out = Vec::new();
                        super::flames(
                            position.to_array(),
                            width,
                            height,
                            camera,
                            yaw as f32,
                            &sprites,
                            0,
                            &mut out,
                        );
                        assert!(!out.is_empty());
                        for v in out {
                            let p = glam::Vec3::from_array(v.pos);
                            assert!(
                                p.cmpge(lo).all() && p.cmple(hi).all(),
                                "{width}x{height}, yaw {yaw}: {p:?} outside {lo:?}..{hi:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn unsupported_dimensions_keep_the_unculled_path() {
        for (width, height) in [
            (0.0, 1.0),
            (f32::NAN, 1.0),
            (1.0, f32::INFINITY),
            (0.001, 100.0),
        ] {
            assert!(super::bounds(glam::Vec3::ZERO, width, height).is_none());
        }
    }

    #[test]
    fn flames_cover_entity_height_and_alternate_animated_sprites() {
        let rect = |u0| super::Rect {
            u0,
            v0: 0.0,
            u1: u0 + 0.1,
            v1: 0.1,
            w: 16,
            h: 16,
        };
        let sprites = super::HashMap::from([
            ("block/fire_0".into(), rect(0.0)),
            ("block/fire_1".into(), rect(0.5)),
        ]);
        let mut out = Vec::new();
        super::flames(
            [0.0; 3],
            0.6,
            1.8,
            glam::DVec3::ZERO,
            0.0,
            &sprites,
            0,
            &mut out,
        );
        assert_eq!(out.len() % 4, 0);
        assert!(
            out[0].pos[2] < 0.0,
            "billboard depth bias must face the camera"
        );
        assert!(
            out.iter().all(|v| v.light == 0xf000),
            "fire forces block light to 15 but preserves scene sky light"
        );
        assert!(out.len() >= 20);
        assert!(out.iter().any(|v| v.pos[1] >= 1.8));
        assert!(
            out.iter().all(|v| v.layer == 0),
            "flames must not use the breaking-cracks UV-remap material"
        );
        assert!(out[..4].iter().all(|v| v.uv[0] <= 0.1));
        assert!(out[4..8].iter().all(|v| v.uv[0] >= 0.5));
    }
}
