//! Names for the trie's frames, merged flame trees by function, and the tables derived from them.

use std::collections::HashMap;

use super::agg::{Aggregate, Frame, Key, State};
use super::symbols::Symbols;

#[derive(Default)]
pub struct Interner {
    pub names: Vec<String>,
    /// File and line where the name was first seen, or empty.
    pub locs: Vec<String>,
    pub pseudo: Vec<bool>,
    ix: HashMap<String, u32>,
}

impl Interner {
    pub fn id(&mut self, name: &str, loc: String, pseudo: bool) -> u32 {
        if let Some(&id) = self.ix.get(name) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(name.to_string());
        self.locs.push(loc);
        self.pseudo.push(pseudo);
        self.ix.insert(name.to_string(), id);
        id
    }
}

/// A function in the merged tree: frames of one function under the same caller are one node, whatever their call sites.
pub struct FNode {
    pub name: u32,
    pub own: u64,
    pub total: u64,
    pub kids: Vec<FNode>,
}

impl FNode {
    fn new(name: u32) -> Self {
        Self {
            name,
            own: 0,
            total: 0,
            kids: Vec::new(),
        }
    }

    fn finish(&mut self) {
        self.kids.iter_mut().for_each(FNode::finish);
        self.total = self.own + self.kids.iter().map(|k| k.total).sum::<u64>();
    }
}

pub const ROOT: u32 = u32::MAX;

/// The trie with every node's frames named (outermost first, inlined callees expanded).
pub struct Resolved<'a> {
    pub agg: &'a Aggregate,
    pub interner: Interner,
    per_node: Vec<Vec<u32>>,
    kids: Vec<Vec<u32>>,
}

/// Samples counted at each trie node by those of `agg`'s counters `keep` accepts.
pub fn weights(agg: &Aggregate, keep: impl Fn(&Key) -> bool) -> HashMap<u32, u64> {
    let mut w = HashMap::new();
    for (key, &n) in agg.counts.iter().filter(|(k, _)| keep(k)) {
        *w.entry(key.node).or_default() += n;
    }
    w
}

impl<'a> Resolved<'a> {
    pub fn new(agg: &'a Aggregate, symbols: &Symbols) -> Self {
        let mut interner = Interner::default();
        let mut per_node = Vec::with_capacity(agg.nodes());
        let mut kids = vec![Vec::new(); agg.nodes()];
        for n in 0..agg.nodes() as u32 {
            if n != 0 {
                kids[agg.parent(n) as usize].push(n);
            }
            per_node.push(match agg.frame(n) {
                _ if n == 0 => Vec::new(),
                Frame::Thread(i) => {
                    let name = format!("thread {}", agg.threads[i as usize]);
                    vec![interner.id(&name, String::new(), true)]
                }
                Frame::Wall(state) => {
                    let name = if state == State::Running {
                        "[running]"
                    } else {
                        "[waiting]"
                    };
                    vec![interner.id(name, String::new(), true)]
                }
                Frame::Addr(addr) => symbols
                    .frames(addr)
                    .iter()
                    .rev()
                    .map(|s| {
                        let loc = s
                            .file
                            .as_ref()
                            .map_or(String::new(), |f| format!("{f}:{}", s.line));
                        interner.id(&s.name, loc, false)
                    })
                    .collect(),
            });
        }
        Self {
            agg,
            interner,
            per_node,
            kids,
        }
    }

    /// The merged tree of the samples `w` counts per trie node.
    pub fn flame(&self, w: &HashMap<u32, u64>) -> FNode {
        let mut sub = vec![0u64; self.per_node.len()];
        for n in (1..sub.len()).rev() {
            sub[n] += w.get(&(n as u32)).copied().unwrap_or(0);
            sub[self.agg.parent(n as u32) as usize] += sub[n];
        }
        let mut root = FNode::new(ROOT);
        self.walk(w, &sub, 0, &mut root);
        root.finish();
        root
    }

    fn walk(&self, w: &HashMap<u32, u64>, sub: &[u64], n: u32, parent: &mut FNode) {
        fn enter<'n>(node: &'n mut FNode, names: &[u32]) -> &'n mut FNode {
            let Some((&first, rest)) = names.split_first() else {
                return node;
            };
            let at = match node.kids.iter().position(|k| k.name == first) {
                Some(at) => at,
                None => {
                    node.kids.push(FNode::new(first));
                    node.kids.len() - 1
                }
            };
            enter(&mut node.kids[at], rest)
        }
        let leaf = enter(parent, &self.per_node[n as usize]);
        leaf.own += w.get(&n).copied().unwrap_or(0);
        for &k in self.kids[n as usize]
            .iter()
            .filter(|&&k| sub[k as usize] > 0)
        {
            self.walk(w, sub, k, leaf);
        }
    }

    /// The names on the path to `node`, outermost first.
    pub fn stack(&self, node: u32) -> Vec<u32> {
        let path = self.agg.path(node);
        path.iter()
            .flat_map(|&n| self.per_node[n as usize].iter().copied())
            .collect()
    }

    pub fn name(&self, id: u32) -> &str {
        if id == ROOT {
            "all"
        } else {
            &self.interner.names[id as usize]
        }
    }
}

/// One function's samples: where it was the innermost frame, and where it was anywhere in the stack.
pub struct Row {
    pub name: u32,
    pub own: u64,
    pub total: u64,
}

/// Functions by their own samples, then by total (a function recursing counts once per sample).
pub fn top_functions(res: &Resolved, root: &FNode, limit: usize) -> Vec<Row> {
    fn tally(node: &FNode, on_path: &mut [u32], rows: &mut HashMap<u32, (u64, u64)>) {
        for kid in &node.kids {
            let first = on_path[kid.name as usize] == 0;
            on_path[kid.name as usize] += 1;
            let row = rows.entry(kid.name).or_default();
            row.0 += kid.own;
            row.1 += if first { kid.total } else { 0 };
            tally(kid, on_path, rows);
            on_path[kid.name as usize] -= 1;
        }
    }
    let mut rows = HashMap::new();
    tally(root, &mut vec![0; res.interner.names.len()], &mut rows);
    let mut rows: Vec<Row> = rows
        .into_iter()
        .filter(|(name, _)| !res.interner.pseudo[*name as usize])
        .map(|(name, (own, total))| Row { name, own, total })
        .collect();
    rows.sort_by_key(|r| (std::cmp::Reverse(r.own), std::cmp::Reverse(r.total), r.name));
    rows.truncate(limit);
    rows
}
