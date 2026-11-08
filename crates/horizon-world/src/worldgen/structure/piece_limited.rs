//! The piece-limited jigsaw assembler of Repurposed Structures and Moog's
//! Structure Lib (PieceLimitedJigsawManager, the same code in both), for
//! their structures carried over from a modpack (docs/modpacks.md). Carry
//! maps their jigsaw types to `minecraft:jigsaw` with a
//! `"horizon:piece_limited"` key; vanilla data never has it.
//!
//! ```json
//! "horizon:piece_limited": {
//!   "library": "moogs",            // or "repurposed": their few differences
//!   "min_y": 40, "max_y": 120,     // y_allowance: pieces stay inside, unless buried
//!   "burying": "lowest_corner",    // or "average_land", "lowest_side"
//!   "no_liquid": true,             // cannot_spawn_in_liquid, as burying reads it
//!   "ignore_bounds_pools": ["x:pool"],
//!   "land_search": "highest_land", // the Nether variants: or "lowest_land", "fixed_height"
//!   "ledge_offset_y": 2,
//!   "pieces": [{"name": "x:piece", "required": 1, "min_depth": 3, "max": 2}],
//!   "layout": "mineshaft"          // Repurposed's mineshafts: moved under the terrain
//! }
//! ```
//!
//! It differs from vanilla's JigsawPlacement: a fresh random seeded by the
//! chunk, the start piece drawn uniformly and moved to the surface at its
//! center, a bounding volume 120 blocks down and 180 up, children drawn by
//! weight from the pools' weighted entries in order (`nextInt(total) + 1`,
//! counted down), pieces capped by template (`max`) and some required
//! (`required`, from `min_depth`), the whole layout retried up to 40 times
//! until every required piece is in, and after it the pieces centred on the
//! chunk corner, then buried in the terrain or moved onto Nether land.
//! Repurposed's overworld mineshafts are not centred: their top goes 15
//! blocks under the terrain around the chunk.
use super::{
    BoundingBox, Rotation, StructureResources, StructureTemplate,
    jigsaw::{
        Free, JigsawConfig, Junction, PoolElement, PoolPiece, Pools, Projection, can_attach,
        shuffle, step,
    },
    start::StartContext,
};
use crate::{
    heightmap::HeightmapType, identifier::Identifier, java_hash_map::JavaHashMap,
    worldgen::block_feature::FeatureBlockBindings,
};
use horizon_core::{
    position::ChunkPos,
    random::{LegacyRandom, RandomSource, WorldgenRandom},
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    rc::Rc,
    sync::Arc,
};

/// Which mod's copy of the code: they differ in a few post-layout steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Library {
    Moogs,
    Repurposed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Burying {
    LowestCorner,
    AverageLand,
    LowestSide,
}

/// The structure type's postLayoutAdjustments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// GenericJigsawStructure's: centred, then buried or moved onto land.
    Generic,
    /// Repurposed Structures' MineshaftStructure.
    Mineshaft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandSearch {
    Highest,
    Lowest,
    Fixed,
}

/// One template's counts (`*_pieces_spawn_counts`).
#[derive(Clone, Debug)]
struct Counted {
    required: Option<i32>,
    min_depth: i32,
    max: Option<i32>,
}

/// `"horizon:piece_limited"` of a jigsaw structure.
#[derive(Debug)]
pub struct Config {
    library: Library,
    min_y: Option<i32>,
    max_y: Option<i32>,
    burying: Option<Burying>,
    no_liquid: bool,
    ignore_bounds: HashSet<String>,
    land_search: Option<LandSearch>,
    ledge: i32,
    layout: Layout,
    /// Counted templates by id, in the order of Java's HashMap of the
    /// required pieces (the first one still short is forced in first).
    pieces: Vec<(Identifier, Counted)>,
}

