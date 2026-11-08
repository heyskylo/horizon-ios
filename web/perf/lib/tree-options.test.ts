import assert from "node:assert/strict";
import { test } from "node:test";
import { groupScope, probeOptions, sampleFilters } from "./tree-options.ts";
import type { Ask } from "./tree-options.ts";
import type { Envelope, GroupRow, Selection } from "./types.ts";

const selection = (samples: number, thread = "tick"): Selection => ({ side: "server", thread, ticks: 10, samples });

function reply<R>(rows: R[], samples = rows.length): Envelope<R> {
  return { ok: true, op: "x", rows, warnings: [], selection: selection(samples) };
}

const failure = <R,>(): Envelope<R> => ({ ok: false, op: "x", rows: [], warnings: [], error: "no samples of that kind" });

const group = (by: string, key: string | null, label: string): GroupRow => ({ by, key, label, value: 1, pct: 50, samples: 5 });

// A capture with cpu samples and some slow ticks, two threads, spans and a dimension, but no entities, chunks or blocks.
const ask: Ask = <R,>(op: string, filters: Record<string, unknown>): Envelope<R> => {
  if (op === "tree") {
    if (filters.select !== "cpu") return failure();
    return reply<unknown>([], filters.slow ? 12 : 275) as Envelope<R>;
  }
  const rows: Record<string, GroupRow[]> = {
    thread: [group("thread", "t:tick", "tick"), group("thread", "t:worker", "worker")],
    span: [group("span", "s:tick_blocks", "tick_blocks"), group("span", null, "(none)")],
    entity: [group("entity", null, "(none)")],
    block: [],
    chunk: [],
    dimension: [group("dim", "d:overworld", "overworld")],
  };
  return reply(rows[String(filters.by)] ?? []) as Envelope<R>;
};

test("only the selections that have samples are offered", () => {
  assert.deepEqual(probeOptions(ask).selections, ["all", "slow"]);
});

test("a group by is offered when some sample has that context, and threads when several have samples", () => {
  const options = probeOptions(ask);
  assert.deepEqual(options.groups, ["none", "thread", "span", "dimension"]);
  assert.deepEqual(options.threads, ["tick", "worker"]);
  assert.equal(options.thread, "tick");
});

test("slow ticks need samples inside them, not just the option to ask", () => {
  const noSlow: Ask = <R,>(op: string, filters: Record<string, unknown>): Envelope<R> =>
    filters.slow ? (reply<unknown>([], 0) as Envelope<R>) : ask<R>(op, filters);
  assert.deepEqual(probeOptions(noSlow).selections, ["all"]);
});

test("a capture without samples offers nothing to group or pick", () => {
  const empty: Ask = <R,>(): Envelope<R> => failure<R>();
  assert.deepEqual(probeOptions(empty), { selections: [], groups: ["none"], threads: [], thread: null });
});

test("filters carry only what the reader changed from the defaults", () => {
  assert.deepEqual(sampleFilters("all", null), { select: "cpu" });
  assert.deepEqual(sampleFilters("slow", "worker"), { select: "cpu", slow: true, thread: "worker" });
  assert.deepEqual(sampleFilters("waiting", null), { select: "waiting" });
});

test("a group narrows the tree to its own samples", () => {
  assert.deepEqual(groupScope("span", { key: "s:tick_blocks", label: "tick_blocks" }), { where: { span: ["s:tick_blocks"] } });
  assert.deepEqual(groupScope("dimension", { key: "d:overworld", label: "overworld" }), { where: { dim: ["d:overworld"] } });
  assert.deepEqual(groupScope("thread", { key: "t:worker", label: "worker" }), { thread: "worker" });
  assert.equal(groupScope("entity", { key: null, label: "(none)" }), null);
  assert.equal(groupScope("none", { key: "x", label: "x" }), null);
});
