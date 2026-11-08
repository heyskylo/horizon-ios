import assert from "node:assert/strict";
import { test } from "node:test";
import { placeTip } from "./tip-place.ts";

const tip = { width: 200, height: 60 };
const view = { width: 1000, height: 800 };
const anchor = (left: number, top: number) => ({ left, top, width: 16, height: 16 });

test("a tooltip sits above its anchor, centred on it", () => {
  // above: 200 - 6 - 60 = 134; centred: 300 + 8 - 100 = 208
  assert.deepEqual(placeTip(anchor(300, 200), tip, view), { left: 208, top: 134, side: "above" });
});

test("with no room above it goes below", () => {
  // above would be 20 - 6 - 60 = -46; below: 20 + 16 + 6 = 42
  assert.deepEqual(placeTip(anchor(300, 20), tip, view), { left: 208, top: 42, side: "below" });
});

test("room above that leaves exactly the margin is still above", () => {
  // 74 - 6 - 60 = 8
  assert.equal(placeTip(anchor(300, 74), tip, view).side, "above");
  assert.equal(placeTip(anchor(300, 73), tip, view).side, "below");
});

test("a tooltip is kept inside the window on both sides", () => {
  // right edge: 1000 - 200 - 8 = 792; left edge: the 8 px margin
  assert.equal(placeTip(anchor(980, 200), tip, view).left, 792);
  assert.equal(placeTip(anchor(0, 200), tip, view).left, 8);
});

test("a tooltip wider than the window starts at the margin", () => {
  assert.equal(placeTip(anchor(40, 200), tip, { width: 150, height: 800 }).left, 8);
});
