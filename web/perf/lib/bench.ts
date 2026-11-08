// Drawing helpers for a benchmark report: they order and scale what the reader returned and never compute a performance number.

import type { BenchAb, BenchFrames, BenchVerdict } from "./types.ts";

/** A phase block with the widths of its bar, as fractions of the slowest block's 99th percentile. */
export interface Spread {
  frames: BenchFrames;
  typical: number;
  tail: number;
}

/** Every block of a report on one shared scale, so the rows can be compared by eye. */
export function spreads(blocks: readonly BenchFrames[]): Spread[] {
  const scale = blocks.reduce((m, b) => Math.max(m, b.p99_ms), 0) || 1;
  return blocks.map((frames) => ({ frames, typical: frames.p50_ms / scale, tail: frames.p99_ms / scale }));
}

/** The unit a metric's name ends in: the report names its metrics `startup.title_ms`, `memory.rss_peak_mb`. */
export function metricUnit(name: string): string {
  const suffixes: [string, string][] = [
    ["_per_s", "/s"],
    ["_fps", "FPS"],
    ["_ms", "ms"],
    ["_mb", "MiB"],
    ["_pct", "%"],
    ["_s", "s"],
  ];
  return suffixes.find(([suffix]) => name.endsWith(suffix))?.[1] ?? "";
}

/** A block's name for a reader: an A/B block is called by the setting it ran with. */
export function phaseLabel(phase: string, ab: BenchAb | null): string {
  if (ab && phase === "ab.a") return `${ab.switch} ${ab.a}`;
  if (ab && phase === "ab.b") return `${ab.switch} ${ab.b}`;
  return phase;
}

/** Where each block stands among the blocks of its phase, in report order: `n` of `total`. */
export function ordinals(blocks: readonly BenchFrames[]): { n: number; total: number }[] {
  const totals = new Map<string, number>();
  for (const block of blocks) totals.set(block.phase, (totals.get(block.phase) ?? 0) + 1);
  const seen = new Map<string, number>();
  return blocks.map((block) => {
    const n = (seen.get(block.phase) ?? 0) + 1;
    seen.set(block.phase, n);
    return { n, total: totals.get(block.phase) ?? 1 };
  });
}

/** `2026-11-08T12:00:00Z` as `2026-11-08 12:00 UTC`; anything else is shown as it is. */
export function stamp(created: string): string {
  const match = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2}):\d{2}Z$/.exec(created);
  return match ? `${match[1]} ${match[2]} UTC` : created;
}

/** An explicit sign, so a change reads as one. */
export function signed(value: number, digits: number): string {
  const text = value.toFixed(digits);
  if (Number(text) === 0) return (0).toFixed(digits);
  return value > 0 ? `+${text}` : text;
}

const ORDER = ["frame_p50_ms", "cpu_p50_ms", "gpu_p50_ms"];

/** The A/B results with the frame time first, then CPU, then GPU; unknown names follow alphabetically. */
export function abMetrics<T>(result: Record<string, T>): [string, T][] {
  const rank = (name: string) => (ORDER.includes(name) ? ORDER.indexOf(name) : ORDER.length);
  return Object.entries(result).sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b));
}

export type Tone = "good" | "bad" | "neutral";

/** Lower milliseconds are better: faster is good, slower is bad, the rest is no news. */
export function verdictTone(verdict: BenchVerdict): Tone {
  if (verdict === "faster") return "good";
  if (verdict === "slower") return "bad";
  return "neutral";
}
