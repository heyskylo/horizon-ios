// Written for #6741 from a behaviour description, the Java SE API documentation and the iteration
// orders a real JVM produces; the red-black tree is the textbook one (CLRS, chapter 13).
//! Java HashMap traversal for non-null keys with pure hash/equality/order
//! operations. Tree bins preserve native linked iteration order.
//! This is the storage/iterator subset needed by simulation callback lifecycles.
//!
//! Layout: entries live in a slab of nodes addressed by `u32` index. A bucket holds the first
//! node of its bin's list (nodes chain through `next`, in iteration order) and, for a tree bin,
//! the root of a red-black tree over the same nodes. Tree links live in a side table that the
//! map only allocates with its first tree bin.
use std::{
    cmp::Ordering,
    mem,
    sync::atomic::{self, AtomicU64},
};

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

/// No node: the end of a list, an absent child or parent, an empty bucket.
const NIL: u32 = u32::MAX;
/// The table the first insertion allocates, unless `copied` sized it.
const DEFAULT_CAPACITY: usize = 16;
/// The largest table; it never grows past this.
const MAX_CAPACITY: usize = 1 << 30;
/// A plain bin that already holds this many nodes when a new key is appended becomes a tree
/// bin, once the table has `MIN_TREE_CAPACITY` buckets; a smaller table grows instead.
const TREEIFY_LEN: usize = 8;
const MIN_TREE_CAPACITY: usize = 64;
/// When a tree bin splits, a half with at most this many nodes becomes a plain bin.
const UNTREEIFY_LEN: usize = 6;

struct Node<K, V> {
    key: K,
    value: V,
    /// The spread hash: `h ^ (h >>> 16)` of the key's hash code.
    hash: i32,
    /// The next node of the bin's list.
    next: u32,
}

/// A tree bin node's red-black links.
#[derive(Clone, Copy)]
struct Links {
    parent: u32,
    left: u32,
    right: u32,
    red: bool,
}

impl Links {
    const UNUSED: Self = Self {
        parent: NIL,
        left: NIL,
        right: NIL,
        red: false,
    };
}

#[derive(Clone, Copy)]
struct Bucket {
    /// The first node of the bin's list.
    head: u32,
    /// The root of the bin's red-black tree; NIL for a plain bin.
    root: u32,
}

impl Bucket {
    const EMPTY: Self = Self {
        head: NIL,
        root: NIL,
    };
}

pub struct JavaHashMap<K, V> {
    /// Tells this map's cursors from other maps' cursors.
    id: u64,
    nodes: Vec<Option<Node<K, V>>>,
    /// Empty slots of `nodes`.
    free: Vec<u32>,
    /// Tree links by node index: empty until the map makes a tree bin, then as long as `nodes`.
    links: Vec<Links>,
    /// The table; empty until the first insertion.
    buckets: Vec<Bucket>,
    /// The capacity the first insertion allocates.
    initial_capacity: usize,
    /// The table grows when a new key takes the size past this.
    threshold: usize,
    len: usize,
    /// Structural modifications; a cursor fails once it differs from the one it recorded.
    mod_count: u64,
}

pub struct Cursor {
    map: u64,
    expected_mod_count: u64,
    next: u32,
    current: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorError {
    ConcurrentModification,
    NoCurrentEntry,
    WrongMap,
}

impl Cursor {
    pub fn has_next(&self) -> bool {
        self.next != NIL
    }
}

impl<K: JavaKey, V> Default for JavaHashMap<K, V> {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, atomic::Ordering::Relaxed),
            nodes: Vec::new(),
            free: Vec::new(),
            links: Vec::new(),
            buckets: Vec::new(),
            initial_capacity: DEFAULT_CAPACITY,
            threshold: 0,
            len: 0,
            mod_count: 0,
        }
    }
}

/// `h ^ (h >>> 16)`: the hash the map places a key by.
fn spread(hash: i32) -> i32 {
    hash ^ ((hash as u32) >> 16) as i32
}

/// Three quarters of the capacity; the largest table never grows.
fn threshold_for(capacity: usize) -> usize {
    if capacity >= MAX_CAPACITY {
        usize::MAX
    } else {
        capacity * 3 / 4
    }
}

