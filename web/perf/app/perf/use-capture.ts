"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import type { Envelope, MetaRow, QueryRequest, Side } from "@/lib/types";
import { t } from "@/lib/i18n";

export interface LoadedCapture {
  handle: number;
  name: string;
  meta: Envelope<MetaRow>;
  /** The side the panels show: the server when the capture has one, else the client. */
  side: Side;
  sides: Side[];
}

/** What a comparison needs of a capture. */
export type CaptureRef = Pick<LoadedCapture, "handle" | "name">;

/** A query failure is a sentence; a failure of the wasm call itself has a code and a message. */
export function replyError(error: string | { message?: string } | undefined): string {
  return (typeof error === "string" ? error : error?.message) ?? t("error.queryFailed");
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Loads the reader once, and opens captures with it. A capture lives in the wasm's memory only. */
export function useReader() {
  const [reader, setReader] = useState<HorizonPerf | null>(null);
  const [readerError, setReaderError] = useState<string | null>(null);
  const [capture, setCapture] = useState<LoadedCapture | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const openHandle = useRef<number | null>(null);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const { HorizonPerf } = await import("@/lib/generated/horizon_perf");
        const loaded = await HorizonPerf.load("/perf/horizon_perf.wasm");
        if (live) setReader(loaded);
      } catch (failure) {
        if (live) setReaderError(message(failure));
      }
    })();
    return () => {
      live = false;
    };
  }, []);

  const open = useCallback(
    async (file: File, name: string) => {
      if (!reader) {
        setError(t("error.readerLoading"));
        return;
      }
      setLoading(true);
      setError(null);
      let handle: number | null = null;
      try {
        const bytes = await file.arrayBuffer();
        handle = reader.open(bytes);
        const meta = reader.query<MetaRow>(handle, { op: "meta", file: name });
        if (!meta.ok) throw new Error(meta.error ? replyError(meta.error) : t("error.metaFailed"));
        const sides = meta.rows.map((row) => row.side);
        const side: Side = sides.includes("server") ? "server" : (sides[0] ?? "client");
        if (openHandle.current !== null) reader.close(openHandle.current);
        openHandle.current = handle;
        setCapture({ handle, name, meta, side, sides });
      } catch (failure) {
        if (handle !== null) reader.close(handle);
        setCapture(null);
        setError(message(failure));
      } finally {
        setLoading(false);
      }
    },
    [reader],
  );

  const close = useCallback(() => {
    if (reader && openHandle.current !== null) reader.close(openHandle.current);
    openHandle.current = null;
    setCapture(null);
    setError(null);
  }, [reader]);

  const showSide = useCallback((side: Side) => {
    setCapture((current) => (current ? { ...current, side } : current));
  }, []);

  return { reader, readerError, capture, loading, error, open, close, showSide };
}

/** One query's rows for the side on show; `filters` and `args` compare by value, and `enabled` false skips it. */
export function useRows<R>(
  reader: HorizonPerf,
  capture: LoadedCapture,
  op: string,
  filters: QueryRequest["filters"] = {},
  enabled = true,
  args: readonly string[] = [],
): { rows: R[]; error: string | null; reply: Envelope<R> | null } {
  const key = JSON.stringify(filters);
  const argsKey = JSON.stringify(args);
  return useMemo(() => {
    if (!enabled) return { rows: [], error: null, reply: null };
    try {
      const reply = reader.query<R>(capture.handle, {
        op,
        file: capture.name,
        args: JSON.parse(argsKey) as string[],
        filters: { side: capture.side, ...(JSON.parse(key) as QueryRequest["filters"]) },
      });
      return reply.ok
        ? { rows: reply.rows, error: null, reply }
        : { rows: [], error: replyError(reply.error), reply };
    } catch (failure) {
      return { rows: [], error: message(failure), reply: null };
    }
  }, [reader, capture.handle, capture.name, capture.side, op, key, argsKey, enabled]);
}
