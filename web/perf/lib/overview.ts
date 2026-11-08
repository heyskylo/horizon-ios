// What the Summary tab reads from the wasm; it derives only drawing (span tree, bar lengths), never a performance number.
import type { HorizonPerf } from "./generated/horizon_perf";
import { fieldsFor, stepFor } from "./health";
import { windowsWithin } from "./trailing";
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
} from "./types";

export interface Overview {
  seconds: number;
  /** Ticks (or frames) the selection covers, as the replies report them. */
  ticks: number | null;
  samples: number | null;
  series: SeriesRow[];
  /** The last point of each trailing window the capture can fill, in the order of `windowsWithin`. */
  trailing: (number | null)[];
  spans: SpanRow[];
  hot: HotRow[];
  entities: GroupRow[];
  blocks: GroupRow[];
  costs: CostRow[];
}

function count(selection: Selection | undefined, key: "ticks" | "samples"): number | null {
  const value = selection?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Every reply the Summary tab draws; a section the capture lacks has empty rows and its card is left out. */
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
  const series = ask<SeriesRow>("series", fieldsFor(side), { every: stepFor(seconds), stats: true });
  const spans = ask<SpanRow>("spans", [], { limit: 40 });
  const hot = ask<HotRow>("hot", [], { limit: 12 });

  const trailing =
    side === "server" && seconds > 0
      ? windowsWithin(seconds).map(({ seconds: window }) => {
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

/** The span rows as a tree by `parent`; a row whose parent fell outside the reply's limit is a root, children tallest first. */
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
