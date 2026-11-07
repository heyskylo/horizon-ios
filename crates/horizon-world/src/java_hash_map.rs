//! Java HashMap traversal for non-null keys with pure hash/equality/order
//! operations. Tree bins preserve native linked iteration order.
//! This is the storage/iterator subset needed by simulation callback lifecycles.
use std::{cmp::Ordering, sync::Arc};
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
type Link = Option<usize>;
struct Node<K, V> {
    key: K,
    value: V,
    hash: i32,
    next: Link,
    prev: Link,
    parent: Link,
    left: Link,
    right: Link,
    red: bool,
    tree: bool,
}
pub struct JavaHashMap<K, V> {
    table: Vec<Link>,
    nodes: Vec<Option<Node<K, V>>>,
    free: Vec<usize>,
    size: usize,
    threshold: usize,
    modification: u32,
    owner: Arc<()>,
}
pub struct Cursor {
    next: Link,
    current: Link,
    bucket: usize,
    expected: u32,
    owner: Arc<()>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorError {
    ConcurrentModification,
    NoCurrentEntry,
    WrongMap,
}
impl Cursor {
    pub fn has_next(&self) -> bool {
        self.next.is_some()
    }
}
impl<K: JavaKey, V> Default for JavaHashMap<K, V> {
    fn default() -> Self {
        Self {
            table: Vec::new(),
            nodes: Vec::new(),
            free: Vec::new(),
            size: 0,
            threshold: 0,
            modification: 0,
            owner: Arc::new(()),
        }
    }
}
impl<K: JavaKey, V> JavaHashMap<K, V> {
    fn node(&self, i: usize) -> &Node<K, V> {
        self.nodes[i].as_ref().unwrap()
    }
    fn node_mut(&mut self, i: usize) -> &mut Node<K, V> {
        self.nodes[i].as_mut().unwrap()
    }
    fn parent(&self, i: Link) -> Link {
        i.and_then(|i| self.node(i).parent)
    }
    fn left(&self, i: Link) -> Link {
        i.and_then(|i| self.node(i).left)
    }
    fn right(&self, i: Link) -> Link {
        i.and_then(|i| self.node(i).right)
    }
    fn red(&self, i: Link) -> bool {
        i.is_some_and(|i| self.node(i).red)
    }
    fn color(&mut self, i: Link, red: bool) {
        if let Some(i) = i {
            self.node_mut(i).red = red;
        }
    }
    fn hash(key: &K) -> i32 {
        let h = key.java_hash() as u32;
        (h ^ (h >> 16)) as i32
    }
    fn allocate(&mut self, key: K, value: V, hash: i32) -> usize {
        let node = Some(Node {
            key,
            value,
            hash,
            next: None,
            prev: None,
            parent: None,
            left: None,
            right: None,
            red: false,
            tree: false,
        });
        if let Some(i) = self.free.pop() {
            self.nodes[i] = node;
            i
        } else {
            self.nodes.push(node);
            self.nodes.len() - 1
        }
    }
    pub fn len(&self) -> usize {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    fn find(&self, key: &K) -> Link {
        if self.table.is_empty() {
            return None;
        }
        let h = Self::hash(key);
        let mut p = self.table[h as usize & (self.table.len() - 1)];
        if let Some(first) = p.filter(|i| self.node(*i).tree) {
            return self.find_tree(Some(self.root(first)), h, key);
        }
        while let Some(i) = p {
            let n = self.node(i);
            if n.hash == h && n.key == *key {
                return Some(i);
            }
            p = n.next;
        }
        None
    }
    fn find_tree(&self, mut p: Link, hash: i32, key: &K) -> Link {
        while let Some(i) = p {
            let node = self.node(i);
            if hash < node.hash {
                p = node.left;
            } else if hash > node.hash {
                p = node.right;
            } else if key == &node.key {
                return Some(i);
            } else if node.left.is_none() {
                p = node.right;
            } else if node.right.is_none() {
                p = node.left;
            } else {
                match key.comparable_compare(&node.key) {
                    Some(Ordering::Less) => p = node.left,
                    Some(Ordering::Greater) => p = node.right,
                    _ => {
                        if let Some(found) = self.find_tree(node.right, hash, key) {
                            return Some(found);
                        }
                        p = node.left;
                    }
                }
            }
        }
        None
    }
    /// Native new HashMap(source), including sizing before entry insertion.
    /// Values are cloned according to their storage semantics (shared handles
    /// retain identity). This may change bucket order relative to the source.
    pub fn copied(&self) -> Self
    where
        K: Clone,
        V: Clone,
    {
        let mut result = Self::default();
        if self.size > 0 {
            let capacity = ((self.size as f64 / 0.75).ceil() as usize).min(1 << 30);
            result.threshold = capacity.next_power_of_two();
            for (key, value) in self.entries() {
                result.put(key.clone(), value.clone());
            }
        }
        result
    }
    pub fn get(&self, key: &K) -> Option<&V> {
        self.find(key).map(|i| &self.node(i).value)
    }
    pub fn entries(&self) -> impl Iterator<Item = (&K, &V)> {
        self.table
            .iter()
            .flat_map(|first| std::iter::successors(*first, |i| self.node(*i).next))
            .map(|i| {
                let n = self.node(i);
                (&n.key, &n.value)
            })
    }
    fn root(&self, mut p: usize) -> usize {
        while let Some(parent) = self.node(p).parent {
            p = parent;
        }
        p
    }
    fn compare(&self, a: usize, b: usize) -> Ordering {
        self.node(a).hash.cmp(&self.node(b).hash).then_with(|| {
            let a = &self.node(a).key;
            let b = &self.node(b).key;
            a.comparable_compare(b)
                .filter(|v| *v != Ordering::Equal)
                .unwrap_or_else(|| a.tie_break(b))
        })
    }
    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        let Some(r) = self.node(p).right else {
            return root;
        };
        let rl = self.node(r).left;
        self.node_mut(p).right = rl;
        if let Some(rl) = rl {
            self.node_mut(rl).parent = Some(p);
        }
        let pp = self.node(p).parent;
        self.node_mut(r).parent = pp;
        if let Some(pp) = pp {
            if self.node(pp).left == Some(p) {
                self.node_mut(pp).left = Some(r);
            } else {
                self.node_mut(pp).right = Some(r);
            }
        } else {
            root = r;
            self.node_mut(r).red = false;
        }
        self.node_mut(r).left = Some(p);
        self.node_mut(p).parent = Some(r);
        root
    }
    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        let Some(l) = self.node(p).left else {
            return root;
        };
        let lr = self.node(l).right;
        self.node_mut(p).left = lr;
        if let Some(lr) = lr {
            self.node_mut(lr).parent = Some(p);
        }
        let pp = self.node(p).parent;
        self.node_mut(l).parent = pp;
        if let Some(pp) = pp {
            if self.node(pp).right == Some(p) {
                self.node_mut(pp).right = Some(l);
            } else {
                self.node_mut(pp).left = Some(l);
            }
        } else {
            root = l;
            self.node_mut(l).red = false;
        }
        self.node_mut(l).right = Some(p);
        self.node_mut(p).parent = Some(l);
        root
    }
    fn balance_insert(&mut self, mut root: usize, mut x: usize) -> usize {
        self.node_mut(x).red = true;
        loop {
            let Some(mut p) = self.node(x).parent else {
                self.node_mut(x).red = false;
                return x;
            };
            let Some(mut g) = self.node(p).parent else {
                return root;
            };
            if !self.node(p).red {
                return root;
            }
            let left = self.node(g).left == Some(p);
            let uncle = if left {
                self.node(g).right
            } else {
                self.node(g).left
            };
            if self.red(uncle) {
                self.color(uncle, false);
                self.node_mut(p).red = false;
                self.node_mut(g).red = true;
                x = g;
                continue;
            }
            if (if left {
                self.node(p).right
            } else {
                self.node(p).left
            }) == Some(x)
            {
                x = p;
                root = if left {
                    self.rotate_left(root, x)
                } else {
                    self.rotate_right(root, x)
                };
                p = self.node(x).parent.unwrap();
                g = self.node(p).parent.unwrap();
            }
            self.node_mut(p).red = false;
            self.node_mut(g).red = true;
            root = if left {
                self.rotate_right(root, g)
            } else {
                self.rotate_left(root, g)
            };
        }
    }
    fn front(&mut self, bucket: usize, root: usize) {
        let first = self.table[bucket];
        if first == Some(root) {
            return;
        }
        let (prev, next) = (self.node(root).prev, self.node(root).next);
        if let Some(next) = next {
            self.node_mut(next).prev = prev;
        }
        if let Some(prev) = prev {
            self.node_mut(prev).next = next;
        }
        if let Some(first) = first {
            self.node_mut(first).prev = Some(root);
        }
        self.node_mut(root).next = first;
        self.node_mut(root).prev = None;
        self.table[bucket] = Some(root);
    }
    fn treeify(&mut self, bucket: usize) {
        let mut current = self.table[bucket];
        let mut root = None;
        while let Some(x) = current {
            current = self.node(x).next;
            {
                let n = self.node_mut(x);
                n.tree = true;
                n.left = None;
                n.right = None;
                n.parent = None;
                n.red = false;
            }
            if let Some(r) = root {
                let mut p = r;
                loop {
                    let left = self.compare(x, p) != Ordering::Greater;
                    let child = if left {
                        self.node(p).left
                    } else {
                        self.node(p).right
                    };
                    if let Some(c) = child {
                        p = c;
                    } else {
                        self.node_mut(x).parent = Some(p);
                        if left {
                            self.node_mut(p).left = Some(x);
                        } else {
                            self.node_mut(p).right = Some(x);
                        }
                        root = Some(self.balance_insert(r, x));
                        break;
                    }
                }
            } else {
                root = Some(x);
            }
        }
        if let Some(root) = root {
            self.front(bucket, root);
        }
    }
    fn untreeify(&mut self, mut p: Link) {
        while let Some(i) = p {
            let n = self.node_mut(i);
            n.tree = false;
            n.parent = None;
            n.left = None;
            n.right = None;
            n.red = false;
            p = n.next;
        }
    }
    fn resize(&mut self) {
        let old_len = self.table.len();
        if old_len >= 1 << 30 {
            self.threshold = i32::MAX as usize;
            return;
        }
        let new_len = if old_len == 0 {
            if self.threshold > 0 {
                self.threshold
            } else {
                16
            }
        } else {
            old_len * 2
        };
        let old = std::mem::replace(&mut self.table, vec![None; new_len]);
        self.threshold = new_len * 3 / 4;
        for (bucket, first) in old.into_iter().enumerate() {
            let Some(first) = first else {
                continue;
            };
            let tree = self.node(first).tree;
            let mut groups = [Vec::new(), Vec::new()];
            let mut p = Some(first);
            while let Some(i) = p {
                p = self.node(i).next;
                groups[usize::from((self.node(i).hash as usize & old_len) != 0)].push(i);
            }
            for side in 0..2 {
                let group = &groups[side];
                if group.is_empty() {
                    continue;
                }
                let at = bucket + side * old_len;
                self.table[at] = Some(group[0]);
                for (j, &i) in group.iter().enumerate() {
                    let n = self.node_mut(i);
                    n.prev = j.checked_sub(1).map(|j| group[j]);
                    n.next = group.get(j + 1).copied();
                }
                if tree {
                    if group.len() <= 6 {
                        self.untreeify(Some(group[0]));
                    } else if !groups[1 - side].is_empty() {
                        self.treeify(at);
                    }
                }
            }
        }
    }
    pub fn put(&mut self, key: K, value: V) -> Option<V> {
        if let Some(i) = self.find(&key) {
            return Some(std::mem::replace(&mut self.node_mut(i).value, value));
        }
        if self.table.is_empty() {
            self.resize();
        }
        let hash = Self::hash(&key);
        let bucket = hash as usize & (self.table.len() - 1);
        let x = self.allocate(key, value, hash);
        if let Some(first) = self.table[bucket] {
            if self.node(first).tree {
                self.node_mut(x).tree = true;
                let root = self.root(first);
                let mut p = root;
                loop {
                    let left = self.compare(x, p) != Ordering::Greater;
                    let child = if left {
                        self.node(p).left
                    } else {
                        self.node(p).right
                    };
                    if let Some(child) = child {
                        p = child;
                    } else {
                        let next = self.node(p).next;
                        {
                            let n = self.node_mut(x);
                            n.parent = Some(p);
                            n.prev = Some(p);
                            n.next = next;
                        }
                        if let Some(next) = next {
                            self.node_mut(next).prev = Some(x);
                        }
                        self.node_mut(p).next = Some(x);
                        if left {
                            self.node_mut(p).left = Some(x);
                        } else {
                            self.node_mut(p).right = Some(x);
                        }
                        let root = self.balance_insert(root, x);
                        self.front(bucket, root);
                        break;
                    }
                }
            } else {
                let mut tail = first;
                let mut count = 1;
                while let Some(next) = self.node(tail).next {
                    tail = next;
                    count += 1;
                }
                self.node_mut(tail).next = Some(x);
                self.node_mut(x).prev = Some(tail);
                if count >= 8 {
                    if self.table.len() < 64 {
                        self.resize();
                    } else {
                        self.treeify(bucket);
                    }
                }
            }
        } else {
            self.table[bucket] = Some(x);
        }
        self.size += 1;
        self.modification = self.modification.wrapping_add(1);
        if self.size > self.threshold {
            self.resize();
        }
        None
    }
    fn balance_delete(&mut self, mut root: usize, mut x: Link) -> usize {
        loop {
            let Some(xi) = x else {
                return root;
            };
            if xi == root {
                return root;
            }
            let Some(p) = self.parent(x) else {
                self.color(x, false);
                return xi;
            };
            if self.red(x) {
                self.color(x, false);
                return root;
            }
            let left = self.left(Some(p)) == x;
            let mut sibling = if left {
                self.right(Some(p))
            } else {
                self.left(Some(p))
            };
            if self.red(sibling) {
                self.color(sibling, false);
                self.color(Some(p), true);
                root = if left {
                    self.rotate_left(root, p)
                } else {
                    self.rotate_right(root, p)
                };
                let parent = self.parent(x);
                sibling = if left {
                    self.right(parent)
                } else {
                    self.left(parent)
                };
            }
            let parent = self.parent(x);
            let Some(s) = sibling else {
                x = parent;
                continue;
            };
            let near = if left {
                self.left(sibling)
            } else {
                self.right(sibling)
            };
            let far = if left {
                self.right(sibling)
            } else {
                self.left(sibling)
            };
            if !self.red(near) && !self.red(far) {
                self.color(sibling, true);
                x = parent;
                continue;
            }
            if !self.red(far) {
                self.color(near, false);
                self.color(sibling, true);
                root = if left {
                    self.rotate_right(root, s)
                } else {
                    self.rotate_left(root, s)
                };
                let parent = self.parent(x);
                sibling = if left {
                    self.right(parent)
                } else {
                    self.left(parent)
                };
            }
            let parent = self.parent(x);
            self.color(sibling, self.red(parent));
            let far = if left {
                self.right(sibling)
            } else {
                self.left(sibling)
            };
            self.color(far, false);
            self.color(parent, false);
            if let Some(parent) = parent {
                root = if left {
                    self.rotate_left(root, parent)
                } else {
                    self.rotate_right(root, parent)
                };
            }
            x = Some(root);
        }
    }
    fn remove_tree(&mut self, bucket: usize, p: usize, movable: bool) {
        let first = self.table[bucket].unwrap();
        let mut root = self.root(first);
        let (prev, next) = (self.node(p).prev, self.node(p).next);
        if let Some(prev) = prev {
            self.node_mut(prev).next = next;
        } else {
            self.table[bucket] = next;
        }
        if let Some(next) = next {
            self.node_mut(next).prev = prev;
        }
        if self.table[bucket].is_none() {
            return;
        }
        if movable && (self.node(root).right.is_none() || self.left(self.node(root).left).is_none())
        {
            self.untreeify(self.table[bucket]);
            return;
        }
        let (left, right) = (self.node(p).left, self.node(p).right);
        let replacement;
        if let (Some(left), Some(right)) = (left, right) {
            let mut successor = right;
            while let Some(l) = self.node(successor).left {
                successor = l;
            }
            let red = self.node(successor).red;
            self.node_mut(successor).red = self.node(p).red;
            self.node_mut(p).red = red;
            let sr = self.node(successor).right;
            let pp = self.node(p).parent;
            if successor == right {
                self.node_mut(p).parent = Some(successor);
                self.node_mut(successor).right = Some(p);
            } else {
                let sp = self.node(successor).parent;
                self.node_mut(p).parent = sp;
                if let Some(sp) = sp {
                    if self.node(sp).left == Some(successor) {
                        self.node_mut(sp).left = Some(p);
                    } else {
                        self.node_mut(sp).right = Some(p);
                    }
                }
                self.node_mut(successor).right = Some(right);
                self.node_mut(right).parent = Some(successor);
            }
            self.node_mut(p).left = None;
            self.node_mut(p).right = sr;
            if let Some(sr) = sr {
                self.node_mut(sr).parent = Some(p);
            }
            self.node_mut(successor).left = Some(left);
            self.node_mut(left).parent = Some(successor);
            self.node_mut(successor).parent = pp;
            if let Some(pp) = pp {
                if self.node(pp).left == Some(p) {
                    self.node_mut(pp).left = Some(successor);
                } else {
                    self.node_mut(pp).right = Some(successor);
                }
            } else {
                root = successor;
            }
            replacement = sr.unwrap_or(p);
        } else {
            replacement = left.or(right).unwrap_or(p);
        }
        if replacement != p {
            let pp = self.node(p).parent;
            self.node_mut(replacement).parent = pp;
            if let Some(pp) = pp {
                if self.node(pp).left == Some(p) {
                    self.node_mut(pp).left = Some(replacement);
                } else {
                    self.node_mut(pp).right = Some(replacement);
                }
            } else {
                root = replacement;
                self.node_mut(replacement).red = false;
            }
            let n = self.node_mut(p);
            n.left = None;
            n.right = None;
            n.parent = None;
        }
        if !self.node(p).red {
            root = self.balance_delete(root, Some(replacement));
        }
        if replacement == p
            && let Some(pp) = self.node(p).parent
        {
            self.node_mut(p).parent = None;
            if self.node(pp).left == Some(p) {
                self.node_mut(pp).left = None;
            } else if self.node(pp).right == Some(p) {
                self.node_mut(pp).right = None;
            }
        }
        if movable {
            self.front(bucket, root);
        }
    }
    fn remove_node(&mut self, i: usize, movable: bool) -> V {
        let bucket = self.node(i).hash as usize & (self.table.len() - 1);
        if self.node(i).tree {
            self.remove_tree(bucket, i, movable);
        } else {
            let (prev, next) = (self.node(i).prev, self.node(i).next);
            if let Some(prev) = prev {
                self.node_mut(prev).next = next;
            } else {
                self.table[bucket] = next;
            }
            if let Some(next) = next {
                self.node_mut(next).prev = prev;
            }
        }
        self.size -= 1;
        self.modification = self.modification.wrapping_add(1);
        let n = self.nodes[i].take().unwrap();
        self.free.push(i);
        n.value
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.find(key).map(|i| self.remove_node(i, true))
    }
    pub fn clear(&mut self) {
        self.modification = self.modification.wrapping_add(1);
        if self.size != 0 {
            self.table.fill(None);
            self.nodes.clear();
            self.free.clear();
            self.size = 0;
        }
    }
    pub fn cursor(&self) -> Cursor {
        let mut c = Cursor {
            next: None,
            current: None,
            bucket: 0,
            expected: self.modification,
            owner: self.owner.clone(),
        };
        self.advance_bucket(&mut c);
        c
    }
    fn advance_bucket(&self, c: &mut Cursor) {
        while c.next.is_none() && c.bucket < self.table.len() {
            c.next = self.table[c.bucket];
            c.bucket += 1;
        }
    }
    fn check_cursor(&self, c: &Cursor) -> Result<(), CursorError> {
        if !Arc::ptr_eq(&self.owner, &c.owner) {
            Err(CursorError::WrongMap)
        } else if self.modification != c.expected {
            Err(CursorError::ConcurrentModification)
        } else {
            Ok(())
        }
    }
    /// Call while has_next is true. The returned values reflect replacements made
    /// after iterator creation; structural changes are rejected before access.
    pub fn next_entry(&self, c: &mut Cursor) -> Result<Option<(&K, &V)>, CursorError> {
        self.check_cursor(c)?;
        let Some(i) = c.next else {
            return Ok(None);
        };
        c.current = Some(i);
        c.next = self.node(i).next;
        self.advance_bucket(c);
        let n = self.node(i);
        Ok(Some((&n.key, &n.value)))
    }
    pub fn remove_current(&mut self, c: &mut Cursor) -> Result<(), CursorError> {
        if !Arc::ptr_eq(&self.owner, &c.owner) {
            return Err(CursorError::WrongMap);
        }
        let Some(i) = c.current else {
            return Err(CursorError::NoCurrentEntry);
        };
        self.check_cursor(c)?;
        c.current = None;
        self.remove_node(i, false);
        c.expected = self.modification;
        Ok(())
    }
}
