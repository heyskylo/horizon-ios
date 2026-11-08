"use client";

import { createContext, useContext, useMemo } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { entry, last } from "@/lib/tree-model";
import type { OpenState, TreeNode } from "@/lib/tree-model";
import { probeOptions } from "@/lib/tree-options";
import type { Ask, SampleFilters, SampleOptions } from "@/lib/tree-options";
import type { TreeRow, Unit } from "@/lib/types";
import type { AmountKind } from "@/lib/units";
import type { LoadedCapture } from "./use-capture";

/** The shape of the tree the wasm answers with: deep, with siblings under 0.05 % folded into their parent's `folded`. */
export const TREE_SHAPE = { depth: 64, min_pct: 0.05, limit: 1000, self_share: true } as const;

/** Where a body of rows sits: the whole tree, or one group's samples. */
export interface Scope {
  id: string;
  filters: Partial<SampleFilters> | null;
}

export const WHOLE_TREE: Scope = { id: "", filters: null };

/** The line the detail panel describes. */
export interface Target {
  scope: Scope;
  key: string;
  /** What `split` takes: the row's own id, or its function when merge mode made the id up. */
  id: string;
  /** What `callers` takes. */
  function: string;
  names: string[];
  entry: TreeRow;
  last: TreeRow;
}

export function targetOf(scope: Scope, node: TreeNode, merge: boolean): Target {
  const first = entry(node);
  return {
    scope,
    key: node.key,
    id: merge ? first.function : first.id,
    function: first.function,
    names: node.members.map((member) => member.name),
    entry: first,
    last: last(node),
  };
}

/** What every part of the tree reads and changes: the reader's choices and the state of the list. */
export interface TreeEnv {
  reader: HorizonPerf;
  capture: LoadedCapture;
  filters: SampleFilters;
  merge: boolean;
  query: string;
  mods: ReadonlySet<string>;
  kind: AmountKind;
  unit: Unit | undefined;
  open: OpenState;
  setOpen: (next: OpenState) => void;
  selected: Target | null;
  select: (target: Target | null) => void;
  focus: (target: Target) => void;
}

const Env = createContext<TreeEnv | null>(null);

export const TreeEnvProvider = Env.Provider;

export function useTreeEnv(): TreeEnv {
  const env = useContext(Env);
  if (!env) throw new Error("the call tree's parts need their panel");
  return env;
}

const NO_OPTIONS: SampleOptions = { selections: [], groups: ["none"], threads: [], thread: null };

/** What the capture's samples can be shown as, found once per capture and side. */
export function useSampleOptions(reader: HorizonPerf, capture: LoadedCapture): SampleOptions {
  return useMemo(() => {
    const ask: Ask = <R,>(op: string, filters: Record<string, unknown>) =>
      reader.query<R>(capture.handle, { op, file: capture.name, filters: { side: capture.side, ...filters } });
    try {
      return probeOptions(ask);
    } catch {
      return NO_OPTIONS;
    }
  }, [reader, capture.handle, capture.name, capture.side]);
}
