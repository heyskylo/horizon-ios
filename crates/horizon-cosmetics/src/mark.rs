//! Horizon's mark as the game draws it before a Horizon player's name (and the launcher before
//! yours): two white bars over a violet line, their reflection fainter under it, 8×8.

/// The mark's rectangles in its 8×8 square: `(x, y, width, height, ARGB)`.
pub const MARK: [(u8, u8, u8, u8, u32); 5] = [
    (2, 1, 1, 3, 0xFFFF_FFFF),
    (5, 1, 1, 3, 0xFFFF_FFFF),
    (0, 4, 8, 1, 0xFFA9_8FFF),
    (2, 5, 1, 2, 0x66FF_FFFF),
    (5, 5, 1, 2, 0x66FF_FFFF),
];

/// The mark as 8×8 RGBA texels, rows from the top.
pub fn texels() -> [[u8; 4]; 64] {
    let mut out = [[0; 4]; 64];
    for (x, y, w, h, argb) in MARK {
        let [a, r, g, b] = argb.to_be_bytes();
        for yy in y..y + h {
            for xx in x..x + w {
                out[usize::from(yy) * 8 + usize::from(xx)] = [r, g, b, a];
            }
        }
    }
    out
}
