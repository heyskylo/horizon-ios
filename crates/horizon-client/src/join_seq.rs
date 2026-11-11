//! PURPOSE: the join sequence: when each entity joined the level, kept in one
//! counter that rises with every join, whatever the kind.
//! WHY: ids do not say it (primed TNT, falling blocks, projectiles, tridents and
//! rockets take theirs late, in kind-list order), and vanilla ticks entities in
//! the order they were added, so a walk in that order needs a number that does
//! (#6509, tick order slice).
//! REFS: vanilla `EntityLookup` / `EntitySectionStorage` insertion order,
//! `PersistentEntitySectionManager.addEntity`; issue #6509; PR #7808.
//! LOGS: none. READS-BY: `game/entity.rs` (the join-order walk, the save order).
//! ENDS-WHEN: kinds share one stored entity list, which is its own order.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
static NEXT_LIST: AtomicU64 = AtomicU64::new(1);

/// When an entity joined: a number above every earlier join's. Zero means it
/// has not joined a list yet. One counter for the process, so entities of
/// different levels compare too; only the order of the numbers means anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JoinSeq(u64);

impl JoinSeq {
    /// Not joined yet.
    pub const UNJOINED: Self = Self(0);
    /// The next number: one relaxed add, and only when something joins.
    fn next() -> Self {
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
    /// Above every number a join can get.
    pub const MAX: Self = Self(u64::MAX);
    /// The number the next join will get or a later one: every entity that has
    /// joined so far is below it.
    pub fn upcoming() -> Self {
        Self(NEXT.load(Ordering::Relaxed))
    }
    pub fn is_joined(self) -> bool {
        self.0 != 0
    }
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A kind that stores when it joined.
pub trait Joined {
    fn join_seq(&self) -> JoinSeq;
    fn set_join_seq(&mut self, seq: JoinSeq);
}

/// Implements [`Joined`] for a struct with a `join: JoinSeq` field.
#[macro_export]
macro_rules! impl_joined {
    ($($ty:ty),* $(,)?) => {$(
        impl $crate::join_seq::Joined for $ty {
            fn join_seq(&self) -> $crate::join_seq::JoinSeq {
                self.join
            }
            fn set_join_seq(&mut self, seq: $crate::join_seq::JoinSeq) {
                self.join = seq;
            }
        }
    )*};
}

/// Which list and how many writes it has seen: two equal stamps mean the same
/// list with nothing written in between, so what was read from it still holds.
/// No list shares its first half with another, so one taken out and put back,
/// or a level swapped in, cannot pass for the old one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListStamp {
    list: u64,
    writes: u64,
}

impl ListStamp {
    fn new() -> Self {
        Self {
            list: NEXT_LIST.fetch_add(1, Ordering::Relaxed),
            writes: 0,
        }
    }
}

/// One kind's entity list. It is a `Vec` in every way but two: an entity that
/// is put in without a join number (`push`, `insert`, `extend`) gets the next
/// one, so a join cannot be missed by a spawn site. One already numbered (taken
/// out and put back, or a list rebuilt) keeps its number. And every way to
/// reach it mutably counts as a write ([`JoinList::stamp`]), whether or not it
/// changes anything, so a scan of the list can be kept until the next write.
#[derive(Debug)]
pub struct JoinList<T> {
    list: Vec<T>,
    stamp: ListStamp,
}

impl<T> Default for JoinList<T> {
    fn default() -> Self {
        Self::from(Vec::new())
    }
}

impl<T: Clone> Clone for JoinList<T> {
    /// A list of its own from here on: the two change apart.
    fn clone(&self) -> Self {
        Self::from(self.list.clone())
    }
}

impl<T> From<Vec<T>> for JoinList<T> {
    /// A list put back after a tick took it: the same storage, nothing copied
    /// and nothing renumbered. An entity in it that never joined stays
    /// unnumbered, which the join-order walk reports.
    fn from(list: Vec<T>) -> Self {
        Self {
            list,
            stamp: ListStamp::new(),
        }
    }
}

impl<T> From<JoinList<T>> for Vec<T> {
    fn from(list: JoinList<T>) -> Self {
        list.list
    }
}

impl<T> Deref for JoinList<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.list
    }
}
impl<T> DerefMut for JoinList<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        self.stamp.writes += 1;
        &mut self.list
    }
}