impl Config {
    pub fn decode(v: &Value) -> Self {
        let int = |k: &str| v[k].as_i64().map(|n| n as i32);
        let mut pieces: Vec<(Identifier, Counted)> = Vec::new();
        for p in v["pieces"].as_array().into_iter().flatten() {
            let Some(id) = p["name"].as_str().and_then(|n| Identifier::parse(n).ok()) else {
                continue;
            };
            let counted = Counted {
                required: p["required"].as_i64().map(|n| n as i32),
                min_depth: p["min_depth"].as_i64().unwrap_or(0) as i32,
                max: p["max"].as_i64().map(|n| n as i32),
            };
            // A later entry for the same piece replaces it, as HashMap.put.
            match pieces.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = counted,
                None => pieces.push((id, counted)),
            }
        }
        // getRequirePieces builds a HashMap of the required ones and the
        // assembler copies it (new HashMap(map)): iterate as that copy does.
        let mut map = JavaHashMap::default();
        for (i, (id, c)) in pieces.iter().enumerate() {
            if c.required.is_some() {
                map.put(id.clone(), i);
            }
        }
        let order: Vec<usize> = map.copied().entries().map(|(_, &i)| i).collect();
        let mut sorted: Vec<(Identifier, Counted)> =
            order.iter().map(|&i| pieces[i].clone()).collect();
        sorted.extend(pieces.iter().filter(|(_, c)| c.required.is_none()).cloned());
        Self {
            library: match v["library"].as_str() {
                Some("repurposed") => Library::Repurposed,
                _ => Library::Moogs,
            },
            min_y: int("min_y"),
            max_y: int("max_y"),
            burying: match v["burying"].as_str() {
                Some("lowest_corner") => Some(Burying::LowestCorner),
                Some("average_land") => Some(Burying::AverageLand),
                Some("lowest_side") => Some(Burying::LowestSide),
                _ => None,
            },
            no_liquid: v["no_liquid"].as_bool().unwrap_or(false),
            ignore_bounds: v["ignore_bounds_pools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some(p.as_str()?.to_owned()))
                .collect(),
            land_search: match v["land_search"].as_str() {
                Some("highest_land") => Some(LandSearch::Highest),
                Some("lowest_land") => Some(LandSearch::Lowest),
                Some("fixed_height") => Some(LandSearch::Fixed),
                _ => None,
            },
            ledge: int("ledge_offset_y").unwrap_or(0),
            layout: match v["layout"].as_str() {
                Some("mineshaft") => Layout::Mineshaft,
                _ => Layout::Generic,
            },
            pieces: sorted,
        }
    }
    fn required(&self) -> impl Iterator<Item = &Counted> {
        self.pieces
            .iter()
            .map(|(_, c)| c)
            .filter(|c| c.required.is_some())
    }
}

/// The counted templates, loaded, in [`Config::pieces`] order.
struct Counts(Vec<Arc<StructureTemplate>>);
impl Counts {
    fn new(limits: &Config, resources: &StructureResources) -> Self {
        Self(
            limits
                .pieces
                .iter()
                .map(|(id, _)| {
                    resources
                        .template(&id.to_string())
                        .unwrap_or_else(|| Arc::new(StructureTemplate::empty()))
                })
                .collect(),
        )
    }
    /// The index of a single element's template among the counted ones.
    fn of(&self, element: &PoolElement) -> Option<usize> {
        let PoolElement::Single { template, .. } = element else {
            return None;
        };
        self.0.iter().position(|t| Arc::ptr_eq(t, template.arc()))
    }
}

