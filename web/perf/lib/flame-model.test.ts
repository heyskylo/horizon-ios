import assert from "node:assert/strict";
import { test } from "node:test";
import { fitText, frameAt, layoutFlame, moveFrom, nameMatches, zoomPath } from "./flame-model.ts";
import type { TreeRow } from "./types.ts";

function row(name: string, total: number, children: TreeRow[] = []): TreeRow {
  return { id: `p:${name}`, function: `f:${name}`, name, total, self: null, pct: total, samples: total, children, folded: null };
}

// Two tops of 80 and 20 ms: the first calls a (50: a1 30, a2 15) and b (20).
const rows = [row("tick", 80, [row("a", 50, [row("a1", 30), row("a2", 15)]), row("b", 20)]), row("other", 20)];

const near = (actual: number, expected: number) => assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} is not ${expected}`);

function box(layout: ReturnType<typeof layoutFlame>, key: string): [number, number, number] {
  const frame = layout.byKey.get(key);
  assert.ok(frame, `no frame ${key}`);
  return [frame.depth, frame.start, frame.width];
}

test("frames are as wide as their time, side by side, with children inside their parent", () => {
  const layout = layoutFlame(rows, null, 0);
  const expected: Record<string, [number, number, number]> = {
    "0": [0, 0, 0.8],
    "1": [0, 0.8, 0.2],
    "0.0": [1, 0, 0.5],
    "0.1": [1, 0.5, 0.2],
    "0.0.0": [2, 0, 0.3],
    "0.0.1": [2, 0.3, 0.15],
  };
  for (const [key, [depth, start, width]] of Object.entries(expected)) {
    const [d, s, w] = box(layout, key);
    assert.equal(d, depth);
    near(s, start);
    near(w, width);
  }
  assert.equal(layout.byKey.size, 6);
  assert.deepEqual(layout.levels.map((level) => level.map((frame) => frame.row.name)), [["tick", "other"], ["a", "b"], ["a1", "a2"]]);
});

test("a zoomed frame fills the width at depth 0 and keeps the keys it had", () => {
  const layout = layoutFlame(rows, "0.0", 0);
  assert.deepEqual([...layout.byKey.keys()], ["0.0", "0.0.0", "0.0.1"]);
  assert.deepEqual(box(layout, "0.0").map(Math.round), [0, 0, 1]);
  const [depth, start, width] = box(layout, "0.0.1");
  assert.equal(depth, 1);
  near(start, 0.6);
  near(width, 0.3);
});

test("a frame narrower than the minimum is left out with everything under it", () => {
  const layout = layoutFlame(rows, null, 0.19);
  assert.deepEqual([...layout.byKey.keys()].sort(), ["0", "0.0", "0.0.0", "0.1", "1"]);
  assert.deepEqual(layout.byKey.get("0.0")?.children, ["0.0.0"]);
});

test("a child that outweighs its parent is cut at the parent's edge", () => {
  const layout = layoutFlame([row("p", 10, [row("big", 12)]), row("q", 10)], null, 0);
  const [, start, width] = box(layout, "0.0");
  near(start, 0);
  near(width, 0.5);
});

test("rows without time draw nothing", () => {
  assert.equal(layoutFlame([row("idle", 0)], null, 0).byKey.size, 0);
  assert.equal(layoutFlame([], null, 0).byKey.size, 0);
});

test("a zoom that no longer points at a row shows the whole graph", () => {
  assert.equal(layoutFlame(rows, "5.1", 0).byKey.size, 6);
});

test("the path out of a zoom lists every frame from the top", () => {
  assert.deepEqual(zoomPath(rows, "0.0.1").map((step) => [step.key, step.row.name]), [
    ["0", "tick"],
    ["0.0", "a"],
    ["0.0.1", "a2"],
  ]);
  assert.deepEqual(zoomPath(rows, null), []);
  assert.deepEqual(zoomPath(rows, "0.7"), []);
});

test("a point finds the frame covering it, and nothing in a gap or past the end", () => {
  const layout = layoutFlame(rows, null, 0);
  assert.equal(frameAt(layout, 0, 0.79)?.row.name, "tick");
  assert.equal(frameAt(layout, 0, 0.8)?.row.name, "other");
  assert.equal(frameAt(layout, 1, 0.55)?.row.name, "b");
  assert.equal(frameAt(layout, 1, 0.75), null);
  assert.equal(frameAt(layout, 2, 0.4)?.row.name, "a2");
  assert.equal(frameAt(layout, 2, 0.46), null);
  assert.equal(frameAt(layout, 3, 0.1), null);
});

test("arrow moves lead to the parent, the biggest child and the neighbours", () => {
  const layout = layoutFlame(rows, null, 0);
  assert.equal(moveFrom(layout, "0.0.1", "parent"), "0.0");
  assert.equal(moveFrom(layout, "0", "parent"), null);
  assert.equal(moveFrom(layout, "0", "child"), "0.0");
  assert.equal(moveFrom(layout, "0.0.1", "child"), null);
  assert.equal(moveFrom(layout, "0.0", "next"), "0.1");
  assert.equal(moveFrom(layout, "0.1", "next"), null);
  assert.equal(moveFrom(layout, "0.1", "previous"), "0.0");
  assert.equal(moveFrom(layout, "0", "next"), "1");
  assert.equal(moveFrom(layout, "1", "previous"), "0");
  assert.equal(moveFrom(layout, "9", "next"), null);
});

// Every character is 10 px wide.
const measure = (text: string) => text.length * 10;

test("a name that fits stays, a longer one is cut with an ellipsis, and a sliver shows nothing", () => {
  assert.equal(fitText("tick", 40, measure), "tick");
  assert.equal(fitText("tick_world", 55, measure), "tick…");
  assert.equal(fitText("tick_world", 20, measure), "t…");
  assert.equal(fitText("tick_world", 10, measure), "…");
  assert.equal(fitText("tick_world", 9, measure), "");
});

test("a search matches part of a name regardless of case, and an empty search matches nothing", () => {
  assert.ok(nameMatches("Game::tick_world", "TICK"));
  assert.ok(!nameMatches("Game::tick_world", "chunk"));
  assert.ok(!nameMatches("Game::tick_world", "  "));
});
