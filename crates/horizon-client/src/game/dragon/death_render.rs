//! The dying dragon's dissolve and death rays (`EnderDragonRenderer`).
use super::*;

/// The dying model's colour, `ARGB.white(1 - deathTime / 200)`: `Mth.floor` of the alpha
/// shifted as a Java int, so past 200 it wraps as vanilla's does.
pub(super) fn dissolve_color(death: f32) -> u32 {
    let alpha = ((1.0 - death / 200.0) * 255.0).floor() as i32;
    (alpha << 24) as u32 | 0x00FF_FFFF
}
/// `DissolveMaskSampler` at the vertex's `UV0`: the same place in the mask's sprite as in the
/// skin's, as the dissolve overlay reads it from `glint_uv` (entity.wgsl).
pub(super) fn dissolve_mask(quads: &mut [Vertex], skin: [f32; 4], mask: [f32; 4]) {
    for v in quads {
        let u = (v.uv[0] - skin[0]) / (skin[2] - skin[0]);
        let w = (v.uv[1] - skin[1]) / (skin[3] - skin[1]);
        v.glint_uv = [
            mask[0] + (mask[2] - mask[0]) * u,
            mask[1] + (mask[3] - mask[1]) * w,
        ];
    }
}
/// `submitRays`: the death rays from a fixed seed, fading in white to magenta.
pub(super) fn dragon_rays(
    base: glam::Mat4,
    t: f32,
    white: horizon_client::hud::Rect,
    out: &mut Vec<Vertex>,
) {
    // Vanilla's dragonRays has no texture. Sample inside the white sprite:
    // its border can round into transparent neighbours during interpolation.
    let uv = [(white.u0 + white.u1) / 2.0, (white.v0 + white.v1) / 2.0];
    let over = if t > 0.8 {
        ((t - 0.8) / 0.2).min(1.0)
    } else {
        0.0
    };
    let inner = (((1.0 - over) * 255.0) as u32) << 24 | 0x00FF_FFFF;
    let outer = 0x00FF_00FF;
    let mut random = LegacyRandom::new(432);
    let mut m = base;
    let count = ((t + t * t) / 2.0 * 60.0).floor() as i32;
    let half_sqrt3 = 3f32.sqrt() / 2.0;
    let tau = std::f32::consts::TAU;
    for _ in 0..count {
        let mut f = || random.next_float() * tau;
        let q1 = glam::Quat::from_rotation_x(f())
            * glam::Quat::from_rotation_y(f())
            * glam::Quat::from_rotation_z(f());
        let (a, b, c) = (f(), f(), f());
        let q2 = glam::Quat::from_rotation_x(a)
            * glam::Quat::from_rotation_y(b)
            * glam::Quat::from_rotation_z(c + t * std::f32::consts::FRAC_PI_2);
        m *= glam::Mat4::from_quat(q1 * q2);
        let length = random.next_float() * 20.0 + 5.0 + over * 10.0;
        let width = random.next_float() * 2.0 + 1.0 + over * 2.0;
        let o = glam::Vec3::ZERO;
        let left = glam::Vec3::new(-half_sqrt3 * width, length, -0.5 * width);
        let right = glam::Vec3::new(half_sqrt3 * width, length, -0.5 * width);
        let bottom = glam::Vec3::new(0.0, length, width);
        for (p1, p2) in [(left, right), (right, bottom), (bottom, left)] {
            // A triangle as a quad with its last corner repeated.
            for (p, c) in [(o, inner), (p1, outer), (p2, outer), (p2, outer)] {
                out.push(Vertex {
                    normal: [0; 4],
                    pos: m.transform_point3(p).to_array(),
                    uv,
                    layer: horizon_client::gfx::DRAGON_RAYS_OVERLAY,
                    color: c,
                    light: 0,

                    glint_uv: [0.0, 0.0],
                });
            }
        }
    }
}
