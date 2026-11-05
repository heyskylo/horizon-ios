use horizon_core::{
    geometry::{Aabb, Vec3, VoxelShape},
    position::{BlockPos, ChunkPos},
};
use horizon_world::{
    block_collision_scan::{
        BlockCollisionScan, BlockCollisionWorld, CollisionBlock, CollisionScanError,
    },
    collision_context::CollisionContext,
    registry::BlockStateId,
};
use std::io;

struct FailedChunk {
    calls: usize,
}
impl BlockCollisionWorld for FailedChunk {
    type Error = io::Error;
    type Chunk = ();
    fn collision_chunk(&mut self, _: ChunkPos) -> io::Result<Option<()>> {
        self.calls += 1;
        Err(io::Error::other("missing test provider"))
    }
    fn collision_block(&mut self, _: (), _: BlockPos) -> io::Result<CollisionBlock> {
        unreachable!()
    }
    fn is_suffocating(&mut self, _: (), _: CollisionBlock, _: BlockPos) -> io::Result<bool> {
        unreachable!()
    }
    fn context_shape(
        &mut self,
        _: CollisionBlock,
        _: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        unreachable!()
    }
}
fn bounds() -> Aabb {
    Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0))
}

#[test]
fn budgets_include_skipped_corners_and_errors_fuse_iteration() {
    for budget in [0, 1] {
        let mut world = FailedChunk { calls: 0 };
        let mut scan = BlockCollisionScan::new(
            &mut world,
            CollisionContext::empty(false),
            bounds(),
            false,
            budget,
        )
        .unwrap();
        assert!(matches!(
            scan.next(),
            Some(Err(CollisionScanError::VisitBudget))
        ));
        assert!(scan.next().is_none());
        drop(scan);
        assert_eq!(world.calls, 0);
    }
    let mut world = FailedChunk { calls: 0 };
    let mut scan = BlockCollisionScan::new(
        &mut world,
        CollisionContext::empty(false),
        bounds(),
        false,
        1000,
    )
    .unwrap();
    assert!(matches!(
        scan.next(),
        Some(Err(CollisionScanError::World(_)))
    ));
    assert!(scan.next().is_none());
    drop(scan);
    assert_eq!(world.calls, 1);
}

#[test]
fn invalid_or_excessive_domains_fail_before_world_queries() {
    let mut world = FailedChunk { calls: 0 };
    for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let bounds = Aabb::new(Vec3::ZERO, Vec3::new(coordinate, 1.0, 1.0));
        assert!(matches!(
            BlockCollisionScan::new(
                &mut world,
                CollisionContext::empty(false),
                bounds,
                false,
                1000
            ),
            Err(CollisionScanError::InvalidBounds)
        ));
    }
    let bounds = Aabb::new(Vec3::ZERO, Vec3::new(100000.0, 100000.0, 100000.0));
    assert!(matches!(
        BlockCollisionScan::new(
            &mut world,
            CollisionContext::empty(false),
            bounds,
            false,
            1000
        ),
        Err(CollisionScanError::TooLarge)
    ));
    assert_eq!(world.calls, 0);
}

/// Records every cell the scan reads; every block passes the edge filters
/// and has an empty shape.
struct Recording {
    cells: Vec<BlockPos>,
    /// Sections (block y >> 4) whose blocks never reach out of their cell.
    contained: Vec<i32>,
}
impl BlockCollisionWorld for Recording {
    type Error = io::Error;
    type Chunk = ();
    fn collision_chunk(&mut self, _: ChunkPos) -> io::Result<Option<()>> {
        Ok(Some(()))
    }
    fn collision_block(&mut self, _: (), p: BlockPos) -> io::Result<CollisionBlock> {
        self.cells.push(p);
        Ok(CollisionBlock {
            state: BlockStateId(0),
            large_collision_shape: true,
            moving_piston: true,
            empty_shape: true,
        })
    }
    fn is_suffocating(&mut self, _: (), _: CollisionBlock, _: BlockPos) -> io::Result<bool> {
        unreachable!()
    }
    fn context_shape(
        &mut self,
        _: CollisionBlock,
        _: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        unreachable!()
    }
    fn may_reach_out(&mut self, _: (), section_y: i32) -> io::Result<bool> {
        Ok(!self.contained.contains(&section_y))
    }
}