impl<K: JavaKey, V> JavaHashMap<K, V> {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Native new HashMap(source), including sizing before entry insertion.
    /// Values are cloned according to their storage semantics (shared handles
    /// retain identity). This may change bucket order relative to the source.
    pub fn copied(&self) -> Self
    where
        K: Clone,
        V: Clone,
    {
        let mut copy = Self::default();
        if self.len > 0 {
            // The smallest power of two at least len / 0.75.
            let wanted = self.len.saturating_mul(4).div_ceil(3);
            copy.initial_capacity = wanted.min(MAX_CAPACITY).next_power_of_two();
            copy.nodes.reserve(self.len);
            for (key, value) in self.entries() {
                copy.put(key.clone(), value.clone());
            }
        }
        copy
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        let (_, i) = self.find(key)?;
        Some(&self.node(i).value)
    }
    pub fn entries(&self) -> impl Iterator<Item = (&K, &V)> {
        let mut next = self.first_node_from(0);
        std::iter::from_fn(move || {
            if next == NIL {
                return None;
            }
            let node = self.node(next);
            next = self.successor(next);
            Some((&node.key, &node.value))
        })
    }
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        let hash = spread(key.java_hash());
        if self.buckets.is_empty() {
            self.grow();
        }
        let b = self.bucket_index(hash);
        let Bucket { head, root } = self.buckets[b];
        if head == NIL {
            self.buckets[b].head = self.alloc(key, value, hash);
        } else if root != NIL {
            let found = self.tree_find(root, hash, &key);
            if found != NIL {
                return Some(mem::replace(&mut self.node_mut(found).value, value));
            }
            self.tree_put(b, key, value, hash);
        } else {
            let mut len = 0;
            let mut last = head;
            let mut i = head;
            while i != NIL {
                if self.is_key(i, hash, &key) {
                    return Some(mem::replace(&mut self.node_mut(i).value, value));
                }
                len += 1;
                last = i;
                i = self.node(i).next;
            }
            let new = self.alloc(key, value, hash);
            self.node_mut(last).next = new;
            if len >= TREEIFY_LEN {
                if self.buckets.len() < MIN_TREE_CAPACITY {
                    self.grow();
                } else {
                    self.treeify(b);
                }
            }
        }
        self.mod_count += 1;
        self.len += 1;
        if self.len > self.threshold {
            self.grow();
        }
        None
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (b, i) = self.find(key)?;
        Some(self.remove_node(b, i, true))
    }
    pub fn clear(&mut self) {
        self.mod_count += 1;
        if self.len > 0 {
            self.buckets.fill(Bucket::EMPTY);
            self.len = 0;
        }
        self.nodes.clear();
        self.free.clear();
        self.links.clear();
    }
    pub fn cursor(&self) -> Cursor {
        Cursor {
            map: self.id,
            expected_mod_count: self.mod_count,
            next: self.first_node_from(0),
            current: NIL,
        }
    }
    /// Call while has_next is true. The returned values reflect replacements made
    /// after iterator creation; structural changes are rejected before access.
    pub fn next_entry(&self, c: &mut Cursor) -> Result<Option<(&K, &V)>, CursorError> {
        if c.map != self.id {
            return Err(CursorError::WrongMap);
        }
        if c.expected_mod_count != self.mod_count {
            return Err(CursorError::ConcurrentModification);
        }
        if c.next == NIL {
            return Ok(None);
        }
        let i = c.next;
        c.current = i;
        c.next = self.successor(i);
        let node = self.node(i);
        Ok(Some((&node.key, &node.value)))
    }
    pub fn remove_current(&mut self, c: &mut Cursor) -> Result<(), CursorError> {
        if c.map != self.id {
            return Err(CursorError::WrongMap);
        }
        if c.current == NIL {
            return Err(CursorError::NoCurrentEntry);
        }
        if c.expected_mod_count != self.mod_count {
            return Err(CursorError::ConcurrentModification);
        }
        let i = mem::replace(&mut c.current, NIL);
        let b = self.bucket_index(self.node(i).hash);
        self.remove_node(b, i, false);
        c.expected_mod_count = self.mod_count;
        Ok(())
    }

    fn node(&self, i: u32) -> &Node<K, V> {
        self.nodes[i as usize].as_ref().expect("live node")
    }
    fn node_mut(&mut self, i: u32) -> &mut Node<K, V> {
        self.nodes[i as usize].as_mut().expect("live node")
    }
    fn alloc(&mut self, key: K, value: V, hash: i32) -> u32 {
        let node = Some(Node {
            key,
            value,
            hash,
            next: NIL,
        });
        if let Some(i) = self.free.pop() {
            self.nodes[i as usize] = node;
            return i;
        }
        self.nodes.push(node);
        if !self.links.is_empty() {
            self.links.push(Links::UNUSED);
        }
        (self.nodes.len() - 1) as u32
    }
    fn bucket_index(&self, hash: i32) -> usize {
        hash as u32 as usize & (self.buckets.len() - 1)
    }
    fn is_key(&self, i: u32, hash: i32, key: &K) -> bool {
        let node = self.node(i);
        node.hash == hash && node.key == *key
    }
    /// The bucket and node holding the key equal to `key`.
    fn find(&self, key: &K) -> Option<(usize, u32)> {
        if self.buckets.is_empty() {
            return None;
        }
        let hash = spread(key.java_hash());
        let b = self.bucket_index(hash);
        let Bucket { head, root } = self.buckets[b];
        let found = if root != NIL {
            self.tree_find(root, hash, key)
        } else {
            let mut i = head;
            while i != NIL && !self.is_key(i, hash, key) {
                i = self.node(i).next;
            }
            i
        };
        (found != NIL).then_some((b, found))
    }
    /// The node of the key equal to `key` in the subtree at `p`. Trees are ordered by hash
    /// first, so a different hash rules out one side; nodes with the same hash can be on both.
    fn tree_find(&self, mut p: u32, hash: i32, key: &K) -> u32 {
        while p != NIL {
            let node = self.node(p);
            let links = self.links[p as usize];
            p = match node.hash.cmp(&hash) {
                Ordering::Greater => links.left,
                Ordering::Less => links.right,
                Ordering::Equal if node.key == *key => return p,
                Ordering::Equal => {
                    let found = self.tree_find(links.right, hash, key);
                    if found != NIL {
                        return found;
                    }
                    links.left
                }
            };
        }
        NIL
    }
    /// Whether `key`, with spread hash `hash`, goes left of tree node `p`: by hash, then by
    /// `comparable_compare`, then by `tie_break` when that is absent or equal.
    fn goes_left(&self, key: &K, hash: i32, p: u32) -> bool {
        let node = self.node(p);
        match node.hash.cmp(&hash) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => match key.comparable_compare(&node.key) {
                Some(Ordering::Less) => true,
                Some(Ordering::Greater) => false,
                _ => key.tie_break(&node.key) != Ordering::Greater,
            },
        }
    }
    /// RB-INSERT of node `i` into the tree at `root` (NIL for an empty tree), descending by
    /// `goes_left`, then RB-INSERT-FIXUP. Returns the new root and the node `i` hangs from.
    fn attach(&mut self, root: u32, i: u32) -> (u32, u32) {
        let mut parent = NIL;
        let mut to_left = false;
        let node = self.node(i);
        let mut p = root;
        while p != NIL {
            parent = p;
            to_left = self.goes_left(&node.key, node.hash, p);
            let links = self.links[p as usize];
            p = if to_left { links.left } else { links.right };
        }
        self.links[i as usize] = Links {
            parent,
            red: true,
            ..Links::UNUSED
        };
        let mut tree = Tree {
            links: &mut self.links,
            root,
            nil_parent: NIL,
        };
        if parent == NIL {
            tree.root = i;
        } else if to_left {
            tree.at(parent).left = i;
        } else {
            tree.at(parent).right = i;
        }
        tree.insert_fixup(i);
        (tree.root, parent)
    }
    /// Turns bucket `b`'s plain bin into a tree bin: its nodes go into a new tree one by one in
    /// list order, then the root moves to the front. The list order is otherwise unchanged.
    fn treeify(&mut self, b: usize) {
        if self.links.len() < self.nodes.len() {
            self.links.resize(self.nodes.len(), Links::UNUSED);
        }
        let mut root = NIL;
        let mut i = self.buckets[b].head;
        while i != NIL {
            root = self.attach(root, i).0;
            i = self.node(i).next;
        }
        self.buckets[b].root = root;
        self.move_root_to_front(b);
    }
    /// A new key into the tree bin of bucket `b`: it joins the tree, and the list right after
    /// its tree parent.
    fn tree_put(&mut self, b: usize, key: K, value: V, hash: i32) {
        let new = self.alloc(key, value, hash);
        let (root, parent) = self.attach(self.buckets[b].root, new);
        self.buckets[b].root = root;
        let after = self.node(parent).next;
        self.node_mut(new).next = after;
        self.node_mut(parent).next = new;
        self.move_root_to_front(b);
    }
    /// Puts the tree's root first in the bin's list, the other nodes keeping their order.
    fn move_root_to_front(&mut self, b: usize) {
        let Bucket { head, root } = self.buckets[b];
        if root != head {
            self.unlink(b, root);
            self.node_mut(root).next = head;
            self.buckets[b].head = root;
        }
    }
    /// Takes node `i` out of bucket `b`'s list; the others keep their order.
    fn unlink(&mut self, b: usize, i: u32) {
        let next = self.node(i).next;
        let head = self.buckets[b].head;
        if head == i {
            self.buckets[b].head = next;
            return;
        }
        let mut p = head;
        while self.node(p).next != i {
            p = self.node(p).next;
        }
        self.node_mut(p).next = next;
    }
    /// Removes node `i` from bucket `b`. In a tree bin, a movable removal (`remove`) turns a
    /// small tree back into a plain bin, judged on the tree before the removal, and otherwise
    /// moves the new root to the front; a cursor's removal does neither.
    fn remove_node(&mut self, b: usize, i: u32, movable: bool) -> V {
        let root = self.buckets[b].root;
        self.unlink(b, i);
        if self.buckets[b].head == NIL {
            self.buckets[b].root = NIL;
        } else if root != NIL {
            let small = movable && {
                let top = self.links[root as usize];
                top.right == NIL || top.left == NIL || self.links[top.left as usize].left == NIL
            };
            if small {
                self.buckets[b].root = NIL;
            } else {
                let mut tree = Tree {
                    links: &mut self.links,
                    root,
                    nil_parent: NIL,
                };
                tree.delete(i);
                self.buckets[b].root = tree.root;
                if movable {
                    self.move_root_to_front(b);
                }
            }
        }
        self.mod_count += 1;
        self.len -= 1;
        let node = self.nodes[i as usize].take().expect("live node");
        self.free.push(i);
        node.value
    }
    /// The node after `i` in iteration order.
    fn successor(&self, i: u32) -> u32 {
        let node = self.node(i);
        if node.next != NIL {
            node.next
        } else {
            self.first_node_from(self.bucket_index(node.hash) + 1)
        }
    }
    /// The first node of the first non-empty bucket from `b` on.
    fn first_node_from(&self, b: usize) -> u32 {
        self.buckets[b..]
            .iter()
            .find(|bucket| bucket.head != NIL)
            .map_or(NIL, |bucket| bucket.head)
    }
    /// Allocates the first table, or doubles it. Old bucket `j` splits into `j` and `j + old`
    /// by the hash bit `old`, each half in its old order; a single node keeps its bin kind.
    fn grow(&mut self) {
        let old = self.buckets.len();
        if old == 0 {
            self.buckets = vec![Bucket::EMPTY; self.initial_capacity];
            self.threshold = threshold_for(self.initial_capacity);
            return;
        }
        if old >= MAX_CAPACITY {
            self.threshold = usize::MAX;
            return;
        }
        let old_buckets = mem::replace(&mut self.buckets, vec![Bucket::EMPTY; old * 2]);
        for (j, bucket) in old_buckets.into_iter().enumerate() {
            if bucket.head == NIL {
                continue;
            }
            let first = self.node(bucket.head);
            if first.next == NIL {
                let to = self.bucket_index(first.hash);
                self.buckets[to] = bucket;
                continue;
            }
            let halves = self.split_list(bucket.head, old);
            for (half, &(head, len)) in halves.iter().enumerate() {
                if head == NIL {
                    continue;
                }
                let to = j + half * old;
                if bucket.root == NIL || len <= UNTREEIFY_LEN {
                    self.buckets[to] = Bucket { head, root: NIL };
                } else if halves[1 - half].0 != NIL {
                    self.buckets[to].head = head;
                    self.treeify(to);
                } else {
                    // Every node landed here: same tree, same list, same first node.
                    self.buckets[to] = bucket;
                }
            }
        }
        self.threshold = threshold_for(old * 2);
    }
    /// Splits the list at `head` into the nodes whose hash has `bit` clear and those with it
    /// set, each in its old order. Returns each half's first node and length.
    fn split_list(&mut self, head: u32, bit: usize) -> [(u32, usize); 2] {
        let mut halves = [(NIL, 0); 2];
        let mut tails = [NIL; 2];
        let mut i = head;
        while i != NIL {
            let node = self.node(i);
            let next = node.next;
            let half = usize::from(node.hash as u32 as usize & bit != 0);
            if tails[half] == NIL {
                halves[half].0 = i;
            } else {
                self.node_mut(tails[half]).next = i;
            }
            tails[half] = i;
            halves[half].1 += 1;
            i = next;
        }
        for tail in tails {
            if tail != NIL {
                self.node_mut(tail).next = NIL;
            }
        }
        halves
    }
}

