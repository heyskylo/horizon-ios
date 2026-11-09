//! Small checks on the numbers and boxes native mods hand the game.

/// Whether `rgba` is a whole `width` x `height` image that fits a
/// `slot_width` x `slot_height` atlas slot (a native mod's `mod_image`).
pub(super) fn mod_image_fits(
    width: u32,
    height: u32,
    rgba: usize,
    slot_width: u32,
    slot_height: u32,
) -> bool {
    width > 0
        && height > 0
        && width <= slot_width
        && height <= slot_height
        && rgba == width as usize * height as usize * 4
}

/// `params` with each value that isn't finite as 0, so a mod's mistake can't
/// put a NaN in every pixel of a pack's picture.
pub(super) fn finite_params(params: [[f32; 4]; 8]) -> [[f32; 4]; 8] {
    params.map(|v| v.map(|x| if x.is_finite() { x } else { 0.0 }))
}

/// `v` within `lo..=hi`; `lo` when it is not a number.
pub(super) fn finite(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { lo }
}

/// `AABB.intersects`: the boxes overlap, touching faces excluded.
pub(super) fn box_meets(b: &horizon_core::geometry::Aabb, min: [f64; 3], max: [f64; 3]) -> bool {
    let (lo, hi) = (b.min_position(), b.max_position());
    lo.x < max[0]
        && hi.x > min[0]
        && lo.y < max[1]
        && hi.y > min[1]
        && lo.z < max[2]
        && hi.z > min[2]
}
