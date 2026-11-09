//! Where the level's dropped items lie, in 8-block cells, for the passes that
//! test each mob's reach against every item (`Mob.aiStep`'s looting, the
//! foxes' and dolphins' item searches). A reach that meets no cell holding an
//! item meets no item, so its scan, which would find nothing, is skipped; a
//! reach near an item scans as before, in the same order, with the same tests,
//! jumping over the items of the cells it doesn't meet ([`ItemScan`]).
use horizon_client::{entities::ItemEntity, fast_hash::FxHashMap, join_seq::JoinSeq};
use horizon_core::geometry::Aabb;

/// Cells of 8 blocks on each axis.
const SHIFT: i32 = 3;

#[derive(Default)]
pub(super) struct ItemAreas {
    /// The items in each cell, by index, in the list's order.
    cells: FxHashMap<[i32; 3], Vec<u32>>,
    /// What the cells were built from: the list's length, the last entity id
    /// the game handed out and the next join number. An item spawned or
    /// removed since changes one of them, and the cells are built again.
    built: Option<(usize, u32, u64)>,
}

impl ItemAreas {
    /// Whether an item of `items` may meet `area`: false only when none can.
    pub(super) fn may_meet(&mut self, items: &[ItemEntity], next_entity: u32, area: Aabb) -> bool {
        self.build(items, next_entity);
        if self.cells.is_empty() {
            return false;
        }
        let Some((from, to)) = cell_span(area) else {
            return true;
        };
        if span(from, to) > self.cells.len() as i64 {
            return self
                .cells
                .keys()
                .any(|c| (0..3).all(|a| (from[a]..=to[a]).contains(&c[a])));
        }
        (from[2]..=to[2]).any(|z| {
            (from[1]..=to[1])
                .any(|y| (from[0]..=to[0]).any(|x| self.cells.contains_key(&[x, y, z])))
        })
    }

    /// A scan of `items` for `area`, which jumps over the items that can't
    /// meet it; empty when none can ([`Self::may_meet`] false).
    pub(super) fn scan(&mut self, items: &[ItemEntity], next_entity: u32, area: Aabb) -> ItemScan {
        let all = ItemScan {
            near: None,
            passed: 0,
            len: items.len(),
        };
        if !self.may_meet(items, next_entity, area) {
            return ItemScan {
                near: Some(Vec::new()),
                ..all
            };
        }
        let Some((from, to)) = cell_span(area) else {
            return all;
        };
        if span(from, to) > self.cells.len() as i64 {
            return all;
        }
        let mut near = Vec::new();
        for z in from[2]..=to[2] {
            for y in from[1]..=to[1] {
                for x in from[0]..=to[0] {
                    if let Some(list) = self.cells.get(&[x, y, z]) {
                        near.extend(list.iter().map(|&i| i as usize));
                    }
                }
            }
        }
        near.sort_unstable();
        ItemScan {
            near: Some(near),
            ..all
        }
    }

    fn build(&mut self, items: &[ItemEntity], next_entity: u32) {
        let key = (items.len(), next_entity, JoinSeq::upcoming().get());
        if self.built == Some(key) {
            return;
        }
        self.cells.clear();
        for (i, e) in items.iter().enumerate() {
            let p = e.position;
            let at = [cell(p.x), cell(p.y), cell(p.z)];
            self.cells.entry(at).or_default().push(i as u32);
        }
        self.built = Some(key);
    }
}

/// One reach's walk over the items, in the list's order: [`Self::next_from`] gives
/// the next index to test, past the items that lie in cells the reach doesn't
/// meet. Told of each item the walk removes, it keeps up; if the list changes
/// any other way (an item joins), the walk tests every item from there on.
pub(super) struct ItemScan {
    /// The items in the cells the reach meets, by index; `None`: every item.
    near: Option<Vec<usize>>,
    /// The first of `near` not yet passed.
    passed: usize,
    /// The list's length as the walk knows it.
    len: usize,
}