/// PieceLimitedJigsawManager.assembleJigsawStructure and the structure's
/// postLayoutAdjustments: the start's center and the pieces, or None when
/// nothing starts. `start` is the chunk's corner at the sampled start height.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    config: &JigsawConfig<'_>,
    limits: &Config,
    context: &mut impl StartContext,
    pools: &Pools,
    resources: &StructureResources,
    bindings: &FeatureBlockBindings<'_>,
    chunk: ChunkPos,
    start: [i32; 3],
) -> Option<([i32; 3], Vec<PoolPiece>)> {
    let mut random = WorldgenRandom::new(LegacyRandom::new(0));
    random.set_large_feature_seed(context.seed(), chunk.x, chunk.z);
    let rotation = Rotation::random(&mut random);
    let pool = pools.get(config.start_pool, resources, bindings)?;
    // An empty start pool crashes the mod's generation; nothing starts.
    if pool.templates.is_empty() {
        return None;
    }
    let element = pool.random_template(&mut random);
    if matches!(element.as_ref(), PoolElement::Empty) {
        return None;
    }
    let mut first = PoolPiece {
        bounding_box: element.bounding_box(start, rotation),
        ground_level_delta: element.ground_level_delta(),
        element,
        position: start,
        rotation,
        junctions: Vec::new(),
        waterlogging: config.waterlogging,
    };
    let b = first.bounding_box;
    let (cx, cz) = ((b.max[0] + b.min[0]) / 2, (b.max[2] + b.min[2]) / 2);
    let top = limits.max_y.unwrap_or(i32::MAX);
    let bottom = limits.min_y.unwrap_or(i32::MIN);
    let cy = match config.project_to {
        Some(kind) => {
            let y = start[1] + context.base_height(cx, cz, kind);
            if y > top || y < bottom {
                return None;
            }
            y
        }
        None => start[1],
    };
    first.moved(cy - (b.min[1] + first.ground_level_delta));
    let start_box = first.bounding_box;
    let (start_position, start_rotation) = (first.position, first.rotation);
    let counts = Counts::new(limits, resources);
    let run_once = limits.required().next().is_none();
    let mut pieces = vec![first];
    let mut attempts = 0;
    // `for (attempts = 0; runOnce || doesNotHaveAllRequiredPieces(...); attempts++)`:
    // the first test sees the start piece alone.
    while run_once || lacks_required(limits, &counts, &pieces) {
        if attempts == 40 {
            break;
        }
        if attempts > 0 {
            let element = pool.random_template(&mut random);
            pieces = vec![PoolPiece {
                bounding_box: element.bounding_box(start_position, start_rotation),
                ground_level_delta: element.ground_level_delta(),
                element,
                position: start_position,
                rotation: start_rotation,
                junctions: Vec::new(),
                waterlogging: config.waterlogging,
            }];
        } else {
            pieces.truncate(1);
        }
        if config.max_depth > 0 {
            let r = config.max_distance[0];
            let free = Rc::new(RefCell::new(Free {
                container: BoundingBox::new([cx - r, cy - 120, cz - r], [cx + r, cy + 180, cz + r]),
                removed: vec![start_box],
            }));
            let buried = limits.burying.is_some();
            let mut assembler = Assembler {
                config,
                limits,
                counted: &counts,
                pools,
                resources,
                bindings,
                pieces: std::mem::take(&mut pieces),
                states: vec![(free, 0)],
                counts: vec![0; limits.pieces.len()],
                max_y: if buried { i32::MAX } else { top },
                min_y: if buried { i32::MIN } else { bottom },
            };
            let mut queue = VecDeque::from([0]);
            while let Some(piece) = queue.pop_front() {
                assembler.children(piece, &mut queue, context, &mut random);
            }
            pieces = assembler.pieces;
        }
        if run_once {
            break;
        }
        attempts += 1;
    }
    adjust(config, limits, context, start, &mut pieces);
    if pieces.is_empty() {
        return None;
    }
    let mut whole = pieces[0].bounding_box;
    for p in &pieces[1..] {
        whole.encapsulate(&p.bounding_box);
    }
    // The builder is cleared when the layout reaches past the build height.
    if whole.max[1] > context.max_y() + 1 {
        return None;
    }
    Some(([cx, cy, cz], pieces))
}

/// doesNotHaveAllRequiredPieces.
fn lacks_required(limits: &Config, counts: &Counts, pieces: &[PoolPiece]) -> bool {
    let mut left: Vec<i32> = limits
        .pieces
        .iter()
        .map(|(_, c)| c.required.unwrap_or(0))
        .collect();
    for p in pieces {
        if let Some(i) = counts.of(&p.element) {
            left[i] -= 1;
        }
    }
    left.iter().any(|&n| n > 0)
}

/// Moves a piece; Repurposed Structures' movePieceProperly also writes its
/// junctions, all three setters into the source X, so only the last sticks.
fn shift(limits: &Config, piece: &mut PoolPiece, d: [i32; 3]) {
    piece.position = std::array::from_fn(|i| piece.position[i] + d[i]);
    piece.bounding_box = piece.bounding_box.moved(d);
    if limits.library == Library::Repurposed {
        for j in &mut piece.junctions {
            j.source_x = j.source_z + d[2];
        }
    }
}
fn shift_all(limits: &Config, pieces: &mut [PoolPiece], d: [i32; 3]) {
    for p in pieces {
        shift(limits, p, d);
    }
}