/// One tree bin's red-black tree, as in CLRS chapter 13. NIL plays the sentinel `T.nil`: it
/// reads black, and `nil_parent` is the `T.nil.p` that RB-DELETE sets for RB-DELETE-FIXUP.
struct Tree<'a> {
    links: &'a mut [Links],
    root: u32,
    nil_parent: u32,
}

impl Tree<'_> {
    fn at(&mut self, x: u32) -> &mut Links {
        &mut self.links[x as usize]
    }
    fn left(&self, x: u32) -> u32 {
        self.links[x as usize].left
    }
    fn right(&self, x: u32) -> u32 {
        self.links[x as usize].right
    }
    fn parent(&self, x: u32) -> u32 {
        if x == NIL {
            self.nil_parent
        } else {
            self.links[x as usize].parent
        }
    }
    fn set_parent(&mut self, x: u32, parent: u32) {
        if x == NIL {
            self.nil_parent = parent;
        } else {
            self.at(x).parent = parent;
        }
    }
    fn is_red(&self, x: u32) -> bool {
        x != NIL && self.links[x as usize].red
    }
    fn set_red(&mut self, x: u32, red: bool) {
        if x != NIL {
            self.at(x).red = red;
        }
    }
    /// Points `parent`'s link to its child `old` (the root's link when `parent` is NIL) at `new`.
    fn replace_child(&mut self, parent: u32, old: u32, new: u32) {
        if parent == NIL {
            self.root = new;
        } else if self.left(parent) == old {
            self.at(parent).left = new;
        } else {
            self.at(parent).right = new;
        }
    }
    /// LEFT-ROTATE (CLRS 13.2).
    fn rotate_left(&mut self, x: u32) {
        let y = self.right(x);
        let beta = self.left(y);
        self.at(x).right = beta;
        if beta != NIL {
            self.at(beta).parent = x;
        }
        let parent = self.parent(x);
        self.at(y).parent = parent;
        self.replace_child(parent, x, y);
        self.at(y).left = x;
        self.at(x).parent = y;
    }
    /// RIGHT-ROTATE: LEFT-ROTATE with left and right exchanged.
    fn rotate_right(&mut self, x: u32) {
        let y = self.left(x);
        let beta = self.right(y);
        self.at(x).left = beta;
        if beta != NIL {
            self.at(beta).parent = x;
        }
        let parent = self.parent(x);
        self.at(y).parent = parent;
        self.replace_child(parent, x, y);
        self.at(y).right = x;
        self.at(x).parent = y;
    }
    /// RB-INSERT-FIXUP (CLRS 13.3) for the red node `z` just attached.
    fn insert_fixup(&mut self, mut z: u32) {
        while self.is_red(self.parent(z)) {
            let zp = self.parent(z);
            let zpp = self.parent(zp);
            if zp == self.left(zpp) {
                let y = self.right(zpp);
                if self.is_red(y) {
                    self.set_red(zp, false);
                    self.set_red(y, false);
                    self.set_red(zpp, true);
                    z = zpp;
                } else {
                    if z == self.right(zp) {
                        z = zp;
                        self.rotate_left(z);
                    }
                    let zp = self.parent(z);
                    let zpp = self.parent(zp);
                    self.set_red(zp, false);
                    self.set_red(zpp, true);
                    self.rotate_right(zpp);
                }
            } else {
                let y = self.left(zpp);
                if self.is_red(y) {
                    self.set_red(zp, false);
                    self.set_red(y, false);
                    self.set_red(zpp, true);
                    z = zpp;
                } else {
                    if z == self.left(zp) {
                        z = zp;
                        self.rotate_right(z);
                    }
                    let zp = self.parent(z);
                    let zpp = self.parent(zp);
                    self.set_red(zp, false);
                    self.set_red(zpp, true);
                    self.rotate_left(zpp);
                }
            }
        }
        let root = self.root;
        self.set_red(root, false);
    }
    /// RB-TRANSPLANT (CLRS 13.4): `v` takes `u`'s place under `u`'s parent.
    fn transplant(&mut self, u: u32, v: u32) {
        let parent = self.parent(u);
        self.replace_child(parent, u, v);
        self.set_parent(v, parent);
    }
    /// RB-DELETE (CLRS 13.4, third edition). A node with two children is replaced by its
    /// successor, which takes its color.
    fn delete(&mut self, z: u32) {
        let mut removed_red = self.is_red(z);
        let x = if self.left(z) == NIL {
            let x = self.right(z);
            self.transplant(z, x);
            x
        } else if self.right(z) == NIL {
            let x = self.left(z);
            self.transplant(z, x);
            x
        } else {
            let mut y = self.right(z);
            while self.left(y) != NIL {
                y = self.left(y);
            }
            removed_red = self.is_red(y);
            let x = self.right(y);
            if self.parent(y) == z {
                self.set_parent(x, y);
            } else {
                self.transplant(y, x);
                let right = self.right(z);
                self.at(y).right = right;
                self.at(right).parent = y;
            }
            self.transplant(z, y);
            let left = self.left(z);
            self.at(y).left = left;
            self.at(left).parent = y;
            let red = self.is_red(z);
            self.at(y).red = red;
            x
        };
        if !removed_red {
            self.delete_fixup(x);
        }
    }
    /// RB-DELETE-FIXUP (CLRS 13.4).
    fn delete_fixup(&mut self, mut x: u32) {
        while x != self.root && !self.is_red(x) {
            let parent = self.parent(x);
            if x == self.left(parent) {
                let mut w = self.right(parent);
                if self.is_red(w) {
                    self.set_red(w, false);
                    self.set_red(parent, true);
                    self.rotate_left(parent);
                    w = self.right(self.parent(x));
                }
                if !self.is_red(self.left(w)) && !self.is_red(self.right(w)) {
                    self.set_red(w, true);
                    x = self.parent(x);
                } else {
                    if !self.is_red(self.right(w)) {
                        let inner = self.left(w);
                        self.set_red(inner, false);
                        self.set_red(w, true);
                        self.rotate_right(w);
                        w = self.right(self.parent(x));
                    }
                    let parent = self.parent(x);
                    let red = self.is_red(parent);
                    self.set_red(w, red);
                    self.set_red(parent, false);
                    let outer = self.right(w);
                    self.set_red(outer, false);
                    self.rotate_left(parent);
                    x = self.root;
                }
            } else {
                let mut w = self.left(parent);
                if self.is_red(w) {
                    self.set_red(w, false);
                    self.set_red(parent, true);
                    self.rotate_right(parent);
                    w = self.left(self.parent(x));
                }
                if !self.is_red(self.right(w)) && !self.is_red(self.left(w)) {
                    self.set_red(w, true);
                    x = self.parent(x);
                } else {
                    if !self.is_red(self.left(w)) {
                        let inner = self.right(w);
                        self.set_red(inner, false);
                        self.set_red(w, true);
                        self.rotate_left(w);
                        w = self.left(self.parent(x));
                    }
                    let parent = self.parent(x);
                    let red = self.is_red(parent);
                    self.set_red(w, red);
                    self.set_red(parent, false);
                    let outer = self.left(w);
                    self.set_red(outer, false);
                    self.rotate_right(parent);
                    x = self.root;
                }
            }
        }
        self.set_red(x, false);
    }
}

