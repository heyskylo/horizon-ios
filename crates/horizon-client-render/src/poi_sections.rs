//! The points of interest of each resident section (`PoiSection`), so a
//! search visits the few POI blocks of the sections in range instead of
//! reading every block of them.
//!
//! Vanilla keeps its records up to date from `onBlockStateChange`; here a
//! section's list is rebuilt from its blocks after any edit. Resident
//! sections are immutable behind `Arc` and edited through `Arc::make_mut`, so
//! a weak reference tells an edited or replaced section from the one the list
//! was built from (see `random_tick_sections`), without keeping its blocks.
use crate::{
    chunk::{Chunk, Section},
    fast_hash::FxHashMap,
};
use std::sync::{Arc, Mutex, Weak};

/// A section's POIs: the block index in the section (`y << 8 | z << 4 | x`)
/// and its kind, in ascending index order.
pub type SectionPois<T> = Arc<[(u16, T)]>;

struct Cached<T> {
    section: Weak<Section>,
    pois: SectionPois<T>,
}
/// Cached POI lists by chunk column `(chunk x, chunk z)`, each by section
/// index: a search through a column looks it up once.
pub struct PoiSections<T> {
    sections: Mutex<Lists<T>>,
}
struct Lists<T> {
    by_column: FxHashMap<(i32, i32), Vec<Option<Cached<T>>>>,
    /// How many section lists are kept, and the count above which the lists
    /// of sections no longer resident are dropped.
    len: usize,
    prune_at: usize,
}
const MIN_PRUNE: usize = 4096;
impl<T> Default for PoiSections<T> {
    fn default() -> Self {
        Self {
            sections: Mutex::new(Lists {
                by_column: FxHashMap::default(),
                len: 0,
                prune_at: MIN_PRUNE,
            }),
        }
    }
}
impl<T: Copy> Lists<T> {
    /// The column's lists, for sections up to `last`, after dropping the
    /// lists of sections no longer resident if there are too many.
    fn column(
        &mut self,
        column: (i32, i32),
        last: usize,
    ) -> (&mut [Option<Cached<T>>], &mut usize) {
        if self.len >= self.prune_at {
            self.prune();
        }
        let lists = self.by_column.entry(column).or_default();
        if lists.len() <= last {
            lists.resize_with(last + 1, || None);
        }
        (lists, &mut self.len)
    }

    /// Drops the lists of sections no longer resident.
    fn prune(&mut self) {
        let mut len = 0;
        self.by_column.retain(|_, lists| {
            for c in lists.iter_mut() {
                if c.as_ref().is_some_and(|c| c.section.strong_count() == 0) {
                    *c = None;
                }
            }
            let kept = lists.iter().filter(|c| c.is_some()).count();
            len += kept;
            kept > 0
        });
        self.len = len;
        self.prune_at = (len * 2).max(MIN_PRUNE);
    }
}

/// The POIs of `section` from `slot`, listed again (and kept there) unless
/// `slot` was listed from this very section; `len` counts the slots filled.
fn list<'a, T: Copy>(
    slot: &'a mut Option<Cached<T>>,
    len: &mut usize,
    section: &Arc<Section>,
    kind: &mut impl FnMut(u32) -> Option<T>,
) -> &'a SectionPois<T> {
    let fresh = slot
        .as_ref()
        .is_some_and(|c| c.section.as_ptr() == Arc::as_ptr(section));
    if !fresh {
        let pois: SectionPois<T> = if section.blocks.maybe_has(|v| kind(v).is_some()) {
            let mut states = [0u32; 4096];
            section.unpack(&mut states);
            states
                .iter()
                .enumerate()
                .filter_map(|(i, &state)| kind(state).map(|poi| (i as u16, poi)))
                .collect()
        } else {
            Arc::from([])
        };
        *len += usize::from(slot.is_none());
        *slot = Some(Cached {
            section: Arc::downgrade(section),
            pois,
        });
    }
    &slot.as_ref().expect("just listed").pois
}

impl<T: Copy> PoiSections<T> {
    /// The POIs of `section`, the `key.1`-th section of the column
    /// `(key.0, key.2)`, whose block states `kind` classifies. `kind` must give
    /// the same answer for a state on every call.
    pub fn get(
        &self,
        key: (i32, i32, i32),
        section: &Arc<Section>,
        mut kind: impl FnMut(u32) -> Option<T>,
    ) -> SectionPois<T> {
        let mut lists = self.sections.lock().unwrap_or_else(|e| e.into_inner());
        let s = usize::try_from(key.1).expect("a section index");
        let (column, len) = lists.column((key.0, key.2), s);
        list(&mut column[s], len, section, &mut kind).clone()
    }

    /// The POIs of `chunk` from block Y `y0` to `y1`, its first section at
    /// `min_y`, as [`Self::get`] gives them: each section that has any, with
    /// the Y of its lowest blocks, in order, put in `out`. One lookup of the
    /// column.
    pub fn column(
        &self,
        chunk: &Chunk,
        min_y: i32,
        (y0, y1): (i32, i32),
        mut kind: impl FnMut(u32) -> Option<T>,
        out: &mut Vec<(i32, SectionPois<T>)>,
    ) {
        out.clear();
        let (first, last) = ((y0 - min_y) >> 4, (y1 - min_y) >> 4);
        if first > last {
            return;
        }
        let mut lists = self.sections.lock().unwrap_or_else(|e| e.into_inner());
        let (slots, len) = lists.column((chunk.x, chunk.z), last as usize);
        for s in first as usize..=last as usize {
            let pois = list(&mut slots[s], len, &chunk.sections[s], &mut kind);
            if !pois.is_empty() {
                out.push((min_y + s as i32 * 16, pois.clone()));
            }
        }
    }
}

