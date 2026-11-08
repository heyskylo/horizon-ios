import type { ModInfo, TreeRow } from "./types";

/** What the call tree's colours stand for, from the game's own code outwards. */
export type CrateGroup = "world" | "client" | "horizon" | "std" | "third" | "mod";

export const CRATE_GROUPS: readonly CrateGroup[] = ["world", "client", "horizon", "std", "third", "mod"];

const STD = new Set(["std", "core", "alloc"]);

/** Cargo names a crate `my-mod` where its symbols say `my_mod`. */
function symbolName(crate: string): string {
  return crate.replaceAll("-", "_");
}

/** The crates of the native mods a capture lists. */
export function modCrates(mods: readonly ModInfo[]): ReadonlySet<string> {
  return new Set(mods.filter((mod) => mod.kind === "native" && mod.crate).map((mod) => symbolName(mod.crate)));
}

/** The group a crate is coloured as; none when the capture did not record one. */
export function crateGroup(crate: string | null | undefined, mods: ReadonlySet<string>): CrateGroup | null {
  if (!crate) return null;
  const name = symbolName(crate);
  if (mods.has(name)) return "mod";
  if (name === "horizon_world") return "world";
  if (name.startsWith("horizon_client")) return "client";
  if (name === "horizon" || name.startsWith("horizon_")) return "horizon";
  if (STD.has(name)) return "std";
  return "third";
}

/** The groups that some row of the tree belongs to, in the legend's order. */
export function groupsIn(rows: readonly TreeRow[], mods: ReadonlySet<string>): CrateGroup[] {
  const seen = new Set<CrateGroup>();
  const visit = (row: TreeRow) => {
    const group = crateGroup(row.crate, mods);
    if (group) seen.add(group);
    row.children?.forEach(visit);
  };
  rows.forEach(visit);
  return CRATE_GROUPS.filter((group) => seen.has(group));
}
