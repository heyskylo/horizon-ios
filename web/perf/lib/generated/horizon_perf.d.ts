// The type side of the JS wrapper `cargo xtask perf-wasm` writes (crates/horizon-perf/web/horizon_perf.js).
import type { Envelope, QueryRequest } from "../types";

export type ReplyKind = "json" | "bytes" | "text";

export declare class HorizonPerfError extends Error {
  readonly code: string;
}

export interface DiffReply {
  /** False when the two captures cannot be compared; `text` is then absent. */
  ok?: boolean;
  text?: string;
  reply?: unknown;
  error?: { code: string; message: string };
}

export interface SummaryRequest {
  file?: string;
  budget?: number;
  side?: string;
  thread?: string;
  slow?: boolean;
  baseline?: number;
}

export interface DiffRequest {
  side?: string;
  thread?: string;
  min_ms?: number;
  limit?: number;
}

export interface ExportRequest {
  kind: string;
  side?: string;
  thread?: string;
  select?: string;
  slow?: boolean;
}

export declare class HorizonPerf {
  /** Fetches and instantiates the wasm at `url`. */
  static load(url: string): Promise<HorizonPerf>;
  /** Decodes a capture and returns its handle. */
  open(bytes: ArrayBuffer | Uint8Array): number;
  /** Runs a query; the envelope's `ok` says whether it answered. */
  query<R = unknown>(handle: number, request: QueryRequest | string): Envelope<R>;
  /** The agent summary text. */
  summary(handle: number, request?: SummaryRequest | string): string;
  /** B's statistics against A, with the text table. */
  diff(a: number, b: number, request?: DiffRequest | string): DiffReply;
  /** pprof, folded or Perfetto bytes. */
  export(handle: number, request?: ExportRequest | string): Uint8Array;
  /** Forgets a decoded capture. */
  close(handle: number): void;
}
