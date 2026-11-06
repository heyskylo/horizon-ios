//! Immutable climate search tree with an explicit per-worker previous result.
use super::{TargetPoint, quantize};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub min: i64,
    pub max: i64,
}
impl Parameter {
    /// The record constructor does not validate endpoint order.
    pub const fn new(min: i64, max: i64) -> Self {
        Self { min, max }
    }
    pub fn point(value: f32) -> Self {
        let value = quantize(value);
        Self::new(value, value)
    }
    pub fn span(min: f32, max: f32) -> Result<Self, IndexError> {
        if min > max {
            return Err(IndexError::InvertedSpan);
        }
        Ok(Self::new(quantize(min), quantize(max)))
    }
    pub fn distance(self, value: i64) -> i64 {
        let above = value.wrapping_sub(self.max);
        if above > 0 {
            above
        } else {
            self.min.wrapping_sub(value).max(0)
        }
    }
    fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }
    fn center(self) -> i64 {
        self.min.wrapping_add(self.max) / 2
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterPoint {
    /// Temperature, humidity, continentalness, erosion, depth, weirdness.
    pub coordinates: [Parameter; 6],
    pub offset: i64,
}
impl ParameterPoint {
    pub fn fitness(self, target: TargetPoint) -> i64 {
        self.coordinates.iter().zip(target.parameters()).fold(
            self.offset.wrapping_mul(self.offset),
            |sum, (parameter, value)| {
                let distance = parameter.distance(value);
                sum.wrapping_add(distance.wrapping_mul(distance))
            },
        )
    }
    pub fn parameter_space(self) -> [Parameter; 7] {
        std::array::from_fn(|i| {
            if i < 6 {
                self.coordinates[i]
            } else {
                Parameter::new(self.offset, self.offset)
            }
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexError {
    Empty,
    TooManyEntries,
    InvalidBranching,
    InvertedSpan,
    NoSplit,
    NoResult,
}
impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "climate search requires at least one entry",
            Self::TooManyEntries => "climate search is limited to 65536 entries",
            Self::InvalidBranching => "climate branching must be in 2..=32",
            Self::InvertedSpan => "climate interval endpoints are inverted",
            Self::NoSplit => "climate tree could not select a split",
            Self::NoResult => "climate search found no result",
        })
    }
}
impl std::error::Error for IndexError {}

#[derive(Debug)]
enum Children {
    Leaf(usize),
    Branch(Vec<usize>),
}
#[derive(Debug)]
struct Node {
    space: [Parameter; 7],
    children: Children,
}
/// A node's box as the search reads it: the six climate intervals, and the
/// squared distance of its offset interval from the target's offset, which
/// is always 0, so it is the same for every target.
#[derive(Clone, Copy, Debug)]
struct SearchSpace {
    coordinates: [Parameter; 6],
    offset: i64,
}
impl SearchSpace {
    fn new(space: &[Parameter; 7]) -> Self {
        let offset = space[6].distance(0);
        Self {
            coordinates: std::array::from_fn(|i| space[i]),
            offset: offset.wrapping_mul(offset),
        }
    }
    /// Vanilla's `Node.distance`: the sum of the seven squared distances, in
    /// wrapping long arithmetic, whose sums do not depend on their order.
    fn distance(&self, values: &[i64; 6]) -> i64 {
        self.coordinates
            .iter()
            .zip(values)
            .fold(self.offset, |sum, (parameter, value)| {
                let distance = parameter.distance(*value);
                sum.wrapping_add(distance.wrapping_mul(distance))
            })
    }
    /// `distance` when it is below `limit`, else at least `limit`: the sum
    /// stops once it reaches `limit`. Only for bounded boxes and targets, whose
    /// terms are squares below 2^60 that cannot overflow even all seven
    /// together, so the sum only grows and a part of it is at most the whole.
    /// A search only compares a distance with `limit`, its best so far, and
    /// uses it only when it is below, so the stop changes no result.
    fn distance_until(&self, values: &[i64; 6], limit: i64) -> i64 {
        let mut sum = self.offset;
        for axis in AXES {
            let distance = self.coordinates[axis].distance(values[axis]);
            sum += distance * distance;
            if sum >= limit {
                break;
            }
        }
        sum
    }
}
/// The order `distance_until` adds the axes in, which does not change a full
/// sum. Depth first: the overworld's surface biomes are points in depth, so
/// for most targets depth alone takes a box past the best leaf so far.
const AXES: [usize; 6] = [4, 2, 0, 1, 3, 5];
/// Values within `BOUND` are bounded: the distance of a bounded value from a
/// bounded interval is below 2^30.
const BOUND: i64 = 1 << 29;
fn bounded(value: i64) -> bool {
    -BOUND < value && value < BOUND
}
fn compare_space(a: &[Parameter; 7], b: &[Parameter; 7], axis: usize, absolute: bool) -> Ordering {
    for i in 0..7 {
        let coordinate = (axis + i) % 7;
        let key = |p: Parameter| {
            if absolute {
                p.center().wrapping_abs()
            } else {
                p.center()
            }
        };
        let result = key(a[coordinate]).cmp(&key(b[coordinate]));
        if result != Ordering::Equal {
            return result;
        }
    }
    Ordering::Equal
}
#[derive(Debug)]
struct Bucket {
    space: [Parameter; 7],
    children: Vec<usize>,
}
fn space(nodes: &[Node], children: &[usize]) -> [Parameter; 7] {
    let mut space = nodes[children[0]].space;
    for child in &children[1..] {
        for (parameter, other) in space.iter_mut().zip(nodes[*child].space) {
            *parameter = parameter.union(other);
        }
    }
    space
}
fn build(
    nodes: &mut Vec<Node>,
    mut children: Vec<usize>,
    branching: usize,
) -> Result<usize, IndexError> {
    if children.len() == 1 {
        return Ok(children[0]);
    }
    if children.len() <= branching {
        // Stable sorting matters when centers and total center distance tie.
        children.sort_by_key(|id| {
            nodes[*id]
                .space
                .iter()
                .fold(0i64, |sum, p| sum.wrapping_add(p.center().wrapping_abs()))
        });
    } else {
        // For this bounded domain this is the same bucket size as vanilla's
        // pow(branching,floor(log(length-0.01)/log(branching))). Integer powers
        // avoid depending on platform double logarithms during construction.
        let mut bucket_size = 1;
        while bucket_size * branching < children.len() {
            bucket_size *= branching;
        }
        let mut best_cost = i64::MAX;
        let mut best = None;
        for axis in 0..7 {
            children.sort_by(|a, b| compare_space(&nodes[*a].space, &nodes[*b].space, axis, false));
            let buckets: Vec<_> = children
                .chunks(bucket_size)
                .map(|children| Bucket {
                    space: space(nodes, children),
                    children: children.to_vec(),
                })
                .collect();
            let cost = buckets.iter().fold(0i64, |sum, bucket| {
                bucket.space.iter().fold(sum, |sum, p| {
                    sum.wrapping_add(p.max.wrapping_sub(p.min).wrapping_abs())
                })
            });
            if best_cost > cost {
                best_cost = cost;
                best = Some((axis, buckets));
            }
        }
        let (axis, mut buckets) = best.ok_or(IndexError::NoSplit)?;
        buckets.sort_by(|a, b| compare_space(&a.space, &b.space, axis, true));
        children = buckets
            .into_iter()
            .map(|bucket| build(nodes, bucket.children, branching))
            .collect::<Result<_, _>>()?;
    }
    let space = space(nodes, &children);
    let id = nodes.len();
    nodes.push(Node {
        space,
        children: Children::Branch(children),
    });
    Ok(id)
}

/// The index is immutable and can be shared; each searcher owns its cache.
/// Entry count and branching are explicit limits of this bounded Rust API.
#[derive(Debug)]
pub struct ClimateIndex<T> {
    entries: Vec<(ParameterPoint, T)>,
    /// The built tree renumbered breadth first, root at 0, so each branch's
    /// children are one contiguous run and a search walks memory in order.
    spaces: Vec<SearchSpace>,
    /// Every bound of every box, offsets included, is bounded.
    bounded: bool,
    /// Per node: its first child and child count, or its entry and 0 for a leaf.
    links: Vec<(u32, u32)>,
}
impl<T> ClimateIndex<T> {
    pub fn new(entries: Vec<(ParameterPoint, T)>) -> Result<Self, IndexError> {
        Self::with_branching(entries, 19)
    }
    pub fn with_branching(
        entries: Vec<(ParameterPoint, T)>,
        branching: usize,
    ) -> Result<Self, IndexError> {
        if entries.is_empty() {
            return Err(IndexError::Empty);
        }
        if entries.len() > 65536 {
            return Err(IndexError::TooManyEntries);
        }
        if !(2..=32).contains(&branching) {
            return Err(IndexError::InvalidBranching);
        }
        let mut nodes: Vec<_> = entries
            .iter()
            .enumerate()
            .map(|(id, (point, _))| Node {
                space: point.parameter_space(),
                children: Children::Leaf(id),
            })
            .collect();
        let children = (0..nodes.len()).collect();
        let root = build(&mut nodes, children, branching)?;
        let mut order = vec![root];
        let mut links = Vec::with_capacity(nodes.len());
        let mut next = 0;
        while next < order.len() {
            let node = &nodes[order[next]];
            next += 1;
            links.push(match &node.children {
                Children::Leaf(entry) => (*entry as u32, 0),
                Children::Branch(children) => {
                    let first = order.len() as u32;
                    order.extend_from_slice(children);
                    (first, children.len() as u32)
                }
            });
        }
        let bounded = order.iter().all(|id| {
            nodes[*id]
                .space
                .iter()
                .all(|parameter| bounded(parameter.min) && bounded(parameter.max))
        });
        Ok(Self {
            entries,
            spaces: order
                .iter()
                .map(|id| SearchSpace::new(&nodes[*id].space))
                .collect(),
            bounded,
            links,
        })
    }
    pub fn entries(&self) -> &[(ParameterPoint, T)] {
        &self.entries
    }
    pub fn searcher(&self) -> ClimateSearch<'_, T> {
        ClimateSearch {
            index: self,
            last: None,
        }
    }
    /// Vanilla's separate reference path: ties retain input-list order and do
    /// not consult or update the searcher's previous indexed result.
    pub fn brute_force(&self, target: TargetPoint) -> &T {
        let mut best = 0;
        let mut fitness = self.entries[0].0.fitness(target);
        for (id, (point, _)) in self.entries.iter().enumerate().skip(1) {
            let next = point.fitness(target);
            if next < fitness {
                fitness = next;
                best = id;
            }
        }
        &self.entries[best].1
    }
    /// Preorder leaf entry indices expose stable tree layout for comparison.
    pub fn leaf_order(&self) -> Vec<usize> {
        fn walk(links: &[(u32, u32)], id: usize, out: &mut Vec<usize>) {
            match links[id] {
                (entry, 0) => out.push(entry as usize),
                (first, count) => {
                    for child in first..first + count {
                        walk(links, child as usize, out);
                    }
                }
            }
        }
        let mut result = Vec::with_capacity(self.entries.len());
        walk(&self.links, 0, &mut result);
        result
    }
    /// Vanilla's `SubTree.search` below `id`, whose best leaf so far is `best`
    /// at distance `fitness` (`i64::MAX` with none), returning the best leaf
    /// and its distance. Vanilla measures the best leaf again in every subtree
    /// and after each one; passing its distance along gives the same values.
    /// `bounded`: the tree and the target are, so distances may stop early.
    fn search(
        &self,
        id: usize,
        values: &[i64; 6],
        bounded: bool,
        mut best: Option<usize>,
        mut fitness: i64,
    ) -> Result<(usize, i64), IndexError> {
        let (first, count) = self.links[id];
        let children = first as usize..(first + count) as usize;
        for (child, space) in children.clone().zip(&self.spaces[children]) {
            let lower = if bounded {
                space.distance_until(values, fitness)
            } else {
                space.distance(values)
            };
            if fitness > lower {
                let (result, next) = if self.links[child].1 == 0 {
                    (child, lower)
                } else {
                    self.search(child, values, bounded, best, fitness)?
                };
                if fitness > next {
                    fitness = next;
                    best = Some(result);
                }
            }
        }
        best.map(|best| (best, fitness)).ok_or(IndexError::NoResult)
    }
}
#[derive(Debug)]
pub struct ClimateSearch<'a, T> {
    index: &'a ClimateIndex<T>,
    last: Option<usize>,
}
impl<'a, T> ClimateSearch<'a, T> {
    pub fn find(&mut self, target: TargetPoint) -> Result<&'a T, IndexError> {
        let parameters = target.parameters();
        let values = std::array::from_fn(|i| parameters[i]);
        let result = if self.index.links[0].1 == 0 {
            0
        } else {
            let fitness = self
                .last
                .map_or(i64::MAX, |id| self.index.spaces[id].distance(&values));
            let bounded = self.index.bounded && values.iter().all(|value| bounded(*value));
            self.index
                .search(0, &values, bounded, self.last, fitness)?
                .0
        };
        self.last = Some(result);
        let (entry, 0) = self.index.links[result] else {
            unreachable!("search returns a leaf");
        };
        Ok(&self.index.entries[entry as usize].1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The search as it was before it passed distances along: a copy of
    /// vanilla's `SubTree.search` over the built nodes, before the breadth
    /// first renumbering, measuring all seven coordinates of every box.
    struct Reference {
        nodes: Vec<Node>,
        root: usize,
        last: Option<usize>,
    }
    impl Reference {
        fn new(entries: &[(ParameterPoint, usize)], branching: usize) -> Self {
            let mut nodes: Vec<_> = entries
                .iter()
                .enumerate()
                .map(|(id, (point, _))| Node {
                    space: point.parameter_space(),
                    children: Children::Leaf(id),
                })
                .collect();
            let children = (0..nodes.len()).collect();
            let root = build(&mut nodes, children, branching).unwrap();
            Self {
                nodes,
                root,
                last: None,
            }
        }
        fn distance(&self, id: usize, values: &[i64; 7]) -> i64 {
            self.nodes[id]
                .space
                .iter()
                .zip(values)
                .fold(0i64, |sum, (parameter, value)| {
                    let distance = parameter.distance(*value);
                    sum.wrapping_add(distance.wrapping_mul(distance))
                })
        }
        fn search(
            &self,
            id: usize,
            values: &[i64; 7],
            previous: Option<usize>,
        ) -> Result<usize, IndexError> {
            let Children::Branch(children) = &self.nodes[id].children else {
                return Ok(id);
            };
            let mut best = previous;
            let mut fitness = previous.map_or(i64::MAX, |id| self.distance(id, values));
            for &child in children {
                let lower = self.distance(child, values);
                if fitness > lower {
                    let result = self.search(child, values, best)?;
                    let next = if result == child {
                        lower
                    } else {
                        self.distance(result, values)
                    };
                    if fitness > next {
                        fitness = next;
                        best = Some(result);
                    }
                }
            }
            best.ok_or(IndexError::NoResult)
        }
        fn find(&mut self, target: TargetPoint) -> Result<usize, IndexError> {
            let result = self.search(self.root, &target.parameters(), self.last)?;
            self.last = Some(result);
            let Children::Leaf(entry) = self.nodes[result].children else {
                unreachable!("search returns a leaf");
            };
            Ok(entry)
        }
    }

    /// SplitMix64, so the corpus needs no random crate.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn range(&mut self, min: i64, max: i64) -> i64 {
            min + (self.next() % (max - min + 1) as u64) as i64
        }
        fn pick(&mut self, values: &[i64]) -> i64 {
            values[self.below(values.len())]
        }
    }

    /// Runs one target sequence through the index and the reference, each
    /// keeping its previous result. Returns false if the tree cannot be built.
    fn compare(
        entries: Vec<(ParameterPoint, usize)>,
        branching: usize,
        targets: &[TargetPoint],
    ) -> bool {
        let Ok(index) = ClimateIndex::with_branching(entries.clone(), branching) else {
            return false;
        };
        let mut reference = Reference::new(&entries, branching);
        let mut search = index.searcher();
        for (step, target) in targets.iter().enumerate() {
            // A fresh search now and then, as each chunk's biome fill starts one.
            if step % 1000 == 999 {
                search = index.searcher();
                reference.last = None;
            }
            let expected = reference.find(*target);
            let actual = search.find(*target).copied();
            assert_eq!(actual, expected, "step {step}, target {target:?}");
        }
        true
    }

    fn target(values: [i64; 6]) -> TargetPoint {
        let [
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        ] = values;
        TargetPoint {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        }
    }

    /// Targets near each other in sequence, as neighbouring quarts are, with
    /// coordinates often set to an entry's endpoint so neighbouring boxes tie,
    /// and now and then a jump anywhere.
    fn walk(
        rng: &mut Rng,
        entries: &[(ParameterPoint, usize)],
        count: usize,
        span: i64,
    ) -> Vec<TargetPoint> {
        let mut values = [0i64; 6];
        (0..count)
            .map(|_| {
                for (axis, value) in values.iter_mut().enumerate() {
                    *value = match rng.below(16) {
                        0 => rng.range(-span, span),
                        1..=3 => {
                            let parameter = entries[rng.below(entries.len())].0.coordinates[axis];
                            if rng.below(2) == 0 {
                                parameter.min
                            } else {
                                parameter.max
                            }
                        }
                        _ => value.wrapping_add(rng.range(-span / 64, span / 64)),
                    };
                }
                target(values)
            })
            .collect()
    }

    fn vanilla_list(name: &str) -> Option<Vec<(ParameterPoint, usize)>> {
        let root = std::env::var_os("HORIZON_TEST_RESOURCES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.local/horizon-client/26.3")
            });
        let path = root.join(format!("worldgen/biome_parameters/{name}.json"));
        let Ok(report) = std::fs::read_to_string(&path) else {
            eprintln!("skipping the {name} list: no {}", path.display());
            return None;
        };
        let json = crate::worldgen::generator::quantize_parameter_report(&report).unwrap();
        let source =
            crate::worldgen::biome::MultiNoiseBiomeSource::from_quantized_json(&json).unwrap();
        Some(
            source
                .parameters()
                .entries()
                .iter()
                .enumerate()
                .map(|(id, (point, _))| (*point, id))
                .collect(),
        )
    }

    #[test]
    fn search_matches_the_reference_on_the_vanilla_lists() {
        for (name, count) in [("overworld", 200_000), ("nether", 50_000)] {
            let Some(entries) = vanilla_list(name) else {
                continue;
            };
            let targets = walk(&mut Rng(name.len() as u64), &entries, count, 25_000);
            assert!(compare(entries, 19, &targets));
        }
    }

    #[test]
    fn search_matches_the_reference_with_ties_and_wrapping() {
        let mut rng = Rng(0x9997);
        let mut built = 0;
        let extremes = [
            i64::MIN,
            i64::MIN + 1,
            -(1 << 31),
            -1,
            0,
            1,
            (1 << 31) - 1,
            i64::MAX - 1,
            i64::MAX,
        ];
        // Around the bound of the early stop, which needs both the tree and
        // the target bounded: with `BOUND` itself in half of these trees.
        let edges = [
            -BOUND,
            1 - BOUND,
            -BOUND / 2,
            0,
            BOUND / 2,
            BOUND - 1,
            BOUND,
        ];
        for case in 0..240 {
            let branching = 2 + case % 31;
            let count = 1 + rng.below([8, 40, 400, 2000][case % 4]);
            let wrapping = case % 6 == 5;
            let edge = case % 6 == 4;
            let edges =
                &edges[usize::from(case % 12 == 4)..edges.len() - usize::from(case % 12 == 4)];
            // A small grid makes many boxes and offsets equally far.
            let value = |rng: &mut Rng| {
                if wrapping {
                    rng.pick(&extremes)
                } else if edge {
                    rng.pick(edges)
                } else {
                    rng.range(-6, 6) * 100
                }
            };
            let mut entries: Vec<(ParameterPoint, usize)> = Vec::with_capacity(count);
            for id in 0..count {
                let point = if id > 0 && rng.below(5) == 0 {
                    entries[rng.below(id)].0
                } else {
                    ParameterPoint {
                        coordinates: std::array::from_fn(|_| {
                            let (a, b) = (value(&mut rng), value(&mut rng));
                            // Some spans inverted: the record does not check them.
                            if rng.below(8) == 0 {
                                Parameter::new(a, b)
                            } else {
                                Parameter::new(a.min(b), a.max(b))
                            }
                        }),
                        offset: if rng.below(3) == 0 {
                            value(&mut rng)
                        } else {
                            0
                        },
                    }
                };
                entries.push((point, id));
            }
            let targets: Vec<_> = if wrapping {
                (0..500)
                    .map(|_| target(std::array::from_fn(|_| rng.pick(&extremes))))
                    .collect()
            } else if edge {
                walk(&mut rng, &entries, 2000, BOUND + (1 << 24))
            } else {
                walk(&mut rng, &entries, 2000, 800)
            };
            built += compare(entries, branching, &targets) as usize;
        }
        // Extreme spans can leave no finite split cost; most trees build.
        assert!(built > 220, "{built} trees built");
    }
}