/// The structure's postLayoutAdjustments.
fn adjust(
    config: &JigsawConfig<'_>,
    limits: &Config,
    context: &mut impl StartContext,
    start: [i32; 3],
    pieces: &mut Vec<PoolPiece>,
) {
    if pieces.is_empty() {
        return;
    }
    if limits.layout == Layout::Mineshaft {
        below_terrain(limits, context, start, pieces);
        return;
    }
    // GeneralUtils.centerAllPieces on the chunk's corner.
    let center = pieces[0].bounding_box.center();
    let d = [start[0] - center[0], 0, start[2] - center[2]];
    for p in pieces.iter_mut() {
        // Centring uses StructurePiece.move in both mods.
        p.position = std::array::from_fn(|i| p.position[i] + d[i]);
        p.bounding_box = p.bounding_box.moved(d);
    }
    let offset_y = start[1];
    let sea = context.sea_level();
    if let Some(search) = limits.land_search {
        nether(limits, context, search, offset_y, pieces);
        return;
    }
    let Some(burying) = limits.burying else {
        return;
    };
    let kind = config.project_to.unwrap_or(HeightmapType::WorldSurfaceWg);
    let ocean_floor = matches!(
        kind,
        HeightmapType::OceanFloorWg | HeightmapType::OceanFloor
    );
    let box0 = pieces[0].bounding_box;
    let mut land = match burying {
        Burying::LowestCorner => [
            (box0.min[0], box0.min[2]),
            (box0.min[0], box0.max[2]),
            (box0.max[0], box0.min[2]),
            (box0.max[0], box0.max[2]),
        ]
        .into_iter()
        .map(|(x, z)| context.base_height(x, z, kind) - 1)
        .min()
        .unwrap_or(0),
        Burying::LowestSide => {
            let c = box0.center();
            let mut highest = i32::MAX;
            for (x, z) in [
                (box0.min[0], c[2]),
                (c[0], box0.max[2]),
                (c[0], box0.min[2]),
                (box0.max[0], c[2]),
            ] {
                // getFirstOccupiedHeight (Repurposed's cached free height - 1).
                let land = context.base_height(x, z, kind) - 1;
                match limits.min_y {
                    Some(min) if land >= min => highest = land,
                    Some(_) => {}
                    None => highest = highest.min(land),
                }
            }
            if let Some(min) = limits.min_y.filter(|_| highest == i32::MAX) {
                highest = min;
            }
            highest
        }
        Burying::AverageLand => {
            average_land(limits, context, kind, ocean_floor, offset_y, pieces);
            return;
        }
    };
    if limits.no_liquid || !ocean_floor {
        land = land.max(sea);
    } else {
        land = land.min(sea - box0.span()[1]);
    }
    // offsetToNewHeight.
    if limits.max_y.is_some()
        && let Some(min) = limits.min_y
        && box0.max[1] + offset_y < min
    {
        land = land.max(limits.max_y.unwrap_or(land));
    }
    if let Some(min) = limits.min_y
        && box0.min[1] + offset_y < min
    {
        land = land.min(min);
    }
    let dy = land - box0.min[1] + offset_y;
    shift_all(limits, pieces, [0, dy, 0]);
}

/// MineshaftStructure.postLayoutAdjustments: the layout's top moved to 15
/// blocks under the lowest ocean floor at the chunk's middle and 16 blocks
/// to each side, within y_allowance; never up.
fn below_terrain(
    limits: &Config,
    context: &mut impl StartContext,
    start: [i32; 3],
    pieces: &mut [PoolPiece],
) {
    let target = mineshaft_top(start, limits.min_y, |x, z| {
        context.base_height(x, z, HeightmapType::OceanFloorWg)
    });
    let top = pieces
        .iter()
        .map(|p| p.bounding_box.max[1])
        .max()
        .unwrap_or(i32::MIN);
    if target < limits.max_y.unwrap_or(i32::MAX) && target < top {
        shift_all(limits, pieces, [0, target - top, 0]);
    }
}

/// Where [`below_terrain`] puts a mineshaft's top: `floor` is the first free
/// height over the ocean floor (getFirstFreeHeight, OCEAN_FLOOR_WG).
fn mineshaft_top(
    start: [i32; 3],
    min_y: Option<i32>,
    mut floor: impl FnMut(i32, i32) -> i32,
) -> i32 {
    let (x, z) = (start[0] + 7, start[2] + 7);
    let mut terrain = floor(x, z) - 1;
    // Plane.HORIZONTAL's order; the mod takes one more off after each.
    for (dx, dz) in [(0, -16), (16, 0), (0, 16), (-16, 0)] {
        terrain = terrain.min(floor(x + dx, z + dz)) - 1;
    }
    (terrain - 15).max(min_y.unwrap_or(i32::MIN))
}

/// AVERAGE_LAND: the mean land height over a grid on the first piece's
/// footprint; no structure when none of the heights is allowed.
fn average_land(
    limits: &Config,
    context: &mut impl StartContext,
    kind: HeightmapType,
    ocean_floor: bool,
    offset_y: i32,
    pieces: &mut Vec<PoolPiece>,
) {
    let b = pieces[0].bounding_box;
    let c = b.center();
    let (lx, lz) = (b.max[0] - b.min[0], b.max[2] - b.min[2]);
    let radius = (f64::from(lx * lx + lz * lz).sqrt() as i32) / 2;
    // The mod's loop never ends with a step of 0; one block apart instead.
    let step = (radius / 2).max(1);
    let (min, max) = (
        limits.min_y.unwrap_or(i32::MIN),
        limits.max_y.unwrap_or(i32::MAX),
    );
    let (mut sum, mut n) = (0i64, 0i64);
    let mut x = -radius;
    while x <= radius {
        let mut z = -radius;
        while z <= radius {
            let h = context.base_height(c[0] + x, c[2] + z, kind) - 1;
            if h > min && h < max {
                sum += i64::from(h);
                n += 1;
            }
            z += step;
        }
        x += step;
    }
    let mut average = (n > 0).then(|| sum as f64 / n as f64);
    if average.is_none() {
        average = limits.max_y.or(limits.min_y).map(f64::from);
    }
    let Some(mut average) = average else {
        pieces.clear();
        return;
    };
    if limits.no_liquid && !ocean_floor {
        average = average.max(f64::from(context.sea_level()));
        if let Some(max) = limits.max_y {
            // Math.max, as the mod has it.
            average = average.max(f64::from(max));
        }
    }
    let dy = average as i32 - b.min[1] + offset_y;
    shift_all(limits, pieces, [0, dy, 0]);
}

