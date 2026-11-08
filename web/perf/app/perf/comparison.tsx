"use client";

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { indexChanges } from "@/lib/compare";
import type { Changes } from "@/lib/compare";
import type { DiffRow } from "@/lib/types";
import { replyError } from "./use-capture";
import type { CaptureRef, LoadedCapture } from "./use-capture";

/** Every function and path the views might draw, down to a thousandth of a millisecond. */
const ASK = { limit: 1_000_000, min_ms: 0.002, calls: true } as const;

/** The second capture and what it changed: the one place every view reads a comparison from. */
export interface Comparison {
  /** The second capture, while one is picked. */
  other: CaptureRef | null;
  /** Every row the diff answered with. */
  rows: readonly DiffRow[];
  /** The same rows by stable id. */
  changes: Changes;
  warnings: string[];
  /** Why the second capture could not be opened or compared, when it could not. */
  error: string | null;
  /** Whether the views can show changes: a second capture is picked and the two compare. */
  ready: boolean;
  pick: (file: File) => Promise<void>;
  clear: () => void;
}

const NONE: Comparison = {
  other: null,
  rows: [],
  changes: new Map(),
  warnings: [],
  error: null,
  ready: false,
  pick: async () => {},
  clear: () => {},
};

const Context = createContext<Comparison>(NONE);

/** What the views read: the rows' changes, or null while nothing is compared (or the two cannot be). */
export function useChanges(): Comparison | null {
  const comparison = useContext(Context);
  return comparison.ready ? comparison : null;
}

export function useComparison(): Comparison {
  return useContext(Context);
}

function message(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

interface Props {
  reader: HorizonPerf;
  base: LoadedCapture;
  children: ReactNode;
}

/** Holds the second capture and its diff against the open one, for the side on show. */
export function ComparisonProvider({ reader, base, children }: Props) {
  const [other, setOther] = useState<CaptureRef | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  const handle = useRef<number | null>(null);

  const release = useCallback(() => {
    if (handle.current !== null) reader.close(handle.current);
    handle.current = null;
  }, [reader]);

  useEffect(() => release, [release]);

  const pick = useCallback(
    async (file: File) => {
      try {
        const opened = reader.open(await file.arrayBuffer());
        release();
        handle.current = opened;
        setPickError(null);
        setOther({ handle: opened, name: file.name });
      } catch (failure) {
        setPickError(message(failure));
      }
    },
    [reader, release],
  );

  const clear = useCallback(() => {
    release();
    setOther(null);
    setPickError(null);
  }, [release]);

  const diffed = useMemo(() => {
    if (!other) return { rows: [] as DiffRow[], warnings: [] as string[], error: null };
    try {
      const reply = reader.diff(base.handle, other.handle, { side: base.side, ...ASK });
      if (reply.ok === false || !reply.reply) {
        return { rows: [] as DiffRow[], warnings: [] as string[], error: replyError(reply.error) };
      }
      return {
        rows: reply.reply.rows,
        warnings: reply.reply.warnings.filter((warning) => !warning.includes("--")),
        error: null,
      };
    } catch (failure) {
      return { rows: [] as DiffRow[], warnings: [] as string[], error: message(failure) };
    }
  }, [reader, base.handle, base.side, other]);

  const value = useMemo<Comparison>(
    () => ({
      other,
      rows: diffed.rows,
      changes: indexChanges(diffed.rows),
      warnings: diffed.warnings,
      error: pickError ?? diffed.error,
      ready: other !== null && diffed.error === null,
      pick,
      clear,
    }),
    [other, diffed, pickError, pick, clear],
  );

  return <Context.Provider value={value}>{children}</Context.Provider>;
}
