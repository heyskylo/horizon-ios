//! The tick's mob snapshot indexed by where the boxes are, so a box query
//! (`LivingEntity.pushEntities`) reads the mobs near it instead of every
//! mob, and still meets them in snapshot order.
use crate::fast_hash::FxHashMap;
use horizon_core::geometry::{Aabb, Axis};

/// Width of a grid column, in blocks.
const CELL: f64 = 4.0;

/// Snapshot indices by the column holding each box's minimum corner.
#[derive(Default)]
pub struct SeenGrid {
    cells: FxHashMap<(i32, i32), Vec<u32>>,
    cell_of: Vec<(i32, i32)>,
    /// The widest box (along x or z) the grid has held: no box reaches
    /// further than this past its minimum corner.
    reach: f64,
}

fn column(x: f64, z: f64) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}
fn corner(b: Aabb) -> (i32, i32) {
    column(b.min(Axis::X), b.min(Axis::Z))
}
fn width(b: Aabb) -> f64 {
    (b.max(Axis::X) - b.min(Axis::X)).max(b.max(Axis::Z) - b.min(Axis::Z))
}

impl SeenGrid {
    /// The boxes of the snapshot's entries, in order.
    pub fn new(boxes: impl Iterator<Item = Aabb>) -> Self {
        let mut grid = Self::default();
        for (i, b) in boxes.enumerate() {
            let at = corner(b);
            grid.reach = grid.reach.max(width(b));
            grid.cells.entry(at).or_default().push(i as u32);
            grid.cell_of.push(at);
        }
        grid
    }
    /// Entry `i`'s box is now `b` (the mob moved or resized).
    pub fn update(&mut self, i: usize, b: Aabb) {
        self.reach = self.reach.max(width(b));
        let at = corner(b);
        let old = std::mem::replace(&mut self.cell_of[i], at);
        if old == at {
            return;
        }
        if let Some(list) = self.cells.get_mut(&old)
            && let Ok(k) = list.binary_search(&(i as u32))
        {
            list.remove(k);
        }
        let list = self.cells.entry(at).or_default();
        let idx = match list.binary_search(&(i as u32)) {
            Ok(k) | Err(k) => k,
        };
        list.insert(idx, i as u32);
    }
    /// The indices, ascending, of every entry whose box can meet `area`
    /// (a superset: callers still test it). False when the area spans more
    /// columns than the grid has entries (at least 64), or is not finite:
    /// scan them all.
    pub fn candidates(&self, area: Aabb, out: &mut Vec<u32>) -> bool {
        out.clear();
        // A box meets `area` only if its minimum corner is below area.max
        // and within `reach` (plus a margin for rounding) of area.min.
        let margin = self.reach + 1.0;
        let min_x = area.min(Axis::X) - margin;
        let min_z = area.min(Axis::Z) - margin;
        let max_x = area.max(Axis::X);
        let max_z = area.max(Axis::Z);
        if !min_x.is_finite() || !min_z.is_finite() || !max_x.is_finite() || !max_z.is_finite() {
            return false;
        }
        let lo = column(min_x, min_z);
        let hi = column(max_x, max_z);
        let span = |a: i32, b: i32| i64::from(b) - i64::from(a) + 1;
        let (nx, nz) = (span(lo.0, hi.0), span(lo.1, hi.1));
        let max_cells = (self.cell_of.len() as i64).max(64);
        if nx < 1 || nz < 1 || nx.saturating_mul(nz) > max_cells {
            return false;
        }
        let mut slices = [&[][..]; 16];
        let mut num_slices = 0;
        let mut overflow = false;
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                if let Some(list) = self.cells.get(&(x, z))
                    && !list.is_empty()
                {
                    if num_slices < slices.len() {
                        slices[num_slices] = list.as_slice();
                        num_slices += 1;
                    } else {
                        overflow = true;
                        out.extend_from_slice(list);
                    }
                }
            }
        }
        if overflow {
            for s in &slices[..num_slices] {
                out.extend_from_slice(s);
            }
            out.sort_unstable();
            return true;
        }
        merge_slices(&slices[..num_slices], out);
        true
    }
}

fn merge_2(s0: &[u32], s1: &[u32], out: &mut Vec<u32>) {
    out.reserve(s0.len() + s1.len());
    let (mut i, mut j) = (0, 0);
    while i < s0.len() && j < s1.len() {
        if s0[i] < s1[j] {
            out.push(s0[i]);
            i += 1;
        } else {
            out.push(s1[j]);
            j += 1;
        }
    }
    out.extend_from_slice(&s0[i..]);
    out.extend_from_slice(&s1[j..]);
}