#[cfg(test)]
mod tests {
    // Every expected order below was printed by java.util.HashMap on OpenJDK 25.0.4.1, from a
    // small Java program kept outside the repository. Its keys are the java_hash_map harness's:
    // `record Key(int id, int hash) implements Comparable<Key>`, whose hashCode() returns `hash`
    // and whose compareTo orders by `id`; each value is its key's id unless a test says otherwise.
    // Orders list the ids of `map.entrySet()` in iteration order. Each test names the steps.
    use super::*;

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Key {
        id: u32,
        hash: i32,
    }
    impl JavaKey for Key {
        fn java_hash(&self) -> i32 {
            self.hash
        }
        fn comparable_compare(&self, other: &Self) -> Option<Ordering> {
            Some(self.id.cmp(&other.id))
        }
        fn tie_break(&self, other: &Self) -> Ordering {
            self.id.cmp(&other.id)
        }
    }
    fn key(id: u32, hash: i32) -> Key {
        Key { id, hash }
    }
    /// Ids 3 and up share bucket 0 of tables up to 64 buckets: hash (id - 3) * 64.
    fn collider(id: u32) -> Key {
        key(id, (id as i32 - 3) * 64)
    }
    fn order(map: &JavaHashMap<Key, u32>) -> Vec<u32> {
        map.entries().map(|(k, _)| k.id).collect()
    }
    fn next(
        map: &JavaHashMap<Key, u32>,
        cursor: &mut Cursor,
    ) -> Result<Option<(u32, u32)>, CursorError> {
        map.next_entry(cursor)
            .map(|entry| entry.map(|(k, v)| (k.id, *v)))
    }
    /// Java: `new HashMap<>()`, put ids 0, 1, 2 with hashes 17, 2, 33, then ids 3..=13 (colliders).
    fn colliders() -> JavaHashMap<Key, u32> {
        let mut map = JavaHashMap::default();
        for (id, hash) in [(0, 17), (1, 2), (2, 33)] {
            map.put(key(id, hash), id);
        }
        for id in 3..=13 {
            map.put(collider(id), id);
        }
        map
    }

