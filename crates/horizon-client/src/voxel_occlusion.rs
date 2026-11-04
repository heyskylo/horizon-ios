//! Conservative, budgeted occlusion against full opaque block cross-sections.
//!
//! A covered projection rectangle proves that an entire bounding box is hidden.
//! Testing a few rays would miss openings between those rays.
use horizon_core::geometry::Aabb;
use std::cell::Cell;

const EPSILON: f64 = 1.0e-5;

pub struct Culler {
    pub camera: [f64; 3],
    remaining: Cell<usize>,
}

impl Culler {
    pub fn new(camera: [f64; 3], budget: usize) -> Self {
        Self {
            camera,
            remaining: Cell::new(budget),
        }
    }

    /// Remaining block lookups in this frame's shared culling budget.
    pub fn remaining(&self) -> usize {
        self.remaining.get()
    }

    /// Remove only this entity's completed camera-relative mesh when hidden.
    pub fn cull_vertices(
        &self,
        vertices: &mut Vec<crate::mesher::Vertex>,
        start: usize,
        opaque: impl FnMut([i32; 3]) -> bool,
    ) -> bool {
        let hidden = self.hidden(vertices, start, opaque);
        if hidden {
            vertices.truncate(start);
        }
        hidden
    }

    /// Whether this entity's completed camera-relative mesh (`vertices[start..]`) is hidden.
    pub fn hidden(
        &self,
        vertices: &[crate::mesher::Vertex],
        start: usize,
        opaque: impl FnMut([i32; 3]) -> bool,
    ) -> bool {
        if self.remaining.get() == 0 {
            return false;
        }
        let Some((min, max)) = Self::bounds(self.camera, vertices, start) else {
            return false;
        };
        self.occluded(min, max, opaque)
    }

    /// Whether a declared world-space entity AABB is hidden. Entity model
    /// bounds receive the same small edge margin as mesh-derived bounds.
    pub fn hidden_bounds(&self, bounds: Aabb, opaque: impl FnMut([i32; 3]) -> bool) -> bool {
        if self.remaining.get() == 0 {
            return false;
        }
        let Some((min, max)) = Self::aabb_bounds(bounds) else {
            return false;
        };
        self.occluded(min, max, opaque)
    }

    /// Whether camera-relative model bounds are hidden. This avoids building
    /// synthetic vertices when a renderer already has a conservative AABB.
    pub fn hidden_camera_bounds(
        &self,
        lo: [f32; 3],
        hi: [f32; 3],
        opaque: impl FnMut([i32; 3]) -> bool,
    ) -> bool {
        if self.remaining.get() == 0
            || self.camera.into_iter().any(|value| !value.is_finite())
            || lo.into_iter().chain(hi).any(|value| !value.is_finite())
        {
            return false;
        }
        let min = std::array::from_fn(|axis| {
            f64::from(lo[axis].min(hi[axis])) + self.camera[axis] - 0.05
        });
        let max = std::array::from_fn(|axis| {
            f64::from(lo[axis].max(hi[axis])) + self.camera[axis] + 0.05
        });
        self.occluded(min, max, opaque)
    }

    /// World-space bounds expanded for floating-point and rasterization edges.
    pub fn aabb_bounds(bounds: Aabb) -> Option<([f64; 3], [f64; 3])> {
        let lo = bounds.min_position();
        let hi = bounds.max_position();
        let min = [lo.x, lo.y, lo.z];
        let max = [hi.x, hi.y, hi.z];
        if (0..3)
            .any(|axis| !min[axis].is_finite() || !max[axis].is_finite() || min[axis] > max[axis])
        {
            return None;
        }
        Some((min.map(|value| value - 0.05), max.map(|value| value + 0.05)))
    }

    /// World-space bounds from camera-relative vertices, grown for floating-point
    /// and rasterization edges. The asynchronous path sends these to its worker.
    pub fn bounds(
        camera: [f64; 3],
        vertices: &[crate::mesher::Vertex],
        start: usize,
    ) -> Option<([f64; 3], [f64; 3])> {
        if start >= vertices.len() || camera.into_iter().any(|value| !value.is_finite()) {
            return None;
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for vertex in &vertices[start..] {
            let [x, y, z] = vertex.pos;
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return None;
            }
            let [x, y, z] = [f64::from(x), f64::from(y), f64::from(z)];
            min[0] = min[0].min(x);
            max[0] = max[0].max(x);
            min[1] = min[1].min(y);
            max[1] = max[1].max(y);
            min[2] = min[2].min(z);
            max[2] = max[2].max(z);
        }
        // Camera translation is monotonic, so offsetting the extrema is exact
        // and avoids three f64 additions for every scanned vertex.
        let min = std::array::from_fn(|axis| min[axis] + camera[axis]);
        let max = std::array::from_fn(|axis| max[axis] + camera[axis]);
        // Cover floating-point and rasterization edges as well as the vertices.
        let min = min.map(|value| value - 0.05);
        let max = max.map(|value| value + 0.05);
        Some((min, max))
    }

    /// Returns true only when a complete opaque block plane covers the bounds.
    /// Unknown blocks, invalid bounds and an exhausted lookup budget stay visible.
    pub fn occluded(
        &self,
        min: [f64; 3],
        max: [f64; 3],
        mut opaque: impl FnMut([i32; 3]) -> bool,
    ) -> bool {
        let mut remaining = self.remaining.get();
        let hidden = Self::occluded_with_budget(self.camera, &mut remaining, min, max, &mut opaque);
        self.remaining.set(remaining);
        hidden
    }

