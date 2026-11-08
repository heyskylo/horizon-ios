//! The row of each query op (docs/perf-platform.md §6.3), exactly as it is written to JSON.

use std::collections::BTreeMap;

use horizon_bench::compare::Ab;
use horizon_bench::{Frames, Summary};
use serde::{Deserialize, Serialize};

use crate::wire::{
    Amount, GpuBackendName, GpuFragmentUnitName, GpuModeName, GpuPartsModeName, GpuSourceName,
    KindName, MemoryUnit, ModKindName, SampleBy, SeriesUnit, SideName, StopReasonName, SymbolsName,
    ThreadSelectionName, TriggerName, WarningCodeName,
};

/// A row of whichever op answered; each op's rows are all of one kind, except `net`'s first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Row {
    Span(SpanRow),
    Tick(TickRow),
    Series(SeriesRow),
    Dimension(DimensionRow),
    Count(CountRow),
    Cost(CostRow),
    /// Every player together: the first row of `net`.
    NetTotal(NetTotal),
    NetPlayer(NetPlayer),
    /// The device the passes were timed on: the first row of `gpu`.
    GpuDevice(GpuDeviceRow),
    GpuPass(GpuPassRow),
    Meta(MetaRow),
    Memory(MemoryRow),
    MemoryDiff(MemoryDiffRow),
    Warning(WarningRow),
    Mark(MarkRow),
    Hot(HotRow),
    Tree(TreeRow),
    Split(SplitRow),
    Frame(FrameRow),
    Bench(BenchRow),
}

/// One span, one value per capture asked about; a capture without the span has null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanRow {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub values: Vec<Option<f64>>,
    pub cpu: Vec<Option<f64>>,
    pub waited: Vec<Option<f64>>,
    pub seen: Vec<Option<u64>>,
}