impl<T> JoinList<T> {
    /// This list as it stands now: see [`ListStamp`].
    pub fn stamp(&self) -> ListStamp {
        self.stamp
    }
}

impl<T: Joined> JoinList<T> {
    fn number(entity: &mut T) {
        if !entity.join_seq().is_joined() {
            entity.set_join_seq(JoinSeq::next());
        }
    }
    pub fn push(&mut self, mut entity: T) {
        Self::number(&mut entity);
        self.deref_mut().push(entity);
    }
    pub fn insert(&mut self, index: usize, mut entity: T) {
        Self::number(&mut entity);
        self.deref_mut().insert(index, entity);
    }
    pub fn extend(&mut self, entities: impl IntoIterator<Item = T>) {
        for entity in entities {
            self.push(entity);
        }
    }
    /// An entity that never left its level keeps its place in
    /// `EntityTickList`: one a pass took out and puts back (an arrow bouncing
    /// off) goes back before the first entity that joined after it. One that
    /// has not joined yet is appended and numbered, as by `push`.
    pub fn put_back(&mut self, entity: T) {
        let seq = entity.join_seq();
        let at = if seq.is_joined() {
            self.list
                .iter()
                .position(|q| q.join_seq() > seq)
                .unwrap_or(self.list.len())
        } else {
            self.list.len()
        };
        self.insert(at, entity);
    }
}

impl<'a, T> IntoIterator for &'a JoinList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}
impl<'a, T> IntoIterator for &'a mut JoinList<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.deref_mut().iter_mut()
    }
}
impl<T> IntoIterator for JoinList<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct E {
        join: JoinSeq,
    }
    crate::impl_joined!(E);

    /// A numbered entity put back lands by its join number; a new one goes last.
    #[test]
    fn put_back_keeps_the_join_order() {
        let mut list = JoinList::default();
        for _ in 0..3 {
            list.push(E {
                join: JoinSeq::UNJOINED,
            });
        }
        let first = list.remove(0);
        let first_seq = first.join;
        list.put_back(first);
        assert_eq!(list[0].join, first_seq, "back before the later joins");
        list.put_back(E {
            join: JoinSeq::UNJOINED,
        });
        let seqs: Vec<_> = list.iter().map(|e| e.join).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
        assert_eq!(seqs.len(), 4);
    }

    /// Whatever can change a list changes its stamp; only reading leaves it.
    #[test]
    fn a_stamp_holds_until_the_list_can_have_changed() {
        let mut list = JoinList::from(vec![1, 2, 3]);
        let stamp = list.stamp();
        let _ = (list.len(), list.iter().sum::<i32>(), list[0]);
        for _ in &list {}
        assert_eq!(list.stamp(), stamp, "reading is no write");

        let writes: [fn(&mut JoinList<i32>); 5] = [
            |l| l[0] = 9,
            |l| l.retain(|&v| v != 2),
            |l| l.iter_mut().for_each(|v| *v += 1),
            |l| {
                for v in &mut *l {
                    *v += 1;
                }
            },
            |l| l.clear(),
        ];
        let mut last = list.stamp();
        for write in writes {
            write(&mut list);
            assert_ne!(list.stamp(), last);
            last = list.stamp();
        }
    }

    /// A list that is not this one never passes for it, however many writes
    /// each has seen: a copy, a level swapped in, one taken out and put back.
    #[test]
    fn a_stamp_belongs_to_one_list() {
        let list = JoinList::from(vec![1]);
        let copy = list.clone();
        assert_ne!(copy.stamp(), list.stamp());
        assert_ne!(JoinList::<i32>::default().stamp(), list.stamp());

        let mut level = list;
        let stamp = level.stamp();
        let taken = std::mem::take(&mut level);
        assert_ne!(level.stamp(), stamp);
        let back = JoinList::from(Vec::from(taken));
        assert_ne!(back.stamp(), stamp);
    }
}
