import assert from "node:assert/strict";
import { test } from "node:test";
import { amount, amountKind, bytes } from "./units.ts";

test("bytes pick the largest step that stays at or above one", () => {
  assert.equal(bytes(0), "0 B");
  assert.equal(bytes(1023), "1023 B");
  assert.equal(bytes(1536), "1.5 KiB");
  assert.equal(bytes(3 * 1024 * 1024), "3.0 MiB");
  assert.equal(bytes(5 * 1024 ** 4), "5120.0 GiB");
});

test("allocation replies are bytes and every other selection is time", () => {
  assert.equal(amountKind("bytes_per_slow_tick"), "bytes");
  assert.equal(amountKind("ms_per_frame"), "time");
  assert.equal(amountKind(undefined), "time");
});

test("a weight prints with its unit", () => {
  assert.equal(amount(8.754, "time"), "8.75 ms");
  assert.equal(amount(2048, "bytes"), "2.0 KiB");
});
