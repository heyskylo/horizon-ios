import type { Envelope, GroupRow } from "./types";

/** Which samples the tree weighs, as the reader picks them. */
export type SampleSelection = "all" | "slow" | "running" | "waiting" | "alloc" | "live";

export const SAMPLE_SELECTIONS: readonly SampleSelection[] = ["all", "slow", "running", "waiting", "alloc", "live"];

type WireSelect = "cpu" | "running" | "waiting" | "alloc" | "live";

const SELECTIONS: Record<SampleSelection, { select: WireSelect; slow: boolean }> = {
  all: { select: "cpu", slow: false },
  slow: { select: "cpu", slow: true },
  running: { select: "running", slow: false },
  waiting: { select: "waiting", slow: false },
  alloc: { select: "alloc", slow: false },
  live: { select: "live", slow: false },
};

/** What the tree is divided by before it is a tree. */
export type GroupKind = "none" | "thread" | "span" | "entity" | "block" | "chunk" | "dimension";

export const GROUP_KINDS: readonly GroupKind[] = ["none", "thread", "span", "entity", "block", "chunk", "dimension"];

export type ContextKey = "span" | "entity" | "block" | "chunk" | "dim";

/** The contexts a sample can carry; a node's weight can be split by each. */
export const CONTEXT_KEYS: readonly ContextKey[] = ["span", "entity", "block", "chunk", "dim"];

const CONTEXT_OF: Record<Exclude<GroupKind, "none" | "thread">, ContextKey> = {
  span: "span",
  entity: "entity",
  block: "block",
  chunk: "chunk",
  dimension: "dim",
};

/** The filters of a samples query: the selection, the thread, and what a group narrowed it to. */
export interface SampleFilters {
  select: WireSelect;
  slow?: boolean;
  thread?: string;
  where?: Partial<Record<ContextKey, string[]>>;
}

export function sampleFilters(selection: SampleSelection, thread: string | null): SampleFilters {
  const { select, slow } = SELECTIONS[selection];
  return { select, ...(slow ? { slow } : {}), ...(thread === null ? {} : { thread }) };
}

/** The filters of the samples inside one group of a `group` reply; a group without a context cannot be narrowed to. */
export function groupScope(group: GroupKind, row: Pick<GroupRow, "key" | "label">): Partial<SampleFilters> | null {
  if (group === "none" || row.key === null) return null;
  if (group === "thread") return { thread: row.label };
  return { where: { [CONTEXT_OF[group]]: [row.key] } };
}

/** What a capture's samples can be shown as. */
export interface SampleOptions {
  selections: SampleSelection[];
  groups: GroupKind[];
  /** The threads with samples, when there is more than one to pick from. */
  threads: string[];
  /** The thread a query without one reads. */
  thread: string | null;
}

export type Ask = <R>(op: string, filters: Record<string, unknown>) => Envelope<R>;

/** Probes the capture with the queries the tree itself runs: an option is offered only when it answers with samples. */
export function probeOptions(ask: Ask): SampleOptions {
  const selections: SampleSelection[] = [];
  let thread: string | null = null;
  for (const id of SAMPLE_SELECTIONS) {
    const reply = ask<unknown>("tree", { ...sampleFilters(id, null), depth: 0, limit: 1 });
    if (!reply.ok || (reply.selection?.samples ?? 0) === 0) continue;
    selections.push(id);
    thread ??= reply.selection?.thread ?? null;
  }
  const groups: GroupKind[] = ["none"];
  let threads: string[] = [];
  if (selections.length > 0) {
    const filters = sampleFilters(selections[0], null);
    const rowsOf = (by: string) => {
      const reply = ask<GroupRow>("group", { ...filters, by, limit: 50 });
      return reply.ok ? reply.rows : [];
    };
    const named = rowsOf("thread");
    if (named.length > 1) {
      threads = named.map((row) => row.label);
      groups.push("thread");
    }
    for (const kind of GROUP_KINDS) {
      if (kind === "none" || kind === "thread") continue;
      if (rowsOf(kind).some((row) => row.key !== null)) groups.push(kind);
    }
  }
  return { selections, groups, threads, thread };
}