    /// Java: ids 0..=12 with hashes 1, 17, 33, 49, 0, 16, 2, 18, 34, 3, 35, 4, 20, put into
    /// `new HashMap<>()` in the order 5, 1, 4, 0, 7, 3, 6, 2, 12, 8, 10, 9, 11.
    #[test]
    fn thirteenth_key_doubles_the_table_and_halves_keep_their_order() {
        const HASHES: [i32; 13] = [1, 17, 33, 49, 0, 16, 2, 18, 34, 3, 35, 4, 20];
        let mut map = JavaHashMap::default();
        for id in [5, 1, 4, 0, 7, 3, 6, 2, 12, 8, 10, 9] {
            map.put(key(id, HASHES[id as usize]), id);
        }
        assert_eq!(order(&map), [5, 4, 1, 0, 3, 2, 7, 6, 8, 10, 9, 12]);
        map.put(key(11, 4), 11);
        assert_eq!(order(&map), [4, 0, 2, 6, 8, 10, 9, 11, 5, 1, 3, 7, 12]);
    }

    /// Java: the steps of `colliders`, printing the order after each put of ids 10..=13.
    #[test]
    fn crowded_bin_grows_a_small_table_and_becomes_a_tree_at_64() {
        let mut map = JavaHashMap::default();
        for (id, hash) in [(0, 17), (1, 2), (2, 33)] {
            map.put(key(id, hash), id);
        }
        for id in 3..=10 {
            map.put(collider(id), id);
        }
        // 16 buckets: the eight colliders of bucket 0, then 0 and 2 (bucket 1), then 1.
        assert_eq!(order(&map), [3, 4, 5, 6, 7, 8, 9, 10, 0, 2, 1]);
        // The ninth collider doubles the table instead of making a tree: 0 moves to bucket 17.
        map.put(collider(11), 11);
        assert_eq!(order(&map), [3, 4, 5, 6, 7, 8, 9, 10, 11, 2, 1, 0]);
        // The tenth doubles it again: 2 moves to bucket 33.
        map.put(collider(12), 12);
        assert_eq!(order(&map), [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 1, 0, 2]);
        // The eleventh makes a tree bin; its root, 6, moves to the front of the list.
        map.put(collider(13), 13);
        assert_eq!(order(&map), [6, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 1, 0, 2]);
    }