fn merge_slices(slices: &[&[u32]], out: &mut Vec<u32>) {
    match slices.len() {
        0 => {}
        1 => out.extend_from_slice(slices[0]),
        2 => merge_2(slices[0], slices[1], out),
        3 => {
            let (mut i, mut j, mut k) = (0, 0, 0);
            let (s0, s1, s2) = (slices[0], slices[1], slices[2]);
            out.reserve(s0.len() + s1.len() + s2.len());
            while i < s0.len() || j < s1.len() || k < s2.len() {
                let v0 = if i < s0.len() { s0[i] } else { u32::MAX };
                let v1 = if j < s1.len() { s1[j] } else { u32::MAX };
                let v2 = if k < s2.len() { s2[k] } else { u32::MAX };
                if v0 <= v1 && v0 <= v2 {
                    out.push(v0);
                    i += 1;
                } else if v1 <= v2 {
                    out.push(v1);
                    j += 1;
                } else {
                    out.push(v2);
                    k += 1;
                }
            }
        }
        4 => {
            let (mut i, mut j, mut k, mut l) = (0, 0, 0, 0);
            let (s0, s1, s2, s3) = (slices[0], slices[1], slices[2], slices[3]);
            out.reserve(s0.len() + s1.len() + s2.len() + s3.len());
            while i < s0.len() || j < s1.len() || k < s2.len() || l < s3.len() {
                let v0 = if i < s0.len() { s0[i] } else { u32::MAX };
                let v1 = if j < s1.len() { s1[j] } else { u32::MAX };
                let v2 = if k < s2.len() { s2[k] } else { u32::MAX };
                let v3 = if l < s3.len() { s3[l] } else { u32::MAX };
                if v0 <= v1 && v0 <= v2 && v0 <= v3 {
                    out.push(v0);
                    i += 1;
                } else if v1 <= v2 && v1 <= v3 {
                    out.push(v1);
                    j += 1;
                } else if v2 <= v3 {
                    out.push(v2);
                    k += 1;
                } else {
                    out.push(v3);
                    l += 1;
                }
            }
        }
        _ => {
            for s in slices {
                out.extend_from_slice(s);
            }
            out.sort_unstable();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;

    fn boxed(x: f64, y: f64, z: f64, w: f64) -> Aabb {
        Aabb::new(Vec3::new(x, y, z), Vec3::new(x + w, y + 1.8, z + w))
    }

    #[test]
    fn candidates_hold_every_meeting_box_in_order() {
        // A small LCG keeps the test free of dependencies.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |range: f64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * range
        };
        let mut boxes: Vec<Aabb> = (0..300)
            .map(|_| {
                boxed(
                    next(64.0) - 32.0,
                    next(8.0),
                    next(64.0) - 32.0,
                    0.3 + next(1.2),
                )
            })
            .collect();
        let mut grid = SeenGrid::new(boxes.iter().copied());
        let mut out = Vec::new();
        let mut indexed = 0;
        for round in 0..2000 {
            // Move one box, sometimes far and wider, as mobs do between queries.
            let i = round % boxes.len();
            let w = if round % 97 == 0 { 3.9 } else { 0.6 };
            boxes[i] = boxed(next(80.0) - 40.0, next(8.0), next(80.0) - 40.0, w);
            grid.update(i, boxes[i]);
            let area = boxed(next(70.0) - 35.0, next(8.0), next(70.0) - 35.0, next(2.0));
            let expected: Vec<u32> = (0..boxes.len() as u32)
                .filter(|&j| boxes[j as usize].intersects(area))
                .collect();
            if grid.candidates(area, &mut out) {
                indexed += 1;
                assert!(out.windows(2).all(|p| p[0] < p[1]), "ascending, no repeats");
                let met: Vec<u32> = out
                    .iter()
                    .copied()
                    .filter(|&j| boxes[j as usize].intersects(area))
                    .collect();
                assert_eq!(met, expected);
            }
        }
        assert!(indexed > 1900, "the grid answered {indexed} of 2000");
    }

    #[test]
    fn unbounded_areas_fall_back_to_a_full_scan() {
        let grid = SeenGrid::new([boxed(0.0, 0.0, 0.0, 0.6)].into_iter());
        let mut out = Vec::new();
        let huge = Aabb::new(
            Vec3::new(f64::NEG_INFINITY, 0.0, 0.0),
            Vec3::new(f64::INFINITY, 1.0, 1.0),
        );
        assert!(!grid.candidates(huge, &mut out));
    }

    #[test]
    fn candidates_merges_multiple_cells_in_strict_order() {
        // Place boxes across 4 adjacent grid cells: (0, 0), (4, 0), (0, 4), (4, 4)
        // With interleaving snapshot indices to test 1, 2, 3, and 4-way merges.
        let boxes = vec![
            boxed(0.5, 64.0, 0.5, 0.6), // 0: cell (0, 0)
            boxed(4.5, 64.0, 0.5, 0.6), // 1: cell (1, 0)
            boxed(0.5, 64.0, 4.5, 0.6), // 2: cell (0, 1)
            boxed(4.5, 64.0, 4.5, 0.6), // 3: cell (1, 1)
            boxed(0.8, 64.0, 0.8, 0.6), // 4: cell (0, 0)
            boxed(4.8, 64.0, 0.8, 0.6), // 5: cell (1, 0)
            boxed(0.8, 64.0, 4.8, 0.6), // 6: cell (0, 1)
            boxed(4.8, 64.0, 4.8, 0.6), // 7: cell (1, 1)
        ];
        let grid = SeenGrid::new(boxes.into_iter());
        let mut out = Vec::new();

        // Query covering only cell (0, 0): indices 0, 4
        let q1 = boxed(0.1, 64.0, 0.1, 1.5);
        assert!(grid.candidates(q1, &mut out));
        assert_eq!(out, vec![0, 4]);

        // Query covering cells (0, 0) and (1, 0): indices 0, 1, 4, 5
        let q2 = Aabb::new(Vec3::new(0.0, 64.0, 0.0), Vec3::new(6.0, 66.0, 1.5));
        assert!(grid.candidates(q2, &mut out));
        assert_eq!(out, vec![0, 1, 4, 5]);

        // Query covering all 4 cells: indices 0..8 in order
        let q4 = Aabb::new(Vec3::new(0.0, 64.0, 0.0), Vec3::new(6.0, 66.0, 6.0));
        assert!(grid.candidates(q4, &mut out));
        assert_eq!(out, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    #[ignore]
    fn bench_dense_farm() {
        let n = 300;
        let mut seed = 0x1234_5678_9abc_def0_u64;
        let mut next = |range: f64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 11) as f64 / (1u64 << 53) as f64 * range
        };
        // 300 mobs in a 1.5 x 1.5 block pen around (3.5, 64.0, 3.5) crossing cell boundary at 4.0
        let mut boxes: Vec<Aabb> = (0..n)
            .map(|_| {
                let x = 3.5 + next(1.5);
                let z = 3.5 + next(1.5);
                boxed(x, 64.0, z, 0.6)
            })
            .collect();
        let mut grid = SeenGrid::new(boxes.iter().copied());
        let mut out = Vec::new();

        let t = std::time::Instant::now();
        let rounds = 100;
        let mut fallback_count = 0;
        let mut query_dur = std::time::Duration::ZERO;
        let mut update_dur = std::time::Duration::ZERO;
        for _ in 0..rounds {
            for i in 0..n {
                let area = boxes[i];
                let t0 = std::time::Instant::now();
                if !grid.candidates(area, &mut out) {
                    fallback_count += 1;
                    out.clear();
                    out.extend(0..n as u32);
                }
                query_dur += t0.elapsed();
                // small jitter
                let x = 3.5 + next(1.5);
                let z = 3.5 + next(1.5);
                boxes[i] = boxed(x, 64.0, z, 0.6);
                let t1 = std::time::Instant::now();
                grid.update(i, boxes[i]);
                update_dur += t1.elapsed();
            }
        }
        let elapsed = t.elapsed();
        println!(
            "{rounds} ticks of {n} mobs in 1.5x1.5 pen: {elapsed:?} (query: {:.3} ms, update: {:.3} ms, per tick: {:.3} ms, fallback: {fallback_count}/{})",
            query_dur.as_secs_f64() * 1e3 / rounds as f64,
            update_dur.as_secs_f64() * 1e3 / rounds as f64,
            elapsed.as_secs_f64() * 1e3 / rounds as f64,
            rounds * n
        );
    }
}
