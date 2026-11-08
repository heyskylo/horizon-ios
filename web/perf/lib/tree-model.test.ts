import assert from "node:assert/strict";
import { test } from "node:test";
import { ALL_CLOSED, HOT_OPEN, buildNodes, entry, findNode, flatten, highlight, hottestPath, isOpen, last, search, toggled } from "./tree-model.ts";
import type { TreeRow } from "./types.ts";

function row(name: string, total: number, self: number, children: TreeRow[] = [], folded: TreeRow["folded"] = null): TreeRow {
  return { id: `p:${name}`, function: `f:${name}`, name, total, self, pct: total, samples: total, children, folded };
}

// tick > run > level (10 ms of its own) calls mob_ai > path_find (60) and sound (30), and left 3 rows of 0.4 ms out.
const rows = [
  row("tick", 100, 0, [
    row("run", 100, 0, [
      row("level", 100, 10, [row("mob_ai", 60, 0, [row("path_find", 60, 60)]), row("sound", 30, 30)], { rows: 3, total: 0.4 }),
    ]),
  ]),
];

const names = (node: { members: TreeRow[] }) => node.members.map((member) => member.name);

test("a function with one child and no time of its own shares a line with it", () => {
  const nodes = buildNodes(rows);
  assert.equal(nodes.length, 1);
  assert.deepEqual(names(nodes[0]), ["tick", "run", "level"]);
  assert.equal(entry(nodes[0]).total, 100);
  assert.equal(last(nodes[0]).self, 10);
  assert.deepEqual(nodes[0].children.map(names), [["mob_ai", "path_find"], ["sound"]]);
  assert.deepEqual(nodes[0].folded, { rows: 3, total: 0.4 });
});

test("time of its own or a folded sibling keeps a function on its own line", () => {
  const own = buildNodes([row("a", 10, 2, [row("b", 8, 8)])]);
  assert.deepEqual(names(own[0]), ["a"]);
  const folded = buildNodes([row("a", 10, 0, [row("b", 8, 8)], { rows: 1, total: 0.01 })]);
  assert.deepEqual(names(folded[0]), ["a"]);
  assert.equal(folded[0].children.length, 1);
});

test("only the children of open lines are listed, and a fold row ends each open list", () => {
  const nodes = buildNodes(rows);
  const shut = flatten(nodes, () => false);
  assert.equal(shut.length, 1);
  const open = flatten(nodes, (node) => node.key === nodes[0].key);
  assert.deepEqual(
    open.map((flat) => [flat.kind, flat.depth, flat.kind === "node" ? names(flat.node).join(">") : `${flat.folded.rows} smaller`]),
    [
      ["node", 0, "tick>run>level"],
      ["node", 1, "mob_ai>path_find"],
      ["node", 1, "sound"],
      ["fold", 1, "3 smaller"],
    ],
  );
  assert.equal(open[0].kind === "node" && open[0].open, true);
});

test("a search finds a function inside a chain and opens the lines above it", () => {
  const nodes = buildNodes(rows);
  const deep = search(nodes, "PATH");
  assert.deepEqual([...deep.hits], [nodes[0].children[0].key]);
  assert.deepEqual([...deep.parents], [nodes[0].key]);
  const inChain = search(nodes, "level");
  assert.deepEqual([...inChain.hits], [nodes[0].key]);
  assert.equal(inChain.parents.size, 0);
  assert.equal(search(nodes, "  ").hits.size, 0);
  assert.equal(search(nodes, "nothing like it").hits.size, 0);
});

test("highlight cuts every occurrence whatever its case", () => {
  assert.deepEqual(highlight("Path_find path", "path"), [
    { text: "Path", hit: true },
    { text: "_find ", hit: false },
    { text: "path", hit: true },
  ]);
  assert.deepEqual(highlight("sound", ""), [{ text: "sound", hit: false }]);
});

test("the hottest path takes the biggest child at every level", () => {
  const nodes = buildNodes(rows);
  assert.deepEqual([...hottestPath(nodes)], [nodes[0].key, nodes[0].children[0].key]);
});

test("a toggle beats a search, and a search beats the starting point", () => {
  assert.equal(isOpen(HOT_OPEN, "k", true, false), true);
  assert.equal(isOpen(HOT_OPEN, "k", false, false), false);
  assert.equal(isOpen(ALL_CLOSED, "k", true, false), false);
  assert.equal(isOpen(ALL_CLOSED, "k", false, true), true);
  const closedByHand = toggled(HOT_OPEN, "k", true);
  assert.equal(isOpen(closedByHand, "k", true, true), false);
  assert.equal(isOpen(toggled(closedByHand, "k", false), "k", false, false), true);
});

test("a line is found again by its key", () => {
  const nodes = buildNodes(rows);
  const sound = nodes[0].children[1];
  assert.equal(findNode(nodes, sound.key), sound);
  assert.equal(findNode(nodes, "/missing"), null);
});