/// The Nether variants: onto the highest or lowest land at the layout's
/// center (or a fixed height), a ledge above it, within y_allowance.
fn nether(
    limits: &Config,
    context: &mut impl StartContext,
    search: LandSearch,
    offset_y: i32,
    pieces: &mut [PoolPiece],
) {
    let mut whole = pieces[0].bounding_box;
    for p in &pieces[1..] {
        whole.encapsulate(&p.bounding_box);
    }
    let sea = context.sea_level();
    let generation = context.generation();
    let terrain_limit = generation.min_y + generation.height;
    let base = pieces[0].bounding_box.min[1];
    let found = |context: &mut dyn ColumnQuery, highest: bool| {
        let c = whole.center();
        context.land(c[0], c[2], highest, !limits.no_liquid, sea, terrain_limit)
    };
    let mut target = match (limits.library, search) {
        (Library::Moogs, LandSearch::Fixed) => offset_y + limits.ledge,
        (library, search) => {
            let y = found(context, search == LandSearch::Highest);
            let at = if y < terrain_limit && y > sea + 1 {
                y
            } else {
                sea
            };
            if library == Library::Repurposed {
                shift_all(limits, pieces, [0, at + limits.ledge - base, 0]);
                return;
            }
            at + limits.ledge + offset_y
        }
    };
    let height = whole.max[1] - whole.min[1];
    let below = whole.min[1] - base;
    if let Some(top) = limits.max_y {
        target = target.min(top - height - below);
    }
    if let Some(bottom) = limits.min_y {
        target = target.max(bottom - below);
    }
    shift_all(limits, pieces, [0, target - base, 0]);
}

/// GeneralUtils.getHighestLand and getLowestLand on the noise column.
trait ColumnQuery {
    fn land(
        &mut self,
        x: i32,
        z: i32,
        highest: bool,
        on_liquid: bool,
        sea: i32,
        terrain_limit: i32,
    ) -> i32;
}
impl<C: StartContext> ColumnQuery for C {
    fn land(
        &mut self,
        x: i32,
        z: i32,
        highest: bool,
        on_liquid: bool,
        sea: i32,
        terrain_limit: i32,
    ) -> i32 {
        let min = self.generation().min_y;
        let mut columns = self.base_columns(
            x,
            z,
            &[HeightmapType::WorldSurfaceWg, HeightmapType::OceanFloorWg],
        );
        let solid = columns.pop().unwrap_or_default();
        let filled = columns.pop().unwrap_or_default();
        land_in(&filled, &solid, min, highest, on_liquid, sea, terrain_limit)
    }
}

/// The land search over one noise column: `filled` (not air) and `solid`
/// (canOcclude, for the noise's own blocks) from `min` up; past the
/// column's ends a NoiseColumn reads air.
fn land_in(
    filled: &[bool],
    solid: &[bool],
    min: i32,
    highest: bool,
    on_liquid: bool,
    sea: i32,
    terrain_limit: i32,
) -> i32 {
    let at = |c: &[bool], y: i32| {
        usize::try_from(y - min)
            .ok()
            .and_then(|i| c.get(i).copied())
            .unwrap_or(false)
    };
    let air = |y: i32| !at(filled, y);
    let stands = |y: i32| if on_liquid { !air(y) } else { at(solid, y) };
    if highest {
        let mut y = terrain_limit - 40;
        while y > sea {
            if at(solid, y) && air(y + 3) && stands(y) {
                return y;
            }
            y -= 1;
        }
        y
    } else {
        let mut y = sea + 1;
        while y <= terrain_limit - 40 {
            if stands(y) && air(y + 1) && air(y + 5) {
                return y + 1;
            }
            y += 1;
        }
        sea
    }
}

/// PieceLimitedJigsawManager.Assembler.
struct Assembler<'a> {
    config: &'a JigsawConfig<'a>,
    limits: &'a Config,
    counted: &'a Counts,
    pools: &'a Pools,
    resources: &'a StructureResources,
    bindings: &'a FeatureBlockBindings<'a>,
    pieces: Vec<PoolPiece>,
    /// Per piece: the free space it was placed in, and its depth.
    states: Vec<(Rc<RefCell<Free>>, i32)>,
    /// Pieces placed per counted template.
    counts: Vec<i32>,
    max_y: i32,
    min_y: i32,
}