impl ItemScan {
    /// Whether no item can meet the reach.
    pub(super) fn is_empty(&self) -> bool {
        self.near.as_ref().is_some_and(Vec::is_empty)
    }
    /// The first index from `j` the walk must test (`len` or more: done).
    pub(super) fn next_from(&mut self, j: usize, len: usize) -> usize {
        if len != self.len {
            self.near = None;
        }
        let Some(near) = &self.near else {
            return j;
        };
        while near.get(self.passed).is_some_and(|&i| i < j) {
            self.passed += 1;
        }
        near.get(self.passed).copied().unwrap_or(len).max(j)
    }
    /// The walk removed item `j`: the items after it moved down one.
    pub(super) fn removed(&mut self, j: usize) {
        self.len -= 1;
        if let Some(near) = &mut self.near {
            for i in near.iter_mut().filter(|i| **i > j) {
                *i -= 1;
            }
        }
    }
}

/// The cells an item meeting `area` may lie in: an item's box spans its width
/// sideways and upwards from its position, and a margin of a whole width
/// keeps rounding on the safe side. `None` for an area that isn't finite.
fn cell_span(area: Aabb) -> Option<([i32; 3], [i32; 3])> {
    let (lo, hi) = (area.min_position(), area.max_position());
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let m = ItemEntity::WIDTH;
    let from = [cell(lo.x - m), cell(lo.y - 2.0 * m), cell(lo.z - m)];
    let to = [cell(hi.x + m), cell(hi.y + m), cell(hi.z + m)];
    Some((from, to))
}

fn span(from: [i32; 3], to: [i32; 3]) -> i64 {
    (0..3)
        .map(|a| i64::from(to[a]) - i64::from(from[a]) + 1)
        .product()
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
            random: horizon_core::random::LegacyRandom::new(0),
            rotation: [0.0, 0.0],
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
            fall_distance: 0.0,
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

    /// The items a walk tests that meet `area`, by position: what the scans
    /// act on. `take` removes the item it names when the walk meets it.
    fn walk(items: &mut Vec<ItemEntity>, area: Aabb, take: Option<f64>) -> Vec<f64> {
        let mut cells = ItemAreas::default();
        let mut scan = cells.scan(items, 1, area);
        let (mut met, mut j) = (Vec::new(), 0);
        loop {
            j = scan.next_from(j, items.len());
            let Some(e) = items.get(j) else {
                break;
            };
            if !area.intersects(e.bounds()) {
                j += 1;
                continue;
            }
            met.push(e.position.x);
            if take == Some(e.position.x) {
                items.remove(j);
                scan.removed(j);
            } else {
                j += 1;
            }
        }
        met
    }

    #[test]
    fn a_walk_meets_what_a_full_scan_meets_in_the_same_order() {
        let mut items = Vec::new();
        for i in 0..60 {
            let x = -40.0 + f64::from(i) * 1.37;
            items.push(item_at(
                x,
                64.0 + f64::from(i % 3) * 0.6,
                (x * 0.7).sin() * 9.0,
            ));
        }
        for k in 0..40 {
            let x = -42.0 + f64::from(k) * 2.11;
            let area = Aabb::new(Vec3::new(x, 63.5, -10.0), Vec3::new(x + 3.3, 65.5, 10.0));
            let full: Vec<f64> = items
                .iter()
                .filter(|e| area.intersects(e.bounds()))
                .map(|e| e.position.x)
                .collect();
            assert_eq!(walk(&mut items.clone(), area, None), full, "{area:?}");
        }
    }

    #[test]
    fn a_walk_keeps_up_with_the_items_it_removes() {
        let xs = [1.0, 30.0, 1.5, 31.0, 2.0, 2.5];
        let mut items: Vec<ItemEntity> = xs.iter().map(|&x| item_at(x, 64.0, 1.0)).collect();
        let area = Aabb::new(Vec3::new(0.0, 63.0, 0.0), Vec3::new(3.0, 65.0, 2.0));
        assert_eq!(walk(&mut items, area, Some(1.5)), vec![1.0, 1.5, 2.0, 2.5]);
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn a_walk_tests_an_item_that_joins_during_it() {
        let mut items = vec![item_at(1.0, 64.0, 1.0), item_at(40.0, 64.0, 1.0)];
        let area = Aabb::new(Vec3::new(0.0, 63.0, 0.0), Vec3::new(3.0, 65.0, 2.0));
        let mut cells = ItemAreas::default();
        let mut scan = cells.scan(&items, 1, area);
        assert_eq!(scan.next_from(0, items.len()), 0);
        // One joins at the end, inside the reach, after the walk began.
        items.push(item_at(2.0, 64.0, 1.0));
        assert_eq!(scan.next_from(1, items.len()), 1);
        assert_eq!(scan.next_from(2, items.len()), 2);
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
