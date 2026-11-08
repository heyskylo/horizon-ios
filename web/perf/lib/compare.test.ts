import assert from "node:assert/strict";
import { test } from "node:test";
import { changeMs, changePct, indexChanges, onlyInComparison, rowName, toneOf } from "./compare.ts";
import type { DiffRow } from "./types.ts";

function row(id: string, extra: Partial<DiffRow> = {}): DiffRow {
  return { id, a: 1, b: 1, diff: 0, diff_pct: 0, verdict: "within_noise", ...extra };
}

test("a change prints its sign and the digits that matter", () => {
  assert.equal(changeMs(row("s:a", { diff: 1.0 })), "+1.00 ms");
  assert.equal(changeMs(row("s:a", { diff: -12.8 })), "-12.8 ms");
  assert.equal(changeMs(row("s:a", { diff: 0.042 })), "+0.042 ms");
  assert.equal(changeMs(row("s:a", { diff: -0.5 })), "-0.500 ms");
  assert.equal(changeMs(row("s:a", { diff: 124.04 })), "+124.0 ms");
  assert.equal(changeMs(row("s:a", { diff: 0 })), "0.000 ms");
});

test("a share rounds to a whole percent from ten up", () => {
  assert.equal(changePct(row("s:a", { diff_pct: -23.7 })), "-24%");
  assert.equal(changePct(row("s:a", { diff_pct: 9.46 })), "+9.5%");
  assert.equal(changePct(row("s:a", { diff_pct: 0 })), "0.0%");
});

test("a row only one capture has carries no change", () => {
  const gone = row("f:x", { b: null, diff: null, diff_pct: null, verdict: "gone" });
  assert.equal(changeMs(gone), null);
  assert.equal(changePct(gone), null);
});

test("verdicts group into colours, neutral ones and rows only one side has", () => {
  assert.equal(toneOf("faster"), "better");
  assert.equal(toneOf("slower"), "worse");
  assert.equal(toneOf("within_noise"), "same");
  assert.equal(toneOf("negligible"), "same");
  assert.equal(toneOf("too_few_blocks"), "unknown");
  assert.equal(toneOf("new"), "edge");
  assert.equal(toneOf("gone"), "edge");
});

test("rows are found by their stable id", () => {
  const changes = indexChanges([row("s:tick_lan", { diff: 2 }), row("f:2n7xq4ka")]);
  assert.equal(changes.get("s:tick_lan")?.diff, 2);
  assert.equal(changes.get("s:missing"), undefined);
  assert.equal(changes.size, 2);
});

test("new rows come biggest first, without the ones a view already draws", () => {
  const fresh = (id: string, b: number, extra: Partial<DiffRow> = {}) =>
    row(id, { a: null, b, diff: null, diff_pct: null, verdict: "new", ...extra });
  const rows = [
    fresh("s:small", 0.2),
    fresh("s:big", 3),
    fresh("s:shown", 9),
    fresh("f:other", 7),
    row("s:old", { verdict: "slower" }),
    fresh("s:mid", 1),
  ];
  const ids = onlyInComparison(rows, "s:", new Set(["s:shown"]), 3).map((r) => r.id);
  assert.deepEqual(ids, ["s:big", "s:mid", "s:small"]);
  assert.deepEqual(onlyInComparison(rows, "s:", new Set(), 1).map((r) => r.id), ["s:shown"]);
});

test("a row is called by its function name, else its id without the kind", () => {
  assert.equal(rowName(row("f:2n7xq4ka", { name: "Waypoints::tick_level" })), "Waypoints::tick_level");
  assert.equal(rowName(row("s:tick_lan")), "tick_lan");
});