/// The cells the scan reads for `bounds` (Cursor3D order: x fastest, then
/// y, then z, without the 8 corners), less the outer cells `contained` holds.
fn expected_cells(bounds: Aabb, contained: impl Fn(BlockPos) -> bool) -> Vec<BlockPos> {
    let (min, max) = (bounds.min_position(), bounds.max_position());
    let start = BlockPos::containing(Vec3::new(min.x - 1.0e-7, min.y - 1.0e-7, min.z - 1.0e-7))
        .offset(-1, -1, -1);
    let end = BlockPos::containing(Vec3::new(max.x + 1.0e-7, max.y + 1.0e-7, max.z + 1.0e-7))
        .offset(1, 1, 1);
    let sizes = [
        end.x - start.x + 1,
        end.y - start.y + 1,
        end.z - start.z + 1,
    ];
    let mut expected = Vec::new();
    for i in 0..sizes[0] * sizes[1] * sizes[2] {
        let r = [
            i % sizes[0],
            i / sizes[0] % sizes[1],
            i / sizes[0] / sizes[1],
        ];
        let edges = (0..3)
            .filter(|&a| r[a] == 0 || r[a] == sizes[a] - 1)
            .count();
        let p = start.offset(r[0], r[1], r[2]);
        if edges < 3 && (edges == 0 || !contained(p)) {
            expected.push(p);
        }
    }
    expected
}

#[test]
fn cells_follow_the_cursor_order_without_corners() {
    let boxes = [
        Aabb::new(Vec3::new(0.2, 64.0, 0.3), Vec3::new(0.8, 65.95, 0.9)),
        Aabb::new(Vec3::new(-17.5, -3.25, 31.0), Vec3::new(-15.0, -3.25, 33.5)),
        Aabb::new(Vec3::new(5.0, 5.0, 5.0), Vec3::new(5.0, 5.0, 5.0)),
        Aabb::new(Vec3::new(-0.3, 0.0, -0.3), Vec3::new(0.3, 0.9, 0.3)),
    ];
    for bounds in boxes {
        let mut world = Recording {
            cells: Vec::new(),
            contained: Vec::new(),
        };
        let hits = BlockCollisionScan::new(
            &mut world,
            CollisionContext::empty(false),
            bounds,
            false,
            1_000_000,
        )
        .unwrap()
        .count();
        assert_eq!(hits, 0);
        assert_eq!(world.cells, expected_cells(bounds, |_| false));
    }
}

/// Outer cells are read only in sections that may hold a block reaching
/// out of its cell; the box's own cells always are.
#[test]
fn outer_cells_of_contained_sections_are_not_read() {
    // A mob crossing from section 3 into section 4, and an item in section 0.
    let boxes = [
        (
            Aabb::new(Vec3::new(0.2, 63.5, 0.3), Vec3::new(0.8, 65.3, 0.9)),
            vec![3],
        ),
        (
            Aabb::new(Vec3::new(0.2, 63.5, 0.3), Vec3::new(0.8, 65.3, 0.9)),
            vec![3, 4],
        ),
        (
            Aabb::new(Vec3::new(4.4, 2.0, -7.6), Vec3::new(4.65, 2.25, -7.35)),
            vec![0],
        ),
        // Across a chunk's edge: the rest of a row is passed only up to it.
        (
            Aabb::new(Vec3::new(14.2, 70.5, -0.4), Vec3::new(17.8, 71.5, 0.4)),
            vec![4],
        ),
    ];
    for (bounds, contained) in boxes {
        let mut world = Recording {
            cells: Vec::new(),
            contained: contained.clone(),
        };
        let hits = BlockCollisionScan::new(
            &mut world,
            CollisionContext::empty(false),
            bounds,
            false,
            1_000_000,
        )
        .unwrap()
        .count();
        assert_eq!(hits, 0);
        assert_eq!(
            world.cells,
            expected_cells(bounds, |p| contained.contains(&(p.y >> 4)))
        );
        assert!(world.cells.len() < expected_cells(bounds, |_| false).len());
    }
}

