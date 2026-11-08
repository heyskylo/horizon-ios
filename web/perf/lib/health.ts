// Reading the health series the wasm computed. This module only picks values out of `series` rows
// and orders them; min, median, p95 and max of a returned series are presentation, not a second
// implementation of the capture's statistics.
import type { SeriesRow, Side } from "./types";

export interface Stats {
  min: number;
  mean: number;
  median: number;
  p95: number;
  max: number;
  last: number;
  count: number;
}

/** The finite values of a series, in time order (the last one is `last`). */
export function finite(values: (number | null)[]): number[] {
  return values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
}

export function stats(values: (number | null)[]): Stats | null {
  const seen = finite(values);
  if (seen.length === 0) return null;
  const sorted = [...seen].sort((a, b) => a - b);
  const at = (fraction: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(fraction * (sorted.length - 1)))];
  const sum = seen.reduce((total, value) => total + value, 0);
  return {
    min: sorted[0],
    mean: sum / seen.length,
    median: at(0.5),
    p95: at(0.95),
    max: sorted[sorted.length - 1],
    last: seen[seen.length - 1],
    count: seen.length,
  };
}

export function byField(rows: SeriesRow[]): Map<string, SeriesRow> {
  return new Map(rows.map((row) => [row.field, row]));
}

/** The health fields of a side, in the order the bar shows them. */
export function fieldsFor(side: Side): string[] {
  if (side === "client") {
    return [
      "h:fps",
      "h:frame_ms_p50",
      "h:frame_ms_p95",
      "h:frame_ms_p99",
      "h:frame_ms_max",
      "h:frame_cpu_ms_p50",
      "h:frame_gpu_ms_p50",
      "h:cpu_process_cores",
      "h:cpu_system_cores",
      "h:rss_mb",
      "h:physical_used_mb",
      "h:swap_used_mb",
      "h:allocs_per_s",
      "h:alloc_mib_per_s",
      "h:allocator_commit_mb",
    ];
  }
  return [
    "h:tps",
    "h:mspt_avg",
    "h:mspt_p50",
    "h:mspt_p95",
    "h:mspt_max",
    "h:cpu_process_cores",
    "h:cpu_system_cores",
    "h:rss_mb",
    "h:physical_used_mb",
    "h:swap_used_mb",
    "h:allocs_per_s",
    "h:alloc_mib_per_s",
    "h:allocator_commit_mb",
    "h:players",
    "h:chunks",
    "h:entities",
  ];
}

/** How many seconds one point of the timeline stands for, so a long capture stays under 2000 points. */
export function stepFor(seconds: number): number {
  return Math.max(1, Math.ceil(seconds / 2000));
}

/** The drawn shape of a series as an SVG path in a `width`×`height` box. */
export function sparkline(
  values: (number | null)[],
  width: number,
  height: number,
): { path: string; min: number; max: number } | null {
  const seen = finite(values);
  if (seen.length === 0) return null;
  const min = seen.reduce((a, b) => Math.min(a, b));
  const max = seen.reduce((a, b) => Math.max(a, b));
  const span = max - min || 1;
  // A flat series (a constant core count, a TPS pinned at 20) draws through the middle rather than
  // along the bottom edge, where it would read as zero.
  const flat = max === min;
  const step = seen.length > 1 ? width / (seen.length - 1) : 0;
  const points = seen.map((value, index) => {
    const x = index * step;
    const y = flat ? height / 2 : height - ((value - min) / span) * height;
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  return { path: `M ${points.join(" L ")}`, min, max };
}
