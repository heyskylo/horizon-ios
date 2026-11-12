//! The boxes that stop a dropped item, by cell (`ObstacleGrid`).
use crate::fast_hash::FxHashMap;
use horizon_core::geometry::{Aabb, Vec3};

/// The boxes that stop a dropped item (boats, minecarts, shulkers, a held-still happy ghast),
/// binned by 8-block cell, so an item looks only at the ones near it: a farm with many drops and
/// many vehicles checks a handful of boxes per item, not every box in the level.
#[derive(Default)]
pub struct ObstacleGrid {
    cells: FxHashMap<(i32, i32, i32), Vec<u32>>,
    boxes: Vec<Aabb>,
    /// Each box's entity's `getRootVehicle` when it rides something; empty
    /// when none was given.
    roots: Vec<Option<u32>>,
    found: Vec<u32>,
}
impl ObstacleGrid {
    const CELL: f64 = 8.0;
    pub fn new(boxes: Vec<Aabb>) -> Self {
        Self::with_roots(boxes, Vec::new())
    }
    /// [`Self::new`] with the vehicle at the bottom of what each box's entity
    /// rides (`roots[i]` for `boxes[i]`), so [`Self::near_except`] can leave a
    /// mover's own riders out.
    pub fn with_roots(boxes: Vec<Aabb>, roots: Vec<Option<u32>>) -> Self {
        let mut cells: FxHashMap<(i32, i32, i32), Vec<u32>> = Default::default();
        for (i, b) in boxes.iter().enumerate() {
            let (lo, hi) = Self::span(*b);
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self {
            cells,
            boxes,
            roots,
            found: Vec::new(),
        }
    }
    fn span(b: Aabb) -> ((i32, i32, i32), (i32, i32, i32)) {
        let cell = |v: Vec3| {
            (
                (v.x / Self::CELL).floor() as i32,
                (v.y / Self::CELL).floor() as i32,
                (v.z / Self::CELL).floor() as i32,
            )
        };
        (cell(b.min_position()), cell(b.max_position()))
    }
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }
    /// The boxes meeting `area` (`getEntityCollisions`'s query box), in the order they were given.
    pub fn near(&mut self, area: Aabb, out: &mut Vec<Aabb>) {
        self.near_except(area, None, out);
    }
    /// [`Self::near`] without the boxes of what rides `vehicle` (given with
    /// [`Self::with_roots`]): `Entity.canCollideWith` leaves out
    /// `isPassengerOfSameVehicle`, so a stand's riders do not stop it.
    pub fn near_except(&mut self, area: Aabb, vehicle: Option<u32>, out: &mut Vec<Aabb>) {
        let mut found = std::mem::take(&mut self.found);
        self.near_with(area, vehicle, &mut found, out);
        self.found = found;
    }
    /// [`Self::near`] with the caller's scratch, so several threads can ask one grid.
    pub fn near_shared(&self, area: Aabb, found: &mut Vec<u32>, out: &mut Vec<Aabb>) {
        self.near_with(area, None, found, out);
    }
    fn near_with(
        &self,
        area: Aabb,
        vehicle: Option<u32>,
        found: &mut Vec<u32>,
        out: &mut Vec<Aabb>,
    ) {
        out.clear();
        if self.boxes.is_empty() {
            return;
        }
        let kept = |i: usize| vehicle.is_none() || self.roots.get(i).copied().flatten() != vehicle;
        let (lo, hi) = Self::span(area);
        let side = |lo: i32, hi: i32| (i64::from(hi) - i64::from(lo) + 1).max(0);
        let cells = side(lo.0, hi.0)
            .saturating_mul(side(lo.1, hi.1))
            .saturating_mul(side(lo.2, hi.2));
        // An area over more cells than there are boxes (a stand loaded with a
        // huge Motion asks for one millions of blocks long): test each box
        // once rather than walk the cells.
        if cells > self.boxes.len() as i64 {
            out.extend(
                self.boxes
                    .iter()
                    .enumerate()
                    .filter(|&(i, b)| kept(i) && b.intersects(area))
                    .map(|(_, b)| *b),
            );
            return;
        }
        found.clear();
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    if let Some(list) = self.cells.get(&(x, y, z)) {
                        found.extend(list);
                    }
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        for &i in found.iter() {
            let b = self.boxes[i as usize];
            if kept(i as usize) && b.intersects(area) {
                out.push(b);
            }
        }
    }
}
