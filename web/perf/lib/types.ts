// The shapes `horizon_perf.wasm` replies with (docs/perf-platform.md §6.3), typed for the UI. Only
// the fields the UI reads are listed; the wasm owns the full reply.

export type Side = "server" | "client" | "unspecified";

export interface CaptureInfo {
  file: string;
  sides: Side[];
  kind: string;
  seconds: number;
}

/** What a reply's rows are measured in; the samples queries answer in the first eight. */
export type Unit =
  | "ms_per_tick"
  | "ms_per_frame"
  | "ms_per_slow_tick"
  | "ms_per_slow_frame"
  | "bytes_per_tick"
  | "bytes_per_frame"
  | "bytes_per_slow_tick"
  | "bytes_per_slow_frame"
  | "kib_per_s"
  | "count"
  | "series"
  | "meta"
  | "marks"
  | "warnings"
  | "memory"
  | "memory_diff"
  | "bench";

/** What a samples query looked at: the thread it chose and how many ticks and samples it covered. */
export interface Selection {
  side: Side | null;
  thread: string | null;
  ticks: number | null;
  samples: number | null;
}

export interface Envelope<R> {
  ok: boolean;
  op: string;
  unit?: Unit;
  captures?: CaptureInfo[];
  selection?: Selection;
  rows: R[];
  warnings: string[];
  error?: string | { code: string; message: string };
}

export interface BuildInfo {
  version: string;
  commit: string;
  profile: string;
  target: string;
  features: string[];
  symbols: string;
}

export interface MachineInfo {
  os: string;
  os_version: string;
  arch: string;
  cpu: string;
  cpu_threads: number;
  cpu_cores: number;
  ram_mb: number;
  cgroup_cpu_cores: number | null;
  cgroup_memory_mb: number | null;
  gpu: string;
}

export interface OptionsInfo {
  hz: number;
  threads: string;
  ticks_over_ms: number | null;
  wall: boolean;
  timeline: boolean;
  gpu: string;
  samples: boolean;
  window_s: number | null;
}

export interface SettingsInfo {
  view_distance: number;
  simulation_distance: number;
  max_players: number;
  network_compression_threshold: number;
}

export interface GameRulesInfo {
  random_tick_speed?: number;
  max_entity_cramming?: number;
  fire_spread_radius_around_player?: number;
  advance_time?: boolean;
  mob_griefing?: boolean;
  spawn_mobs?: boolean;
  spawn_monsters?: boolean;
  spawner_blocks_work?: boolean;
}

export interface HorizonOptionsInfo {
  max_view_distance?: number;
  lod_distance?: number;
  anticheat?: boolean;
}

export interface ModInfo {
  id: string;
  version: string;
  kind: string;
  crate_name: string;
}

/** What the profiler itself cost, as `Stop.overhead` records it (the summary's `cost` line). */
export interface OverheadInfo {
  spans_ms_per_tick: number;
  spans_share: number;
  sampler_share_of_thread: number;
  writer_core_share: number;
  dropped_samples: number;
  dropped_messages: number;
  timeline_stopped_t: number | null;
}

export interface MetaRow {
  side: Side;
  seconds: number;
  sections: Record<string, number>;
  kind: string;
  trigger: string;
  started_unix_ms: number;
  uptime_s: number;
  same_process: boolean;
  players: number;
  world_fs_used_mb: number;
  world_fs_total_mb: number;
  build?: BuildInfo | null;
  machine?: MachineInfo | null;
  options?: OptionsInfo | null;
  settings?: SettingsInfo | null;
  game_rules?: GameRulesInfo | null;
  horizon_options?: HorizonOptionsInfo | null;
  mods: ModInfo[];
  stop?: { t: number; reason: string; overhead?: OverheadInfo } | null;
}

export interface SeriesRow {
  field: string;
  unit: string;
  step_s: number;
  t: number[];
  values: (number | null)[];
}

export interface MemoryRow {
  id: string;
  path: string;
  value: number;
  unit: string;
}

export interface NetTotalRow {
  id: string;
  players: number;
  out_bytes: number;
  in_bytes: number;
  out_kib_s: number;
  in_kib_s: number;
}

export interface NetPlayerRow {
  id: string;
  index: number;
  joined_t: number;
  left_t: number | null;
  out_bytes: number;
  in_bytes: number;
  packets_out: number;
  packets_in: number;
  out_kib_s: number;
  in_kib_s: number;
  peak_out_kib_s: number;
}

export interface DimensionRow {
  id: string;
  name: string;
  chunks: number;
  entities: number;
  block_entities: number;
}

export interface MarkRow {
  side: Side;
  t: number;
  name: string;
}

export interface WarningRow {
  side: Side;
  t: number;
  code: string;
  args: number[];
  text: string;
}

export interface SpanRow {
  id: string;
  name: string;
  parent: string | null;
  values: (number | null)[];
  cpu: (number | null)[];
  waited: (number | null)[];
  seen: (number | null)[];
}

/** `[name, depth, wall ms, cpu ms]`: the wasm writes a tick's spans as tuples. */
export type TickSpanRow = [string, number, number, number];

export interface TickRow {
  id: string;
  t: number;
  wall_ms: number;
  cpu_ms: number;
  before_capture: boolean;
  complete: boolean;
  spans: TickSpanRow[];
}

export interface TreeRow {
  id: string;
  function: string;
  name: string;
  /** The crate the function belongs to, when the capture recorded it. */
  crate?: string | null;
  total: number;
  self?: number | null;
  pct: number;
  self_pct?: number | null;
  samples: number;
  children?: TreeRow[];
  folded?: { rows: number; total: number } | null;
}

export interface HotRow {
  id: string;
  name: string;
  full: string;
  file: string | null;
  line: number | null;
  self: number;
  total: number;
  self_pct: number;
  total_pct: number;
  samples: number;
}

/** One group of samples: `key` is null for the samples with no context (no entity, block...). */
export interface GroupRow {
  by: string;
  key: string | null;
  label: string;
  value: number;
  pct: number;
  samples: number;
}

export interface CostRow {
  id: string;
  name: string;
  dimension: string;
  ms_per_tick: number;
  mobs_ticked: number;
  us_per_mob: number;
}

export interface QueryRequest {
  op: string;
  args?: string[];
  by?: string;
  file?: string;
  filters?: Record<string, unknown>;
}

/** One row of a comparison: A is the first capture, B the second. */
export interface DiffRow {
  id: string;
  a: number | null;
  b: number | null;
  diff: number | null;
  diff_pct: number | null;
  verdict: "too_few_blocks" | "within_noise" | "negligible" | "slower" | "faster" | "new" | "gone";
}