    /// Java: `colliders`, then put id 14 with hash 320, the hash of id 8.
    #[test]
    fn new_key_in_a_tree_bin_follows_its_tree_parent() {
        let mut map = colliders();
        map.put(key(14, 320), 14);
        assert_eq!(
            order(&map),
            [6, 3, 4, 5, 7, 8, 9, 14, 10, 11, 12, 13, 1, 0, 2]
        );
    }

    /// Java: `colliders`; remove id 6; put id 15 with hash 704; remove ids 3, 4, 5, 7, 8; put id
    /// 16 with hash 384, the hash of id 9.
    #[test]
    fn map_removal_moves_the_new_root_first_then_untreeifies_a_small_tree() {
        let mut map = colliders();
        // The removed root's successor, 7, becomes the root and moves to the front.
        assert_eq!(map.remove(&collider(6)), Some(6));
        assert_eq!(order(&map), [7, 3, 4, 5, 8, 9, 10, 11, 12, 13, 1, 0, 2]);
        map.put(key(15, 704), 15);
        assert_eq!(order(&map), [7, 3, 4, 5, 8, 9, 10, 11, 12, 13, 15, 1, 0, 2]);
        let expected: [(u32, &[u32]); 5] = [
            (3, &[10, 7, 4, 5, 8, 9, 11, 12, 13, 15, 1, 0, 2]),
            (4, &[10, 7, 5, 8, 9, 11, 12, 13, 15, 1, 0, 2]),
            (5, &[10, 7, 8, 9, 11, 12, 13, 15, 1, 0, 2]),
            (7, &[10, 8, 9, 11, 12, 13, 15, 1, 0, 2]),
            (8, &[10, 9, 11, 12, 13, 15, 1, 0, 2]),
        ];
        for (id, after) in expected {
            assert_eq!(map.remove(&collider(id)), Some(id));
            assert_eq!(order(&map), after, "after removing {id}");
        }
        // Removing 8 left a plain bin: the new key goes last, not after 9 as in a tree.
        map.put(key(16, 384), 16);
        assert_eq!(order(&map), [10, 9, 11, 12, 13, 15, 16, 1, 0, 2]);
    }

