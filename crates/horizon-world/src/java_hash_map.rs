//! Java HashMap traversal for non-null keys with pure hash/equality/order
//! operations. Tree bins preserve native linked iteration order.
//! This is the storage/iterator subset needed by simulation callback lifecycles.
// LEGAL-REWRITE: the implementation was removed here because it was translated from OpenJDK's
// `java/util/HashMap.java` (GPL-2.0), see #6741. Write it again from the behaviour description in
// the brief, the Javadoc and what a JVM returns. Keep the public API below as it is.
#![allow(dead_code, unreachable_code, unused_variables)]
use std::{cmp::Ordering, marker::PhantomData};

pub trait JavaKey: Eq {
    fn java_hash(&self) -> i32;
    /// Comparable comparison when both keys have the appropriate same Java
    /// class. None and Equal both require subtree searches for unequal keys.
    fn comparable_compare(&self, _other: &Self) -> Option<Ordering> {
        None
    }
    /// Native class-name ordering, then identity-hash ordering. Equal identity
    /// hashes break to Less, even for two different objects. Never return Equal.
    fn tie_break(&self, other: &Self) -> Ordering;
}

pub struct JavaHashMap<K, V> {
    todo: PhantomData<(K, V)>,
}

pub struct Cursor {
    todo: (),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorError {
    ConcurrentModification,
    NoCurrentEntry,
    WrongMap,
}

impl Cursor {
    pub fn has_next(&self) -> bool {
        todo!("JavaHashMap rewrite")
    }
}

impl<K: JavaKey, V> Default for JavaHashMap<K, V> {
    fn default() -> Self {
        todo!("JavaHashMap rewrite")
    }
}

impl<K: JavaKey, V> JavaHashMap<K, V> {
    pub fn len(&self) -> usize {
        todo!("JavaHashMap rewrite")
    }
    pub fn is_empty(&self) -> bool {
        todo!("JavaHashMap rewrite")
    }
    /// Native new HashMap(source), including sizing before entry insertion.
    /// Values are cloned according to their storage semantics (shared handles
    /// retain identity). This may change bucket order relative to the source.
    pub fn copied(&self) -> Self
    where
        K: Clone,
        V: Clone,
    {
        todo!("JavaHashMap rewrite")
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        todo!("JavaHashMap rewrite")
    }
    pub fn entries(&self) -> impl Iterator<Item = (&K, &V)> {
        let entries: std::iter::Empty<(&K, &V)> = todo!("JavaHashMap rewrite");
        entries
    }
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        todo!("JavaHashMap rewrite")
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        todo!("JavaHashMap rewrite")
    }
    pub fn clear(&mut self) {
        todo!("JavaHashMap rewrite")
    }
    pub fn cursor(&self) -> Cursor {
        todo!("JavaHashMap rewrite")
    }
    /// Call while has_next is true. The returned values reflect replacements made
    /// after iterator creation; structural changes are rejected before access.
    pub fn next_entry(&self, c: &mut Cursor) -> Result<Option<(&K, &V)>, CursorError> {
        todo!("JavaHashMap rewrite")
    }
    pub fn remove_current(&mut self, c: &mut Cursor) -> Result<(), CursorError> {
        todo!("JavaHashMap rewrite")
    }
}
