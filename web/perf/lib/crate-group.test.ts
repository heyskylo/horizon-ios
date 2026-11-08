import assert from "node:assert/strict";
import { test } from "node:test";
import { crateGroup, groupsIn, modCrates } from "./crate-group.ts";
import type { ModInfo, TreeRow } from "./types.ts";

const mod = (id: string, kind: ModInfo["kind"], crate: string): ModInfo => ({ id, version: "1", kind, crate });
const none = new Set<string>();

test("the game's own crates split into world, client and the rest", () => {
  assert.equal(crateGroup("horizon_world", none), "world");
  assert.equal(crateGroup("horizon_client", none), "client");
  assert.equal(crateGroup("horizon_client_render", none), "client");
  assert.equal(crateGroup("horizon_core", none), "horizon");
  assert.equal(crateGroup("horizon_perf_format", none), "horizon");
});

test("the standard library is one group, and any other crate is third-party", () => {
  for (const name of ["std", "core", "alloc"]) assert.equal(crateGroup(name, none), "std");
  assert.equal(crateGroup("hashbrown", none), "third");
  assert.equal(crateGroup("libnvidia-eglcore.so.595.112.07", none), "third");
});

test("a frame with no recorded crate has no group", () => {
  assert.equal(crateGroup(null, none), null);
  assert.equal(crateGroup(undefined, none), null);
  assert.equal(crateGroup("", none), null);
});

test("the legend lists the groups the tree has, deep rows included, in a fixed order", () => {
  const row = (crate: string | null, children: TreeRow[] = []): TreeRow => ({
    id: "p:x",
    function: "f:x",
    name: "x",
    crate,
    total: 1,
    pct: 1,
    samples: 1,
    children,
  });
  const rows = [row("horizon_client", [row("serde_json", [row("horizon_world")]), row(null)])];
  assert.deepEqual(groupsIn(rows, none), ["world", "client", "third"]);
  assert.deepEqual(groupsIn([], none), []);
});

test("a native mod's crate is a mod, whichever way cargo spelled it", () => {
  const mods = modCrates([mod("big-farm", "native", "big-farm"), mod("data", "content", ""), mod("lib", "content", "horizon_world")]);
  assert.equal(crateGroup("big_farm", mods), "mod");
  assert.equal(crateGroup("big-farm", mods), "mod");
  assert.equal(crateGroup("horizon_world", mods), "world");
  assert.equal(mods.size, 1);
});
