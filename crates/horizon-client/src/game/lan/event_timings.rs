//! Bounded attribution for bursts whose individual events are too short for a slow-step note.

// A busy server can see many connections and packet kinds in one poll. Instrumentation must not
// grow with that cardinality. Individual slow events still use the existing notes independently.
const KEPT: usize = 32;

pub(super) struct Burst<K, D> {
    pub kind: K,
    pub detail: Option<D>,
    pub ms: f32,
    pub count: u32,
    longest: f32,
}

pub(super) struct EventTimings<K, D> {
    groups: Vec<Burst<K, D>>,
    overflow: (f32, u32, f32),
}

impl<K, D> Default for EventTimings<K, D> {
    fn default() -> Self {
        Self {
            groups: Vec::new(),
            overflow: (0.0, 0, 0.0),
        }
    }
}

impl<K: PartialEq, D: PartialEq> EventTimings<K, D> {
    pub fn add(&mut self, kind: K, detail: Option<D>, ms: f32) {
        if let Some(group) = self.groups.iter_mut().find(|g| g.kind == kind) {
            group.ms += ms;
            group.count = group.count.saturating_add(1);
            group.longest = group.longest.max(ms);
            if group.detail != detail {
                // A burst can visit different blocks or change the held item. Do not attribute
                // all of its cost to the first target in that case.
                group.detail = None;
            }
        } else if self.groups.len() < KEPT {
            self.groups.push(Burst {
                kind,
                detail,
                ms,
                count: 1,
                longest: ms,
            });
        } else {
            self.overflow.0 += ms;
            self.overflow.1 = self.overflow.1.saturating_add(1);
            self.overflow.2 = self.overflow.2.max(ms);
        }
    }

    pub fn overflow(&self, threshold: f32) -> Option<(f32, u32)> {
        let (ms, count, longest) = self.overflow;
        (count > 1 && ms >= threshold && longest < threshold).then_some((ms, count))
    }

    pub fn bursts(self, threshold: f32) -> impl Iterator<Item = Burst<K, D>> {
        self.groups
            .into_iter()
            .filter(move |g| g.count > 1 && g.ms >= threshold && g.longest < threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_events_that_add_up_to_a_slow_step_keep_their_culprit() {
        let mut timings = EventTimings::default();
        for _ in 0..200 {
            timings.add(("UseItemOn", 7), Some("diamond axe on chest"), 5.0);
        }
        let bursts: Vec<_> = timings.bursts(10.0).collect();
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].kind, ("UseItemOn", 7));
        assert_eq!(bursts[0].detail, Some("diamond axe on chest"));
        assert_eq!(bursts[0].count, 200);
        assert_eq!(bursts[0].ms, 1000.0);
    }

    #[test]
    fn groups_separate_players_and_packets_and_discard_mixed_target_details() {
        let mut timings = EventTimings::default();
        timings.add(("UseItemOn", 1), Some("chest"), 6.0);
        timings.add(("UseItemOn", 2), Some("sign"), 6.0);
        timings.add(("UseItem", 1), Some("bow"), 6.0);
        timings.add(("UseItemOn", 1), Some("sign"), 6.0);
        let bursts: Vec<_> = timings.bursts(10.0).collect();
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].kind, ("UseItemOn", 1));
        assert_eq!(bursts[0].detail, None);
        assert_eq!(bursts[0].ms, 12.0);
    }

    #[test]
    fn individually_slow_events_are_not_reported_twice() {
        let mut timings = EventTimings::default();
        timings.add(1, None::<()>, 15.0);
        timings.add(1, None, 5.0);
        timings.add(2, None, 4.0);
        timings.add(2, None, 4.0);
        assert_eq!(timings.bursts(10.0).count(), 0);
    }

    #[test]
    fn cardinality_is_bounded() {
        let mut timings = EventTimings::default();
        for key in 0..10_000 {
            timings.add(key, None::<()>, 5.0);
        }
        assert_eq!(timings.groups.len(), KEPT);
        assert_eq!(timings.overflow(10.0), Some((49_840.0, 9968)));
        timings.add(0, None, 5.0);
        assert_eq!(timings.bursts(10.0).count(), 1);
    }
}
