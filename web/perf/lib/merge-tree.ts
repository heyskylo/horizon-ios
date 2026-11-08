import type { TreeRow } from "./types";

const WRAPPERS = ["spans::time", "FnOnce::call_once"];

function isWrapper(row: TreeRow): boolean {
  return WRAPPERS.some((wrapper) => row.name.includes(wrapper));
}

function unwrap(row: TreeRow): TreeRow[] {
  return isWrapper(row) ? (row.children ?? []).flatMap(unwrap) : [row];
}

function sum(values: (number | null | undefined)[]): number | null {
  const known = values.filter((value): value is number => value != null);
  return known.length > 0 ? known.reduce((a, b) => a + b, 0) : null;
}

function group(occurrences: TreeRow[], path: string): TreeRow[] {
  const byFunction = new Map<string, TreeRow[]>();
  for (const row of occurrences) {
    const list = byFunction.get(row.function);
    if (list) list.push(row);
    else byFunction.set(row.function, [row]);
  }
  return [...byFunction.entries()]
    .map(([fn, rows]): TreeRow => {
      const id = `${path}/${fn}`;
      return {
        ...rows[0],
        id,
        total: rows.reduce((total, row) => total + row.total, 0),
        self_ms: sum(rows.map((row) => row.self_ms)),
        pct: rows.reduce((total, row) => total + row.pct, 0),
        samples: rows.reduce((total, row) => total + row.samples, 0),
        children: group(rows.flatMap((row) => (row.children ?? []).flatMap(unwrap)), id),
        folded: null,
      };
    })
    .sort((a, b) => b.total - a.total);
}

/** Merge mode: every function once whatever called it, wrappers hidden, a recursion counted at its outermost call. */
export function mergeTree(rows: TreeRow[]): TreeRow[] {
  const outermost: TreeRow[] = [];
  const visit = (row: TreeRow, above: Set<string>) => {
    const wrapper = isWrapper(row);
    if (!wrapper && !above.has(row.function)) outermost.push(row);
    const next = wrapper ? above : new Set(above).add(row.function);
    for (const child of row.children ?? []) visit(child, next);
  };
  for (const row of rows) visit(row, new Set());
  return group(outermost, "");
}
