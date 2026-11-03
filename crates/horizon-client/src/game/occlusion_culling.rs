//! Shared, conservative block-occlusion queries for renderable geometry.
use super::{Game, Vertex};
use horizon_client::{
    blocks::{BlockFacts, RenderShape, StateFacts},
    chunk::Chunk,
    entity_culling::Culler as OcclusionCuller,
    gfx::Renderer,
    graphics::EntityCullingMode,
    mesher::SectionMesh,
    spans,
    streaming::Streaming,
    world::World,
};
use horizon_core::geometry::Aabb;
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::Instant,
};

type ChunkKey = (i32, i32);
type SectionKey = (i32, i32, i32);
const MAX_SNAPSHOT_CHUNKS: i64 = 4_096;
const MAX_OCCLUSION_RESULT_CACHE_ENTRIES: usize = 16_384;
const RESULT_CACHE_IDLE_FRAMES: u64 = 2;
const OCCLUSION_LOOKUP_BUDGET: usize = 8_192;
const OPACITY_GRID_SIZE: usize = 1_024;
const MAX_OCCLUSION_RAY_STEPS: usize = 512;
pub(super) const ASYNC_MOB_CULLING_ENABLE_MIN: usize = 64;
pub(super) const ASYNC_MOB_CULLING_DISABLE_BELOW: usize = 48;

fn async_mob_culling_ready(mobs: usize, worker_active: bool) -> bool {
    mobs >= if worker_active {
        ASYNC_MOB_CULLING_DISABLE_BELOW
    } else {
        ASYNC_MOB_CULLING_ENABLE_MIN
    }
}
const TIMING_SAMPLE_CAPACITY: usize = 256;
const PROOF_VALIDATION_TIMING_SAMPLE_STRIDE: u64 = 16;

/// A bounded ring of recent timings. Recording a sample is allocation-free;
/// percentile sorting only happens when a control client requests statistics.
struct TimingSamples {
    values: [u64; TIMING_SAMPLE_CAPACITY],
    len: usize,
    next: usize,
}

impl Default for TimingSamples {
    fn default() -> Self {
        Self {
            values: [0; TIMING_SAMPLE_CAPACITY],
            len: 0,
            next: 0,
        }
    }
}

impl TimingSamples {
    fn record(&mut self, nanoseconds: u64) {
        self.values[self.next] = nanoseconds;
        self.next = (self.next + 1) % TIMING_SAMPLE_CAPACITY;
        self.len = (self.len + 1).min(TIMING_SAMPLE_CAPACITY);
    }

    /// Nearest-rank p50 and p95 of the most recent samples.
    fn percentiles_ns(&self) -> (Option<u64>, Option<u64>) {
        if self.len == 0 {
            return (None, None);
        }
        let mut sorted = self.values;
        sorted[..self.len].sort_unstable();
        let p50 = (self.len * 50).div_ceil(100).max(1) - 1;
        let p95 = (self.len * 95).div_ceil(100).max(1) - 1;
        (Some(sorted[p50]), Some(sorted[p95]))
    }
}

/// Identifies which geometry stage consumed a cached answer. Entity AABBs
/// should usually avoid model generation; mesh bounds are the conservative
/// refinement for visible or uncached AABBs.
#[derive(Clone, Copy)]
pub(super) enum QuerySource {
    EntityAabb,
    EntityMesh,
}

#[derive(Clone, Copy)]
enum TimingPhase {
    ResultConsume,
    FrameSubmit,
    JobSend,
    ResultLookup,
}

#[derive(Clone, Copy)]
enum CacheMissReason {
    EmptySet,
    KeyMismatch,
    CameraMismatch,
    Invalidated,
}

#[derive(Clone, Copy)]
struct TimingStart {
    wall: Instant,
    thread_cpu: Option<std::time::Duration>,
}

#[derive(Default)]
struct PhaseTiming {
    wall_ns: u64,
    thread_cpu_ns: u64,
    sample_weight: u64,
    wall_samples: TimingSamples,
    thread_cpu_samples: TimingSamples,
}

impl PhaseTiming {
    fn record(&mut self, wall_ns: u64, thread_cpu_ns: Option<u64>, sample_weight: u64) {
        self.wall_ns = self
            .wall_ns
            .saturating_add(wall_ns.saturating_mul(sample_weight));
        self.sample_weight = sample_weight;
        self.wall_samples.record(wall_ns);
        if let Some(thread_cpu_ns) = thread_cpu_ns {
            self.thread_cpu_ns = self
                .thread_cpu_ns
                .saturating_add(thread_cpu_ns.saturating_mul(sample_weight));
            self.thread_cpu_samples.record(thread_cpu_ns);
        }
    }
}

struct EntityBoundsSample {
    frame: u64,
    entity_id: u32,
    camera: [u64; 3],
    min: [u64; 3],
    max: [u64; 3],
}

#[derive(Clone, Copy, Default)]
struct ProofValidationCounts {
    chunk_checks: u64,
    chunk_cache_hits: u64,
    section_checks: u64,
    section_cache_hits: u64,
}

#[derive(Clone, Copy)]
struct CachedChunkValidation {
    key: ChunkKey,
    allocation: usize,
    current: bool,
}

/// Proofs for nearby entities commonly name the same few occluding chunks and
/// sections. Keep their frame-local validity in sorted vectors so each world,
/// renderer, and streaming lookup is done once per unique key without hashing.
#[derive(Default)]
struct FrameProofValidationCache {
    chunks: Vec<CachedChunkValidation>,
    sections: Vec<(SectionKey, bool)>,
}

impl FrameProofValidationCache {
    fn clear(&mut self) {
        self.chunks.clear();
        self.sections.clear();
    }

    fn chunk_is_current(
        &mut self,
        world: &World,
        key: ChunkKey,
        expected: &Weak<Chunk>,
        counts: &mut ProofValidationCounts,
    ) -> bool {
        let allocation = expected.as_ptr() as usize;
        let cache_key = (key, allocation);
        match self
            .chunks
            .binary_search_by_key(&cache_key, |entry| (entry.key, entry.allocation))
        {
            Ok(index) => {
                counts.chunk_cache_hits = counts.chunk_cache_hits.saturating_add(1);
                self.chunks[index].current
            }
            Err(index) => {
                counts.chunk_checks = counts.chunk_checks.saturating_add(1);
                let current = world
                    .chunks
                    .get(&key)
                    .is_some_and(|chunk| std::ptr::eq(Arc::as_ptr(chunk), expected.as_ptr()));
                self.chunks.insert(
                    index,
                    CachedChunkValidation {
                        key,
                        allocation,
                        current,
                    },
                );
                current
            }
        }
    }

