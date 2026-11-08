import assert from "node:assert/strict";
import { test } from "node:test";
import { tabsFor } from "./tabs.ts";

const answering = (...queries: string[]) => (query: string) => queries.includes(query);

test("a side only offers the tabs whose query has rows", () => {
  assert.deepEqual(tabsFor(answering("spans", "ticks", "hot", "tree"), true), [
    "summary",
    "metadata",
    "spans",
    "ticks",
    "net",
    "hot",
    "tree",
    "compare",
    "query",
  ]);
});

test("a capture without a benchmark report has no Bench tab, and one with a report has it before Marks", () => {
  assert.ok(!tabsFor(answering("marks"), false).includes("bench"));
  const tabs = tabsFor(answering("bench", "marks"), false);
  assert.equal(tabs.indexOf("bench") + 1, tabs.indexOf("marks"));
});

test("a server capture has no GPU tab when the gpu query has no rows", () => {
  assert.ok(!tabsFor(answering("hot"), false).includes("gpu"));
});

test("network needs real traffic, not just a recorded section", () => {
  assert.ok(!tabsFor(answering(), false).includes("net"));
});

test("an empty side still offers what needs no data", () => {
  assert.deepEqual(tabsFor(answering(), false), ["summary", "metadata", "compare", "query"]);
});