/// Where a child goes: the parent's jigsaw and what it needs.
struct Slot<'s> {
    source: usize,
    jigsaw: &'s super::jigsaw::Jigsaw<'s>,
    target_pos: [i32; 3],
    free: Rc<RefCell<Free>>,
    depth: i32,
}

impl Assembler<'_> {
    fn pool(&self, name: &str) -> Option<Arc<super::jigsaw::Pool>> {
        let name = self.config.aliases.get(name).map_or(name, String::as_str);
        self.pools.get(name, self.resources, self.bindings)
    }
    /// generatePiece: one child per jigsaw where one fits, from the target
    /// pool unless the piece is at the last depth, else from its fallback.
    fn children(
        &mut self,
        source: usize,
        queue: &mut VecDeque<usize>,
        context: &mut impl StartContext,
        random: &mut impl RandomSource,
    ) {
        let (element, position, rotation, source_box) = {
            let s = &self.pieces[source];
            (s.element.clone(), s.position, s.rotation, s.bounding_box)
        };
        let (context_free, depth) = self.states[source].clone();
        let mut source_free: Option<Rc<RefCell<Free>>> = None;
        for jigsaw in element.shuffled_jigsaws(self.resources, position, rotation, random) {
            let Some(target) = self.pool(&jigsaw.pool) else {
                continue;
            };
            if target.templates.is_empty() && jigsaw.pool != "minecraft:empty" {
                continue;
            }
            let s = step(jigsaw.front);
            let target_pos: [i32; 3] = std::array::from_fn(|i| jigsaw.pos[i] + s[i]);
            let free = if source_box.is_inside(target_pos) {
                source_free
                    .get_or_insert_with(|| {
                        Rc::new(RefCell::new(Free {
                            container: source_box,
                            removed: Vec::new(),
                        }))
                    })
                    .clone()
            } else {
                context_free.clone()
            };
            let slot = Slot {
                source,
                jigsaw: &jigsaw,
                target_pos,
                free,
                depth,
            };
            if depth != self.config.max_depth
                && self.process(&slot, target.raw.clone(), false, queue, context, random)
            {
                continue;
            }
            let Some(fallback) = self.pool(&target.fallback) else {
                continue;
            };
            let ignore = self.limits.ignore_bounds.contains(&target.fallback);
            self.process(&slot, fallback.raw.clone(), ignore, queue, context, random);
        }
    }
    /// processList: whether a piece was placed.
    fn process(
        &mut self,
        slot: &Slot<'_>,
        mut candidates: Vec<(Arc<PoolElement>, i32)>,
        ignore_bounds: bool,
        queue: &mut VecDeque<usize>,
        context: &mut impl StartContext,
        random: &mut impl RandomSource,
    ) -> bool {
        let jigsaw = slot.jigsaw;
        let (source_rigid, source_y, source_ground) = {
            let s = &self.pieces[slot.source];
            (
                s.element.projection() == Projection::Rigid,
                s.bounding_box.min[1],
                s.ground_level_delta,
            )
        };
        let local_y = jigsaw.pos[1] - source_y;
        let mut surface = None;
        let (x, z) = (jigsaw.pos[0], jigsaw.pos[2]);
        let mut base =
            |context: &mut dyn SurfaceQuery| *surface.get_or_insert_with(|| context.surface(x, z));
        let mut total: i32 = candidates.iter().map(|c| c.1).sum();
        while !candidates.is_empty() {
            let mut chosen = None;
            // The first required piece still short is forced in once deep enough.
            let needed = self
                .limits
                .pieces
                .iter()
                .enumerate()
                .find(|(i, (_, c))| c.required.is_some_and(|r| self.counts[*i] < r));
            if let Some((need_index, (_, need))) = needed {
                let need_template = &self.counted.0[need_index];
                let found = candidates.iter().position(|(e, _)| {
                    matches!(e.as_ref(), PoolElement::Single { template, .. }
                        if Arc::ptr_eq(template.arc(), need_template))
                });
                if let Some(i) = found {
                    if slot.depth >= (self.config.max_depth - 1).min(need.min_depth) {
                        chosen = Some(i);
                    } else {
                        total -= candidates[i].1;
                        candidates.remove(i);
                    }
                }
            }
            let index = match chosen {
                Some(i) => i,
                None => {
                    if total <= 0 || candidates.is_empty() {
                        return false;
                    }
                    let mut weight = random.next_int_bound(total).unwrap_or(0) + 1;
                    let mut found = candidates.len() - 1;
                    for (i, (_, w)) in candidates.iter().enumerate() {
                        weight -= w;
                        if weight <= 0 {
                            found = i;
                            break;
                        }
                    }
                    found
                }
            };
            let (target, weight) = candidates[index].clone();
            if matches!(target.as_ref(), PoolElement::Empty) {
                return false;
            }
            let counted = self.counted.of(&target);
            let full = counted.is_some_and(|i| {
                self.limits.pieces[i]
                    .1
                    .max
                    .is_some_and(|m| self.counts[i] >= m)
            });
            if full {
                total -= weight;
                candidates.remove(index);
                continue;
            }
            let mut rotations = Rotation::ALL;
            shuffle(&mut rotations, random);
            for rotation in rotations {
                let target_jigsaws =
                    target.shuffled_jigsaws(self.resources, [0; 3], rotation, random);
                let hack_box = target.bounding_box([0; 3], rotation);
                let expand_to = if self.config.use_expansion_hack && hack_box.span()[1] <= 16 {
                    target_jigsaws
                        .iter()
                        .map(|tj| {
                            let st = step(tj.front);
                            if !hack_box.is_inside(std::array::from_fn(|i| tj.pos[i] + st[i])) {
                                return 0;
                            }
                            let child = self.pool(&tj.pool);
                            let child_size = child.as_ref().map_or(0, |p| p.max_size());
                            let fallback_size = child
                                .and_then(|p| self.pool(&p.fallback))
                                .map_or(0, |p| p.max_size());
                            child_size.max(fallback_size)
                        })
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };
                for target_jigsaw in &target_jigsaws {
                    if !can_attach(jigsaw, target_jigsaw) {
                        continue;
                    }
                    let target_local = target_jigsaw.pos;
                    let raw_position: [i32; 3] =
                        std::array::from_fn(|i| slot.target_pos[i] - target_local[i]);
                    let raw_box = target.bounding_box(raw_position, rotation);
                    let target_rigid = target.projection() == Projection::Rigid;
                    let delta_y = local_y - target_local[1] + step(jigsaw.front)[1];
                    let target_y = if source_rigid && target_rigid {
                        source_y + delta_y
                    } else {
                        base(context) - target_local[1]
                    };
                    let offset = target_y - raw_box.min[1];
                    let mut target_box = raw_box.moved([0, offset, 0]);
                    let target_position =
                        [raw_position[0], raw_position[1] + offset, raw_position[2]];
                    if expand_to > 0 {
                        let size = (expand_to + 1).max(target_box.max[1] - target_box.min[1]);
                        target_box.max[1] = target_box.min[1] + size;
                    }
                    if target_box.max[1] > self.max_y || target_box.min[1] < self.min_y {
                        continue;
                    }
                    if !ignore_bounds && !slot.free.borrow().fits(&target_box) {
                        continue;
                    }
                    let target_ground = if target_rigid {
                        source_ground - delta_y
                    } else {
                        target.ground_level_delta()
                    };
                    let junction_y = if source_rigid {
                        source_y + local_y
                    } else if target_rigid {
                        target_y + target_local[1]
                    } else {
                        base(context) + delta_y / 2
                    };
                    self.pieces[slot.source].junctions.push(Junction {
                        source_x: slot.target_pos[0],
                        source_ground_y: junction_y - local_y + source_ground,
                        source_z: slot.target_pos[2],
                        delta_y,
                        dest_projection: target.projection(),
                    });
                    slot.free.borrow_mut().removed.push(target_box);
                    self.pieces.push(PoolPiece {
                        element: target.clone(),
                        position: target_position,
                        ground_level_delta: target_ground,
                        rotation,
                        bounding_box: target_box,
                        junctions: vec![Junction {
                            source_x: jigsaw.pos[0],
                            source_ground_y: junction_y - target_local[1] + target_ground,
                            source_z: jigsaw.pos[2],
                            delta_y: -delta_y,
                            dest_projection: if source_rigid {
                                Projection::Rigid
                            } else {
                                Projection::TerrainMatching
                            },
                        }],
                        waterlogging: self.config.waterlogging,
                    });
                    self.states.push((slot.free.clone(), slot.depth + 1));
                    if slot.depth < self.config.max_depth {
                        queue.push_back(self.pieces.len() - 1);
                    }
                    if let Some(i) = counted {
                        self.counts[i] += 1;
                    }
                    return true;
                }
            }
            total -= weight;
            candidates.remove(index);
        }
        false
    }
}

