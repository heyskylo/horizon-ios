//! What the capture records of the world: entity and chunk counts, what mobs cost, what the chunk workers spent.
use horizon_perf_format::Interner;
use horizon_perf_format::v1::{
    DimensionCounts, EntityCost, Side, TypeCount, WorkerKind, Workers, WorldSnapshot,
};

/// Entity types costed in a snapshot, costliest first.
pub const ENTITY_COSTS_KEPT: usize = 40;

/// One level's chunks and its entities and block entities by registry id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelCounts {
    pub dimension: String,
    pub chunks: u32,
    pub entities: Vec<(String, u32)>,
    pub block_entities: Vec<(String, u32)>,
}

/// The sampled tick time of one entity type in one level: nanoseconds over `mob_ticks` ticks of single mobs.
#[derive(Clone, Debug, PartialEq)]
pub struct MobCost {
    pub dimension: String,
    pub kind: String,
    pub nanos: u64,
    pub mob_ticks: u64,
}

/// The mob costs sampled over `sampled_ticks` server ticks, costliest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobCosts {
    pub costs: Vec<MobCost>,
    pub sampled_ticks: u64,
}

/// Chunk-worker time of one job kind: nanoseconds over `count` jobs.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkerJobs {
    pub kind: String,
    pub nanos: u64,
    pub count: u64,
}

fn counts(types: &[(String, u32)], strings: &mut Interner) -> Vec<TypeCount> {
    let mut sorted: Vec<&(String, u32)> = types.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted
        .into_iter()
        .map(|(kind, count)| TypeCount {
            r#type: strings.intern(kind),
            count: *count,
        })
        .collect()
}

/// The world `t` seconds into the capture, named `label`; `costs` is what mobs cost, at the end.
pub fn snapshot(
    label: &str,
    t: f64,
    levels: &[LevelCounts],
    costs: Option<&MobCosts>,
    strings: &mut Interner,
) -> WorldSnapshot {
    let ticks = costs.map_or(1, |c| c.sampled_ticks.max(1)) as f64;
    WorldSnapshot {
        side: Side::Server as i32,
        t,
        label: strings.intern(label),
        dims: levels
            .iter()
            .map(|level| DimensionCounts {
                dimension: strings.intern(&level.dimension),
                chunks: level.chunks,
                entities: counts(&level.entities, strings),
                block_entities: counts(&level.block_entities, strings),
            })
            .collect(),
        entity_costs: costs
            .into_iter()
            .flat_map(|c| &c.costs)
            .take(ENTITY_COSTS_KEPT)
            .map(|c| EntityCost {
                dimension: strings.intern(&c.dimension),
                entity: strings.intern(&c.kind),
                ms_per_tick: (c.nanos as f64 / ticks / 1e6) as f32,
                mobs_ticked_per_tick: (c.mob_ticks as f64 / ticks) as f32,
                us_per_mob: (c.nanos as f64 / c.mob_ticks.max(1) as f64 / 1e3) as f32,
            })
            .collect(),
    }
}

/// The chunk workers' CPU by job kind over a capture of `window_s` seconds; kinds that ran no job are left out.
pub fn workers(jobs: &[WorkerJobs], window_s: f64, strings: &mut Interner) -> Workers {
    Workers {
        side: Side::Server as i32,
        kinds: jobs
            .iter()
            .filter(|j| j.count > 0)
            .map(|j| WorkerKind {
                kind: strings.intern(&j.kind),
                jobs: j.count,
                cpu_cores: (j.nanos as f64 / 1e9 / window_s.max(1e-9)) as f32,
                ms_per_job: (j.nanos as f64 / j.count as f64 / 1e6) as f32,
            })
            .collect(),
    }
}
