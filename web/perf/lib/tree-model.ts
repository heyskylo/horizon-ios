import type { TreeRow } from "./types";

export interface Folded {
  rows: number;
  total: number;
}

/** One line of the call tree: a function, or a chain of single-child functions shown as one line. */
export interface TreeNode {
  /** Where the line sits in the tree; it stays the same whatever is open, so open state can refer to it. */
  key: string;
  /** The chain from its entry to its last function; a lone function is a chain of one. */
  members: TreeRow[];
  children: TreeNode[];
  /** What the last function's smaller children left out. */
  folded: Folded | null;
}

export function entry(node: TreeNode): TreeRow {
  return node.members[0];
}

export function last(node: TreeNode): TreeRow {
  return node.members[node.members.length - 1];
}

/** A function with one child, no time of its own and nothing folded away shares its line with that child. */
function absorbs(row: TreeRow): boolean {
  return row.children?.length === 1 && !row.folded && row.self === 0;
}

function build(row: TreeRow, parentKey: string): TreeNode {
  const key = `${parentKey}/${row.id}`;
  const members = [row];
  let tail = row;
  while (absorbs(tail)) {
    tail = tail.children![0];
    members.push(tail);
  }
  return { key, members, children: (tail.children ?? []).map((child) => build(child, key)), folded: tail.folded ?? null };
}

/** The lines of a reply's rows, single-child chains collapsed. */
export function buildNodes(rows: readonly TreeRow[], scope = ""): TreeNode[] {
  return rows.map((row) => build(row, scope));
}

export function findNode(nodes: readonly TreeNode[], key: string): TreeNode | null {
  for (const node of nodes) {
    if (node.key === key) return node;
    const inside = findNode(node.children, key);
    if (inside) return inside;
  }
  return null;
}

export type FlatRow =
  | { kind: "node"; node: TreeNode; depth: number; open: boolean }
  | { kind: "fold"; key: string; depth: number; folded: Folded };

/** The lines on screen: only the children of open lines are visited, and a fold row closes each open list. */
export function flatten(nodes: readonly TreeNode[], isOpen: (node: TreeNode) => boolean, depth = 0): FlatRow[] {
  const out: FlatRow[] = [];
  for (const node of nodes) {
    const open = (node.children.length > 0 || node.folded !== null) && isOpen(node);
    out.push({ kind: "node", node, depth, open });
    if (!open) continue;
    out.push(...flatten(node.children, isOpen, depth + 1));
    if (node.folded) out.push({ kind: "fold", key: `${node.key}/folded`, depth: depth + 1, folded: node.folded });
  }
  return out;
}

/** Whether a line matches a search: any function of its chain has the text in its name. */
function matches(node: TreeNode, needle: string): boolean {
  return node.members.some((row) => row.name.toLowerCase().includes(needle));
}

/** The lines that match a search, and the lines above them, which a search opens. */
export function search(nodes: readonly TreeNode[], query: string): { hits: Set<string>; parents: Set<string> } {
  const hits = new Set<string>();
  const parents = new Set<string>();
  const needle = query.trim().toLowerCase();
  if (needle === "") return { hits, parents };
  const visit = (node: TreeNode): boolean => {
    let below = false;
    for (const child of node.children) below = visit(child) || below;
    if (below) parents.add(node.key);
    const self = matches(node, needle);
    if (self) hits.add(node.key);
    return below || self;
  };
  nodes.forEach(visit);
  return { hits, parents };
}

export interface Piece {
  text: string;
  hit: boolean;
}

/** A name cut at every occurrence of the search text, ignoring case. */
export function highlight(text: string, query: string): Piece[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [{ text, hit: false }];
  const lower = text.toLowerCase();
  const pieces: Piece[] = [];
  let from = 0;
  for (let at = lower.indexOf(needle); at >= 0; at = lower.indexOf(needle, from)) {
    if (at > from) pieces.push({ text: text.slice(from, at), hit: false });
    pieces.push({ text: text.slice(at, at + needle.length), hit: true });
    from = at + needle.length;
  }
  if (from < text.length) pieces.push({ text: text.slice(from), hit: false });
  return pieces;
}

/** The lines on the hottest path: from the biggest root, each time into the biggest child. */
export function hottestPath(nodes: readonly TreeNode[]): Set<string> {
  const path = new Set<string>();
  let level = nodes;
  while (level.length > 0) {
    const top = level.reduce((best, node) => (entry(node).total > entry(best).total ? node : best));
    path.add(top.key);
    level = top.children;
  }
  return path;
}

/** What is open: the hottest path or nothing as the starting point, and what the reader toggled since. */
export interface OpenState {
  base: "hot" | "none";
  overrides: ReadonlyMap<string, boolean>;
}

export const HOT_OPEN: OpenState = { base: "hot", overrides: new Map() };
export const ALL_CLOSED: OpenState = { base: "none", overrides: new Map() };

/** A toggle of the reader wins over a search, and a search wins over the starting point. */
export function isOpen(state: OpenState, key: string, onHotPath: boolean, opensForSearch: boolean): boolean {
  const toggled = state.overrides.get(key);
  if (toggled !== undefined) return toggled;
  return opensForSearch || (state.base === "hot" && onHotPath);
}

export function toggled(state: OpenState, key: string, wasOpen: boolean): OpenState {
  return { base: state.base, overrides: new Map(state.overrides).set(key, !wasOpen) };
}
