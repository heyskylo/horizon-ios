"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import type { Envelope, MetaRow, Side } from "@/lib/types";
import { t } from "@/lib/i18n";

export interface LoadedCapture {
  handle: number;
  name: string;
  bytes: number;
  meta: Envelope<MetaRow>;
  /** The side the panels show: the server when the capture has one, else the client. */
  side: Side;
  sides: Side[];
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
    async (source: File | ArrayBuffer, name: string) => {
      if (!reader) {
        setError(t("error.readerLoading"));
        return;
      }
      setLoading(true);
      setError(null);
      try {
        const bytes = source instanceof ArrayBuffer ? source : await source.arrayBuffer();
        const handle = reader.open(bytes);
        const meta = reader.query<MetaRow>(handle, { op: "meta", file: name });
        if (!meta.ok) {
          reader.close(handle);
          throw new Error(meta.error?.message ?? t("error.metaFailed"));
        }
        const sides = meta.rows.map((row) => row.side);
        const side: Side = sides.includes("server") ? "server" : (sides[0] ?? "client");
        if (openHandle.current !== null) reader.close(openHandle.current);
        openHandle.current = handle;
        setCapture({ handle, name, bytes: bytes.byteLength, meta, side, sides });
      } catch (failure) {
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
