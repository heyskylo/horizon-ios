use super::{GRADIENTS, GradientTable};
use crate::{geometry::floor_i32, random::RandomSource};

#[derive(Clone, Debug)]
pub struct SimplexNoise {
    table: GradientTable,
}

impl SimplexNoise {
    pub fn new(random: &mut impl RandomSource) -> Self {
        Self::with_skipped_offsets(random, false)
    }
    pub fn with_skipped_offsets(random: &mut impl RandomSource, skip_offsets: bool) -> Self {
        Self {
            table: GradientTable::new(random, if skip_offsets { 0.0 } else { 256.0 }),
        }
    }
    pub fn table(&self) -> &GradientTable {
        &self.table
    }
    pub fn range(&self) -> [f32; 2] {
        [-2.0, 2.0]
    }

    fn corner(gradient: usize, point: [f64; 3], radius: f64) -> f64 {
        let attenuation = radius - point[0] * point[0] - point[1] * point[1] - point[2] * point[2];
        // Negative attenuation contributes zero; NaN follows the arithmetic path.
        if attenuation < 0.0 {
            0.0
        } else {
            let squared = attenuation * attenuation;
            let g = GRADIENTS[gradient];
            squared
                * squared
                * (f64::from(g[0]) * point[0]
                    + f64::from(g[1]) * point[1]
                    + f64::from(g[2]) * point[2])
        }
    }

    pub fn get2(&self, x: f64, y: f64) -> f32 {
        self.get2_f64(x, y) as f32
    }

    /// `get2` before its cast to `float` (1.21's `SimplexNoise.getValue(x, y)` returned the double).
    pub fn get2_f64(&self, x: f64, y: f64) -> f64 {
        let x = x + self.table.offsets[0];
        let y = y + self.table.offsets[1];
        let root3 = 3.0_f64.sqrt();
        let skew = 0.5 * (root3 - 1.0);
        let unskew = (3.0 - root3) / 6.0;
        let shift = (x + y) * skew;
        let i = floor_i32(x + shift);
        let j = floor_i32(y + shift);
        let shift = f64::from(i.wrapping_add(j)) * unskew;
        let dx = x - (f64::from(i) - shift);
        let dy = y - (f64::from(j) - shift);
        let (a, b) = if dx > dy { (1, 0) } else { (0, 1) };
        let i = i & 255;
        let j = j & 255;
        let grad = |a, b| (self.table.permute(i + a + self.table.permute(j + b)) % 12) as usize;
        let first = Self::corner(grad(0, 0), [dx, dy, 0.0], 0.5);
        let second = Self::corner(
            grad(a, b),
            [dx - f64::from(a) + unskew, dy - f64::from(b) + unskew, 0.0],
            0.5,
        );
        let third = Self::corner(
            grad(1, 1),
            [dx - 1.0 + 2.0 * unskew, dy - 1.0 + 2.0 * unskew, 0.0],
            0.5,
        );
        70.0 * (first + second + third)
    }

    pub fn get3(&self, x: f64, y: f64, z: f64) -> f32 {
        self.get3_f64(x, y, z) as f32
    }

    /// `get3` before its cast to `float`.
    pub fn get3_f64(&self, x: f64, y: f64, z: f64) -> f64 {
        let shifted = [
            x + self.table.offsets[0],
            y + self.table.offsets[1],
            z + self.table.offsets[2],
        ];
        let skew = (shifted[0] + shifted[1] + shifted[2]) * (1.0 / 3.0);
        let cell = shifted.map(|v| floor_i32(v + skew));
        let unskew = f64::from(cell[0].wrapping_add(cell[1]).wrapping_add(cell[2])) * (1.0 / 6.0);
        let point: [f64; 3] = std::array::from_fn(|i| shifted[i] - (f64::from(cell[i]) - unskew));
        let [x, y, z] = point;
        let (a, b) = if x >= y {
            if y >= z {
                ([1, 0, 0], [1, 1, 0])
            } else if x >= z {
                ([1, 0, 0], [1, 0, 1])
            } else {
                ([0, 0, 1], [1, 0, 1])
            }
        } else if y < z {
            ([0, 0, 1], [0, 1, 1])
        } else if x < z {
            ([0, 1, 0], [0, 1, 1])
        } else {
            ([0, 1, 0], [1, 1, 0])
        };
        let cell = cell.map(|v| v & 255);
        let grad = |delta: [i32; 3]| {
            (self.table.permute(
                cell[0]
                    + delta[0]
                    + self
                        .table
                        .permute(cell[1] + delta[1] + self.table.permute(cell[2] + delta[2])),
            ) % 12) as usize
        };
        let first = Self::corner(grad([0; 3]), point, 0.6);
        let second = Self::corner(
            grad(a),
            std::array::from_fn(|i| point[i] - f64::from(a[i]) + (1.0 / 6.0)),
            0.6,
        );
        let third = Self::corner(
            grad(b),
            std::array::from_fn(|i| point[i] - f64::from(b[i]) + (1.0 / 3.0)),
            0.6,
        );
        let fourth = Self::corner(grad([1; 3]), point.map(|v| v - 1.0 + 0.5), 0.6);
        32.0 * (first + second + third + fourth)
    }
}
