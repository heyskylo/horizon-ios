import type { MetaRow } from "./types.ts";

/** What each file needs of a capture: sampled functions for pprof and folded, a recorded timeline for Perfetto. */
export const EXPORTS = [
  { kind: "pprof", label: "pprof", extension: "pprof", needs: "samples" },
  { kind: "folded", label: "folded", extension: "folded", needs: "samples" },
  { kind: "perfetto", label: "Perfetto", extension: "pb", needs: "timeline" },
] as const;

export type Export = (typeof EXPORTS)[number];

/** The exports the capture can fill: an option the capture has no data for would only fail. */
export function exportsFor(sides: readonly Pick<MetaRow, "sections" | "options">[]): Export[] {
  const has = {
    samples: sides.some((row) => (row.sections.samples ?? 0) > 0),
    timeline: sides.some((row) => row.options?.timeline === true),
  };
  return EXPORTS.filter((item) => has[item.needs]);
}
