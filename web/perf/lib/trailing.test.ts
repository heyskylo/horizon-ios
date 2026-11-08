import assert from "node:assert/strict";
import { test } from "node:test";
import { windowsWithin } from "./trailing.ts";

const lengths = (seconds: number) => windowsWithin(seconds).map((window) => window.seconds);

test("a capture offers only the windows it is long enough to fill", () => {
  assert.deepEqual(lengths(3.6), []);
  assert.deepEqual(lengths(59.9), []);
  assert.deepEqual(lengths(60), [60]);
  assert.deepEqual(lengths(400), [60, 300]);
  assert.deepEqual(lengths(3600), [60, 300, 900]);
});
