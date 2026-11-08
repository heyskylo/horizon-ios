//! Where a shader pack's block atlas puts each sprite: vanilla's `Stitcher`, so every sprite keeps
//! its own size and the atlas comes out as large as the game's (`atlasSize`). Holders are sorted
//! by height, then width, then (where vanilla compares names) sprite layer.

/// The atlas a set of sprites stitches into.
#[derive(Debug, PartialEq)]
pub(super) struct Stitched {
    pub width: u32,
    pub height: u32,
    /// Each sprite's padded holder: its top left corner. The sprite itself starts `padding` in.
    pub holders: Vec<[u32; 2]>,
    pub padding: u32,
}

/// `Stitcher.smallestFittingMinTexel`.
fn fit(input: u32, mip_level: u32) -> u32 {
    let step = 1 << mip_level;
    input.div_ceil(step) * step
}

/// `Mth.smallestEncompassingPowerOfTwo` (0 stays 0).
fn power_of_two(input: u32) -> u32 {
    if input == 0 {
        0
    } else {
        input.next_power_of_two()
    }
}

/// `Stitcher.Region`.
struct Region {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    holder: bool,
    sub: Vec<Region>,
}

impl Region {
    fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            holder: false,
            sub: Vec::new(),
        }
    }

    /// `Region.add`: the holder's top left corner, when it fits.
    fn add(&mut self, [w, h]: [u32; 2]) -> Option<[u32; 2]> {
        if self.holder || w > self.width || h > self.height {
            return None;
        }
        if w == self.width && h == self.height {
            self.holder = true;
            return Some([self.x, self.y]);
        }
        if self.sub.is_empty() {
            let (x, y) = (self.x, self.y);
            self.sub.push(Region::new(x, y, w, h));
            let (spare_w, spare_h) = (self.width - w, self.height - h);
            if spare_h > 0 && spare_w > 0 {
                let right = self.height.max(spare_w);
                let bottom = self.width.max(spare_h);
                if right >= bottom {
                    self.sub.push(Region::new(x, y + h, w, spare_h));
                    self.sub.push(Region::new(x + w, y, spare_w, self.height));
                } else {
                    self.sub.push(Region::new(x + w, y, spare_w, h));
                    self.sub.push(Region::new(x, y + h, self.width, spare_h));
                }
            } else if spare_w == 0 {
                self.sub.push(Region::new(x, y + h, w, spare_h));
            } else if spare_h == 0 {
                self.sub.push(Region::new(x + w, y, spare_w, h));
            }
        }
        self.sub.iter_mut().find_map(|r| r.add([w, h]))
    }
}

/// `Stitcher` over square sprites of `sides` pixels, with vanilla's padding for `mip_level` and
/// `anisotropy_bit`; `None` when they don't fit in `max` by `max`.
pub(super) fn stitch(
    sides: &[u32],
    mip_level: u32,
    anisotropy_bit: u32,
    max: u32,
) -> Option<Stitched> {
    let padding = (1 << mip_level) << anisotropy_bit.saturating_sub(1).min(4);
    let mut order: Vec<usize> = (0..sides.len()).collect();
    let holder = |i: usize| fit(sides[i] + padding * 2, mip_level);
    order.sort_by_key(|&i| (std::cmp::Reverse(holder(i)), i));
    let (mut storage_x, mut storage_y) = (0u32, 0u32);
    let mut storage: Vec<Region> = Vec::new();
    let mut holders = vec![[0, 0]; sides.len()];
    for i in order {
        let size = [holder(i); 2];
        if let Some(at) = storage.iter_mut().find_map(|r| r.add(size)) {
            holders[i] = at;
            continue;
        }
        // `Stitcher.expand`.
        let (x_now, y_now) = (power_of_two(storage_x), power_of_two(storage_y));
        let (x_new, y_new) = (
            power_of_two(storage_x + size[0]),
            power_of_two(storage_y + size[1]),
        );
        let (x_can, y_can) = (x_new <= max, y_new <= max);
        if !x_can && !y_can {
            return None;
        }
        let (x_will, y_will) = (x_can && x_now != x_new, y_can && y_now != y_new);
        let grow_x = if x_will ^ y_will {
            x_will
        } else {
            x_can && x_now <= y_now
        };
        let mut slot = if grow_x {
            if storage_y == 0 {
                storage_y = y_new;
            }
            let slot = Region::new(storage_x, 0, x_new - storage_x, storage_y);
            storage_x = x_new;
            slot
        } else {
            let slot = Region::new(0, storage_y, storage_x, y_new - storage_y);
            storage_y = y_new;
            slot
        };
        holders[i] = slot.add(size)?;
        storage.push(slot);
    }
    Some(Stitched {
        width: storage_x,
        height: storage_y,
        holders,
        padding,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Five 16 px sprites with one pixel of padding (no mipmaps), worked through vanilla's
    /// `Stitcher` by hand: 18 px holders in a 64 by 64 atlas.
    #[test]
    fn equal_sprites_follow_vanillas_regions() {
        let stitched = stitch(&[16; 5], 0, 0, 1 << 14).unwrap();
        assert_eq!(
            (stitched.width, stitched.height, stitched.padding),
            (64, 64, 1)
        );
        assert_eq!(
            stitched.holders,
            [[0, 0], [32, 0], [0, 32], [18, 32], [36, 32]]
        );
    }

    /// The largest sprite goes first; padding and holders round up to the mip step.
    #[test]
    fn larger_sprites_go_first_and_holders_round_to_the_mip_step() {
        // Mip level 2: 4 px padding, holders 24 (16 + 8) and 40 (32 + 8) px.
        let stitched = stitch(&[16, 32], 2, 0, 1 << 14).unwrap();
        assert_eq!(stitched.padding, 4);
        assert_eq!(stitched.holders[1], [0, 0]);
        // 40 px grows the atlas to 64 by 64; the 24 px holder fits below the first.
        assert_eq!((stitched.width, stitched.height), (64, 64));
        assert_eq!(stitched.holders[0], [0, 40]);
        // Anisotropy 16x (bit 4) multiplies the padding by 8.
        assert_eq!(stitch(&[16], 4, 4, 1 << 14).unwrap().padding, 128);
        assert_eq!(stitch(&[16; 2], 0, 0, 16), None);
    }
}