    /// Java: `colliders`; its entry set's iterator: next() (id 6) and remove(); put id 15 with
    /// hash 704. Then a new `colliders` whose iterator removes the first eight entries; put id 18
    /// with hash 512, the hash of id 11.
    #[test]
    fn cursor_removal_keeps_the_list_order_and_the_tree() {
        let mut map = colliders();
        let mut cursor = map.cursor();
        assert_eq!(next(&map, &mut cursor), Ok(Some((6, 6))));
        assert_eq!(map.remove_current(&mut cursor), Ok(()));
        assert_eq!(order(&map), [3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 1, 0, 2]);
        // The next tree insertion moves the root left by the removal, 7, to the front.
        map.put(key(15, 704), 15);
        assert_eq!(order(&map), [7, 3, 4, 5, 8, 9, 10, 11, 12, 13, 15, 1, 0, 2]);

        let mut map = colliders();
        let mut cursor = map.cursor();
        for _ in 0..8 {
            assert!(matches!(next(&map, &mut cursor), Ok(Some(_))));
            assert_eq!(map.remove_current(&mut cursor), Ok(()));
        }
        assert_eq!(order(&map), [11, 12, 13, 1, 0, 2]);
        // Three nodes, still a tree bin: 18 goes after its tree parent 11, then root 12 moves first.
        map.put(key(18, 512), 18);
        assert_eq!(order(&map), [12, 11, 18, 13, 1, 0, 2]);
    }

