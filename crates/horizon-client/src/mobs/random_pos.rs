//! `RandomPos` helpers of the random-position goals.
use crate::mobs::MobWorld;

/// RandomPos.moveUpToAboveSolid: out of the solid blocks, then up to
/// `above` more blocks of air.
pub(super) fn move_up_to_above_solid(w: &MobWorld, mut p: [i32; 3], above: i32) -> [i32; 3] {
    if !w.solid(p[0], p[1], p[2]) {
        return p;
    }
    p[1] += 1;
    while p[1] < w.world.top_y() && w.solid(p[0], p[1], p[2]) {
        p[1] += 1;
    }
    let first = p[1];
    while p[1] < w.world.top_y() && p[1] - first < above {
        p[1] += 1;
        if w.solid(p[0], p[1], p[2]) {
            p[1] -= 1;
            break;
        }
    }
    p
}
