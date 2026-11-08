import { num } from "./format.ts";
import type { Unit } from "./types";

export type AmountKind = "time" | "bytes";

/** What a samples reply counts: allocations are bytes, every other selection is milliseconds. */
export function amountKind(unit: Unit | undefined): AmountKind {
  return unit !== undefined && unit.startsWith("bytes_") ? "bytes" : "time";
}

const BYTE_STEPS = ["B", "KiB", "MiB", "GiB"] as const;

/** Bytes as `1.5 KiB`: the largest step that keeps the number at or above one. */
export function bytes(value: number): string {
  let step = 0;
  let scaled = Math.abs(value);
  while (scaled >= 1024 && step < BYTE_STEPS.length - 1) {
    scaled /= 1024;
    step += 1;
  }
  const sign = value < 0 ? "-" : "";
  return `${sign}${step === 0 ? Math.round(scaled) : scaled.toFixed(1)} ${BYTE_STEPS[step]}`;
}

/** A weight from a samples reply: milliseconds keep two decimals, bytes pick their step. */
export function amount(value: number, kind: AmountKind): string {
  return kind === "bytes" ? bytes(value) : `${num(value, 2)} ms`;
}
