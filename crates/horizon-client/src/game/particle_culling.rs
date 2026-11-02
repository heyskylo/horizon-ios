//! Particle visibility checks and their worker, separate from entity culling.
use super::Game;
use horizon_client::{
    blocks::{BlockFacts, RenderShape, StateFacts},
    chunk::Chunk,
    entity_culling::Culler,
    graphics::ParticleCullingMode,
    world::World,
};
use std::{
    array,
    cell::RefCell,
    sync::{
        Arc, Weak,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
};

type ChunkKey = (i32, i32);
type SectionKey = (i32, i32, i32);
const MAX_SNAPSHOT_CHUNKS: i64 = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CullKey {
    camera: [u64; 3],
    min: [u64; 3],
    max: [u64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapshotKey {
    world_revision: u64,
    chunks_revision: u64,
    bottom_y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChunkBounds {
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

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
    fn get(&mut self, snapshot: &OcclusionSnapshot, key: ChunkKey) -> Option<&Arc<Chunk>> {
        if self.key != Some(key) {
            self.key = Some(key);
            self.value = snapshot.chunk(key).and_then(Weak::upgrade);
        }
        self.value.as_ref()
    }
}

struct OcclusionProof {
    sections: Vec<SectionKey>,
    chunks: Vec<(ChunkKey, Weak<Chunk>)>,
}

struct CullResults {
    stamp: SnapshotKey,
    answers: Vec<(CullKey, Option<Arc<OcclusionProof>>)>,
    scratch_queries: Vec<CullKey>,
}

struct CullJob {
    stamp: SnapshotKey,
    snapshot: Arc<OcclusionSnapshot>,
    queries: Vec<CullKey>,
}

#[derive(Default)]
pub(super) struct ParticleAsyncState {
    culler: Option<ParticleAsyncCuller>,
    failed: bool,
}

/// Particle occlusion has a dedicated worker so its queue cannot delay entity visibility jobs.
struct ParticleAsyncCuller {
    jobs: SyncSender<CullJob>,
    results_rx: Receiver<CullResults>,
    _thread: JoinHandle<()>,
    snapshot: Option<(SnapshotKey, ChunkBounds, Arc<OcclusionSnapshot>)>,
    opaque_states: Option<Arc<[bool]>>,
    results: Option<CullResults>,
    queries: RefCell<Vec<CullKey>>,
    camera: [f64; 3],
    stamp: SnapshotKey,
    pending: bool,
}

impl ParticleAsyncCuller {
    fn new() -> std::io::Result<Self> {
        let (job_tx, job_rx) = mpsc::sync_channel::<CullJob>(1);
        let (result_tx, result_rx) = mpsc::channel::<CullResults>();
        let thread = std::thread::Builder::new()
            .name("particle-culling".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    if result_tx.send(cull_job(job)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            jobs: job_tx,
            results_rx: result_rx,
            _thread: thread,
            snapshot: None,
            opaque_states: None,
            results: None,
            queries: RefCell::new(Vec::new()),
            camera: [0.0; 3],
            stamp: SnapshotKey {
                world_revision: 0,
                chunks_revision: 0,
                bottom_y: 0,
            },
            pending: false,
        })
    }

    fn begin_frame(&mut self, camera: glam::DVec3, world: &World) {
        while let Ok(mut results) = self.results_rx.try_recv() {
            self.pending = false;
            let scratch = std::mem::take(&mut results.scratch_queries);
            if scratch.capacity() > self.queries.get_mut().capacity() {
                *self.queries.get_mut() = scratch;
            }
            self.results = Some(results);
        }
        self.camera = camera.to_array();
        self.stamp = SnapshotKey {
            world_revision: world.revision,
            chunks_revision: world.chunks.membership_revision(),
            bottom_y: world.bottom_y(),
        };
        if self
            .results
            .as_ref()
            .is_some_and(|results| results.stamp != self.stamp)
        {
            self.results = None;
        }
        self.queries.get_mut().clear();
    }

    fn query(&self, min: [f64; 3], max: [f64; 3]) -> Option<CullKey> {
        if !self.camera.iter().all(|value| value.is_finite())
            || (0..3).any(|axis| {
                !min[axis].is_finite() || !max[axis].is_finite() || min[axis] > max[axis]
            })
        {
            return None;
        }
        Some(CullKey {
            camera: self.camera.map(f64::to_bits),
            min: min.map(f64::to_bits),
            max: max.map(f64::to_bits),
        })
    }

    fn cached(&self, key: &CullKey) -> Option<Option<&Arc<OcclusionProof>>> {
        let results = self
            .results
            .as_ref()
            .filter(|results| results.stamp == self.stamp)?;
        let index = results
            .answers
            .binary_search_by_key(key, |(key, _)| *key)
            .ok()?;
        Some(results.answers[index].1.as_ref())
    }

    fn queue(&self, key: CullKey) {
        self.queries.borrow_mut().push(key);
    }

    fn finish_frame(&mut self, world: &World, facts: &BlockFacts) {
        let queries = self.queries.get_mut();
        if self.pending {
            queries.clear();
            return;
        }
        if queries.is_empty() {
            return;
        }
        queries.sort_unstable();
        queries.dedup();
        let Some(bounds) = snapshot_bounds(self.camera, queries) else {
            queries.clear();
            return;
        };
        let opaque_states = Arc::clone(self.opaque_states.get_or_insert_with(|| {
            facts
                .states
                .iter()
                .map(state_is_opaque)
                .collect::<Vec<_>>()
                .into()
        }));
        let snapshot = match &mut self.snapshot {
            Some((stamp, cached_bounds, snapshot)) => {
                if stamp != &self.stamp || *cached_bounds != bounds || !snapshot.chunks_match(world)
                {
                    let snapshot = Arc::make_mut(snapshot);
                    snapshot.opaque_states = Arc::clone(&opaque_states);
                    snapshot.refresh_chunks(world, bounds);
                    *stamp = self.stamp;
                    *cached_bounds = bounds;
                }
                Arc::clone(snapshot)
            }
            None => {
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
            snapshot,
            queries,
        };
        match self.jobs.try_send(job) {
            Ok(()) => self.pending = true,
            Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                *self.queries.get_mut() = job.queries;
            }
        }
    }
}

fn snapshot_bounds(camera: [f64; 3], queries: &[CullKey]) -> Option<ChunkBounds> {
    let mut min = [i32::MAX; 3];
    let mut max = [i32::MIN; 3];
    let mut include = |point: [f64; 3]| {
        for axis in 0..3 {
            let value = point[axis].floor();
            if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
                return false;
            }
            let value = value as i32;
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
        true
    };
    if !include(camera) {
        return None;
    }
    for query in queries {
        if !include(query.min.map(f64::from_bits)) || !include(query.max.map(f64::from_bits)) {
            return None;
        }
    }
    let bounds = ChunkBounds {
        min_x: min[0] >> 4,
        max_x: max[0] >> 4,
        min_z: min[2] >> 4,
        max_z: max[2] >> 4,
    };
    let width = i64::from(bounds.max_x) - i64::from(bounds.min_x) + 1;
    let depth = i64::from(bounds.max_z) - i64::from(bounds.min_z) + 1;
    (width > 0 && depth > 0 && width * depth <= MAX_SNAPSHOT_CHUNKS).then_some(bounds)
}

fn cull_job(mut job: CullJob) -> CullResults {
    let camera = job.queries[0].camera.map(f64::from_bits);
    let culler = Culler::new(camera, 8_192);
    let mut last_chunk = LastChunk::default();
    let mut answers = Vec::with_capacity(job.queries.len());
    let mut covered = Vec::new();
    for &key in &job.queries {
        covered.clear();
        let min = key.min.map(f64::from_bits);
        let max = key.max.map(f64::from_bits);
        let proof = if culler.occluded(min, max, |pos| {
            let opaque = job.snapshot.opaque(pos, &mut last_chunk);
            if opaque {
                covered.push(pos);
            }
            opaque
        }) {
            let mut sections = Vec::with_capacity(covered.len());
            let mut chunks: Vec<(ChunkKey, Weak<Chunk>)> = Vec::new();
            let mut valid = true;
            for &pos in &covered {
                let chunk_key = (pos[0] >> 4, pos[2] >> 4);
                let Some(y) = pos[1].checked_sub(job.stamp.bottom_y) else {
                    valid = false;
                    break;
                };
                sections.push((chunk_key.0, y.div_euclid(16), chunk_key.1));
                if let Err(index) = chunks.binary_search_by_key(&chunk_key, |(key, _)| *key) {
                    let Some(chunk) = job.snapshot.chunk(chunk_key) else {
                        valid = false;
                        break;
                    };
                    if chunk.strong_count() == 0 {
                        valid = false;
                        break;
                    }
                    chunks.insert(index, (chunk_key, chunk.clone()));
                }
            }
            if valid {
                sections.sort_unstable();
                sections.dedup();
                Some(Arc::new(OcclusionProof { sections, chunks }))
            } else {
                None
            }
        } else {
            None
        };
        answers.push((key, proof));
    }
    job.queries.clear();
    CullResults {
        stamp: job.stamp,
        answers,
        scratch_queries: job.queries,
    }
}

fn state_is_opaque(facts: &StateFacts) -> bool {
    facts.render_shape == RenderShape::Model
        && facts.can_occlude
        && facts.solid_render
        && facts.full_collision
}

const CLUSTER_CACHE_SIZE: usize = 1_024;
const CLUSTER_SIZE: f64 = 8.0;
const MAX_CLUSTER_OCCLUSION_RADIUS: f64 = 1.0;

#[derive(Clone, Copy)]
struct ClusterCacheEntry {
    cluster: [i32; 3],
    hidden: bool,
}

struct ClusterVisibilityCache {
    entries: [Option<ClusterCacheEntry>; CLUSTER_CACHE_SIZE],
}

impl Default for ClusterVisibilityCache {
    fn default() -> Self {
        Self {
            entries: [None; CLUSTER_CACHE_SIZE],
        }
    }
}

impl ClusterVisibilityCache {
    fn get(&self, cluster: [i32; 3]) -> Option<bool> {
        self.entries[cluster_cache_slot(cluster)]
            .filter(|entry| entry.cluster == cluster)
            .map(|entry| entry.hidden)
    }

    fn get_or_compute(&mut self, cluster: [i32; 3], compute: impl FnOnce() -> bool) -> bool {
        if let Some(hidden) = self.get(cluster) {
            return hidden;
        }
        let hidden = compute();
        self.insert(cluster, hidden);
        hidden
    }

    fn insert(&mut self, cluster: [i32; 3], hidden: bool) {
        self.entries[cluster_cache_slot(cluster)] = Some(ClusterCacheEntry { cluster, hidden });
    }
}

fn cluster_cache_slot(cluster: [i32; 3]) -> usize {
    let hash = (cluster[0] as u32).wrapping_mul(73_856_093)
        ^ (cluster[1] as u32).wrapping_mul(19_349_663)
        ^ (cluster[2] as u32).wrapping_mul(83_492_791);
    hash as usize & (CLUSTER_CACHE_SIZE - 1)
}

/// Use one conservatively expanded 8-block cluster bound for small particles.
fn cluster_query(min: [f64; 3], max: [f64; 3]) -> Option<([i32; 3], [f64; 3], [f64; 3])> {
    if (0..3).any(|axis| !min[axis].is_finite() || !max[axis].is_finite() || min[axis] > max[axis])
    {
        return None;
    }
    let center = array::from_fn(|axis| min[axis] + (max[axis] - min[axis]) * 0.5);
    let radius = (0..3)
        .map(|axis| (max[axis] - min[axis]) * 0.5)
        .fold(0.0_f64, f64::max);
    if !radius.is_finite() || radius > MAX_CLUSTER_OCCLUSION_RADIUS {
        return None;
    }
    let cluster = center.map(|value| {
        let coordinate = (value / CLUSTER_SIZE).floor();
        (coordinate.is_finite()
            && coordinate >= f64::from(i32::MIN)
            && coordinate <= f64::from(i32::MAX))
        .then_some(coordinate as i32)
    });
    let [Some(x), Some(y), Some(z)] = cluster else {
        return None;
    };
    let cluster = [x, y, z];
    let min = cluster
        .map(|coordinate| f64::from(coordinate) * CLUSTER_SIZE - MAX_CLUSTER_OCCLUSION_RADIUS);
    let max = cluster.map(|coordinate| {
        (f64::from(coordinate) + 1.0) * CLUSTER_SIZE + MAX_CLUSTER_OCCLUSION_RADIUS
    });
    Some((cluster, min, max))
}

pub(super) struct ParticleOcclusionContext<'borrow, 'game> {
    game: &'borrow Game<'game>,
    mode: ParticleCullingMode,
    sync_culler: Option<Culler>,
    clusters: ClusterVisibilityCache,
}

impl ParticleOcclusionContext<'_, '_> {
    pub(super) fn hidden(&mut self, min: [f64; 3], max: [f64; 3]) -> bool {
        if self.mode == ParticleCullingMode::Off {
            return false;
        }
        let Some((cluster, min, max)) = cluster_query(min, max) else {
            // Large particles can extend beyond the cluster's conservative
            // bounds. Keep them visible rather than scheduling their own ray job.
            return false;
        };
        let mode = self.mode;
        let game = self.game;
        let sync_culler = self.sync_culler.as_ref();
        self.clusters.get_or_compute(cluster, || match mode {
            ParticleCullingMode::Off => false,
            ParticleCullingMode::Synchronous => {
                sync_culler.is_some_and(|culler| game.particle_occluded_sync(culler, min, max))
            }
            ParticleCullingMode::Asynchronous => {
                game.async_particle_hidden(min, max).unwrap_or_else(|| {
                    sync_culler.is_some_and(|culler| game.particle_occluded_sync(culler, min, max))
                })
            }
        })
    }
}

impl Game<'_> {
    pub(super) fn particle_occlusion_context(
        &self,
        camera: glam::DVec3,
    ) -> ParticleOcclusionContext<'_, '_> {
        let at = camera.to_array().map(|value| value.floor() as i32);
        let camera_inside_opaque =
            state_is_opaque(self.res.facts().get(self.world.block(at[0], at[1], at[2])));
        let mode =
            if self.model_packs_are_vanilla() && self.player.is_some() && !camera_inside_opaque {
                self.settings.horizon.particle_culling
            } else {
                ParticleCullingMode::Off
            };
        if mode == ParticleCullingMode::Asynchronous {
            let mut state = self.async_particle_culler.borrow_mut();
            if state.culler.is_none() && !state.failed {
                match ParticleAsyncCuller::new() {
                    Ok(culler) => state.culler = Some(culler),
                    Err(error) => {
                        log::warn!("Horizon: cannot start particle-culling worker: {error}");
                        state.failed = true;
                    }
                }
            }
            if let Some(culler) = &mut state.culler {
                culler.begin_frame(camera, &self.world);
            }
        } else {
            let mut state = self.async_particle_culler.borrow_mut();
            state.culler = None;
            state.failed = false;
        }
        ParticleOcclusionContext {
            game: self,
            mode,
            sync_culler: (mode != ParticleCullingMode::Off)
                .then(|| Culler::new(camera.to_array(), 8_192)),
            clusters: ClusterVisibilityCache::default(),
        }
    }

    pub(super) fn particle_occluded_sync(
        &self,
        culler: &Culler,
        min: [f64; 3],
        max: [f64; 3],
    ) -> bool {
        culler.occluded(min, max, |pos| self.particle_occluder_ready(pos))
    }

    fn particle_occluder_ready(&self, pos: [i32; 3]) -> bool {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if !state_is_opaque(self.res.facts().get(state)) {
            return false;
        }
        let Some(y) = pos[1].checked_sub(self.world.bottom_y()) else {
            return false;
        };
        let section = (pos[0] >> 4, y.div_euclid(16), pos[2] >> 4);
        self.renderer
            .as_ref()
            .is_some_and(|renderer| renderer.has_section(section))
            && !self.world.dirty_sections.contains(&section)
            && !self.streaming.is_meshing(section)
            && !self.pending_uploads.iter().any(|(key, _)| *key == section)
    }

    pub(super) fn async_particle_hidden(&self, min: [f64; 3], max: [f64; 3]) -> Option<bool> {
        enum Answer {
            Miss,
            Visible,
            Hidden(Arc<OcclusionProof>),
        }

        let query = {
            let state = self.async_particle_culler.borrow();
            let culler = state.culler.as_ref()?;
            culler.query(min, max)?
        };
        let answer = {
            let state = self.async_particle_culler.borrow();
            let culler = state.culler.as_ref()?;
            match culler.cached(&query) {
                None => Answer::Miss,
                Some(None) => Answer::Visible,
                Some(Some(proof)) => Answer::Hidden(Arc::clone(proof)),
            }
        };
        match answer {
            Answer::Visible => Some(false),
            Answer::Hidden(proof) if self.particle_proof_is_current(&proof) => Some(true),
            Answer::Miss | Answer::Hidden(_) => {
                if let Some(culler) = self.async_particle_culler.borrow().culler.as_ref() {
                    culler.queue(query);
                }
                Some(false)
            }
        }
    }

    fn particle_proof_is_current(&self, proof: &OcclusionProof) -> bool {
        proof.chunks.iter().all(|(key, expected)| {
            self.world
                .chunks
                .get(key)
                .is_some_and(|current| std::ptr::eq(Arc::as_ptr(current), expected.as_ptr()))
        }) && proof.sections.iter().all(|&section| {
            self.renderer
                .as_ref()
                .is_some_and(|renderer| renderer.has_section(section))
                && !self.world.dirty_sections.contains(&section)
                && !self.streaming.is_meshing(section)
                && !self.pending_uploads.iter().any(|(key, _)| *key == section)
        })
    }

    pub(super) fn finish_async_particle_culling(&self) {
        if let Some(culler) = &mut self.async_particle_culler.borrow_mut().culler {
            culler.finish_frame(&self.world, self.res.facts());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::chunk::ColumnLayout;

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

    fn wall_job(snapshot: Arc<OcclusionSnapshot>) -> CullJob {
        CullJob {
            stamp: SnapshotKey {
                world_revision: 1,
                chunks_revision: 1,
                bottom_y: ColumnLayout::VANILLA.min_y(),
            },
            snapshot,
            queries: vec![CullKey {
                camera: [0.0; 3].map(f64::to_bits),
                min: [7.95, -0.55, -0.55].map(f64::to_bits),
                max: [10.05, 0.55, 0.55].map(f64::to_bits),
            }],
        }
    }

    #[test]
    fn a_cluster_query_covers_small_particles_across_negative_boundaries() {
        let min = [-0.2, 15.6, -16.2];
        let max = [0.2, 16.0, -15.8];
        let (cluster, query_min, query_max) = cluster_query(min, max).unwrap();

        assert_eq!(cluster, [0, 1, -2]);
        for axis in 0..3 {
            assert!(query_min[axis] <= min[axis]);
            assert!(query_max[axis] >= max[axis]);
        }
        assert_eq!(query_min, [-1.0, 7.0, -17.0]);
        assert_eq!(query_max, [9.0, 17.0, -7.0]);
    }

    #[test]
    fn adjacent_clusters_keep_distinct_visibility_bounds() {
        let left = cluster_query([7.0, 0.0, 0.0], [7.2, 0.2, 0.2]).unwrap();
        let right = cluster_query([8.1, 0.0, 0.0], [8.3, 0.2, 0.2]).unwrap();

        assert_eq!(left.0, [0, 0, 0]);
        assert_eq!(right.0, [1, 0, 0]);
        assert_ne!((left.1, left.2), (right.1, right.2));
    }

    #[test]
    fn the_maximum_supported_particle_radius_is_still_fully_covered() {
        let min = [15.0, 15.0, -1.0];
        let max = [17.0, 17.0, 1.0];
        let (cluster, query_min, query_max) = cluster_query(min, max).unwrap();

        assert_eq!(cluster, [2, 2, 0]);
        assert_eq!(query_min, [15.0, 15.0, -1.0]);
        assert_eq!(query_max, [25.0, 25.0, 9.0]);
    }

    #[test]
    fn invalid_or_large_particle_bounds_fail_open() {
        assert!(cluster_query([0.0, f64::NAN, 0.0], [1.0; 3]).is_none());
        assert!(cluster_query([1.0; 3], [0.0; 3]).is_none());
        assert!(cluster_query([-1.1, 0.0, 0.0], [1.1, 0.0, 0.0]).is_none());
        assert!(cluster_query([f64::MAX; 3], [f64::MAX; 3]).is_none());
        assert!(cluster_query([-f64::MAX; 3], [f64::MAX; 3]).is_none());
    }

    #[test]
    fn particles_in_one_cluster_share_one_occlusion_query() {
        let first = cluster_query([0.1, 1.0, 2.0], [0.3, 1.2, 2.2]).unwrap();
        let second = cluster_query([3.0, 3.0, 3.0], [3.4, 3.4, 3.4]).unwrap();

        assert_eq!(first, second);

        let mut cache = ClusterVisibilityCache::default();
        let mut queries = 0;
        assert!(cache.get_or_compute(first.0, || {
            queries += 1;
            true
        }));
        assert!(cache.get_or_compute(second.0, || {
            queries += 1;
            false
        }));
        assert_eq!(queries, 1);
    }

    #[test]
    fn a_direct_mapped_cache_collision_never_reuses_another_cluster_answer() {
        let candidates = (0..=CLUSTER_CACHE_SIZE)
            .map(|x| [i32::try_from(x).unwrap(), 0, 0])
            .collect::<Vec<_>>();
        let (first, second) = (0..candidates.len())
            .find_map(|i| {
                ((i + 1)..candidates.len())
                    .find(|&j| {
                        cluster_cache_slot(candidates[i]) == cluster_cache_slot(candidates[j])
                    })
                    .map(|j| (candidates[i], candidates[j]))
            })
            .expect("1025 distinct keys must contain a collision in a 1024-slot cache");
        let mut cache = ClusterVisibilityCache::default();

        cache.insert(first, true);
        assert_eq!(cache.get(first), Some(true));
        cache.insert(second, false);
        assert_eq!(cache.get(first), None);
        assert_eq!(cache.get(second), Some(false));
    }

    #[test]
    fn particle_worker_requires_a_complete_opaque_plane_to_hide() {
        let (snapshot, live) = wall(None);
        let results = cull_job(wall_job(snapshot));
        let proof = results.answers[0].1.as_ref().unwrap();
        assert_eq!(proof.sections.len(), 4);
        assert_eq!(proof.chunks.len(), 2);
        drop(live);

        let (snapshot, live) = wall(Some([4, 0, 0]));
        let results = cull_job(wall_job(snapshot));
        assert!(results.answers[0].1.is_none());
        drop(live);
    }
}
