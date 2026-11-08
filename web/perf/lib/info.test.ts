import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { infoKey, isId, spanId } from "./info.ts";

const messages: Record<string, string> = JSON.parse(
  readFileSync(new URL("../lang/en_us.json", import.meta.url), "utf8"),
);

test("ids are a kind letter, a colon and a key", () => {
  for (const id of ["s:tick_blocks", "f:2n7xq4ka", "k:48812", "p:k3m0d9tr", "e:minecraft:pig", "c:overworld/12,-4"]) {
    assert.ok(isId(id), id);
  }
  for (const text of ["", "tick_blocks", "s:", "ss:tick", "S:tick", "s: tick", null, undefined]) {
    assert.ok(!isId(text), String(text));
  }
});

test("a tick's span name becomes the span's id", () => {
  assert.equal(spanId("tick_lan"), "s:tick_lan");
});

test("an id's explanation lives under info.<id>", () => {
  assert.equal(infoKey("f:kq3h1qx3"), "info.f:kq3h1qx3");
});

test("every explanation is keyed by a span name or a function id of eight base32 characters", () => {
  const keys = Object.keys(messages).filter((key) => key.startsWith("info.") && key.includes(":"));
  assert.ok(keys.length > 40);
  for (const key of keys) {
    const id = key.slice("info.".length);
    assert.match(id, /^(s:[a-z][a-z0-9_ ]*|f:[0-9a-hjkmnp-tv-z]{8})$/, key);
    assert.ok(messages[key].trim().length > 0, key);
  }
});
