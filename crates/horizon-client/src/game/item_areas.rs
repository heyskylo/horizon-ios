//! Where the level's dropped items lie, in 8-block cells, for the passes that
//! test each mob's reach against every item (`Mob.aiStep`'s looting, the
//! foxes' and dolphins' item searches). A reach that meets no cell holding an
//! item meets no item, so its scan, which would find nothing, is skipped; a
//! reach near an item scans as before, in the same order, with the same tests.
use horizon_client::{entities::ItemEntity, fast_hash::FxHashSet, join_seq::JoinSeq};
use horizon_core::geometry::Aabb;

/// Cells of 8 blocks on each axis.
const SHIFT: i32 = 3;

#[derive(Default)]
pub(super) struct ItemAreas {
    cells: FxHashSet<[i32; 3]>,
    /// What the cells were built from: the list's length, the last entity id
    /// the game handed out and the next join number. An item spawned or
    /// removed since changes one of them, and the cells are built again.
    built: Option<(usize, u32, u64)>,
}

impl ItemAreas {
    /// Whether an item of `items` may meet `area`: false only when none can.
    pub(super) fn may_meet(&mut self, items: &[ItemEntity], next_entity: u32, area: Aabb) -> bool {
        let key = (items.len(), next_entity, JoinSeq::upcoming().get());
        if self.built != Some(key) {
            self.cells.clear();
            self.cells.extend(items.iter().map(|e| {
                let p = e.position;
                [cell(p.x), cell(p.y), cell(p.z)]
            }));
            self.built = Some(key);
        }
        if self.cells.is_empty() {
            return false;
        }
        let (lo, hi) = (area.min_position(), area.max_position());
        if !lo.is_finite() || !hi.is_finite() {
            return true;
        }
        // An item's box spans its width sideways and upwards from its
        // position; a margin of a whole width keeps rounding on the safe side.
        let m = ItemEntity::WIDTH;
        let from = [cell(lo.x - m), cell(lo.y - 2.0 * m), cell(lo.z - m)];
        let to = [cell(hi.x + m), cell(hi.y + m), cell(hi.z + m)];
        let span = (0..3)
            .map(|a| i64::from(to[a]) - i64::from(from[a]) + 1)
            .product::<i64>();
        if span > self.cells.len() as i64 {
            return self
                .cells
                .iter()
                .any(|c| (0..3).all(|a| (from[a]..=to[a]).contains(&c[a])));
        }
        (from[2]..=to[2]).any(|z| {
            (from[1]..=to[1]).any(|y| (from[0]..=to[0]).any(|x| self.cells.contains(&[x, y, z])))
        })
    }
}

fn cell(v: f64) -> i32 {
    (v.floor() as i32) >> SHIFT
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;

    fn item_at(x: f64, y: f64, z: f64) -> ItemEntity {
        let p = Vec3::new(x, y, z);
        ItemEntity {
            join: Default::default(),
            health: 5,
            fire_ticks: 0,
            id: 1,
            stack: horizon_client::items::ItemStack::new("minecraft:egg", 1),
            thrower: None,
            target: None,
            position: p,
            previous: p,
            velocity: Vec3::ZERO,
            on_ground: true,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
            no_gravity: false,
            vertical_collision: false,
            horizontal_collision: false,
        }
    }

    /// Whether some item's own box meets `area`, as the scans test it.
    fn scan_meets(items: &[ItemEntity], area: Aabb) -> bool {
        items.iter().any(|e| area.intersects(e.bounds()))
    }

    #[test]
    fn a_reach_that_meets_an_item_is_never_skipped() {
        // Items just inside and just outside reach of a box, across cell
        // edges and on both sides of zero.
        let mut items = Vec::new();
        for &x in &[-16.13, -8.0, -7.9, -0.125, 0.0, 7.87, 8.0, 15.99] {
            for &y in &[-64.0, -1.0, 0.0, 7.75, 64.3] {
                items.push(item_at(x, y, x * 0.5));
            }
        }
        let mut cells = ItemAreas::default();
        for &x in &[-17.0, -9.0, -8.3, -1.0, -0.4, 0.2, 7.0, 7.9, 8.2, 15.0] {
            for &y in &[-65.0, -2.0, -0.3, 6.9, 7.6, 63.0] {
                for &w in &[0.3, 1.3, 8.3] {
                    let area = Aabb::new(
                        Vec3::new(x - w, y, x * 0.5 - w),
                        Vec3::new(x + w, y + 1.95, x * 0.5 + w),
                    );
                    if scan_meets(&items, area) {
                        assert!(cells.may_meet(&items, 1, area), "{area:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_reach_far_from_every_item_is_skipped() {
        let items = vec![item_at(100.5, 64.0, 100.5)];
        let mut cells = ItemAreas::default();
        let near = Aabb::new(Vec3::new(98.0, 63.0, 98.0), Vec3::new(101.0, 65.0, 101.0));
        let far = Aabb::new(Vec3::new(0.0, 63.0, 0.0), Vec3::new(2.6, 65.0, 2.6));
        assert!(cells.may_meet(&items, 1, near));
        assert!(!cells.may_meet(&items, 1, far));
        assert!(!cells.may_meet(&[], 1, near));
    }

    #[test]
    fn an_item_spawned_after_the_cells_were_built_is_seen() {
        let mut items = vec![item_at(100.5, 64.0, 100.5)];
        let mut cells = ItemAreas::default();
        let area = Aabb::new(Vec3::new(0.0, 63.0, 0.0), Vec3::new(2.6, 65.0, 2.6));
        assert!(!cells.may_meet(&items, 1, area));
        // Spawned where the first one was taken: the same length, a new id.
        items[0] = item_at(1.0, 64.0, 1.0);
        assert!(cells.may_meet(&items, 2, area));
    }
}