    /// The worker-facing form of [`Self::occluded`]. Keeping the budget in a
    /// local integer avoids `Cell` loads and stores for every projected plane
    /// while preserving the same conservative lookup accounting.
    pub fn occluded_with_budget(
        camera: [f64; 3],
        remaining: &mut usize,
        min: [f64; 3],
        max: [f64; 3],
        mut opaque: impl FnMut([i32; 3]) -> bool,
    ) -> bool {
        if (0..3).any(|a| {
            !camera[a].is_finite() || !min[a].is_finite() || !max[a].is_finite() || min[a] > max[a]
        }) {
            return false;
        }
        let mut best = (0, 0.0, 0.0);
        for a in 0..3 {
            let (gap, sign) = if camera[a] < min[a] {
                (min[a] - camera[a], 1.0)
            } else {
                (camera[a] - max[a], -1.0)
            };
            if gap > best.1 {
                best = (a, gap, sign);
            }
        }
        let (axis, gap, sign) = best;
        if !gap.is_finite() || gap <= EPSILON {
            return false;
        }
        let across = [(axis + 1) % 3, (axis + 2) % 3];
        let Some(mut cell) = floor_cell(camera[axis]) else {
            return false;
        };
        let step = if sign > 0.0 { 1 } else { -1 };
        if (f64::from(cell) + 0.5 - camera[axis]) * sign <= EPSILON {
            let Some(next) = cell.checked_add(step) else {
                return false;
            };
            cell = next;
        }
        // These two reciprocals are invariant for every plane along the ray.
        let denominator_min = (min[axis] - camera[axis]) * sign;
        let denominator_max = (max[axis] - camera[axis]) * sign;
        if !denominator_min.is_finite()
            || !denominator_max.is_finite()
            || denominator_min.abs() <= EPSILON
            || denominator_max.abs() <= EPSILON
        {
            return false;
        }
        let reciprocal_min = 1.0 / denominator_min;
        let reciprocal_max = 1.0 / denominator_max;
        let transverse_min = [
            min[across[0]] - camera[across[0]],
            min[across[1]] - camera[across[1]],
        ];
        let transverse_max = [
            max[across[0]] - camera[across[0]],
            max[across[1]] - camera[across[1]],
        ];
        loop {
            let depth = (f64::from(cell) + 0.5 - camera[axis]) * sign;
            if depth >= gap - EPSILON || *remaining == 0 {
                return false;
            }
            let t_min = depth * reciprocal_min;
            let t_max = depth * reciprocal_max;
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for i in 0..2 {
                let camera_axis = camera[across[i]];
                for t in [t_min, t_max] {
                    let p_min = camera_axis + t * transverse_min[i];
                    let p_max = camera_axis + t * transverse_max[i];
                    if !p_min.is_finite() || !p_max.is_finite() {
                        return false;
                    }
                    lo[i] = lo[i].min(p_min);
                    hi[i] = hi[i].max(p_max);
                }
            }
            let bounds = [
                floor_cell(lo[0] - EPSILON),
                floor_cell(hi[0] + EPSILON),
                floor_cell(lo[1] - EPSILON),
                floor_cell(hi[1] + EPSILON),
            ];
            let [Some(x0), Some(x1), Some(y0), Some(y1)] = bounds else {
                return false;
            };
            let center_x = x0 + ((i64::from(x1) - i64::from(x0)) / 2) as i32;
            let center_y = y0 + ((i64::from(y1) - i64::from(y0)) / 2) as i32;
            let mut pos = [0; 3];
            pos[axis] = cell;
            pos[across[0]] = center_x;
            pos[across[1]] = center_y;
            *remaining -= 1;
            if opaque(pos) {
                let area = (i64::from(x1) - i64::from(x0) + 1)
                    .checked_mul(i64::from(y1) - i64::from(y0) + 1)
                    .and_then(|n| usize::try_from(n).ok());
                let Some(additional) = area.and_then(|n| n.checked_sub(1)) else {
                    return false;
                };
                if additional > *remaining {
                    return false;
                }
                // Keep Y in the inner loop for X-facing planes. This keeps
                // consecutive probes in one chunk column. Charge only probes
                // actually visited when a gap short-circuits the rectangle.
                let covered = if across[0] == 1 {
                    (y0..=y1).all(|y| {
                        pos[across[1]] = y;
                        (x0..=x1).all(|x| {
                            if x == center_x && y == center_y {
                                return true;
                            }
                            *remaining -= 1;
                            pos[across[0]] = x;
                            opaque(pos)
                        })
                    })
                } else {
                    (x0..=x1).all(|x| {
                        pos[across[0]] = x;
                        (y0..=y1).all(|y| {
                            if x == center_x && y == center_y {
                                return true;
                            }
                            *remaining -= 1;
                            pos[across[1]] = y;
                            opaque(pos)
                        })
                    })
                };
                if covered {
                    return true;
                }
            }
            let Some(next) = cell.checked_add(step) else {
                return false;
            };
            cell = next;
        }
    }
}

fn floor_cell(value: f64) -> Option<i32> {
    let n = value.floor();
    (n.is_finite() && n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX)).then_some(n as i32)
}

#[cfg(test)]
#[path = "voxel_occlusion_tests.rs"]
mod tests;