/// A span of a tick: name, depth, wall ms, cpu ms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TickSpanRow(pub String, pub u32, pub f64, pub f64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickRow {
    pub id: String,
    pub t: f64,
    pub wall_ms: f64,
    pub cpu_ms: f64,
    pub before_capture: bool,
    pub complete: bool,
    pub spans: Vec<TickSpanRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesRow {
    pub field: String,
    pub unit: SeriesUnit,
    pub step_s: f64,
    pub t: Vec<f64>,
    pub values: Vec<Option<f64>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionRow {
    pub id: String,
    pub name: String,
    pub chunks: u64,
    pub entities: u64,
    pub block_entities: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountRow {
    pub id: String,
    pub name: String,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostRow {
    pub id: String,
    pub name: String,
    pub dimension: String,
    pub ms_per_tick: f64,
    pub mobs_ticked: f64,
    pub us_per_mob: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetTotal {
    pub id: String,
    pub players: usize,
    pub out_bytes: u64,
    pub in_bytes: u64,
    pub out_kib_s: f64,
    pub in_kib_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetPlayer {
    pub id: String,
    pub index: u32,
    pub joined_t: f64,
    pub left_t: Option<f64>,
    pub out_bytes: u64,
    pub in_bytes: u64,
    pub packets_out: u64,
    pub packets_in: u64,
    pub out_kib_s: f64,
    pub in_kib_s: f64,
    pub peak_out_kib_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuDeviceRow {
    pub id: String,
    pub adapter: String,
    pub backend: GpuBackendName,
    pub driver: String,
    pub parts_mode: GpuPartsModeName,
    pub counters: bool,
    pub fragment_unit: GpuFragmentUnitName,
}

/// A pass's median GPU time per frame, its parts and what it counted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuPassRow {
    pub id: String,
    pub name: String,
    pub source: GpuSourceName,
    pub ms: Option<f64>,
    /// Of the source's passes' total time.
    pub share: Option<f64>,
    pub parts: Vec<GpuPartRow>,
    pub counters: Option<GpuCountersRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuPartRow {
    pub name: String,
    pub ms: f64,
}

/// Medians of one pass's pipeline statistics per counted frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuCountersRow {
    pub total: GpuCountsRow,
    pub pixels: u64,
    /// Shaded pixels per pixel of the pass's target; unset when the device's unit is unknown.
    pub fragments_per_pixel: Option<f64>,
    pub parts: Vec<GpuPartCountsRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuCountsRow {
    pub vertex_shader_invocations: u64,
    pub clipper_invocations: u64,
    pub clipper_primitives_out: u64,
    pub fragment_shader_invocations: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuPartCountsRow {
    pub name: String,
    pub counts: GpuCountsRow,
}

/// What one side recorded: the start's facts (when it has a start), the stop (when it has one) and the sections' sizes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetaRow {
    pub side: SideName,
    pub seconds: f64,
    pub sections: Sections,
    #[serde(flatten)]
    pub start: Option<StartInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopInfo>,
}

/// How many messages of each section a side wrote.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sections {
    pub history: usize,
    pub slow_history: usize,
    pub world: usize,
    pub marks: usize,
    pub warnings: usize,
    pub health: usize,
    pub counters: usize,
    pub slow: usize,
    pub span_blocks: usize,
    pub memory: usize,
    pub spans: usize,
    pub workers: usize,
    pub net: usize,
    pub gpu: usize,
    pub samples: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartInfo {
    pub kind: KindName,
    pub trigger: TriggerName,
    pub started_unix_ms: i64,
    pub uptime_s: f64,
    pub same_process: bool,
    pub players: u32,
    pub world_fs_used_mb: u64,
    pub world_fs_total_mb: u64,
    pub build: Option<BuildInfo>,
    pub machine: Option<MachineInfo>,
    pub options: Option<OptionsInfo>,
    pub settings: Option<SettingsInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_rules: Option<GameRulesInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_options: Option<HorizonOptionsInfo>,
    pub mods: Vec<ModInfo>,
}

/// The game rules the capture recorded; an absent rule is one the game did not fill.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameRulesInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random_tick_speed: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entity_cramming: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire_spread_radius_around_player: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_time: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mob_griefing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_mobs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_monsters: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawner_blocks_work: Option<bool>,
}

/// The Horizon switches that change what a tick costs; an absent switch was off or not recorded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HorizonOptionsInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_view_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lod_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anticheat: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildInfo {
    pub version: String,
    pub commit: String,
    pub profile: String,
    pub target: String,
    pub features: Vec<String>,
    pub symbols: SymbolsName,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineInfo {
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub cpu: String,
    pub cpu_threads: u32,
    pub cpu_cores: u32,
    pub ram_mb: u64,
    pub cgroup_cpu_cores: Option<f64>,
    pub cgroup_memory_mb: Option<u64>,
    pub gpu: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionsInfo {
    pub hz: u32,
    pub threads: ThreadSelectionName,
    pub ticks_over_ms: Option<f64>,
    pub wall: bool,
    pub timeline: bool,
    pub gpu: GpuModeName,
    pub samples: bool,
    pub window_s: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsInfo {
    pub view_distance: Option<u32>,
    pub simulation_distance: Option<u32>,
    pub max_players: Option<u32>,
    pub network_compression_threshold: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModInfo {
    pub id: String,
    pub version: String,
    pub kind: ModKindName,
    #[serde(rename = "crate")]
    pub crate_name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopInfo {
    pub t: f64,
    pub reason: StopReasonName,
    pub overhead: Option<OverheadInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverheadInfo {
    pub spans_ms_per_tick: f64,
    pub spans_share: f64,
    pub sampler_share_of_thread: f64,
    pub writer_core_share: f64,
    pub dropped_samples: u64,
    pub dropped_messages: u64,
    pub timeline_stopped_t: Option<f64>,
}

/// A benchmark report embedded in the capture, read back as the report holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchRow {
    pub id: String,
    pub side: SideName,
    pub format: String,
    pub corpus_id: Option<String>,
    pub scenario: Option<String>,
    pub app_version: Option<String>,
    pub created: Option<String>,
    pub metrics: BTreeMap<String, Summary>,
    pub frames: Vec<Frames>,
    pub ab: Option<Ab>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkRow {
    pub side: SideName,
    pub t: f64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarningRow {
    pub side: SideName,
    pub t: f64,
    pub code: WarningCodeName,
    pub args: Vec<f64>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRow {
    pub id: String,
    pub path: String,
    pub value: Amount,
    pub unit: MemoryUnit,
}

/// A megabyte path with its value in the first snapshot, the second and the change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MbChange(pub String, pub f64, pub f64, pub f64);

/// A count path with its value in the first snapshot, the second and the change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CountChange(pub String, pub Amount, pub Amount, pub Amount);

/// What grew and shrank between two memory snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDiffRow {
    pub from: String,
    pub to: String,
    pub seconds: f64,
    /// Resident megabytes in the first snapshot, the second and the change.
    pub resident_mb: Option<[f64; 3]>,
    pub grew_mb: Vec<MbChange>,
    pub shrank_mb: Vec<MbChange>,
    pub counts: Vec<CountChange>,
    pub only_in_a: Vec<String>,
    pub only_in_b: Vec<String>,
}

/// A function ranked by its own and its inclusive time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotRow {
    pub id: String,
    pub name: String,
    pub full: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    #[serde(rename = "self")]
    pub self_ms: f64,
    pub total: f64,
    pub self_pct: f64,
    pub total_pct: f64,
    pub samples: u64,
}

/// What a tree row left out: rows under the share floor or past the limit, and their time together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Folded {
    pub rows: usize,
    pub total: f64,
}

/// A call-tree row for `tree`, `callers` and `callees`; `callers` rows have no own time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeRow {
    pub id: String,
    pub function: String,
    pub name: String,
    #[serde(rename = "crate", default, skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    pub total: f64,
    #[serde(rename = "self", default, skip_serializing_if = "Option::is_none")]
    pub self_ms: Option<f64>,
    pub pct: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_pct: Option<f64>,
    pub samples: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folded: Option<Folded>,
}

/// A weight divided by one context; a null `key` is the samples that had none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitRow {
    pub by: SampleBy,
    pub key: Option<String>,
    pub label: String,
    pub value: f64,
    pub pct: f64,
    pub samples: u64,
}

/// A function's inclusive and own time, one value per capture asked about; a capture without it has null.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameRow {
    pub id: String,
    pub name: String,
    pub full: String,
    pub values: Vec<Option<f64>>,
    #[serde(rename = "self")]
    pub self_ms: Vec<Option<f64>>,
    pub samples: Vec<Option<u64>>,
}