/// Sections whose blocks never reach out, here those of even columns:
/// records the cells read and every section asked about.
struct ByColumn {
    cells: Vec<BlockPos>,
    asked: Vec<(ChunkPos, i32)>,
}
impl BlockCollisionWorld for ByColumn {
    type Error = io::Error;
    type Chunk = ChunkPos;
    fn collision_chunk(&mut self, p: ChunkPos) -> io::Result<Option<ChunkPos>> {
        Ok(Some(p))
    }
    fn collision_block(&mut self, _: ChunkPos, p: BlockPos) -> io::Result<CollisionBlock> {
        self.cells.push(p);
        Ok(CollisionBlock {
            state: BlockStateId(0),
            large_collision_shape: true,
            moving_piston: true,
            empty_shape: true,
        })
    }
    fn is_suffocating(&mut self, _: ChunkPos, _: CollisionBlock, _: BlockPos) -> io::Result<bool> {
        unreachable!()
    }
    fn context_shape(
        &mut self,
        _: CollisionBlock,
        _: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        unreachable!()
    }
    fn may_reach_out(&mut self, chunk: ChunkPos, section_y: i32) -> io::Result<bool> {
        self.asked.push((chunk, section_y));
        Ok(chunk.x & 1 != 0)
    }
}

/// A box across a chunk border and a section border goes back and forth
/// between columns on every row: each section is asked about once, and
/// each column's answer applies to its own cells only.
#[test]
fn each_section_is_asked_once_and_answers_for_its_column() {
    let boxes = [
        // Two columns, sections 3 and 4.
        Aabb::new(Vec3::new(14.6, 63.5, 3.2), Vec3::new(16.4, 65.3, 4.1)),
        // Four columns, one section.
        Aabb::new(Vec3::new(-1.3, 20.0, 15.2), Vec3::new(0.3, 21.8, 16.4)),
        // Four columns, two sections.
        Aabb::new(Vec3::new(-0.7, -0.5, -0.7), Vec3::new(0.7, 1.2, 0.7)),
    ];
    for bounds in boxes {
        let mut world = ByColumn {
            cells: Vec::new(),
            asked: Vec::new(),
        };
        let hits = BlockCollisionScan::new(
            &mut world,
            CollisionContext::empty(false),
            bounds,
            false,
            1_000_000,
        )
        .unwrap()
        .count();
        assert_eq!(hits, 0);
        assert_eq!(
            world.cells,
            expected_cells(bounds, |p| p.chunk().x & 1 == 0)
        );
        let mut distinct = world.asked.clone();
        distinct.sort_by_key(|&(c, y)| (c.x, c.z, y));
        distinct.dedup();
        assert_eq!(world.asked.len(), distinct.len(), "{:?}", world.asked);
        assert!(distinct.len() > 1);
    }
}

/// Passing outer cells at once still stops at the candidate budget, as
/// visiting them one by one does.
#[test]
fn passed_cells_count_toward_the_budget() {
    let bounds = Aabb::new(Vec3::new(0.2, 63.5, 0.3), Vec3::new(0.8, 65.3, 0.9));
    for budget in 0..80 {
        let scan = |contained: Vec<i32>| {
            let mut world = Recording {
                cells: Vec::new(),
                contained,
            };
            BlockCollisionScan::new(
                &mut world,
                CollisionContext::empty(false),
                bounds,
                false,
                budget,
            )
            .unwrap()
            .map(|hit| hit.map(|_| ()).map_err(|e| e.to_string()))
            .collect::<Vec<_>>()
        };
        assert_eq!(scan(vec![3, 4]), scan(Vec::new()), "budget {budget}");
    }
}

/// Every third column along x is missing (chunk x mod 3 = 2), so rows cross both loaded
/// neighbours and missing columns; sections from 4 up (block y 64 and above) hold only
/// air; below, a full block stands wherever x + y + z is a multiple of 7. With `hooks` it says so
/// (`section_is_empty`, `missing_chunks_stay_missing`); without, it is walked cell by cell.
struct Sparse {
    hooks: bool,
    queries: usize,
}
impl Sparse {
    fn solid(p: BlockPos) -> bool {
        p.y < 64 && (p.x + p.y + p.z).rem_euclid(7) == 0
    }
}
impl BlockCollisionWorld for Sparse {
    type Error = io::Error;
    type Chunk = ChunkPos;
    fn collision_chunk(&mut self, p: ChunkPos) -> io::Result<Option<ChunkPos>> {
        self.queries += 1;
        Ok((p.x.rem_euclid(3) != 2).then_some(p))
    }
    fn collision_block(&mut self, chunk: ChunkPos, p: BlockPos) -> io::Result<CollisionBlock> {
        assert_eq!(chunk, p.chunk());
        assert!(
            !self.hooks || p.y < 64,
            "a cell of an empty section was read: {p:?}"
        );
        self.queries += 1;
        let solid = Self::solid(p);
        Ok(CollisionBlock {
            state: BlockStateId(u32::from(solid)),
            large_collision_shape: false,
            moving_piston: false,
            empty_shape: !solid,
        })
    }
    fn is_suffocating(
        &mut self,
        _: ChunkPos,
        block: CollisionBlock,
        _: BlockPos,
    ) -> io::Result<bool> {
        Ok(block.state.0 == 1)
    }
    fn context_shape(
        &mut self,
        _: CollisionBlock,
        _: BlockPos,
        _: &CollisionContext<'_>,
    ) -> io::Result<VoxelShape> {
        Ok(VoxelShape::block())
    }
    fn section_is_empty(&mut self, _: ChunkPos, section_y: i32) -> io::Result<bool> {
        Ok(self.hooks && section_y >= 4)
    }
    fn missing_chunks_stay_missing(&self) -> bool {
        self.hooks
    }
}

