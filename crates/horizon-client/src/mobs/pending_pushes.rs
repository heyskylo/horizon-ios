//! The pushes a mob's tick gives others, waiting for each one's own tick.
use super::Mob;
use horizon_core::geometry::Vec3;

/// Pushes waiting for their mob's tick, in push order. A list while short;
/// past [`PendingPushes::LIST_MAX`] (a crowd pushing each other) one list
/// per mob, as scanning the whole list for each mob grows with its square.
#[derive(Default)]
pub struct PendingPushes {
    list: Vec<(u32, Vec3)>,
    by_mob: crate::fast_hash::FxHashMap<u32, Vec<Vec3>>,
}
impl PendingPushes {
    const LIST_MAX: usize = 64;
    /// `other`'s pushes after these, each mob's in their order.
    pub fn append(&mut self, other: PendingPushes) {
        for (id, v) in other.list {
            self.push(id, v);
        }
        for (id, list) in other.by_mob {
            for v in list {
                self.push(id, v);
            }
        }
    }
    pub fn push(&mut self, id: u32, v: Vec3) {
        if self.by_mob.is_empty() && self.list.len() < Self::LIST_MAX {
            self.list.push((id, v));
            return;
        }
        // Older pushes first, so each mob's stay in order.
        for (id, v) in self.list.drain(..) {
            self.by_mob.entry(id).or_default().push(v);
        }
        self.by_mob.entry(id).or_default().push(v);
    }
}
/// Adds the pending pushes aimed at `m` to its velocity, in push order.
pub fn apply_mob_pushes(m: &mut Mob, pending: &mut PendingPushes) {
    // Entity.push(x, y, z) sets needsSync.
    let mut add = |v: Vec3| {
        m.velocity = Vec3::new(m.velocity.x + v.x, m.velocity.y + v.y, m.velocity.z + v.z);
        m.needs_sync = true;
    };
    if !pending.list.is_empty() {
        pending.list.retain(|(id, v)| {
            if *id != m.id {
                return true;
            }
            add(*v);
            false
        });
    }
    if !pending.by_mob.is_empty()
        && let Some(pushes) = pending.by_mob.remove(&m.id)
    {
        pushes.into_iter().for_each(add);
    }
}

#[cfg(test)]
mod tests {
    use super::PendingPushes;
    use horizon_core::geometry::Vec3;

    /// Each mob's pushes, in the order they apply.
    fn per_mob(p: &PendingPushes) -> std::collections::BTreeMap<u32, Vec<[f64; 3]>> {
        let mut out = std::collections::BTreeMap::<u32, Vec<[f64; 3]>>::new();
        for (id, v) in &p.list {
            out.entry(*id).or_default().push([v.x, v.y, v.z]);
        }
        for (id, list) in &p.by_mob {
            out.entry(*id)
                .or_default()
                .extend(list.iter().map(|v| [v.x, v.y, v.z]));
        }
        out
    }

    /// Two lists of pushes, the second appended to the first, apply as one list of them all in
    /// that order, on either side of the switch from one list to a list per mob.
    #[test]
    fn appended_pushes_apply_in_order() {
        let pushes: Vec<(u32, Vec3)> = (0..200u32)
            .map(|k| (k * 7 % 11, Vec3::new(f64::from(k), 0.0, -f64::from(k))))
            .collect();
        let mut all = PendingPushes::default();
        for &(id, v) in &pushes {
            all.push(id, v);
        }
        for split in [0, 30, 64, 70, 150, 200] {
            let (mut first, mut second) = (PendingPushes::default(), PendingPushes::default());
            for &(id, v) in &pushes[..split] {
                first.push(id, v);
            }
            for &(id, v) in &pushes[split..] {
                second.push(id, v);
            }
            first.append(second);
            assert_eq!(per_mob(&first), per_mob(&all), "split at {split}");
        }
    }
}