/// The POI types villagers use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poi {
    /// A job site, by profession index.
    Job(u8),
    Home,
    Meeting,
}
impl Poi {
    /// `PoiType.maxTickets`.
    pub fn max_tickets(self) -> usize {
        match self {
            Poi::Meeting => 32,
            _ => 1,
        }
    }
    /// `PoiType.validRange`.
    pub fn valid_range(self) -> i32 {
        match self {
            Poi::Meeting => 6,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Chunk, MIN_Y};

    #[test]
    fn lists_follow_edits_and_replacement() {
        let index = PoiSections::<u8>::default();
        let kind = |v: u32| (v == 5 || v == 6).then_some(v as u8);
        let mut chunk = Arc::new(Chunk::empty(0, 0));
        assert!(index.get((0, 0, 0), &chunk.sections[0], kind).is_empty());
        Arc::make_mut(&mut chunk).set(3, MIN_Y + 2, 1, 6);
        Arc::make_mut(&mut chunk).set(0, MIN_Y, 0, 5);
        let found = index.get((0, 0, 0), &chunk.sections[0], kind);
        assert_eq!(&*found, &[(0, 5), (2 << 8 | 1 << 4 | 3, 6)]);
        // Unchanged: the cached list, not a rescan.
        let again = index.get((0, 0, 0), &chunk.sections[0], |_| panic!("rescanned"));
        assert!(Arc::ptr_eq(&found, &again));
        // An edit in place (no other owner) still invalidates the list.
        Arc::make_mut(&mut chunk).set(0, MIN_Y, 0, 1);
        assert_eq!(
            &*index.get((0, 0, 0), &chunk.sections[0], kind),
            &[(2 << 8 | 1 << 4 | 3, 6)]
        );
        // So does replacing the section, or the chunk.
        Arc::make_mut(&mut chunk).sections[0] = Arc::new(Section::default());
        assert!(index.get((0, 0, 0), &chunk.sections[0], kind).is_empty());
        let mut other = Chunk::empty(0, 0);
        other.set(1, MIN_Y, 1, 5);
        chunk = Arc::new(other);
        assert_eq!(
            &*index.get((0, 0, 0), &chunk.sections[0], kind),
            &[(1 << 4 | 1, 5)]
        );
        // An unused palette entry alone yields no POI.
        Arc::make_mut(&mut chunk).set(1, MIN_Y, 1, 2);
        assert!(index.get((0, 0, 0), &chunk.sections[0], kind).is_empty());
    }

    /// A column gives each section's list as `get` does, the sections
    /// without any left out, and keeps them until a section changes.
    #[test]
    fn a_column_gives_what_each_section_gives() {
        let index = PoiSections::<u8>::default();
        let kind = |v: u32| (v == 5).then_some(5);
        let mut chunk = Arc::new(Chunk::empty(2, -3));
        for (x, y, z) in [(1, MIN_Y + 2, 3), (4, MIN_Y + 40, 0), (0, MIN_Y + 41, 15)] {
            Arc::make_mut(&mut chunk).set(x, y, z, 5);
        }
        let mut out = Vec::new();
        index.column(&chunk, MIN_Y, (MIN_Y, MIN_Y + 63), kind, &mut out);
        let bases: Vec<i32> = out.iter().map(|(base, _)| *base).collect();
        assert_eq!(bases, [MIN_Y, MIN_Y + 32]);
        for (base, pois) in &out {
            let s = (base - MIN_Y) / 16;
            let one = index.get((2, s, -3), &chunk.sections[s as usize], kind);
            assert!(Arc::ptr_eq(pois, &one));
        }
        // Kept: no rescan.
        index.column(
            &chunk,
            MIN_Y,
            (MIN_Y, MIN_Y + 63),
            |_| panic!("rescanned"),
            &mut out,
        );
        assert_eq!(out.len(), 2);
        // An edit lists that section again.
        Arc::make_mut(&mut chunk).set(4, MIN_Y + 40, 0, 1);
        let mut rescans = 0;
        let counted = |v: u32| {
            rescans += 1;
            kind(v)
        };
        index.column(&chunk, MIN_Y, (MIN_Y + 20, MIN_Y + 63), counted, &mut out);
        assert!(rescans > 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, MIN_Y + 32);
        assert_eq!(&*out[0].1, &[(9 << 8 | 15 << 4, 5)]);
    }

    #[test]
    fn unloaded_sections_are_dropped() {
        let index = PoiSections::<u8>::default();
        let kind = |v: u32| (v == 5).then_some(5);
        let kept = Arc::new(Section::default());
        index.get((0, 0, 0), &kept, kind);
        for i in 0..MIN_PRUNE as i32 {
            index.get((1, i, 0), &Arc::new(Section::default()), kind);
        }
        let lists = index.sections.lock().unwrap();
        assert!(lists.len <= 2);
        assert!(lists.by_column[&(0, 0)][0].is_some());
    }
}