/// The surface height query, object-safe for the per-call cache.
trait SurfaceQuery {
    fn surface(&mut self, x: i32, z: i32) -> i32;
}
impl<C: StartContext> SurfaceQuery for C {
    fn surface(&mut self, x: i32, z: i32) -> i32 {
        self.base_height(x, z, HeightmapType::WorldSurfaceWg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn required_pieces_follow_javas_hash_map() {
        let limits = Config::decode(&json!({"library": "repurposed", "pieces": [
            {"name": "rs:city/a", "required": 1},
            {"name": "rs:city/x", "max": 2},
            {"name": "rs:city/b", "required": 1, "min_depth": 3},
            {"name": "rs:city/c", "required": 2},
            {"name": "rs:city/a", "required": 4}]}));
        let names: Vec<String> = limits.pieces.iter().map(|(id, _)| id.to_string()).collect();
        // Buckets 3, 2, 1 of the copied map's 4: c, b, a; then the capped one.
        assert_eq!(names, ["rs:city/c", "rs:city/b", "rs:city/a", "rs:city/x"]);
        assert_eq!(
            limits.pieces[2].1.required,
            Some(4),
            "a later entry replaces"
        );
        assert_eq!(limits.pieces[1].1.min_depth, 3);
        assert_eq!(limits.library, Library::Repurposed);
    }

    #[test]
    fn mineshafts_go_under_the_lowest_floor_around() {
        // Floor 80 at the middle (7, 7), 70 north, 90 east, 60 south, 85 west.
        let floor = |x, z| match (x, z) {
            (7, 7) => 80,
            (7, -9) => 70,
            (23, 7) => 90,
            (7, 23) => 60,
            (-9, 7) => 85,
            other => panic!("{other:?}"),
        };
        // 79, min(79, 70) - 1 = 69, 68, min(68, 60) - 1 = 59, 58; 15 under it.
        assert_eq!(mineshaft_top([0, 50, 0], None, floor), 43);
        assert_eq!(mineshaft_top([0, 50, 0], Some(48), floor), 48);
    }

    #[test]
    fn nether_land_is_found_in_the_column() {
        // A column from y 0: netherrack to 40, lava 41..=45, air, a ledge 70..=72.
        let (mut filled, mut solid) = (vec![false; 128], vec![false; 128]);
        for y in 0..=45 {
            filled[y] = true;
            solid[y] = y <= 40;
        }
        for y in 70..=72 {
            filled[y] = true;
            solid[y] = true;
        }
        // Highest: from 88 down, a solid block with air 3 above.
        assert_eq!(land_in(&filled, &solid, 0, true, true, 32, 128), 72);
        // Lowest: from 33 up, a block to stand on with air 1 and 5 above; one up.
        assert_eq!(land_in(&filled, &solid, 0, false, true, 32, 128), 46);
        assert_eq!(land_in(&filled, &solid, 0, false, false, 32, 128), 73);
        // Nothing found: the highest search stops at the sea level.
        let empty = vec![false; 128];
        assert_eq!(land_in(&empty, &empty, 0, true, true, 32, 128), 32);
        assert_eq!(land_in(&empty, &empty, 0, false, true, 32, 128), 32);
    }

    #[test]
    fn repurposed_moves_junctions_as_its_code_does() {
        let template = Arc::new(StructureTemplate::empty());
        let element = Arc::new(PoolElement::Single {
            template: crate::worldgen::structure::LazyTemplate::loaded(template),
            processors: Default::default(),
            processors_source: serde_json::Value::Null,
            legacy: false,
            projection: Projection::Rigid,
            waterlogging: None,
            enhanced_terrain: None,
            ground_level: 1,
        });
        let piece = || PoolPiece {
            bounding_box: BoundingBox::new([0; 3], [4; 3]),
            ground_level_delta: 1,
            element: element.clone(),
            position: [0; 3],
            rotation: Rotation::None,
            junctions: vec![Junction {
                source_x: 1,
                source_ground_y: 2,
                source_z: 3,
                delta_y: 0,
                dest_projection: Projection::Rigid,
            }],
            waterlogging: true,
        };
        let moogs = Config::decode(&json!({}));
        let rs = Config::decode(&json!({"library": "repurposed"}));
        let (mut a, mut b) = (piece(), piece());
        shift(&moogs, &mut a, [10, 20, 30]);
        shift(&rs, &mut b, [10, 20, 30]);
        assert_eq!(a.bounding_box, BoundingBox::new([10, 20, 30], [14, 24, 34]));
        assert_eq!(a.position, [10, 20, 30]);
        assert_eq!(a.junctions[0].source_x, 1);
        // movePieceProperly sets the source X three times; Z + dz is left.
        assert_eq!(b.junctions[0].source_x, 33);
        assert_eq!(b.junctions[0].source_ground_y, 2);
    }
}