    /// Java: the 13 puts of the doubling test, remove id 9, `new HashMap<>(map)`, then put id 9
    /// (hash 3) into the copy. And a map holding id 0 (hash 1), copied, then given ids 1, 2, 3
    /// with hashes 3, 5, 7.
    #[test]
    fn copy_is_sized_for_its_entries_and_refilled_in_source_order() {
        const HASHES: [i32; 13] = [1, 17, 33, 49, 0, 16, 2, 18, 34, 3, 35, 4, 20];
        let mut map = JavaHashMap::default();
        for id in [5, 1, 4, 0, 7, 3, 6, 2, 12, 8, 10, 9, 11] {
            map.put(key(id, HASHES[id as usize]), id);
        }
        // 13 entries need 32 buckets, as the source has: the copy keeps its order.
        assert_eq!(order(&map.copied()), order(&map));
        map.remove(&key(9, 3));
        assert_eq!(order(&map), [4, 0, 2, 6, 8, 10, 11, 5, 1, 3, 7, 12]);
        // 12 entries fit 16 buckets, where the source has 32.
        let mut copy = map.copied();
        assert_eq!(order(&copy), [4, 5, 0, 2, 1, 3, 6, 8, 7, 10, 11, 12]);
        copy.put(key(9, 3), 9);
        assert_eq!(order(&copy), [4, 0, 2, 6, 8, 10, 9, 11, 5, 1, 3, 7, 12]);

        let mut one = JavaHashMap::default();
        one.put(key(0, 1), 0);
        // Two buckets (threshold 1), then four (threshold 3), then eight.
        let mut copy = one.copied();
        let expected: [&[u32]; 3] = [&[0, 1], &[0, 2, 1], &[0, 1, 2, 3]];
        for (id, after) in (1..=3).zip(expected) {
            copy.put(key(id, id as i32 * 2 + 1), id);
            assert_eq!(order(&copy), after, "after putting {id}");
        }
        assert_eq!(JavaHashMap::<Key, u32>::default().copied().len(), 0);
    }

    /// Java: put ids 0 and 1 (hashes 1, 2); iterator A: remove(); put id 2 (hash 3); A: remove(),
    /// next(), hasNext(). Iterator B: next(); put id 1 with value 10; remove a missing key; next(),
    /// remove(), remove(), next(), hasNext(), next(), remove(). Iterator C: next(); put id 3
    /// (hash 4); remove(). An empty map's iterator: hasNext(), clear(), next(), remove().
    #[test]
    fn cursor_errors_come_in_java_order() {
        use CursorError::{ConcurrentModification, NoCurrentEntry, WrongMap};
        let mut map = JavaHashMap::default();
        map.put(key(0, 1), 0);
        map.put(key(1, 2), 1);
        let mut a = map.cursor();
        assert_eq!(map.remove_current(&mut a), Err(NoCurrentEntry));
        map.put(key(2, 3), 2);
        assert_eq!(map.remove_current(&mut a), Err(NoCurrentEntry));
        assert_eq!(next(&map, &mut a), Err(ConcurrentModification));
        assert!(a.has_next());

        let mut b = map.cursor();
        assert_eq!(next(&map, &mut b), Ok(Some((0, 0))));
        assert_eq!(map.put(key(1, 2), 10), Some(1));
        assert_eq!(map.remove(&key(9, 9)), None);
        assert_eq!(next(&map, &mut b), Ok(Some((1, 10))));
        assert_eq!(map.remove_current(&mut b), Ok(()));
        assert_eq!(map.remove_current(&mut b), Err(NoCurrentEntry));
        assert_eq!(next(&map, &mut b), Ok(Some((2, 2))));
        assert!(!b.has_next());
        assert_eq!(next(&map, &mut b), Ok(None));
        assert_eq!(map.remove_current(&mut b), Ok(()));
        assert_eq!(order(&map), [0]);

        let mut c = map.cursor();
        assert_eq!(next(&map, &mut c), Ok(Some((0, 0))));
        map.put(key(3, 4), 3);
        assert_eq!(map.remove_current(&mut c), Err(ConcurrentModification));

        let mut empty = JavaHashMap::default();
        let mut d = empty.cursor();
        assert!(!d.has_next());
        empty.clear();
        assert_eq!(next(&empty, &mut d), Err(ConcurrentModification));
        assert_eq!(empty.remove_current(&mut d), Err(NoCurrentEntry));
        // Not a Java case: a cursor handed to another map.
        assert_eq!(next(&map, &mut d), Err(WrongMap));
        assert_eq!(map.remove_current(&mut d), Err(WrongMap));
    }
}
