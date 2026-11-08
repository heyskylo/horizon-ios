import type { TreeRow } from "./types";

/** One drawn frame: a function of the tree, placed by its share of the width and its depth below the graph's top. */
export interface FlameFrame {
  /** Where the row sits in the reply: child indexes from the top, `0.2.1`; it stays the same when zoomed. */
  key: string;
  row: TreeRow;
  depth: number;
  /** Left edge and width, as fractions of the drawn width. */
  start: number;
  width: number;
  parent: string | null;
  /** The children that were wide enough to draw, biggest first as the reply gives them. */
  children: string[];
}

export interface FlameLayout {
  byKey: ReadonlyMap<string, FlameFrame>;
  /** The frames of each depth, left to right. */
  levels: readonly (readonly FlameFrame[])[];
}

/** The row a key points at, or null when the rows changed under it. */
function rowAt(rows: readonly TreeRow[], key: string): TreeRow | null {
  let level: readonly TreeRow[] = rows;
  let found: TreeRow | null = null;
  for (const part of key.split(".")) {
    found = level[Number(part)] ?? null;
    if (!found) return null;
    level = found.children ?? [];
  }
  return found;
}

/** The rows from the top down to a zoomed frame, each with its key: the way back out. */
export function zoomPath(rows: readonly TreeRow[], zoom: string | null): { key: string; row: TreeRow }[] {
  if (zoom === null) return [];
  const parts = zoom.split(".");
  const path: { key: string; row: TreeRow }[] = [];
  for (let at = 0; at < parts.length; at += 1) {
    const key = parts.slice(0, at + 1).join(".");
    const row = rowAt(rows, key);
    if (!row) return [];
    path.push({ key, row });
  }
  return path;
}

/**
 * Frames side by side as wide as their time. A zoomed frame fills the width and is drawn at depth 0; a frame
 * narrower than `minWidth` and everything under it is left out. The time a frame spent in no child is the gap at its end.
 */
export function layoutFlame(rows: readonly TreeRow[], zoom: string | null, minWidth: number): FlameLayout {
  const byKey = new Map<string, FlameFrame>();
  const levels: FlameFrame[][] = [];
  const zoomed = zoom === null ? null : rowAt(rows, zoom);
  const tops: { key: string; row: TreeRow }[] =
    zoomed && zoom !== null ? [{ key: zoom, row: zoomed }] : rows.map((row, at) => ({ key: `${at}`, row }));
  const sum = tops.reduce((total, top) => total + top.row.total, 0);
  const scale = sum > 0 ? 1 / sum : 0;

  const place = (key: string, row: TreeRow, depth: number, start: number, width: number, parent: string | null) => {
    const frame: FlameFrame = { key, row, depth, start, width, parent, children: [] };
    byKey.set(key, frame);
    (levels[depth] ??= []).push(frame);
    let cursor = start;
    (row.children ?? []).forEach((child, at) => {
      const room = start + width - cursor;
      const wide = Math.min(child.total * scale, room);
      if (wide >= minWidth && wide > 0) {
        frame.children.push(`${key}.${at}`);
        place(`${key}.${at}`, child, depth + 1, cursor, wide, key);
      }
      cursor += child.total * scale;
    });
  };

  let cursor = 0;
  for (const top of tops) {
    const width = top.row.total * scale;
    if (width >= minWidth && width > 0) place(top.key, top.row, 0, cursor, width, null);
    cursor += width;
  }
  return { byKey, levels };
}

/** The frame under a point: `x` is a fraction of the drawn width, `depth` the level. */
export function frameAt(layout: FlameLayout, depth: number, x: number): FlameFrame | null {
  const level = layout.levels[depth];
  if (!level) return null;
  let low = 0;
  let high = level.length - 1;
  let found: FlameFrame | null = null;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (level[middle].start <= x) {
      found = level[middle];
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return found && x < found.start + found.width ? found : null;
}

export type FlameMove = "parent" | "child" | "previous" | "next";

/** The key the arrow keys lead to from a frame, or null at an edge. */
export function moveFrom(layout: FlameLayout, key: string, move: FlameMove): string | null {
  const frame = layout.byKey.get(key);
  if (!frame) return null;
  if (move === "parent") return frame.parent;
  if (move === "child") return frame.children[0] ?? null;
  const siblings = (frame.parent === null ? layout.levels[0] : layout.byKey.get(frame.parent)?.children.map((id) => layout.byKey.get(id)!)) ?? [];
  const at = siblings.findIndex((sibling) => sibling.key === key);
  return siblings[at + (move === "next" ? 1 : -1)]?.key ?? null;
}

/** The text cut at the end, with an ellipsis, to fit `room` pixels by `measure`; empty when not even the ellipsis fits. */
export function fitText(text: string, room: number, measure: (text: string) => number): string {
  if (measure(text) <= room) return text;
  const ellipsis = "…";
  if (measure(ellipsis) > room) return "";
  let low = 0;
  let high = text.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (measure(text.slice(0, middle) + ellipsis) <= room) low = middle;
    else high = middle - 1;
  }
  return text.slice(0, low) + ellipsis;
}

/** Whether a frame's name has the search text in it, ignoring case. */
export function nameMatches(name: string, query: string): boolean {
  const needle = query.trim().toLowerCase();
  return needle !== "" && name.toLowerCase().includes(needle);
}
