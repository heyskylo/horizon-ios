export const TABS = [
  "summary",
  "metadata",
  "gpu",
  "spans",
  "ticks",
  "world",
  "net",
  "memory",
  "marks",
  "warnings",
  "hot",
  "tree",
  "compare",
  "query",
] as const;

export type Tab = (typeof TABS)[number];

const QUERY: Partial<Record<Tab, string>> = {
  gpu: "gpu",
  spans: "spans",
  ticks: "ticks",
  world: "world",
  memory: "memory",
  marks: "marks",
  warnings: "warnings",
  hot: "hot",
  tree: "tree",
};

/** The tabs with something to show: `hasRows` says whether a query answers with any rows for this side. */
export function tabsFor(hasRows: (query: string) => boolean, hasTraffic: boolean): Tab[] {
  return TABS.filter((tab) => {
    if (tab === "net") return hasTraffic;
    const query = QUERY[tab];
    return !query || hasRows(query);
  });
}