/// What a scan of `bounds` gives: each hit's position, then the error that ended it, if any; and
/// the queries it made.
fn sparse_scan(
    hooks: bool,
    bounds: Aabb,
    budget: usize,
    suffocating: bool,
) -> (Vec<Result<BlockPos, String>>, usize) {
    let mut world = Sparse { hooks, queries: 0 };
    let hits = BlockCollisionScan::new(
        &mut world,
        CollisionContext::empty(false),
        bounds,
        suffocating,
        budget,
    )
    .unwrap()
    .map(|hit| hit.map(|h| h.position).map_err(|e| e.to_string()))
    .collect();
    (hits, world.queries)
}

/// Passing the cells of missing chunks and empty sections gives the same hits in the same order,
/// and ends at the budget where a walk cell by cell does.
#[test]
fn passing_missing_chunks_and_empty_sections_changes_nothing() {
    let boxes = [
        // Across loaded and missing columns, sections 3 and 4.
        Aabb::new(Vec3::new(-20.3, 58.2, -3.5), Vec3::new(37.6, 70.1, 18.4)),
        // Only empty sections, across eight columns.
        Aabb::new(Vec3::new(-16.0, 80.0, -16.0), Vec3::new(16.0, 95.5, 16.0)),
        // Thin, along x through many columns.
        Aabb::new(Vec3::new(-100.5, 62.4, 7.2), Vec3::new(140.5, 64.6, 7.8)),
        // Inside one missing column.
        Aabb::new(Vec3::new(33.0, 10.0, 0.5), Vec3::new(46.0, 12.0, 12.5)),
        // Across two loaded neighbours, sections 3 and 4: rows go back and forth between them.
        Aabb::new(Vec3::new(5.3, 60.2, 2.5), Vec3::new(27.6, 67.5, 9.4)),
    ];
    for bounds in boxes {
        for suffocating in [false, true] {
            let (walked, walked_queries) = sparse_scan(false, bounds, 1_000_000, suffocating);
            let (passed, passed_queries) = sparse_scan(true, bounds, 1_000_000, suffocating);
            assert_eq!(passed, walked, "{bounds:?}");
            assert!(passed_queries <= walked_queries);
        }
    }
    let (all, _) = sparse_scan(false, boxes[0], 1_000_000, false);
    assert!(all.len() > 10, "the first box has hits to keep in order");
    let cells = horizon_world::block_collision_scan::scan_cells(boxes[0]).unwrap();
    for budget in (0..cells + 2).step_by(97) {
        assert_eq!(
            sparse_scan(true, boxes[0], budget, false).0,
            sparse_scan(false, boxes[0], budget, false).0,
            "budget {budget}"
        );
    }
}

/// The sweep of a far move is mostly sky and chunks that are not loaded: the scan asks about a
/// row segment of them once, not about each cell.
#[test]
fn a_far_sweep_asks_once_per_row_segment() {
    // Flying height: every section it crosses is air or not loaded.
    let sweep = Aabb::new(
        Vec3::new(-300.0, 100.0, -300.0),
        Vec3::new(300.0, 102.0, 300.0),
    );
    let (walked, walked_queries) = sparse_scan(false, sweep, usize::MAX, false);
    let (passed, passed_queries) = sparse_scan(true, sweep, usize::MAX, false);
    assert_eq!(passed, walked);
    assert!(walked_queries > 1_000_000, "{walked_queries}");
    assert!(
        passed_queries * 10 < walked_queries,
        "{passed_queries} of {walked_queries}"
    );
}
