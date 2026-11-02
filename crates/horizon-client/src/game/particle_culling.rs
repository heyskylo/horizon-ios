//! Particle culling, separated from entity mesh culling.
use super::{
    Game,
    occlusion_culling::{AsyncOcclusionCuller, QuerySource, occluder_ready},
};
use horizon_client::{
    blocks::BlockFacts, entity_culling::Culler as OcclusionCuller, gfx::Renderer,
    graphics::ParticleCullingMode, mesher::SectionMesh, streaming::Streaming, world::World,
};

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
    let center = std::array::from_fn(|axis| min[axis] + (max[axis] - min[axis]) * 0.5);
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

pub(super) struct ParticleOcclusionContext<'a> {
    mode: ParticleCullingMode,
    sync_culler: Option<&'a OcclusionCuller>,
    async_culler: Option<&'a AsyncOcclusionCuller>,
    world: &'a World,
    facts: &'a BlockFacts,
    renderer: Option<&'a Renderer>,
    streaming: &'a Streaming,
    pending_uploads: &'a [((i32, i32, i32), SectionMesh)],
    clusters: ClusterVisibilityCache,
    particle_candidates: u64,
    particle_candidates_culled: u64,
}

impl ParticleOcclusionContext<'_> {
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
        let sync_culler = self.sync_culler;
        let async_culler = self.async_culler;
        let world = self.world;
        let facts = self.facts;
        let renderer = self.renderer;
        let streaming = self.streaming;
        let pending_uploads = self.pending_uploads;
        let hidden = self.clusters.get_or_compute(cluster, || match mode {
            ParticleCullingMode::Off => false,
            ParticleCullingMode::Synchronous => sync_hidden(
                sync_culler,
                world,
                facts,
                renderer,
                streaming,
                pending_uploads,
                min,
                max,
            ),
            ParticleCullingMode::Asynchronous => async_hidden(
                async_culler,
                sync_culler,
                world,
                facts,
                renderer,
                streaming,
                pending_uploads,
                min,
                max,
            ),
        });
        if mode == ParticleCullingMode::Asynchronous && async_culler.is_some() {
            self.particle_candidates = self.particle_candidates.saturating_add(1);
            self.particle_candidates_culled = self
                .particle_candidates_culled
                .saturating_add(u64::from(hidden));
        }
        hidden
    }
}

impl Drop for ParticleOcclusionContext<'_> {
    fn drop(&mut self) {
        if let Some(culler) = self.async_culler {
            culler.record_particle_candidates(
                self.particle_candidates,
                self.particle_candidates_culled,
            );
        }
    }
}

fn sync_hidden(
    culler: Option<&OcclusionCuller>,
    world: &World,
    facts: &BlockFacts,
    renderer: Option<&Renderer>,
    streaming: &Streaming,
    pending_uploads: &[((i32, i32, i32), SectionMesh)],
    min: [f64; 3],
    max: [f64; 3],
) -> bool {
    culler.is_some_and(|culler| {
        culler.occluded(min, max, |pos| {
            occluder_ready(world, facts, renderer, streaming, pending_uploads, pos)
        })
    })
}

fn async_hidden(
    culler: Option<&AsyncOcclusionCuller>,
    sync_culler: Option<&OcclusionCuller>,
    world: &World,
    facts: &BlockFacts,
    renderer: Option<&Renderer>,
    streaming: &Streaming,
    pending_uploads: &[((i32, i32, i32), SectionMesh)],
    min: [f64; 3],
    max: [f64; 3],
) -> bool {
    let Some(culler) = culler else {
        return sync_hidden(
            sync_culler,
            world,
            facts,
            renderer,
            streaming,
            pending_uploads,
            min,
            max,
        );
    };
    let Some(query) = culler.query_particle_bounds(min, max) else {
        return false;
    };
    match culler.query_result(&query, QuerySource::Particle) {
        Some((false, _)) => false,
        Some((true, Some(proof)))
            if culler.proof_is_current(world, renderer, streaming, pending_uploads, proof) =>
        {
            true
        }
        Some(_) | None => {
            culler.queue(query);
            false
        }
    }
}

impl Game<'_> {
    pub(super) fn particle_occlusion_context(&self) -> ParticleOcclusionContext<'_> {
        ParticleOcclusionContext {
            mode: self.settings.horizon.particle_culling,
            sync_culler: self.sync_occlusion_culler.as_ref(),
            async_culler: self.async_occlusion_culler.as_ref(),
            world: &self.world,
            facts: self.res.facts(),
            renderer: self.renderer.as_ref(),
            streaming: &self.streaming,
            pending_uploads: &self.pending_uploads,
            clusters: ClusterVisibilityCache::default(),
            particle_candidates: 0,
            particle_candidates_culled: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
