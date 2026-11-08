import assert from "node:assert/strict";
import { test } from "node:test";
import { mergeTree } from "./merge-tree.ts";
import type { TreeRow } from "./types.ts";

function row(name: string, total: number, children: TreeRow[] = []): TreeRow {
  return { id: `p:${name}:${total}`, function: `f:${name}`, name, total, self: null, pct: total, samples: total, children };
}

function shape(rows: TreeRow[]): unknown[] {
  return rows.map((r) => [r.name, r.total, shape(r.children ?? [])]);
}

test("a wrapper is hidden and what it called stays", () => {
  const merged = mergeTree([row("FnOnce::call_once", 100, [row("tick", 90)])]);
  assert.deepEqual(shape(merged), [["tick", 90, []]]);
});

test("one function called from two paths is one row with both callers' time", () => {
  const merged = mergeTree([row("a", 60, [row("shared", 40, [row("leaf", 10)])]), row("b", 30, [row("shared", 20, [row("leaf", 5)])])]);
  assert.deepEqual(shape(merged), [
    ["a", 60, [["shared", 40, [["leaf", 10, []]]]]],
    ["shared", 60, [["leaf", 15, []]]],
    ["b", 30, [["shared", 20, [["leaf", 5, []]]]]],
    ["leaf", 15, []],
  ]);
});

test("a recursive function counts its outermost call only", () => {
  const merged = mergeTree([row("walk", 50, [row("walk", 30, [row("walk", 10)])])]);
  assert.deepEqual(merged[0].total, 50);
  assert.equal(merged.length, 1);
});

test("merged rows add their own-time shares and keep the crate of the function", () => {
  const a: TreeRow = { ...row("shared", 40), crate: "horizon_world", self: 3, self_pct: 1.5 };
  const b: TreeRow = { ...row("shared", 20), crate: "horizon_world", self: 1, self_pct: 2.5 };
  const merged = mergeTree([row("a", 60, [a]), row("b", 30, [b])]);
  const shared = merged.find((r) => r.name === "shared");
  assert.equal(shared?.self_pct, 4);
  assert.equal(shared?.self, 4);
  assert.equal(shared?.crate, "horizon_world");
});
