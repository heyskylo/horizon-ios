// Drawing helpers for a comparison: they format and sort what `diff` computed, never compute a performance number.
import type { DiffRow } from "./types";

export type Verdict = DiffRow["verdict"];

/** How a verdict is drawn: `better` and `worse` take a colour, `same` and `unknown` stay neutral, `edge` is a row only one capture has. */
export type Tone = "better" | "worse" | "same" | "unknown" | "edge";

const TONES: Record<Verdict, Tone> = {
  faster: "better",
  slower: "worse",
  within_noise: "same",
  negligible: "same",
  too_few_blocks: "unknown",
  new: "edge",
  gone: "edge",
};

export function toneOf(verdict: Verdict): Tone {
  return TONES[verdict];
}

/** The diff's rows by stable id, so a view finds the change of each row it draws. */
export type Changes = ReadonlyMap<string, DiffRow>;

export function indexChanges(rows: readonly DiffRow[]): Changes {
  return new Map(rows.map((row) => [row.id, row]));
}

/** Milliseconds with the digits that tell 0.04 from 0.05 and 124.0 from 124.1, as the CLI prints them. */
function ms(value: number): string {
  const size = Math.abs(value);
  return value.toFixed(size >= 10 ? 1 : size >= 1 ? 2 : 3);
}

function signed(text: string, value: number): string {
  return value > 0 ? `+${text}` : text;
}

/** `+0.420 ms`, or null when the row has no change (a row only one capture has). */
export function changeMs(row: DiffRow): string | null {
  if (row.diff === null) return null;
  return `${signed(ms(row.diff), row.diff)} ms`;
}

/** `+12%`, or null when there is no baseline to take a share of. */
export function changePct(row: DiffRow): string | null {
  if (row.diff_pct === null) return null;
  const text = Math.abs(row.diff_pct) >= 10 ? row.diff_pct.toFixed(0) : row.diff_pct.toFixed(1);
  return `${signed(text, row.diff_pct)}%`;
}

/** The rows only the comparison has, biggest first: a view lists them because it cannot place them among its own. */
export function onlyInComparison(
  rows: readonly DiffRow[],
  prefix: "s:" | "f:",
  shown: ReadonlySet<string>,
  limit: number,
): DiffRow[] {
  return rows
    .filter((row) => row.verdict === "new" && row.id.startsWith(prefix) && !shown.has(row.id))
    .sort((a, b) => (b.b ?? 0) - (a.b ?? 0) || a.id.localeCompare(b.id))
    .slice(0, limit);
}

/** What a row of the comparison is called: a function's name, else its id without the kind letter. */
export function rowName(row: DiffRow): string {
  return row.name ?? row.id.replace(/^[a-z]:/, "");
}
