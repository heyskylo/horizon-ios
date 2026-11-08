// What the Summary tab reads from the wasm, and the two things it derives for drawing: the span
// tree (from the `parent` ids `query spans` returns) and bar lengths (a value against the largest
// one). No performance number is computed here; every value is a wasm reply.
import type { HorizonPerf } from "./generated/horizon_perf";
import { fieldsFor, stepFor } from "./health";
import type {
  CostRow,
  Envelope,
  GroupRow,
  HotRow,
  MetaRow,
  Selection,
  SeriesRow,
  Side,
  SpanRow,
  TickRow,
} from "./types";

/** The trailing TPS windows the summary's health line prints, in seconds. */
export const TRAILING_WINDOWS = [60, 300, 900];

export interface Overview {
  seconds: number;
  /** Ticks (or frames) the selection covers, as the replies report them. */
  ticks: number | null;
  samples: number | null;
  series: SeriesRow[];
  /** The last point of each trailing window, in the same order as `TRAILING_WINDOWS`. */
  trailing: (number | null)[];
  spans: SpanRow[];
  hot: HotRow[];
  worst: TickRow[];
  entities: GroupRow[];
  blocks: GroupRow[];
  costs: CostRow[];
}

function count(selection: Selection | undefined, key: "ticks" | "samples"): number | null {
  const value = selection?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * Every reply the Summary tab draws (docs/perf-platform.md §7.2). A capture that lacks a section
 * (no samples, no world snapshot, no spans) answers with an error there; that section's rows are
 * empty and the panel leaves the card out, so one missing section never hides the rest.
 */
export function loadOverview(
  reader: HorizonPerf,
  handle: number,
  name: string,
  side: Side,
  meta: Envelope<MetaRow>,
): Overview {
  const ask = <R,>(
    op: string,
    args: string[] = [],
    filters: Record<string, unknown> = {},
  ): Envelope<R> =>
    reader.query<R>(handle, { op, args, file: name, filters: { side, ...filters } });
  const rows = <R,>(reply: Envelope<R>): R[] => (reply.ok ? reply.rows : []);

  const seconds = meta.rows.find((row) => row.side === side)?.seconds ?? 0;
  const series = ask<SeriesRow>("series", fieldsFor(side), { every: stepFor(seconds) });
  const spans = ask<SpanRow>("spans", [], { limit: 40 });
  const hot = ask<HotRow>("hot", [], { limit: 12 });
  const worst = ask<TickRow>("ticks", [], { worst: 5 });

  const trailing =
    side === "server" && seconds > 0
      ? TRAILING_WINDOWS.map((window) => {
          const reply = ask<SeriesRow>("series", ["h:tps"], {
            from: Math.max(0, seconds - window),
            every: Math.min(window, seconds),
          });
          return reply.ok ? (reply.rows[0]?.values[0] ?? null) : null;
        })
      : [];

  return {
    seconds,
    ticks: count(series.selection, "ticks"),
    samples: count(hot.selection, "samples"),
    series: rows(series),
    trailing,
    spans: rows(spans),
    hot: rows(hot),
    worst: rows(worst),
    entities: rows(ask<GroupRow>("group", [], { by: "entity", limit: 6 })),
    blocks: rows(ask<GroupRow>("group", [], { by: "block", limit: 6 })),
    costs: rows(ask<CostRow>("world", [], { by: "cost", limit: 6 })),
  };
}

export interface PhaseNode {
  id: string;
  name: string;
  /** Wall time per tick or frame, as the wasm reported it. */
  ms: number;
  cpu: number;
  waited: number;
  seen: number;
  /** Share of the tick, over the roots' time together: the CLI's `%MSPT` column. */
  pct: number;
  children: PhaseNode[];
}

/**
 * The span rows as the tree they came from: `parent` names the caller's id, and a row whose parent
 * fell outside the reply's limit is a root. Children are tallest first, as the CLI's phase table is.
 */
export function phaseTree(rows: SpanRow[]): PhaseNode[] {
  const byId = new Map<string, PhaseNode>();
  for (const row of rows) {
    byId.set(row.id, {
      id: row.id,
      name: row.name,
      ms: row.values[0] ?? 0,
      cpu: row.cpu[0] ?? 0,
      waited: row.waited[0] ?? 0,
      seen: row.seen[0] ?? 0,
      pct: 0,
      children: [],
    });
  }
  const roots: PhaseNode[] = [];
  for (const row of rows) {
    const node = byId.get(row.id);
    if (!node) continue;
    const parent = row.parent ? byId.get(row.parent) : undefined;
    if (parent && parent !== node) parent.children.push(node);
    else roots.push(node);
  }
  const total = roots.reduce((sum, node) => sum + node.ms, 0);
  const setPct = (node: PhaseNode) => {
    node.pct = total > 0 ? (node.ms / total) * 100 : 0;
    node.children.sort((a, b) => b.ms - a.ms);
    node.children.forEach(setPct);
  };
  roots.sort((a, b) => b.ms - a.ms);
  roots.forEach(setPct);
  return roots;
}

/** The tree depth-first, each node with its depth, which is how the phase table is drawn. */
export function flatPhases(nodes: PhaseNode[], depth = 0): [PhaseNode, number][] {
  return nodes.flatMap((node) => [[node, depth] as [PhaseNode, number], ...flatPhases(node.children, depth + 1)]);
}

/** A value's bar length against the largest value drawn, never over the whole track. */
export function share(value: number, largest: number): number {
  if (!Number.isFinite(value) || !Number.isFinite(largest) || largest <= 0) return 0;
  return Math.max(0, Math.min(100, (value / largest) * 100));
}

/** The largest value of a list, for `share`. */
export function largest(values: number[]): number {
  return values.reduce((top, value) => (Number.isFinite(value) && value > top ? value : top), 0);
}

/** Groups with no context (`key === null`: the CLI's omitted "(none)" row) dropped. */
export function attributed(rows: GroupRow[]): GroupRow[] {
  return rows.filter((row) => row.key !== null);
}

/** The spans that made a tick slow: the root's children, tallest first, as the CLI lists them. */
export function topSpans(tick: TickRow, count = 3) {
  const depths = tick.spans.map((span) => span[1]);
  const shallowest = Math.min(...depths);
  const deeper = depths.filter((depth) => depth > shallowest);
  const from = deeper.length > 0 ? Math.min(...deeper) : shallowest;
  return tick.spans
    .filter((span) => span[1] === from)
    .slice()
    .sort((a, b) => b[2] - a[2])
    .slice(0, count);
}
