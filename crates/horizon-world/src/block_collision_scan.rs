//! Lazy block collision selection in vanilla cursor/query order. Providers own
//! chunk availability, state metadata, suffocation predicates and context dispatch.
use crate::{collision_context::CollisionContext, registry::BlockStateId};
use horizon_core::{
    geometry::{Aabb, ShapeError, ShapeOp, Vec3, VoxelShape},
    position::{BlockPos, ChunkPos},
};

#[derive(Clone, Copy, Debug)]
pub struct CollisionBlock {
    pub state: BlockStateId,
    pub large_collision_shape: bool,
    pub moving_piston: bool,
    /// The shape is empty under every context (air, plants): a scan skips
    /// the block without asking for its shape. Providers whose shapes come
    /// from elsewhere leave it false.
    pub empty_shape: bool,
}

pub trait BlockCollisionWorld {
    type Error;
    type Chunk: Copy;
    /// None is a missing collision chunk; native iteration retries missing chunks.
    fn collision_chunk(&mut self, position: ChunkPos) -> Result<Option<Self::Chunk>, Self::Error>;
    fn collision_block(
        &mut self,
        chunk: Self::Chunk,
        position: BlockPos,
    ) -> Result<CollisionBlock, Self::Error>;
    fn is_suffocating(
        &mut self,
        chunk: Self::Chunk,
        block: CollisionBlock,
        position: BlockPos,
    ) -> Result<bool, Self::Error>;
    /// Query against the global collision world, not the cached chunk provider.
    /// Shapes must preserve canonical block identity (including imported shapes).
    fn context_shape(
        &mut self,
        block: CollisionBlock,
        position: BlockPos,
        context: &CollisionContext<'_>,
    ) -> Result<VoxelShape, Self::Error>;
    /// Whether a block of the chunk's section `section_y` (block y >> 4) may
    /// have a large collision shape or be a moving piston: the only blocks
    /// the scan keeps outside the box's own cells. False lets the scan pass
    /// that section's outer cells without reading them; true reads them all.
    fn may_reach_out(&mut self, _chunk: Self::Chunk, _section_y: i32) -> Result<bool, Self::Error> {
        Ok(true)
    }
    /// Whether every block of the chunk's section `section_y` has an empty collision shape under
    /// every context (all air): true lets the scan pass that section's cells without reading
    /// them. False, the default, reads every cell.
    fn section_is_empty(
        &mut self,
        _chunk: Self::Chunk,
        _section_y: i32,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
    /// Whether the chunks `collision_chunk` reports, missing or not, stay as they are for the
    /// whole scan, and asking again has no effect: true lets the scan pass the rest of a row in
    /// a missing chunk without asking, and go back to the chunk it just left without asking
    /// again. False, the default, asks at every cell of a missing chunk and at every change of
    /// chunk, as vanilla's `BlockCollisions` does.
    fn missing_chunks_stay_missing(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub enum CollisionScanError<E> {
    InvalidBounds,
    TooLarge,
    VisitBudget,
    World(E),
    Shape(ShapeError),
}
impl<E: std::fmt::Display> std::fmt::Display for CollisionScanError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBounds => f.write_str("collision scan bounds must be finite"),
            Self::TooLarge => {
                f.write_str("collision scan exceeds the positive Java-int cursor domain")
            }
            Self::VisitBudget => f.write_str("collision scan candidate budget exhausted"),
            Self::World(error) => error.fmt(f),
            Self::Shape(error) => error.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for CollisionScanError<E> {}

#[derive(Debug)]
pub struct BlockCollisionHit {
    pub position: BlockPos,
    pub shape: VoxelShape,
}

#[derive(Clone, Copy, Debug)]
enum CellError {
    InvalidBounds,
    TooLarge,
}

/// The cursor a scan of `bounds` walks: its padded start, the box's extents in
/// cells and the cell count. `new` and [`scan_cells`] share it, so a caller
/// that sizes a budget from a box counts exactly what the scan will visit.
fn cells(bounds: Aabb) -> Result<(BlockPos, [i32; 3], usize), CellError> {
    let min = bounds.min_position();
    let max = bounds.max_position();
    if !min.is_finite() || !max.is_finite() {
        return Err(CellError::InvalidBounds);
    }
    let start = BlockPos::containing(Vec3::new(min.x - 1.0e-7, min.y - 1.0e-7, min.z - 1.0e-7))
        .offset(-1, -1, -1);
    let end = BlockPos::containing(Vec3::new(max.x + 1.0e-7, max.y + 1.0e-7, max.z + 1.0e-7))
        .offset(1, 1, 1);
    let sizes = [
        end.x.wrapping_sub(start.x).wrapping_add(1),
        end.y.wrapping_sub(start.y).wrapping_add(1),
        end.z.wrapping_sub(start.z).wrapping_add(1),
    ];
    if sizes.iter().any(|size| *size <= 0) {
        return Err(CellError::TooLarge);
    }
    let count = sizes
        .into_iter()
        .try_fold(1u64, |n, size| {
            n.checked_mul(size as u64).filter(|n| *n <= i32::MAX as u64)
        })
        .ok_or(CellError::TooLarge)? as usize;
    Ok((start, sizes, count))
}

/// The cells a scan of `bounds` visits, or `None` for a box `new` refuses
/// (`InvalidBounds`, `TooLarge`): the most a budget can usefully be.
pub fn scan_cells(bounds: Aabb) -> Option<usize> {
    cells(bounds).ok().map(|(_, _, count)| count)
}

/// Sections whose facts a scan remembers: 2 x 2 columns by two
/// sections holds every section of a box up to 16 blocks on each side.
const REACH_SECTIONS: usize = 8;

/// What a scan asked the world about one section, once.
#[derive(Clone, Copy)]
struct SectionFacts {
    at: ChunkPos,
    section_y: i32,
    /// `section_is_empty`.
    empty: bool,
    /// `may_reach_out`; not asked (false) for an empty section, whose cells are all passed.
    reaches: bool,
}

pub struct BlockCollisionScan<'w, 'c, W: BlockCollisionWorld> {
    world: &'w mut W,
    context: CollisionContext<'c>,
    bounds: Aabb,
    /// `Shapes.create(box)`, built the first time a block's shape is not
    /// the unit cube (finite bounds always make one).
    entity_shape: Option<VoxelShape>,
    only_suffocating: bool,
    start: BlockPos,
    sizes: [usize; 3],
    index: usize,
    /// The cell at `index`, relative to `start` (x fastest, then y, then z).
    cursor: [usize; 3],
    count: usize,
    budget: usize,
    /// The last cell's chunk and its section's facts: the cells of a row in one chunk share
    /// them, so most cells need one comparison.
    here: Option<(W::Chunk, SectionFacts)>,
    /// The `here` before, with `missing_stays`: a row across a chunk border goes back and forth
    /// between the two.
    there: Option<(W::Chunk, SectionFacts)>,
    /// The facts of the last sections cells were in, replaced in turn. A box
    /// across a chunk border goes back and forth between two columns on every
    /// row, so one remembered section is asked again at each crossing.
    sections: [Option<SectionFacts>; REACH_SECTIONS],
    sections_next: usize,
    /// `missing_chunks_stay_missing`, asked once.
    missing_stays: bool,
    failed: bool,
}
impl<'w, 'c, W: BlockCollisionWorld> BlockCollisionScan<'w, 'c, W> {
    /// Finite bounds and a positive-Java-int candidate volume are supported.
    /// Budget counts every cursor cell, including skipped corners. After an error
    /// iteration terminates; already performed queries are not rolled back.
    pub fn new(
        world: &'w mut W,
        context: CollisionContext<'c>,
        bounds: Aabb,
        only_suffocating: bool,
        candidate_budget: usize,
    ) -> Result<Self, CollisionScanError<W::Error>> {
        let (start, sizes, count) = cells(bounds).map_err(|error| match error {
            CellError::InvalidBounds => CollisionScanError::InvalidBounds,
            CellError::TooLarge => CollisionScanError::TooLarge,
        })?;
        let missing_stays = world.missing_chunks_stay_missing();
        Ok(Self {
            world,
            context,
            bounds,
            entity_shape: None,
            only_suffocating,
            start,
            sizes: sizes.map(|s| s as usize),
            index: 0,
            cursor: [0; 3],
            count,
            budget: candidate_budget,
            here: None,
            there: None,
            sections: [None; REACH_SECTIONS],
            sections_next: 0,
            missing_stays,
            failed: false,
        })
    }

    /// The cells after the current one (cell `x` of its row, block x `block_x`) that are in the
    /// same row and chunk, never more than the budget has left: what can be passed at once.
    fn rest_in_chunk(&self, x: usize, block_x: i32) -> usize {
        (self.sizes[0] - 1 - x)
            .min(15 - (block_x & 15) as usize)
            .min(self.budget.saturating_sub(self.index))
    }

    /// The chunk and section facts of a cell in another chunk or section than the last one's;
    /// None in a missing chunk.
    fn enter(
        &mut self,
        at: ChunkPos,
        section_y: i32,
    ) -> Result<Option<(W::Chunk, SectionFacts)>, CollisionScanError<W::Error>> {
        if self.missing_stays
            && let Some((chunk, facts)) = self.there
            && facts.at == at
            && facts.section_y == section_y
        {
            std::mem::swap(&mut self.here, &mut self.there);
            return Ok(Some((chunk, facts)));
        }
        let chunk = match self.here {
            Some((chunk, facts)) if facts.at == at => chunk,
            _ => match self
                .world
                .collision_chunk(at)
                .map_err(CollisionScanError::World)?
            {
                Some(chunk) => chunk,
                None => {
                    // Vanilla forgets its chunk after a missing one and asks for it again. With
                    // `missing_stays` asking has no effect: `here` stays, as no cell of the
                    // missing chunk matches it.
                    if !self.missing_stays {
                        self.here = None;
                    }
                    return Ok(None);
                }
            },
        };
        let facts = self.section_facts(at, chunk, section_y)?;
        self.there = self.here;
        self.here = Some((chunk, facts));
        Ok(Some((chunk, facts)))
    }

    /// The facts of the section a cell is in, asked once per section the scan enters.
    fn section_facts(
        &mut self,
        at: ChunkPos,
        chunk: W::Chunk,
        section_y: i32,
    ) -> Result<SectionFacts, CollisionScanError<W::Error>> {
        if let Some(&facts) = self
            .sections
            .iter()
            .flatten()
            .find(|facts| facts.at == at && facts.section_y == section_y)
        {
            return Ok(facts);
        }
        let empty = self
            .world
            .section_is_empty(chunk, section_y)
            .map_err(CollisionScanError::World)?;
        let reaches = !empty
            && self
                .world
                .may_reach_out(chunk, section_y)
                .map_err(CollisionScanError::World)?;
        let facts = SectionFacts {
            at,
            section_y,
            empty,
            reaches,
        };
        self.sections[self.sections_next] = Some(facts);
        self.sections_next = (self.sections_next + 1) % REACH_SECTIONS;
        Ok(facts)
    }

    /// Move the cursor `n` cells further along its row; `n` must not pass
    /// the row's end.
    fn skip(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let [width, height, _] = self.sizes;
        self.index += n;
        self.cursor[0] += n;
        if self.cursor[0] == width {
            self.cursor[0] = 0;
            self.cursor[1] += 1;
            if self.cursor[1] == height {
                self.cursor[1] = 0;
                self.cursor[2] += 1;
            }
        }
    }

    fn find_next(&mut self) -> Result<Option<BlockCollisionHit>, CollisionScanError<W::Error>> {
        while self.index < self.count {
            if self.index >= self.budget {
                return Err(CollisionScanError::VisitBudget);
            }
            let [width, height, depth] = self.sizes;
            let relative = self.cursor;
            self.index += 1;
            self.cursor[0] += 1;
            if self.cursor[0] == width {
                self.cursor[0] = 0;
                self.cursor[1] += 1;
                if self.cursor[1] == height {
                    self.cursor[1] = 0;
                    self.cursor[2] += 1;
                }
            }
            let kind = relative
                .into_iter()
                .zip(self.sizes)
                .filter(|(p, size)| *p == 0 || *p == size - 1)
                .count();
            if kind == 3 {
                continue;
            }
            let p = self
                .start
                .offset(relative[0] as i32, relative[1] as i32, relative[2] as i32);
            let chunk_position = p.chunk();
            let section_y = p.y >> 4;
            let (chunk, facts) = match self.here {
                Some((chunk, facts))
                    if facts.at == chunk_position && facts.section_y == section_y =>
                {
                    (chunk, facts)
                }
                _ => match self.enter(chunk_position, section_y)? {
                    Some(here) => here,
                    None => {
                        // A chunk that stays missing has nothing in the rest of the row's cells
                        // in it: they are passed at once, as a section without blocks is below.
                        if self.missing_stays {
                            let rest = self.rest_in_chunk(relative[0], p.x);
                            self.skip(rest);
                        }
                        continue;
                    }
                },
            };
            // A section whose blocks have no collision shape gives nothing for any cell in it,
            // inner or outer: the rest of the row in it is passed at once (never beyond the
            // budget, which still ends the scan where it would have). A far move's sweep is
            // mostly sky and chunks not loaded.
            if facts.empty {
                let rest = self.rest_in_chunk(relative[0], p.x);
                self.skip(rest);
                continue;
            }
            // An outer cell only counts for a block reaching out of its own
            // cell: most sections hold none, so their outer cells go unread
            // (the result is the same, with a fifth of the block reads for a
            // mob's box).
            if kind != 0 && !facts.reaches {
                // On a row along the box's bottom, top or sides every
                // cell is outer: the rest of it in this chunk is passed
                // at once (never beyond the budget, which still ends the
                // scan where it would have).
                if relative[1] == 0
                    || relative[1] == height - 1
                    || relative[2] == 0
                    || relative[2] == depth - 1
                {
                    let rest = self.rest_in_chunk(relative[0], p.x);
                    self.skip(rest);
                }
                continue;
            }
            let block = self
                .world
                .collision_block(chunk, p)
                .map_err(CollisionScanError::World)?;
            if self.only_suffocating
                && !self
                    .world
                    .is_suffocating(chunk, block, p)
                    .map_err(CollisionScanError::World)?
            {
                continue;
            }
            if kind == 1 && !block.large_collision_shape || kind == 2 && !block.moving_piston {
                continue;
            }
            if block.empty_shape {
                continue;
            }
            let shape = self
                .world
                .context_shape(block, p, &self.context)
                .map_err(CollisionScanError::World)?;
            let offset = Vec3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z));
            let moved = if shape.is_block() {
                // Native adds one in floating point, including at i32::MAX.
                let top = Vec3::new(offset.x + 1.0, offset.y + 1.0, offset.z + 1.0);
                if !self.bounds.intersects_corners(offset, top) {
                    continue;
                }
                shape.moved(offset)
            } else {
                // Shapes whose bounds miss the box share no volume with it:
                // joinIsNotEmpty(AND) is false without moving them.
                if !shape
                    .bounds()
                    .is_some_and(|b| b.move_by(offset).intersects(self.bounds))
                {
                    continue;
                }
                let moved = shape.moved(offset);
                let entity_shape = match &self.entity_shape {
                    Some(shape) => shape,
                    None => self.entity_shape.insert(
                        VoxelShape::from_box(self.bounds).map_err(CollisionScanError::Shape)?,
                    ),
                };
                if moved.is_empty()
                    || !moved
                        .join_is_not_empty(entity_shape, ShapeOp::AND)
                        .map_err(CollisionScanError::Shape)?
                {
                    continue;
                }
                moved
            };
            return Ok(Some(BlockCollisionHit {
                position: p,
                shape: moved,
            }));
        }
        Ok(None)
    }
}
impl<W: BlockCollisionWorld> Iterator for BlockCollisionScan<'_, '_, W> {
    type Item = Result<BlockCollisionHit, CollisionScanError<W::Error>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.find_next() {
            Ok(hit) => hit.map(Ok),
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}
impl<W: BlockCollisionWorld> std::iter::FusedIterator for BlockCollisionScan<'_, '_, W> {}