    fn section_is_current(
        &mut self,
        key: SectionKey,
        world: &World,
        renderer: Option<&Renderer>,
        streaming: &Streaming,
        pending_uploads: &[((i32, i32, i32), SectionMesh)],
        counts: &mut ProofValidationCounts,
    ) -> bool {
        match self.sections.binary_search_by_key(&key, |(key, _)| *key) {
            Ok(index) => {
                counts.section_cache_hits = counts.section_cache_hits.saturating_add(1);
                self.sections[index].1
            }
            Err(index) => {
                counts.section_checks = counts.section_checks.saturating_add(1);
                let current = renderer.is_some_and(|renderer| renderer.has_section(key))
                    && !world.dirty_sections.contains(&key)
                    && !streaming.is_meshing(key)
                    && !pending_uploads.iter().any(|(pending, _)| *pending == key);
                self.sections.insert(index, (key, current));
                current
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CullKey {
    camera: [u64; 3],
    min: [u64; 3],
    max: [u64; 3],
}

type QueryBoundsKey = ([u64; 3], [u64; 3]);

fn query_bounds_key(key: &CullKey) -> QueryBoundsKey {
    (key.min, key.max)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapshotKey {
    world_revision: u64,
    chunks_revision: u64,
    bottom_y: i32,
}

impl SnapshotKey {
    fn same_layout(self, other: Self) -> bool {
        self.chunks_revision == other.chunks_revision && self.bottom_y == other.bottom_y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChunkBounds {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

/// Sorted weak chunk references avoid holding gameplay chunks or forcing
/// copy-on-write. A worker keeps only its current chunk strongly referenced.
#[derive(Clone)]
struct OcclusionSnapshot {
    chunks: Vec<(ChunkKey, Weak<Chunk>)>,
    opaque_states: Arc<[bool]>,
}

impl OcclusionSnapshot {
    fn chunk(&self, key: ChunkKey) -> Option<&Weak<Chunk>> {
        self.chunks
            .binary_search_by_key(&key, |(key, _)| *key)
            .ok()
            .map(|index| &self.chunks[index].1)
    }

    fn refresh_chunks(&mut self, world: &World, bounds: ChunkBounds) {
        self.chunks.clear();
        for x in bounds.min_x..=bounds.max_x {
            for z in bounds.min_z..=bounds.max_z {
                if let Some(chunk) = world.chunks.get(&(x, z)) {
                    self.chunks.push(((x, z), Arc::downgrade(chunk)));
                }
            }
        }
    }

    fn chunks_match(&self, world: &World) -> bool {
        self.chunks.iter().all(|(key, expected)| {
            world
                .chunks
                .get(key)
                .is_some_and(|current| std::ptr::eq(expected.as_ptr(), Arc::as_ptr(current)))
        })
    }

    fn opaque(&self, pos: [i32; 3], last_chunk: &mut LastChunk) -> bool {
        let key = (pos[0] >> 4, pos[2] >> 4);
        let Some(chunk) = last_chunk.get(self, key) else {
            return false;
        };
        let state = chunk.block(pos[0] & 15, pos[1], pos[2] & 15);
        self.opaque_states
            .get(state.0 as usize)
            .copied()
            .unwrap_or(false)
    }
}

#[derive(Default)]
struct LastChunk {
    key: Option<ChunkKey>,
    value: Option<Arc<Chunk>>,
}

impl LastChunk {
    fn clear(&mut self) {
        self.key = None;
        self.value = None;
    }

    fn get(&mut self, snapshot: &OcclusionSnapshot, key: ChunkKey) -> Option<&Arc<Chunk>> {
        if self.key != Some(key) {
            self.key = Some(key);
            self.value = snapshot.chunk(key).and_then(Weak::upgrade);
        }
        self.value.as_ref()
    }
}

#[derive(Clone, Copy)]
struct OpacityCell {
    position: [i32; 3],
    opaque: bool,
}

/// A direct-mapped voxel cache shared by the rays and box checks in one job.
struct OpacityGrid {
    cells: [Option<OpacityCell>; OPACITY_GRID_SIZE],
}

impl Default for OpacityGrid {
    fn default() -> Self {
        Self {
            cells: [None; OPACITY_GRID_SIZE],
        }
    }
}

impl OpacityGrid {
    fn clear(&mut self) {
        self.cells.fill(None);
    }

    fn opaque(
        &mut self,
        snapshot: &OcclusionSnapshot,
        position: [i32; 3],
        last_chunk: &mut LastChunk,
    ) -> bool {
        let hash = (position[0] as u32).wrapping_mul(73_856_093)
            ^ (position[1] as u32).wrapping_mul(19_349_663)
            ^ (position[2] as u32).wrapping_mul(83_492_791);
        let slot = hash as usize & (OPACITY_GRID_SIZE - 1);
        if let Some(cell) = self.cells[slot]
            && cell.position == position
        {
            return cell.opaque;
        }
        let opaque = snapshot.opaque(position, last_chunk);
        self.cells[slot] = Some(OpacityCell { position, opaque });
        opaque
    }
}

fn bounds_have_clear_ray(
    camera: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
    snapshot: &OcclusionSnapshot,
    last_chunk: &mut LastChunk,
    opacity_grid: &mut OpacityGrid,
) -> bool {
    let center = std::array::from_fn(|axis| min[axis] + (max[axis] - min[axis]) * 0.5);
    ray_reaches_without_opaque(camera, center, snapshot, last_chunk, opacity_grid)
}

/// A clear sample only short-circuits culling; blocked samples still need the
/// conservative complete-plane test before the worker can hide geometry.
fn ray_reaches_without_opaque(
    from: [f64; 3],
    to: [f64; 3],
    snapshot: &OcclusionSnapshot,
    last_chunk: &mut LastChunk,
    opacity_grid: &mut OpacityGrid,
) -> bool {
    if from.into_iter().chain(to).any(|value| !value.is_finite()) {
        return true;
    }
    let [Some(x), Some(y), Some(z)] = from.map(snapshot_cell) else {
        return true;
    };
    let mut cell = [x, y, z];
    let [Some(x), Some(y), Some(z)] = to.map(snapshot_cell) else {
        return true;
    };
    let target = [x, y, z];
    if opacity_grid.opaque(snapshot, cell, last_chunk) {
        return false;
    }
    if cell == target {
        return true;
    }

    let delta: [f64; 3] = std::array::from_fn(|axis| to[axis] - from[axis]);
    let mut step = [0_i32; 3];
    let mut t_max = [f64::INFINITY; 3];
    let mut t_delta = [f64::INFINITY; 3];
    for axis in 0..3 {
        if delta[axis] > 0.0 {
            step[axis] = 1;
            t_max[axis] = (f64::from(cell[axis]) + 1.0 - from[axis]) / delta[axis];
            t_delta[axis] = 1.0 / delta[axis];
        } else if delta[axis] < 0.0 {
            step[axis] = -1;
            t_max[axis] = (from[axis] - f64::from(cell[axis])) / -delta[axis];
            t_delta[axis] = -1.0 / delta[axis];
        }
    }

    for _ in 0..MAX_OCCLUSION_RAY_STEPS {
        let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
            0
        } else if t_max[1] <= t_max[2] {
            1
        } else {
            2
        };
        if t_max[axis] > 1.0 {
            return true;
        }
        let Some(next) = cell[axis].checked_add(step[axis]) else {
            return true;
        };
        cell[axis] = next;
        if opacity_grid.opaque(snapshot, cell, last_chunk) {
            return false;
        }
        if cell == target {
            return true;
        }
        t_max[axis] += t_delta[axis];
    }
    // An overlong ray is not a proof of occlusion, so keep geometry visible.
    true
}

#[derive(Clone)]
pub(super) struct OcclusionProof {
    sections: Vec<SectionKey>,
    chunks: Vec<(ChunkKey, Weak<Chunk>)>,
}

struct CullResults {
    stamp: SnapshotKey,
    camera: [u64; 3],
    generation: u64,
    cancelled: bool,
    hidden: Vec<(CullKey, OcclusionProof)>,
    visible: Vec<CullKey>,
    scratch_queries: Vec<CullKey>,
    worker_ns: u64,
    hidden_entities: usize,
    visible_entities: usize,
    deferred_entities: usize,
}

struct CachedQueryResult {
    key: CullKey,
    stamp: SnapshotKey,
    camera_cell: Option<[i32; 3]>,
    proof: Option<OcclusionProof>,
    last_used_frame: Cell<u64>,
}

#[derive(Default)]
struct AsyncCullingStats {
    frames: u64,
    entity_queries: u64,
    entity_cache_hits: u64,
    entity_cache_misses: u64,
    entity_aabb_cache_hits: u64,
    entity_aabb_cache_misses: u64,
    entity_mesh_cache_hits: u64,
    entity_mesh_cache_misses: u64,
    result_cache_evictions: u64,
    result_cache_capacity_evictions: u64,
    result_cache_idle_evictions: u64,
    result_cache_invalidation_replacements: u64,
    cache_miss_empty_set: u64,
    cache_miss_key_mismatch: u64,
    cache_miss_camera_mismatch: u64,
    cache_miss_invalidated: u64,
    entity_vertices_scanned: u64,
    entity_vertices_culled: u64,
    entity_bytes_culled: u64,
    entities_culled: u64,
    queued_queries: u64,
    queries_dropped_while_pending: u64,
    worker_jobs: u64,
    worker_jobs_completed: u64,
    worker_job_query_count_samples: TimingSamples,
    worker_job_queries: u64,
    job_send_full: u64,
    job_send_disconnected: u64,
    stale_camera_jobs: u64,
    stale_world_jobs: u64,
    worker_cancelled_jobs: u64,
    worker_ns: u64,
    worker_job_samples: TimingSamples,
    bounds_scan_ns: u64,
    bounds_scan_samples: TimingSamples,
    worker_hidden_entities: u64,
    worker_visible_entities: u64,
    worker_deferred_entities: u64,
    entity_budget_exhausted_jobs: u64,
    result_consume_timing: PhaseTiming,
    frame_submit_timing: PhaseTiming,
    job_send_timing: PhaseTiming,
    result_lookup_timing: PhaseTiming,
    proof_validation_timing: PhaseTiming,
    proof_validation_counts: ProofValidationCounts,
    result_lookup_calls: u64,
    sampled_entity_id: Option<u32>,
    sampled_entity_last_frame: u64,
    entity_bounds_samples: Vec<EntityBoundsSample>,
    previous_world_revision: Option<u64>,
    previous_chunks_revision: Option<u64>,
    world_revision_changes: u64,
    chunks_revision_changes: u64,
}

struct CullJob {
    stamp: SnapshotKey,
    snapshot_epoch: u64,
    generation: u64,
    snapshot: Arc<OcclusionSnapshot>,
    queries: Vec<CullKey>,
    clear_visible: Vec<CullKey>,
    hidden: Vec<(CullKey, OcclusionProof)>,
    rays_filtered: bool,
}

/// Scratch owned by the ray stage. The query vectors cross the channel with a
/// job, while these buffers retain their capacity for the next job. Keeping
/// the direct-mapped grid between unchanged snapshots also preserves spatial
/// coherence across frames without retaining a chunk allocation.
#[derive(Default)]
struct RayWorkerScratch {
    input: Vec<CullKey>,
    blocked: Vec<CullKey>,
    clear_visible: Vec<CullKey>,
    last_chunk: LastChunk,
    opacity_grid: OpacityGrid,
    snapshot_epoch: Option<u64>,
}

impl RayWorkerScratch {
    fn prepare(&mut self, snapshot_epoch: u64) {
        if self.snapshot_epoch != Some(snapshot_epoch) {
            self.opacity_grid.clear();
            self.snapshot_epoch = Some(snapshot_epoch);
        }
        self.last_chunk.clear();
        self.blocked.clear();
        self.clear_visible.clear();
    }
}

/// Scratch owned by the plane stage. The covered-cell buffer is the only
/// temporary collection needed while constructing an occlusion proof and is
/// reused for every entity and every job.
#[derive(Default)]
struct PlaneWorkerScratch {
    covered: Vec<[i32; 3]>,
    last_chunk: LastChunk,
    opacity_grid: OpacityGrid,
    snapshot_epoch: Option<u64>,
}

impl PlaneWorkerScratch {
    fn prepare(&mut self, snapshot_epoch: u64) {
        if self.snapshot_epoch != Some(snapshot_epoch) {
            self.opacity_grid.clear();
            self.snapshot_epoch = Some(snapshot_epoch);
        }
        self.last_chunk.clear();
        self.covered.clear();
    }
}

/// Non-blocking world occlusion for entity rendering.
/// Hidden proofs require byte-identical camera and bounds, and their chunks and
/// rendered sections are revalidated. Fail-open visible results survive camera
/// movement within one block because they can only reduce culling.
pub(super) struct AsyncOcclusionCuller {
    jobs: SyncSender<CullJob>,
    _ray_thread: JoinHandle<()>,
    results_rx: Receiver<CullResults>,
    _thread: JoinHandle<()>,
    job_generation: Arc<AtomicU64>,
    snapshot: Option<(SnapshotKey, ChunkBounds, Arc<OcclusionSnapshot>)>,
    snapshot_epoch: u64,
    opaque_states: Option<Arc<[bool]>>,
    queries: RefCell<Vec<CullKey>>,
    query_results: Vec<CachedQueryResult>,
    cache_updates: Vec<CachedQueryResult>,
    cache_merge: Vec<CachedQueryResult>,
    recycled_visible: Vec<CullKey>,
    recycled_hidden: Vec<(CullKey, OcclusionProof)>,
    stats: RefCell<AsyncCullingStats>,
    proof_validation_cache: RefCell<FrameProofValidationCache>,
    measure_bounds_timing: Cell<bool>,
    query_cache_stamp: Option<SnapshotKey>,
    query_cache_camera_cell: Option<[i32; 3]>,
    camera: [f64; 3],
    stamp: SnapshotKey,
    pending: bool,
    frame: u64,
    proof_validation_sample_counter: Cell<u64>,
}

impl AsyncOcclusionCuller {
    fn timing_start(&self) -> Option<TimingStart> {
        self.measure_bounds_timing.get().then(|| TimingStart {
            wall: Instant::now(),
            thread_cpu: spans::thread_cpu(),
        })
    }

    fn record_timing(&self, phase: TimingPhase, started: Option<TimingStart>) {
        let Some(started) = started else {
            return;
        };
        let wall_ns = started.wall.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let thread_cpu_ns = spans::thread_cpu()
            .zip(started.thread_cpu)
            .map(|(end, start)| {
                end.saturating_sub(start)
                    .as_nanos()
                    .min(u128::from(u64::MAX)) as u64
            });
        let mut stats = self.stats.borrow_mut();
        let timing = match phase {
            TimingPhase::ResultConsume => &mut stats.result_consume_timing,
            TimingPhase::FrameSubmit => &mut stats.frame_submit_timing,
            TimingPhase::JobSend => &mut stats.job_send_timing,
            TimingPhase::ResultLookup => &mut stats.result_lookup_timing,
        };
        timing.record(wall_ns, thread_cpu_ns, 1);
    }

    fn timing_value(timing: &PhaseTiming) -> serde_json::Value {
        let (wall_p50_ns, wall_p95_ns) = timing.wall_samples.percentiles_ns();
        let (cpu_p50_ns, cpu_p95_ns) = timing.thread_cpu_samples.percentiles_ns();
        serde_json::json!({
            "wall_ms_total": timing.wall_ns as f64 / 1_000_000.0,
            "wall_ns_p50_recent": wall_p50_ns,
            "wall_ns_p95_recent": wall_p95_ns,
            "wall_samples": timing.wall_samples.len,
            "thread_cpu_ms_total": timing.thread_cpu_ns as f64 / 1_000_000.0,
            "thread_cpu_ns_p50_recent": cpu_p50_ns,
            "thread_cpu_ns_p95_recent": cpu_p95_ns,
            "thread_cpu_samples": timing.thread_cpu_samples.len,
            "sample_weight": timing.sample_weight.max(1),
            "totals_are_sampled_estimates": timing.sample_weight > 1,
        })
    }

    pub(super) fn new() -> std::io::Result<Self> {
        let (job_tx, job_rx) = mpsc::sync_channel::<CullJob>(1);
        let (plane_tx, plane_rx) = mpsc::sync_channel::<CullJob>(1);
        let (result_tx, result_rx) = mpsc::channel::<CullResults>();
        let job_generation = Arc::new(AtomicU64::new(0));
        let worker_generation = Arc::clone(&job_generation);
        let plane_generation = Arc::clone(&job_generation);
        let thread = std::thread::Builder::new()
            .name("world-occlusion-culling".into())
            .spawn(move || {
                let mut scratch = PlaneWorkerScratch::default();
                while let Ok(job) = plane_rx.recv() {
                    if result_tx
                        .send(cull_job(job, &plane_generation, &mut scratch))
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        let ray_plane_tx = plane_tx.clone();
        let ray_thread = std::thread::Builder::new()
            .name("entity-ray-cull".into())
            .spawn(move || {
                let mut scratch = RayWorkerScratch::default();
                while let Ok(mut job) = job_rx.recv() {
                    filter_rays(&mut job, &worker_generation, &mut scratch);
                    if ray_plane_tx.send(job).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            jobs: job_tx,
            _ray_thread: ray_thread,
            results_rx: result_rx,
            _thread: thread,
            job_generation,
            snapshot: None,
            snapshot_epoch: 0,
            opaque_states: None,
            queries: RefCell::new(Vec::new()),
            query_results: Vec::new(),
            cache_updates: Vec::new(),
            cache_merge: Vec::new(),
            recycled_visible: Vec::new(),
            recycled_hidden: Vec::new(),
            stats: RefCell::new(AsyncCullingStats::default()),
            proof_validation_cache: RefCell::new(FrameProofValidationCache::default()),
            measure_bounds_timing: Cell::new(
                std::env::var("HORIZON_OCCLUSION_PROFILE").is_ok_and(|v| v == "1"),
            ),
            query_cache_stamp: None,
            query_cache_camera_cell: None,
            camera: [0.0; 3],
            stamp: SnapshotKey {
                world_revision: 0,
                chunks_revision: 0,
                bottom_y: 0,
            },
            pending: false,
            frame: 0,
            proof_validation_sample_counter: Cell::new(0),
        })
    }

    pub(super) fn begin_frame(&mut self, camera: glam::DVec3, world: &World) {
        let started = self.timing_start();
        self.begin_frame_inner(camera, world);
        self.record_timing(TimingPhase::ResultConsume, started);
    }

    fn begin_frame_inner(&mut self, camera: glam::DVec3, world: &World) {
        self.frame = self.frame.saturating_add(1);
        self.proof_validation_cache.get_mut().clear();
        self.camera = camera.to_array();
        self.stamp = SnapshotKey {
            world_revision: world.revision,
            chunks_revision: world.chunks.membership_revision(),
            bottom_y: world.bottom_y(),
        };
        {
            let mut stats = self.stats.borrow_mut();
            stats.frames = stats.frames.saturating_add(1);
            if stats
                .previous_world_revision
                .is_some_and(|revision| revision != self.stamp.world_revision)
            {
                stats.world_revision_changes = stats.world_revision_changes.saturating_add(1);
            }
            if stats
                .previous_chunks_revision
                .is_some_and(|revision| revision != self.stamp.chunks_revision)
            {
                stats.chunks_revision_changes = stats.chunks_revision_changes.saturating_add(1);
            }
            stats.previous_world_revision = Some(self.stamp.world_revision);
            stats.previous_chunks_revision = Some(self.stamp.chunks_revision);
        }
        let camera_key = self.camera.map(f64::to_bits);
        let current_camera_cell = camera_cell(self.camera);
        let cache_initialized = self.query_cache_stamp.is_some();
        let layout_changed = self
            .query_cache_stamp
            .is_some_and(|stamp| !stamp.same_layout(self.stamp));
        let camera_cell_changed =
            cache_initialized && self.query_cache_camera_cell != current_camera_cell;
        if layout_changed || camera_cell_changed {
            // Invalidate in-flight work in O(1). Cached entries carry their
            // own stamp and are rejected lazily by query_result(). Individual
            // block edits are handled by each hidden proof's chunk references.
            self.job_generation.fetch_add(1, Ordering::Relaxed);
        }
        self.query_cache_stamp = Some(self.stamp);
        self.query_cache_camera_cell = current_camera_cell;
        while let Ok(mut results) = self.results_rx.try_recv() {
            self.pending = false;
            {
                let mut stats = self.stats.borrow_mut();
                stats.worker_jobs_completed = stats.worker_jobs_completed.saturating_add(1);
                stats.worker_ns = stats.worker_ns.saturating_add(results.worker_ns);
                stats.worker_job_samples.record(results.worker_ns);
                stats.worker_hidden_entities = stats
                    .worker_hidden_entities
                    .saturating_add(results.hidden_entities as u64);
                stats.worker_visible_entities = stats
                    .worker_visible_entities
                    .saturating_add(results.visible_entities as u64);
                stats.worker_deferred_entities = stats
                    .worker_deferred_entities
                    .saturating_add(results.deferred_entities as u64);
                stats.entity_budget_exhausted_jobs = stats
                    .entity_budget_exhausted_jobs
                    .saturating_add(u64::from(results.deferred_entities != 0));
            }
            let generation = self.job_generation.load(Ordering::Relaxed);
            if results.stamp.same_layout(self.stamp)
                && results.generation == generation
                && !results.cancelled
            {
                if results.stamp.world_revision != self.stamp.world_revision {
                    let mut stats = self.stats.borrow_mut();
                    stats.stale_world_jobs = stats.stale_world_jobs.saturating_add(1);
                }
                if results.camera != camera_key {
                    let mut stats = self.stats.borrow_mut();
                    stats.stale_camera_jobs = stats.stale_camera_jobs.saturating_add(1);
                }
                let same_camera_cell =
                    camera_cell(results.camera.map(f64::from_bits)) == current_camera_cell;
                if results.camera == camera_key || same_camera_cell {
                    self.cache_query_results(&mut results);
                } else {
                    // A stale-camera visible answer is fail-open. Never carry a
                    // hidden proof across viewpoints.
                    self.cache_visible_results(&mut results);
                }
                let scratch = std::mem::take(&mut results.scratch_queries);
                if scratch.capacity() > self.queries.get_mut().capacity() {
                    *self.queries.get_mut() = scratch;
                }
            } else {
                let mut stats = self.stats.borrow_mut();
                stats.worker_cancelled_jobs = stats
                    .worker_cancelled_jobs
                    .saturating_add(u64::from(results.cancelled));
                stats.stale_camera_jobs = stats.stale_camera_jobs.saturating_add(u64::from(
                    results.camera != camera_key || results.generation != generation,
                ));
                stats.stale_world_jobs = stats
                    .stale_world_jobs
                    .saturating_add(u64::from(!results.stamp.same_layout(self.stamp)));
            }
            let visible = std::mem::take(&mut results.visible);
            if visible.capacity() > self.recycled_visible.capacity() {
                self.recycled_visible = visible;
            }
            let hidden = std::mem::take(&mut results.hidden);
            if hidden.capacity() > self.recycled_hidden.capacity() {
                self.recycled_hidden = hidden;
            }
        }
        self.queries.get_mut().clear();
    }

    pub(super) fn query(&self, vertices: &[Vertex], start: usize) -> Option<CullKey> {
        let scan_started = self.measure_bounds_timing.get().then(Instant::now);
        let bounds = OcclusionCuller::bounds(self.camera, vertices, start);
        let mut stats = self.stats.borrow_mut();
        stats.entity_queries = stats.entity_queries.saturating_add(1);
        stats.entity_vertices_scanned = stats
            .entity_vertices_scanned
            .saturating_add(vertices.len().saturating_sub(start) as u64);
        if let Some(started) = scan_started {
            let scan_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            stats.bounds_scan_ns = stats.bounds_scan_ns.saturating_add(scan_ns);
            stats.bounds_scan_samples.record(scan_ns);
        }
        drop(stats);
        let (min, max) = bounds?;
        self.query_bounds(min, max)
    }

    pub(super) fn query_entity_bounds(&self, bounds: Aabb) -> Option<CullKey> {
        let (min, max) = OcclusionCuller::aabb_bounds(bounds)?;
        let mut stats = self.stats.borrow_mut();
        stats.entity_queries = stats.entity_queries.saturating_add(1);
        drop(stats);
        self.query_bounds(min, max)
    }

    fn query_bounds(&self, min: [f64; 3], max: [f64; 3]) -> Option<CullKey> {
        if (0..3)
            .any(|axis| !min[axis].is_finite() || !max[axis].is_finite() || min[axis] > max[axis])
        {
            return None;
        }
        Some(CullKey {
            camera: self.camera.map(f64::to_bits),
            min: min.map(f64::to_bits),
            max: max.map(f64::to_bits),
        })
    }

    pub(super) fn query_result(
        &self,
        key: &CullKey,
        source: QuerySource,
    ) -> Option<(bool, Option<&OcclusionProof>)> {
        let sample_lookup = {
            let mut stats = self.stats.borrow_mut();
            stats.result_lookup_calls = stats.result_lookup_calls.saturating_add(1);
            self.measure_bounds_timing.get() && stats.result_lookup_calls.is_multiple_of(16)
        };
        let timing_start = sample_lookup.then(|| self.timing_start()).flatten();
        let entry = self
            .query_results
            .binary_search_by_key(&query_bounds_key(key), |entry| query_bounds_key(&entry.key))
            .ok()
            .map(|index| &self.query_results[index]);
        let result = entry.and_then(|entry| {
            let camera_matches = entry.key.camera == key.camera
                || (entry.proof.is_some()
                    && camera_cell(entry.key.camera.map(f64::from_bits))
                        == self.query_cache_camera_cell);
            let exact = camera_matches
                && query_bounds_key(&entry.key) == query_bounds_key(key)
                && entry.stamp.same_layout(self.stamp)
                && (entry.proof.is_some()
                    || entry.stamp.world_revision == self.stamp.world_revision);
            let reusable_visible = entry.proof.is_none()
                && entry.stamp == self.stamp
                && self
                    .query_cache_camera_cell
                    .is_some_and(|cell| entry.camera_cell == Some(cell));
            (exact || reusable_visible).then(|| {
                entry.last_used_frame.set(self.frame);
                (entry.proof.is_some(), entry.proof.as_ref())
            })
        });
        let miss_reason = result.is_none().then(|| {
            cache_miss_reason(
                entry,
                key,
                self.stamp,
                self.query_cache_camera_cell,
                self.query_results.is_empty(),
            )
        });
        let mut stats = self.stats.borrow_mut();
        match result.is_some() {
            true => stats.entity_cache_hits = stats.entity_cache_hits.saturating_add(1),
            false => stats.entity_cache_misses = stats.entity_cache_misses.saturating_add(1),
        }
        match (source, result.is_some()) {
            (QuerySource::EntityAabb, true) => {
                stats.entity_aabb_cache_hits = stats.entity_aabb_cache_hits.saturating_add(1);
            }
            (QuerySource::EntityAabb, false) => {
                stats.entity_aabb_cache_misses = stats.entity_aabb_cache_misses.saturating_add(1);
            }
            (QuerySource::EntityMesh, true) => {
                stats.entity_mesh_cache_hits = stats.entity_mesh_cache_hits.saturating_add(1);
            }
            (QuerySource::EntityMesh, false) => {
                stats.entity_mesh_cache_misses = stats.entity_mesh_cache_misses.saturating_add(1);
            }
        }
        match miss_reason {
            Some(CacheMissReason::EmptySet) => {
                stats.cache_miss_empty_set = stats.cache_miss_empty_set.saturating_add(1);
            }
            Some(CacheMissReason::KeyMismatch) => {
                stats.cache_miss_key_mismatch = stats.cache_miss_key_mismatch.saturating_add(1);
            }
            Some(CacheMissReason::CameraMismatch) => {
                stats.cache_miss_camera_mismatch =
                    stats.cache_miss_camera_mismatch.saturating_add(1);
            }
            Some(CacheMissReason::Invalidated) => {
                stats.cache_miss_invalidated = stats.cache_miss_invalidated.saturating_add(1);
            }
            None => {}
        }
        drop(stats);
        if sample_lookup {
            self.record_timing(TimingPhase::ResultLookup, timing_start);
        }
        result
    }

    pub(super) fn sample_entity_aabb(&self, entity_id: u32, key: &CullKey) {
        if !self.measure_bounds_timing.get() {
            return;
        }
        let mut stats = self.stats.borrow_mut();
        if stats.entity_bounds_samples.len() >= 10 {
            return;
        }
        let sampled_id = *stats.sampled_entity_id.get_or_insert(entity_id);
        if sampled_id != entity_id || stats.sampled_entity_last_frame == stats.frames {
            return;
        }
        let frame = stats.frames;
        stats.sampled_entity_last_frame = frame;
        stats.entity_bounds_samples.push(EntityBoundsSample {
            frame,
            entity_id,
            camera: key.camera,
            min: key.min,
            max: key.max,
        });
    }

    pub(super) fn proof_is_current(
        &self,
        world: &World,
        renderer: Option<&Renderer>,
        streaming: &Streaming,
        pending_uploads: &[((i32, i32, i32), SectionMesh)],
        proof: &OcclusionProof,
    ) -> bool {
        let started = if self.measure_bounds_timing.get() {
            let sample = self.proof_validation_sample_counter.get().wrapping_add(1);
            self.proof_validation_sample_counter.set(sample);
            sample
                .is_multiple_of(PROOF_VALIDATION_TIMING_SAMPLE_STRIDE)
                .then(|| self.timing_start())
                .flatten()
        } else {
            None
        };
        let (current, counts) = proof_is_current(
            world,
            renderer,
            streaming,
            pending_uploads,
            proof,
            &mut self.proof_validation_cache.borrow_mut(),
        );
        if self.measure_bounds_timing.get() {
            let mut stats = self.stats.borrow_mut();
            stats.proof_validation_counts.chunk_checks = stats
                .proof_validation_counts
                .chunk_checks
                .saturating_add(counts.chunk_checks);
            stats.proof_validation_counts.chunk_cache_hits = stats
                .proof_validation_counts
                .chunk_cache_hits
                .saturating_add(counts.chunk_cache_hits);
            stats.proof_validation_counts.section_checks = stats
                .proof_validation_counts
                .section_checks
                .saturating_add(counts.section_checks);
            stats.proof_validation_counts.section_cache_hits = stats
                .proof_validation_counts
                .section_cache_hits
                .saturating_add(counts.section_cache_hits);
            if let Some(started) = started {
                let wall_ns = started.wall.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                let thread_cpu_ns =
                    spans::thread_cpu()
                        .zip(started.thread_cpu)
                        .map(|(end, start)| {
                            end.saturating_sub(start)
                                .as_nanos()
                                .min(u128::from(u64::MAX)) as u64
                        });
                stats.proof_validation_timing.record(
                    wall_ns,
                    thread_cpu_ns,
                    PROOF_VALIDATION_TIMING_SAMPLE_STRIDE,
                );
            }
        }
        current
    }

    fn cache_query_results(&mut self, results: &mut CullResults) {
        self.cache_results(results, true);
    }

    fn cache_visible_results(&mut self, results: &mut CullResults) {
        self.cache_results(results, false);
    }

    fn cache_results(&mut self, results: &mut CullResults, cache_hidden: bool) {
        self.cache_updates.clear();
        let result_camera_cell = camera_cell(results.camera.map(f64::from_bits));

        // Visible results are fail-open but can hide later culling opportunities
        // after a block edit, so retain them only for the world revision tested.
        if results.stamp.world_revision == self.stamp.world_revision {
            self.cache_updates
                .extend(results.visible.drain(..).map(|key| CachedQueryResult {
                    key,
                    stamp: results.stamp,
                    camera_cell: result_camera_cell,
                    proof: None,
                    last_used_frame: Cell::new(self.frame),
                }));
        } else {
            results.visible.clear();
        }

        if cache_hidden {
            for (key, proof) in results.hidden.drain(..) {
                let has_newer_hidden_proof = results.stamp.world_revision
                    != self.stamp.world_revision
                    && self
                        .query_results
                        .binary_search_by_key(&query_bounds_key(&key), |entry| {
                            query_bounds_key(&entry.key)
                        })
                        .ok()
                        .is_some_and(|index| {
                            let entry = &self.query_results[index];
                            entry.key == key
                                && entry.proof.is_some()
                                && entry.stamp.same_layout(self.stamp)
                                && entry.stamp.world_revision == self.stamp.world_revision
                        });
                if has_newer_hidden_proof {
                    continue;
                }
                self.cache_updates.push(CachedQueryResult {
                    key,
                    stamp: results.stamp,
                    camera_cell: result_camera_cell,
                    proof: Some(proof),
                    last_used_frame: Cell::new(self.frame),
                });
            }
        } else {
            results.hidden.clear();
        }

        if self.cache_updates.is_empty() {
            return;
        }
        self.cache_updates
            .sort_unstable_by_key(|entry| query_bounds_key(&entry.key));

        let existing_len = self.query_results.len();
        let updates_len = self.cache_updates.len();
        let mut existing = std::mem::take(&mut self.query_results);
        let mut updates = std::mem::take(&mut self.cache_updates);
        let mut merged = std::mem::take(&mut self.cache_merge);
        merged.clear();
        merged.reserve(
            existing_len
                .saturating_add(updates_len)
                .min(MAX_OCCLUSION_RESULT_CACHE_ENTRIES),
        );

        let mut existing_iter = existing.drain(..).peekable();
        let mut updates_iter = updates.drain(..).peekable();
        let mut invalidation_replacements = 0_u64;
        let mut capacity_evictions = 0_u64;
        let mut idle_evictions = 0_u64;
        while existing_iter.peek().is_some() || updates_iter.peek().is_some() {
            let entry = match (existing_iter.peek(), updates_iter.peek()) {
                (Some(old), Some(new)) => {
                    match query_bounds_key(&old.key).cmp(&query_bounds_key(&new.key)) {
                        std::cmp::Ordering::Less => existing_iter.next().unwrap(),
                        std::cmp::Ordering::Greater => updates_iter.next().unwrap(),
                        std::cmp::Ordering::Equal => {
                            let old = existing_iter.next().unwrap();
                            let new = updates_iter.next().unwrap();
                            let invalidated = !old.stamp.same_layout(self.stamp)
                                || (old.proof.is_none()
                                    && (old.stamp.world_revision != self.stamp.world_revision
                                        || old.camera_cell != self.query_cache_camera_cell));
                            invalidation_replacements =
                                invalidation_replacements.saturating_add(u64::from(invalidated));
                            new
                        }
                    }
                }
                (Some(_), None) => existing_iter.next().unwrap(),
                (None, Some(_)) => updates_iter.next().unwrap(),
                (None, None) => unreachable!(),
            };
            // Animated mesh extrema often never repeat. Drop their unused
            // answers during this existing merge, while frequently queried
            // AABBs and mesh bounds retain their entries without extra scans.
            if self.frame.saturating_sub(entry.last_used_frame.get()) > RESULT_CACHE_IDLE_FRAMES {
                idle_evictions = idle_evictions.saturating_add(1);
            } else if merged.len() < MAX_OCCLUSION_RESULT_CACHE_ENTRIES {
                merged.push(entry);
            } else {
                capacity_evictions = capacity_evictions.saturating_add(1);
            }
        }
        drop(existing_iter);
        drop(updates_iter);

        let mut stats = self.stats.borrow_mut();
        stats.result_cache_evictions = stats
            .result_cache_evictions
            .saturating_add(capacity_evictions)
            .saturating_add(idle_evictions);
        stats.result_cache_idle_evictions = stats
            .result_cache_idle_evictions
            .saturating_add(idle_evictions);
        stats.result_cache_capacity_evictions = stats
            .result_cache_capacity_evictions
            .saturating_add(capacity_evictions);
        stats.result_cache_invalidation_replacements = stats
            .result_cache_invalidation_replacements
            .saturating_add(invalidation_replacements);
        drop(stats);

        self.query_results = merged;
        self.cache_updates = updates;
        self.cache_merge = existing;
    }

    pub(super) fn queue(&self, key: CullKey) {
        let mut stats = self.stats.borrow_mut();
        stats.queued_queries = stats.queued_queries.saturating_add(1);
        drop(stats);
        self.queries.borrow_mut().push(key);
    }

    pub(super) fn finish_frame(&mut self, world: &World, facts: &BlockFacts) {
        let started = self.timing_start();
        self.finish_frame_inner(world, facts);
        self.record_timing(TimingPhase::FrameSubmit, started);
    }

    fn finish_frame_inner(&mut self, world: &World, facts: &BlockFacts) {
        let queries = self.queries.get_mut();
        if self.pending {
            let mut stats = self.stats.borrow_mut();
            stats.queries_dropped_while_pending = stats
                .queries_dropped_while_pending
                .saturating_add(queries.len() as u64);
            drop(stats);
            queries.clear();
            return;
        }
        if queries.is_empty() {
            return;
        }
        // Coalesce duplicates before spending the worker's bounded lookup budget.
        queries.sort_unstable();
        queries.dedup();
        queries.sort_by(|a, b| {
            query_priority(b)
                .total_cmp(&query_priority(a))
                .then_with(|| a.cmp(b))
        });
        let Some(bounds) = snapshot_bounds(self.camera, queries) else {
            queries.clear();
            return;
        };
        let opaque_states = Arc::clone(
            self.opaque_states
                .get_or_insert_with(|| facts.states.iter().map(opaque).collect::<Vec<_>>().into()),
        );
        let snapshot = match &mut self.snapshot {
            Some((stamp, cached_bounds, snapshot)) => {
                if !stamp.same_layout(self.stamp)
                    || *cached_bounds != bounds
                    || !snapshot.chunks_match(world)
                {
                    self.snapshot_epoch = self.snapshot_epoch.wrapping_add(1);
                    let snapshot = Arc::make_mut(snapshot);
                    snapshot.opaque_states = Arc::clone(&opaque_states);
                    snapshot.refresh_chunks(world, bounds);
                    *stamp = self.stamp;
                    *cached_bounds = bounds;
                }
                Arc::clone(snapshot)
            }
            None => {
                self.snapshot_epoch = self.snapshot_epoch.wrapping_add(1);
                let mut snapshot = OcclusionSnapshot {
                    chunks: Vec::new(),
                    opaque_states,
                };
                snapshot.refresh_chunks(world, bounds);
                let snapshot = Arc::new(snapshot);
                self.snapshot = Some((self.stamp, bounds, Arc::clone(&snapshot)));
                snapshot
            }
        };
        let queries = std::mem::take(queries);
        let job = CullJob {
            stamp: self.stamp,
            snapshot_epoch: self.snapshot_epoch,
            generation: self.job_generation.load(Ordering::Relaxed),
            snapshot,
            queries,
            clear_visible: std::mem::take(&mut self.recycled_visible),
            hidden: std::mem::take(&mut self.recycled_hidden),
            rays_filtered: false,
        };
        let mut stats = self.stats.borrow_mut();
        stats.worker_jobs = stats.worker_jobs.saturating_add(1);
        drop(stats);
        let query_count = job.queries.len();
        let send_started = self.timing_start();
        match self.jobs.try_send(job) {
            Ok(()) => {
                self.pending = true;
                let mut stats = self.stats.borrow_mut();
                stats.worker_job_queries =
                    stats.worker_job_queries.saturating_add(query_count as u64);
                stats
                    .worker_job_query_count_samples
                    .record(query_count as u64);
            }
            Err(TrySendError::Full(job)) => {
                let mut stats = self.stats.borrow_mut();
                stats.worker_jobs = stats.worker_jobs.saturating_sub(1);
                stats.job_send_full = stats.job_send_full.saturating_add(1);
                drop(stats);
                *self.queries.get_mut() = job.queries;
                self.recycled_visible = job.clear_visible;
                self.recycled_hidden = job.hidden;
            }
            Err(TrySendError::Disconnected(job)) => {
                let mut stats = self.stats.borrow_mut();
                stats.worker_jobs = stats.worker_jobs.saturating_sub(1);
                stats.job_send_disconnected = stats.job_send_disconnected.saturating_add(1);
                drop(stats);
                *self.queries.get_mut() = job.queries;
                self.recycled_visible = job.clear_visible;
                self.recycled_hidden = job.hidden;
            }
        }
        self.record_timing(TimingPhase::JobSend, send_started);
    }

    pub(super) fn record_entity_vertices_culled(&self, vertices: usize) {
        let mut stats = self.stats.borrow_mut();
        stats.entities_culled = stats.entities_culled.saturating_add(1);
        stats.entity_vertices_culled = stats.entity_vertices_culled.saturating_add(vertices as u64);
        stats.entity_bytes_culled = stats
            .entity_bytes_culled
            .saturating_add((vertices.saturating_mul(std::mem::size_of::<Vertex>())) as u64);
    }

    pub(super) fn record_entity_culled(&self) {
        let mut stats = self.stats.borrow_mut();
        stats.entities_culled = stats.entities_culled.saturating_add(1);
    }

    pub(super) fn stats_value(&self) -> serde_json::Value {
        let stats = self.stats.borrow();
        let (worker_p50_ns, worker_p95_ns) = stats.worker_job_samples.percentiles_ns();
        let (bounds_p50_ns, bounds_p95_ns) = stats.bounds_scan_samples.percentiles_ns();
        let (job_queries_p50, job_queries_p95) =
            stats.worker_job_query_count_samples.percentiles_ns();
        let bounds_samples = stats
            .entity_bounds_samples
            .iter()
            .map(|sample| {
                let hex = |values: [u64; 3]| values.map(|value| format!("0x{value:016x}"));
                serde_json::json!({
                    "frame": sample.frame,
                    "entity_id": sample.entity_id,
                    "camera_bits": hex(sample.camera),
                    "min_bits": hex(sample.min),
                    "max_bits": hex(sample.max),
                })
            })
            .collect::<Vec<_>>();
        let completed_per_frame = if stats.frames == 0 {
            0.0
        } else {
            stats.worker_jobs_completed as f64 / stats.frames as f64
        };
        let queued_per_frame = if stats.frames == 0 {
            0.0
        } else {
            stats.worker_jobs as f64 / stats.frames as f64
        };
        let mut value = serde_json::json!({
            "frames": stats.frames,
            "entity_queries": stats.entity_queries,
            "entity_cache_hits": stats.entity_cache_hits,
            "entity_cache_misses": stats.entity_cache_misses,
            "result_cache_evictions": stats.result_cache_evictions,
            "entity_vertices_scanned": stats.entity_vertices_scanned,
            "entities_culled": stats.entities_culled,
            "entity_vertices_culled": stats.entity_vertices_culled,
            "entity_bytes_avoided": stats.entity_bytes_culled,
            "queued_queries": stats.queued_queries,
            "queries_dropped_while_pending": stats.queries_dropped_while_pending,
            "worker_jobs": stats.worker_jobs,
            "worker_jobs_completed": stats.worker_jobs_completed,
            "stale_camera_jobs": stats.stale_camera_jobs,
            "stale_world_jobs": stats.stale_world_jobs,
            "worker_cancelled_jobs": stats.worker_cancelled_jobs,
            "worker_ms": stats.worker_ns as f64 / 1_000_000.0,
            "worker_job_ns_p50_recent": worker_p50_ns,
            "worker_job_ns_p95_recent": worker_p95_ns,
            "worker_job_timing_samples": stats.worker_job_samples.len,
            "worker_jobs_completed_per_frame": completed_per_frame,
            "worker_jobs_queued_per_frame": queued_per_frame,
            "bounds_scan_ms": stats.bounds_scan_ns as f64 / 1_000_000.0,
            "bounds_scan_ns_p50_recent": bounds_p50_ns,
            "bounds_scan_ns_p95_recent": bounds_p95_ns,
            "bounds_scan_timing_samples": stats.bounds_scan_samples.len,
            "worker_hidden_entities": stats.worker_hidden_entities,
            "worker_visible_entities": stats.worker_visible_entities,
            "worker_deferred_entities": stats.worker_deferred_entities,
            "entity_budget_exhausted_jobs": stats.entity_budget_exhausted_jobs,
            "result_cache_slots": MAX_OCCLUSION_RESULT_CACHE_ENTRIES,
            "result_cache_entries": self.query_results.len(),
            "profiling_enabled": self.measure_bounds_timing.get(),
            "state_bytes": std::mem::size_of::<Self>(),
        });
        value["entity_aabb_cache_hits"] = serde_json::json!(stats.entity_aabb_cache_hits);
        value["entity_aabb_cache_misses"] = serde_json::json!(stats.entity_aabb_cache_misses);
        value["entity_mesh_cache_hits"] = serde_json::json!(stats.entity_mesh_cache_hits);
        value["entity_mesh_cache_misses"] = serde_json::json!(stats.entity_mesh_cache_misses);
        value["result_cache_capacity_evictions"] =
            serde_json::json!(stats.result_cache_capacity_evictions);
        value["result_cache_idle_evictions"] = serde_json::json!(stats.result_cache_idle_evictions);
        value["result_cache_invalidation_replacements"] =
            serde_json::json!(stats.result_cache_invalidation_replacements);
        value["cache_miss_empty_set"] = serde_json::json!(stats.cache_miss_empty_set);
        value["cache_miss_key_mismatch"] = serde_json::json!(stats.cache_miss_key_mismatch);
        value["cache_miss_camera_mismatch"] = serde_json::json!(stats.cache_miss_camera_mismatch);
        value["cache_miss_invalidated"] = serde_json::json!(stats.cache_miss_invalidated);
        value["worker_job_queries"] = serde_json::json!(stats.worker_job_queries);
        value["worker_job_query_count_p50_recent"] = serde_json::json!(job_queries_p50);
        value["worker_job_query_count_p95_recent"] = serde_json::json!(job_queries_p95);
        value["worker_job_query_count_samples"] =
            serde_json::json!(stats.worker_job_query_count_samples.len);
        value["job_send_full"] = serde_json::json!(stats.job_send_full);
        value["job_send_disconnected"] = serde_json::json!(stats.job_send_disconnected);
        value["world_revision_changes"] = serde_json::json!(stats.world_revision_changes);
        value["chunks_revision_changes"] = serde_json::json!(stats.chunks_revision_changes);
        value["worker_blocking_wait_ms"] = serde_json::json!(0.0);
        value["worker_wait_path"] = serde_json::json!("render thread uses try_recv/try_send");
        value["phase_timings"] = serde_json::json!({
            "result_consume": Self::timing_value(&stats.result_consume_timing),
            "frame_submit": Self::timing_value(&stats.frame_submit_timing),
            "job_send": Self::timing_value(&stats.job_send_timing),
            "result_lookup_sampled_1_in_16": Self::timing_value(&stats.result_lookup_timing),
            "proof_validation": Self::timing_value(&stats.proof_validation_timing),
        });
        value["proof_validation"] = serde_json::json!({
            "unique_chunk_checks": stats.proof_validation_counts.chunk_checks,
            "cached_chunk_checks": stats.proof_validation_counts.chunk_cache_hits,
            "unique_section_checks": stats.proof_validation_counts.section_checks,
            "cached_section_checks": stats.proof_validation_counts.section_cache_hits,
        });
        value["sampled_entity_bounds"] = serde_json::json!(bounds_samples);
        value
    }

    pub(super) fn reset_stats(&self) {
        *self.stats.borrow_mut() = AsyncCullingStats::default();
        self.proof_validation_sample_counter.set(0);
    }
}

fn cache_miss_reason(
    entry: Option<&CachedQueryResult>,
    key: &CullKey,
    stamp: SnapshotKey,
    camera_cell: Option<[i32; 3]>,
    cache_empty: bool,
) -> CacheMissReason {
    let Some(entry) = entry else {
        return if cache_empty {
            CacheMissReason::EmptySet
        } else {
            CacheMissReason::KeyMismatch
        };
    };
    if !entry.stamp.same_layout(stamp)
        || (entry.proof.is_none() && entry.stamp.world_revision != stamp.world_revision)
    {
        return CacheMissReason::Invalidated;
    }
    if entry.key.camera != key.camera
        && (entry.proof.is_some()
            || entry.stamp != stamp
            || !camera_cell.is_some_and(|cell| entry.camera_cell == Some(cell)))
    {
        return CacheMissReason::CameraMismatch;
    }
    CacheMissReason::KeyMismatch
}

fn query_priority(key: &CullKey) -> f64 {
    let camera = key.camera.map(f64::from_bits);
    let min = key.min.map(f64::from_bits);
    let max = key.max.map(f64::from_bits);
    let mut extent = 0.0_f64;
    let mut distance_squared = 0.0_f64;
    for axis in 0..3 {
        let center = min[axis] + (max[axis] - min[axis]) * 0.5;
        extent = extent.max(max[axis] - min[axis]);
        distance_squared += (center - camera[axis]).powi(2);
    }
    if extent.is_finite() && distance_squared.is_finite() {
        extent * extent / distance_squared.max(0.01)
    } else {
        0.0
    }
}

fn camera_cell(camera: [f64; 3]) -> Option<[i32; 3]> {
    let [Some(x), Some(y), Some(z)] = camera.map(snapshot_cell) else {
        return None;
    };
    Some([x, y, z])
}

fn filter_rays(job: &mut CullJob, generation: &AtomicU64, scratch: &mut RayWorkerScratch) {
    let Some(first) = job.queries.first() else {
        job.rays_filtered = true;
        return;
    };
    let camera = first.camera.map(f64::from_bits);
    scratch.prepare(job.snapshot_epoch);
    std::mem::swap(&mut job.queries, &mut scratch.input);
    job.clear_visible.clear();
    let mut input = std::mem::take(&mut scratch.input);
    for key in input.drain(..) {
        if generation.load(Ordering::Relaxed) != job.generation {
            break;
        }
        let min = key.min.map(f64::from_bits);
        let max = key.max.map(f64::from_bits);
        if bounds_have_clear_ray(
            camera,
            min,
            max,
            &job.snapshot,
            &mut scratch.last_chunk,
            &mut scratch.opacity_grid,
        ) {
            scratch.clear_visible.push(key);
        } else {
            scratch.blocked.push(key);
        }
    }
    scratch.input = input;
    std::mem::swap(&mut job.queries, &mut scratch.blocked);
    std::mem::swap(&mut job.clear_visible, &mut scratch.clear_visible);
    scratch.last_chunk.clear();
    job.rays_filtered = true;
}

fn cull_job(
    mut job: CullJob,
    generation: &AtomicU64,
    scratch: &mut PlaneWorkerScratch,
) -> CullResults {
    let started = Instant::now();
    let camera = job
        .queries
        .first()
        .or_else(|| job.clear_visible.first())
        .map(|key| key.camera.map(f64::from_bits))
        .unwrap_or([0.0; 3]);
    let culler = OcclusionCuller::new(camera, OCCLUSION_LOOKUP_BUDGET);
    scratch.prepare(job.snapshot_epoch);
    let mut hidden = job.hidden;
    hidden.clear();
    if hidden.capacity() < job.queries.len() {
        hidden.reserve(job.queries.len() - hidden.capacity());
    }
    let mut visible = job.clear_visible;
    visible.clear();
    let mut deferred_entities = 0;
    let mut cancelled = false;
    for &key in &job.queries {
        if generation.load(Ordering::Relaxed) != job.generation {
            cancelled = true;
            break;
        }
        scratch.covered.clear();
        let min = key.min.map(f64::from_bits);
        let max = key.max.map(f64::from_bits);
        if !job.rays_filtered
            && bounds_have_clear_ray(
                camera,
                min,
                max,
                &job.snapshot,
                &mut scratch.last_chunk,
                &mut scratch.opacity_grid,
            )
        {
            visible.push(key);
            continue;
        }
        if culler.remaining() == 0 {
            deferred_entities += 1;
            continue;
        }
        let remaining_before = culler.remaining();
        let is_hidden = culler.occluded(min, max, |pos| {
            let opaque = scratch
                .opacity_grid
                .opaque(&job.snapshot, pos, &mut scratch.last_chunk);
            if opaque {
                scratch.covered.push(pos);
            }
            opaque
        });
        if !is_hidden && remaining_before != 0 && culler.remaining() == 0 {
            deferred_entities += 1;
            continue;
        }
        if !is_hidden {
            visible.push(key);
            continue;
        }
        let mut sections = Vec::with_capacity(scratch.covered.len());
        let mut proof_chunks: Vec<(ChunkKey, Weak<Chunk>)> = Vec::new();
        let mut valid = true;
        for &pos in &scratch.covered {
            let chunk_key = (pos[0] >> 4, pos[2] >> 4);
            let Some(y) = pos[1].checked_sub(job.stamp.bottom_y) else {
                valid = false;
                break;
            };
            sections.push((chunk_key.0, y.div_euclid(16), chunk_key.1));
            if let Err(index) = proof_chunks.binary_search_by_key(&chunk_key, |(key, _)| *key) {
                let Some(chunk) = job.snapshot.chunk(chunk_key) else {
                    valid = false;
                    break;
                };
                if chunk.strong_count() == 0 {
                    valid = false;
                    break;
                }
                proof_chunks.insert(index, (chunk_key, chunk.clone()));
            }
        }
        if valid {
            sections.sort_unstable();
            sections.dedup();
            hidden.push((
                key,
                OcclusionProof {
                    sections,
                    chunks: proof_chunks,
                },
            ));
        } else {
            visible.push(key);
        }
    }
    hidden.sort_unstable_by_key(|(key, _)| *key);
    visible.sort_unstable();
    scratch.last_chunk.clear();
    let hidden_entities = hidden.len();
    let visible_entities = visible.len();
    job.queries.clear();
    CullResults {
        stamp: job.stamp,
        camera: camera.map(f64::to_bits),
        generation: job.generation,
        cancelled,
        hidden,
        visible,
        scratch_queries: job.queries,
        worker_ns: started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
        hidden_entities,
        visible_entities,
        deferred_entities,
    }
}

pub(super) fn opaque(facts: &StateFacts) -> bool {
    facts.render_shape == RenderShape::Model
        && facts.can_occlude
        && facts.solid_render
        && facts.full_collision
}

fn snapshot_bounds(camera: [f64; 3], queries: &[CullKey]) -> Option<ChunkBounds> {
    let mut min_x = camera[0];
    let mut max_x = camera[0];
    let mut min_z = camera[2];
    let mut max_z = camera[2];
    for query in queries {
        let min = query.min.map(f64::from_bits);
        let max = query.max.map(f64::from_bits);
        min_x = min_x.min(min[0]);
        max_x = max_x.max(max[0]);
        min_z = min_z.min(min[2]);
        max_z = max_z.max(max[2]);
    }
    // Culler::occluded expands projected cells by EPSILON. One block of slack
    // also covers the camera's first cell when it lies exactly on a boundary.
    let min_x = snapshot_cell(min_x - 1.0)? >> 4;
    let max_x = snapshot_cell(max_x + 1.0)? >> 4;
    let min_z = snapshot_cell(min_z - 1.0)? >> 4;
    let max_z = snapshot_cell(max_z + 1.0)? >> 4;
    let width = i64::from(max_x) - i64::from(min_x) + 1;
    let depth = i64::from(max_z) - i64::from(min_z) + 1;
    if width <= 0 || depth <= 0 || width.checked_mul(depth)? > MAX_SNAPSHOT_CHUNKS {
        return None;
    }
    Some(ChunkBounds {
        min_x,
        max_x,
        min_z,
        max_z,
    })
}

fn snapshot_cell(value: f64) -> Option<i32> {
    let cell = value.floor();
    (cell.is_finite() && cell >= f64::from(i32::MIN) && cell <= f64::from(i32::MAX))
        .then_some(cell as i32)
}

fn proof_is_current(
    world: &World,
    renderer: Option<&Renderer>,
    streaming: &Streaming,
    pending_uploads: &[((i32, i32, i32), SectionMesh)],
    proof: &OcclusionProof,
    cache: &mut FrameProofValidationCache,
) -> (bool, ProofValidationCounts) {
    let mut counts = ProofValidationCounts::default();
    for (key, expected) in &proof.chunks {
        if !cache.chunk_is_current(world, *key, expected, &mut counts) {
            return (false, counts);
        }
    }
    for &section in &proof.sections {
        if !cache.section_is_current(
            section,
            world,
            renderer,
            streaming,
            pending_uploads,
            &mut counts,
        ) {
            return (false, counts);
        }
    }
    (true, counts)
}

pub(super) fn occluder_ready(
    world: &World,
    facts: &BlockFacts,
    renderer: Option<&Renderer>,
    streaming: &Streaming,
    pending_uploads: &[((i32, i32, i32), SectionMesh)],
    pos: [i32; 3],
) -> bool {
    let state = world.block(pos[0], pos[1], pos[2]);
    if !opaque(facts.get(state)) {
        return false;
    }
    let Some(y) = pos[1].checked_sub(world.bottom_y()) else {
        return false;
    };
    let section = (pos[0] >> 4, y.div_euclid(16), pos[2] >> 4);
    renderer.is_some_and(|r| r.has_section(section))
        && !world.dirty_sections.contains(&section)
        && !streaming.is_meshing(section)
        && !pending_uploads.iter().any(|(key, _)| *key == section)
}

impl Game<'_> {
    pub(super) fn prepare_occlusion_culling(&mut self, camera: glam::DVec3) {
        let entity_mode = self.settings.horizon.entity_culling;
        let requested = entity_mode != EntityCullingMode::Off;
        let at = camera.to_array().map(|v| v.floor() as i32);
        let eligible = requested
            // Packs can replace opaque models, so their terrain is not a safe occluder.
            && self.model_packs_are_vanilla()
            && self.player.is_some()
            && !self.spectator()
            && !opaque(self.res.facts().get(self.world.block(at[0], at[1], at[2])));
        if !eligible {
            self.sync_occlusion_culler = None;
            self.async_occlusion_culler = None;
            self.async_occlusion_culling_failed = false;
            return;
        }

        let wants_async = entity_mode == EntityCullingMode::Asynchronous;
        let wants_sync = entity_mode == EntityCullingMode::Synchronous;
        if wants_async {
            if !async_mob_culling_ready(self.mobs.len(), self.async_occlusion_culler.is_some()) {
                self.async_occlusion_culler = None;
                self.async_occlusion_culling_failed = false;
                self.sync_occlusion_culler = None;
                return;
            }
            if self.async_occlusion_culler.is_none() && !self.async_occlusion_culling_failed {
                match AsyncOcclusionCuller::new() {
                    Ok(culler) => self.async_occlusion_culler = Some(Box::new(culler)),
                    Err(error) => {
                        log::warn!("Horizon: cannot start world-occlusion-culling worker: {error}");
                        self.async_occlusion_culling_failed = true;
                    }
                }
            }
            if let Some(culler) = &mut self.async_occlusion_culler {
                culler.begin_frame(camera, &self.world);
            }
        } else {
            self.async_occlusion_culler = None;
            self.async_occlusion_culling_failed = false;
        }

        let async_unavailable = wants_async && self.async_occlusion_culler.is_none();
        self.sync_occlusion_culler = (wants_sync || async_unavailable)
            .then(|| OcclusionCuller::new(camera.to_array(), 8_192));
    }

    pub(super) fn finish_async_occlusion_culling(&mut self) {
        if let Some(culler) = &mut self.async_occlusion_culler {
            culler.finish_frame(&self.world, self.res.facts());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::chunk::ColumnLayout;

    #[test]
    fn timing_samples_keep_a_bounded_recent_window_and_use_nearest_rank() {
        let mut samples = TimingSamples::default();
        assert_eq!(samples.percentiles_ns(), (None, None));
        for value in 10..=270 {
            samples.record(value);
        }
        assert_eq!(samples.len, TIMING_SAMPLE_CAPACITY);
        assert_eq!(samples.percentiles_ns(), (Some(142), Some(258)));
    }

    fn wall(hole: Option<[i32; 3]>) -> (Arc<OcclusionSnapshot>, Vec<Arc<Chunk>>) {
        let mut live = Vec::new();
        let mut chunks = Vec::new();
        for chunk_z in [-1, 0] {
            let mut chunk = Chunk::with_layout(0, chunk_z, ColumnLayout::VANILLA);
            for y in -1..=0 {
                for z in -1..=0 {
                    let pos = [4, y, z];
                    if hole != Some(pos) {
                        chunk.set(pos[0] & 15, pos[1], pos[2] & 15, 1);
                    }
                }
            }
            let chunk = Arc::new(chunk);
            chunks.push(((0, chunk_z), Arc::downgrade(&chunk)));
            live.push(chunk);
        }
        chunks.sort_unstable_by_key(|(key, _)| *key);
        (
            Arc::new(OcclusionSnapshot {
                chunks,
                opaque_states: Arc::from([false, true]),
            }),
            live,
        )
    }

    fn run_job(job: CullJob) -> CullResults {
        cull_job(job, &AtomicU64::new(0), &mut PlaneWorkerScratch::default())
    }

    fn job(snapshot: Arc<OcclusionSnapshot>) -> CullJob {
        let camera = [0.0; 3];
        let min = [7.95, -0.55, -0.55];
        let max = [10.05, 0.55, 0.55];
        CullJob {
            stamp: SnapshotKey {
                world_revision: 1,
                chunks_revision: 1,
                bottom_y: ColumnLayout::VANILLA.min_y(),
            },
            snapshot_epoch: 0,
            generation: 0,
            snapshot,
            queries: vec![CullKey {
                camera: camera.map(f64::to_bits),
                min: min.map(f64::to_bits),
                max: max.map(f64::to_bits),
            }],
            clear_visible: Vec::new(),
            hidden: Vec::new(),
            rays_filtered: false,
        }
    }

    #[test]
    fn async_snapshot_is_bounded_and_handles_negative_chunk_edges() {
        let camera = [-16.0, 0.0, -16.0];
        let key = CullKey {
            camera: camera.map(f64::to_bits),
            min: [-8.0, -1.0, -8.0].map(f64::to_bits),
            max: [8.0, 1.0, 8.0].map(f64::to_bits),
        };
        assert_eq!(
            snapshot_bounds(camera, &[key]),
            Some(ChunkBounds {
                min_x: -2,
                max_x: 0,
                min_z: -2,
                max_z: 0,
            })
        );

        let huge = CullKey {
            camera: [0.0; 3].map(f64::to_bits),
            min: [5_000.0, 0.0, 5_000.0].map(f64::to_bits),
            max: [5_001.0, 1.0, 5_001.0].map(f64::to_bits),
        };
        assert_eq!(snapshot_bounds([0.0; 3], &[huge]), None);
    }

    #[test]
    fn async_worker_only_hides_bounds_behind_a_complete_opaque_plane() {
        let (snapshot, live) = wall(None);
        let results = run_job(job(snapshot));
        assert_eq!(results.hidden.len(), 1);
        assert!(results.visible.is_empty());
        assert_eq!(results.hidden[0].1.sections.len(), 4);
        assert_eq!(results.hidden[0].1.chunks.len(), 2);
        drop(live);

        let (snapshot, live) = wall(Some([4, 0, 0]));
        let results = run_job(job(snapshot));
        assert!(results.hidden.is_empty());
        assert_eq!(results.visible.len(), 1);
        drop(live);
    }

    #[test]
    fn entity_rays_only_short_circuit_when_a_sample_path_is_clear() {
        let (snapshot, live) = wall(None);
        let mut last_chunk = LastChunk::default();
        let mut opacity_grid = OpacityGrid::default();
        assert!(!ray_reaches_without_opaque(
            [0.0; 3],
            [8.0, 0.0, 0.0],
            &snapshot,
            &mut last_chunk,
            &mut opacity_grid,
        ));
        drop(live);

        let (snapshot, live) = wall(Some([4, 0, 0]));
        let mut last_chunk = LastChunk::default();
        let mut opacity_grid = OpacityGrid::default();
        assert!(bounds_have_clear_ray(
            [0.0; 3],
            [7.95, -0.55, -0.55],
            [10.05, 0.55, 0.55],
            &snapshot,
            &mut last_chunk,
            &mut opacity_grid,
        ));
        drop(live);
    }

    #[test]
    fn ray_scratch_invalidates_cached_voxels_when_the_snapshot_refreshes() {
        let (opaque_snapshot, opaque_live) = wall(None);
        let (hole_snapshot, hole_live) = wall(Some([4, 0, 0]));
        let mut scratch = RayWorkerScratch::default();
        let generation = AtomicU64::new(0);

        let mut blocked = job(opaque_snapshot);
        blocked.snapshot_epoch = 1;
        filter_rays(&mut blocked, &generation, &mut scratch);
        assert!(blocked.clear_visible.is_empty());
        assert_eq!(blocked.queries.len(), 1);

        let mut clear = job(hole_snapshot);
        clear.snapshot_epoch = 2;
        filter_rays(&mut clear, &generation, &mut scratch);
        assert!(clear.queries.is_empty());
        assert_eq!(clear.clear_visible.len(), 1);

        drop((opaque_live, hole_live));
    }
}

#[cfg(test)]
#[path = "occlusion_culling_tests.rs"]
mod external_tests;
